// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! GPUI adapter for the app-wide notification registry.
//!
//! The retained data model and lifecycle live in
//! [`labonair_notifications_core`]. This crate adds GPUI invalidation, the
//! statusbar dropdown, and the opt-in toast overlay.
//!
//! Notifications carry data-only [`NotificationAction`]s. Owner actions emit
//! a typed [`NotificationActionEvent`]; external-URL actions are opened by
//! this adapter. A notification never stores a closure.

use std::time::{Duration, Instant};

use gpui::{App, AppContext, Context, Entity, Global, SharedString};
pub use labonair_notifications_core::TOAST_DURATION;
pub use labonair_notifications_core::{NotificationAction, NotificationActionTarget};
use labonair_notifications_core::{NotificationDraft, NotificationKind, NotificationRegistry};

mod os_notify;
mod status_item;
mod toast_overlay;

pub use os_notify::{notify_os_with_target, route_click, set_os_click_handler, ClickRoute};
pub use status_item::{registration, NotificationsStatusItem};
pub use toast_overlay::{registration as toast_overlay_registration, NotificationToastOverlay};

/// Severity of a notification. This is the GPUI-facing spelling retained for
/// current feature callers; the UI-free registry uses [`NotificationKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Message,
    Info,
    Success,
    Warning,
    Error,
}

impl From<Severity> for NotificationKind {
    fn from(value: Severity) -> Self {
        match value {
            Severity::Message => Self::Message,
            Severity::Info => Self::Info,
            Severity::Success => Self::Success,
            Severity::Warning => Self::Warning,
            Severity::Error => Self::Error,
        }
    }
}

/// A notification draft to publish. Identity and read state are assigned by
/// the UI-free registry on insert.
#[derive(Debug)]
pub struct Notification {
    pub severity: Severity,
    pub title: SharedString,
    pub body: SharedString,
    pub source: Option<SharedString>,
    pub details: Option<SharedString>,
    pub dedupe_key: Option<SharedString>,
    pub actions: Vec<NotificationAction>,
    pub toast_duration: Option<Duration>,
}

impl Notification {
    fn new(
        severity: Severity,
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
    ) -> Self {
        Self {
            severity,
            title: title.into(),
            body: body.into(),
            source: None,
            details: None,
            dedupe_key: None,
            actions: Vec::new(),
            toast_duration: None,
        }
    }

    pub fn message(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self::new(Severity::Message, title, body)
    }

    pub fn info(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self::new(Severity::Info, title, body)
    }

    pub fn success(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self::new(Severity::Success, title, body)
    }

    pub fn warning(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self::new(Severity::Warning, title, body)
    }

    pub fn error(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self::new(Severity::Error, title, body)
    }

    pub fn source(mut self, source: impl Into<SharedString>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn details(mut self, details: impl Into<SharedString>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn dedupe_key(mut self, key: impl Into<SharedString>) -> Self {
        self.dedupe_key = Some(key.into());
        self
    }

    /// Offer a user action. `id` is a stable, owner-interpreted identifier.
    pub fn action(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.actions.push(NotificationAction::new(id, label));
        self
    }

    /// Show this retained notification as a toast for `duration` as well.
    pub fn toast_for(mut self, duration: Duration) -> Self {
        self.toast_duration = Some(duration);
        self
    }

    /// Offer an action that opens an external URL in the user's browser.
    pub fn action_open_url(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        self.actions
            .push(NotificationAction::open_url(id, label, url));
        self
    }
}

/// Read-only view of a retained notification for the dropdown and toast.
#[derive(Debug, Clone)]
pub struct NotificationSnapshot {
    pub id: u64,
    pub severity: Severity,
    pub title: SharedString,
    pub body: SharedString,
    pub details: Option<SharedString>,
    pub source: Option<SharedString>,
    pub read: bool,
    pub actions: Vec<NotificationAction>,
    pub toast_until: Option<Instant>,
}

/// Emitted by the [`NotificationCenter`] when the user activates an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationActionEvent {
    pub notification_id: u64,
    pub action_id: String,
}

impl gpui::EventEmitter<NotificationActionEvent> for NotificationCenter {}

/// GPUI-facing adapter around [`NotificationRegistry`].
pub struct NotificationCenter {
    registry: NotificationRegistry,
}

impl Default for NotificationCenter {
    fn default() -> Self {
        Self {
            registry: NotificationRegistry::new(),
        }
    }
}

impl NotificationCenter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish a retained notification and start its optional toast lifetime.
    /// The retained record remains in the dropdown when that lifetime ends.
    pub fn push(&mut self, notif: Notification, cx: &mut Context<Self>) -> Option<u64> {
        let toast_duration = notif.toast_duration;
        let id = self.insert(notif, Instant::now(), cx)?;
        if toast_duration.is_some() {
            let center = cx.entity();
            cx.spawn(async move |_, cx| {
                cx.background_executor().timer(TOAST_DURATION).await;
                let _ = center.update(cx, |center, cx| {
                    center.expire_toasts(Instant::now(), cx);
                });
            })
            .detach();
        }
        Some(id)
    }

    /// Compatibility alias for callers that distinguish action results. All
    /// user-visible messages now use the same retained registry.
    pub fn push_action_result(
        &mut self,
        notif: Notification,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        self.push(notif, cx)
    }

    /// Insert with an explicit clock value for deterministic adapter tests.
    pub fn insert(
        &mut self,
        notif: Notification,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let toast_duration = notif.toast_duration;
        let default_key = format!(
            "{:?}|{}|{}|{}",
            notif.severity,
            notif.title,
            notif.body,
            notif
                .source
                .as_ref()
                .map(|value| value.to_string())
                .unwrap_or_default()
        );
        let mut draft = NotificationDraft::new(
            notif.severity.into(),
            notif.title.to_string(),
            notif.body.to_string(),
        )
        .dedupe_key(
            notif
                .dedupe_key
                .as_ref()
                .map(|value| value.to_string())
                .unwrap_or(default_key),
        );
        if let Some(source) = notif.source {
            draft = draft.source(source.to_string());
        }
        if let Some(details) = notif.details {
            draft = draft.details(details.to_string());
        }
        for action in notif.actions {
            draft = draft.action(action);
        }
        if let Some(duration) = toast_duration {
            draft = draft.toast_for(duration);
        }
        let id = self.registry.insert(draft, now)?;
        cx.notify();
        Some(id)
    }

    pub fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.registry.dismiss(id) {
            cx.notify();
        }
    }

    /// Activate a notification action: mark it read, then open its URL target
    /// or emit the owner action through [`NotificationActionEvent`].
    pub fn activate_action(&mut self, id: u64, action_id: String, cx: &mut Context<Self>) {
        let target = self
            .registry
            .action(id, &action_id)
            .map(|action| action.target.clone());
        self.dismiss_toast(id, cx);
        self.mark_read(id, cx);
        match target {
            Some(NotificationActionTarget::OpenUrl(url)) if url.starts_with("https://") => {
                cx.open_url(&url);
            }
            Some(NotificationActionTarget::OpenUrl(_)) => {}
            _ => cx.emit(NotificationActionEvent {
                notification_id: id,
                action_id,
            }),
        }
    }

    pub fn dismiss_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.registry.dismiss_toast(id) {
            cx.notify();
        }
    }

    fn expire_toasts(&mut self, now: Instant, cx: &mut Context<Self>) {
        if self.registry.expire_toasts(now) {
            cx.notify();
        }
    }

    pub fn mark_read(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.registry.mark_read(id) {
            cx.notify();
        }
    }

    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        if self.registry.clear() {
            cx.notify();
        }
    }

    pub fn len(&self) -> usize {
        self.registry.len()
    }

    pub fn unread_count(&self) -> usize {
        self.registry.unread_count()
    }

    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }

    pub fn snapshots(&self) -> Vec<NotificationSnapshot> {
        self.registry
            .list()
            .iter()
            .map(|item| NotificationSnapshot {
                id: item.id,
                severity: match item.kind {
                    NotificationKind::Message => Severity::Message,
                    NotificationKind::Info => Severity::Info,
                    NotificationKind::Success => Severity::Success,
                    NotificationKind::Warning => Severity::Warning,
                    NotificationKind::Error => Severity::Error,
                },
                title: SharedString::from(item.title.clone()),
                body: SharedString::from(item.summary.clone()),
                details: item.details.clone().map(SharedString::from),
                source: item.source.clone().map(SharedString::from),
                read: item.read,
                actions: item.actions.clone(),
                toast_until: item.toast_until,
            })
            .collect()
    }

    pub fn toast_snapshots(&self) -> Vec<NotificationSnapshot> {
        let now = Instant::now();
        self.snapshots()
            .into_iter()
            .filter(|snapshot| snapshot.toast_until.is_some_and(|deadline| deadline > now))
            .collect()
    }
}

/// App-wide handle to the [`NotificationCenter`] entity.
pub struct GlobalNotificationCenter(pub Entity<NotificationCenter>);

impl Global for GlobalNotificationCenter {}

/// Creates the [`NotificationCenter`] and installs it as a global.
pub fn init(cx: &mut App) -> Entity<NotificationCenter> {
    let center = cx.new(|_| NotificationCenter::new());
    cx.set_global(GlobalNotificationCenter(center.clone()));
    center
}

/// The [`NotificationCenter`] entity from the global.
pub fn notification_center(cx: &App) -> Entity<NotificationCenter> {
    cx.global::<GlobalNotificationCenter>().0.clone()
}

/// Fire-and-forget native OS notification without a click target.
pub fn notify_os(title: &str, body: &str) {
    os_notify::notify_os_with_target(title, body, None);
}

/// Turns a `Result<T, String>` into a retained error notification.
pub fn notify_err<T>(
    title: impl Into<SharedString>,
    result: Result<T, String>,
    cx: &mut App,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(message) => {
            let title = title.into();
            let dedupe_key = format!("{}:{message}", title);
            notification_center(cx).update(cx, |center, cx| {
                center.push_action_result(
                    Notification::error(title, "Operation failed")
                        .details(message)
                        .dedupe_key(dedupe_key),
                    cx,
                );
            });
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn base() -> Notification {
        Notification::info("Test", "Hello")
    }

    #[gpui::test]
    fn push_adds_newest_first_and_tracks_unread(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let center = cx.new(|_| NotificationCenter::new());
            center.update(cx, |center, cx| {
                let a = center.push(Notification::info("A", "first"), cx).unwrap();
                let b = center.push(Notification::info("B", "second"), cx).unwrap();
                assert_ne!(a, b);
                assert_eq!(center.snapshots()[0].title, "B");
                assert_eq!(center.unread_count(), 2);
                center.mark_read(b, cx);
                assert_eq!(center.unread_count(), 1);
            });
        });
    }

    #[gpui::test]
    fn equivalent_recent_messages_are_deduplicated(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let center = cx.new(|_| NotificationCenter::new());
            center.update(cx, |center, cx| {
                let now = Instant::now();
                assert!(center.insert(base(), now, cx).is_some());
                assert!(center
                    .insert(base(), now + std::time::Duration::from_millis(500), cx)
                    .is_none());
                assert!(center
                    .insert(
                        Notification::info("Test", "Other"),
                        now + std::time::Duration::from_millis(500),
                        cx,
                    )
                    .is_some());
            });
        });
    }

    #[gpui::test]
    fn details_and_clear_are_available_to_dropdown(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let center = cx.new(|_| NotificationCenter::new());
            center.update(cx, |center, cx| {
                center.push(
                    Notification::error("Failed", "Could not connect")
                        .details("The server rejected the connection."),
                    cx,
                );
                assert_eq!(
                    center.snapshots()[0]
                        .details
                        .as_ref()
                        .map(|value| value.to_string()),
                    Some("The server rejected the connection.".to_string())
                );
                center.clear_all(cx);
                assert!(center.is_empty());
            });
        });
    }
}
