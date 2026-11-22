// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! UI-independent notification registry.
//!
//! This crate is the only place where notification retention, ordering,
//! deduplication, read state, and notification metadata are defined. GPUI,
//! popovers, and feature-specific actions belong to adapters and consumers.

use std::time::{Duration, Instant};

/// The user-visible category of a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Message,
    Info,
    Success,
    Warning,
    Error,
}

/// A non-executable action exposed by a notification.
///
/// Owner actions use the stable `id` when activated. Open-URL actions carry
/// their destination as data; the registry never stores callbacks or state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationAction {
    pub id: String,
    pub label: String,
    pub target: NotificationActionTarget,
}

/// Data-only action behavior. Owner actions emit an event; open-URL actions
/// are handled by the notifications adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationActionTarget {
    Owner,
    OpenUrl(String),
}

impl NotificationAction {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            target: NotificationActionTarget::Owner,
        }
    }

    pub fn open_url(
        id: impl Into<String>,
        label: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            target: NotificationActionTarget::OpenUrl(url.into()),
        }
    }
}

/// Data submitted by a feature module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDraft {
    pub kind: NotificationKind,
    pub title: String,
    pub summary: String,
    pub details: Option<String>,
    pub source: Option<String>,
    pub actions: Vec<NotificationAction>,
    /// Optional transient presentation lifetime. The record remains retained
    /// after this duration expires.
    pub toast_duration: Option<Duration>,
    /// Optional stable key used to suppress repeated equivalent events.
    pub dedupe_key: Option<String>,
}

impl NotificationDraft {
    pub fn new(
        kind: NotificationKind,
        title: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            title: title.into(),
            summary: summary.into(),
            details: None,
            source: None,
            actions: Vec::new(),
            toast_duration: None,
            dedupe_key: None,
        }
    }

    pub fn details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn action(mut self, action: NotificationAction) -> Self {
        self.actions.push(action);
        self
    }

    pub fn toast_for(mut self, duration: Duration) -> Self {
        self.toast_duration = Some(duration);
        self
    }

    pub fn dedupe_key(mut self, key: impl Into<String>) -> Self {
        self.dedupe_key = Some(key.into());
        self
    }
}

/// A retained notification with registry-owned identity and state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRecord {
    pub id: u64,
    pub kind: NotificationKind,
    pub title: String,
    pub summary: String,
    pub details: Option<String>,
    pub source: Option<String>,
    pub actions: Vec<NotificationAction>,
    pub dedupe_key: Option<String>,
    pub created_at: Instant,
    pub toast_until: Option<Instant>,
    pub read: bool,
}

const DEDUPE_WINDOW: Duration = Duration::from_secs(2);
const MAX_ITEMS: usize = 100;
const MAX_TOASTS: usize = 3;

/// In-memory notification registry used by the application.
///
/// Retained records stay until dismissed or cleared. Toast deadlines control
/// only transient visibility; they never auto-dismiss the retained record.
#[derive(Debug, Default)]
pub struct NotificationRegistry {
    items: Vec<NotificationRecord>,
    next_id: u64,
}

impl NotificationRegistry {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
        }
    }

    pub fn publish(&mut self, draft: NotificationDraft) -> Option<u64> {
        self.insert(draft, Instant::now())
    }

    /// Insert with an explicit clock value for deterministic tests.
    pub fn insert(&mut self, draft: NotificationDraft, now: Instant) -> Option<u64> {
        if let (Some(key), Some(newest_key), Some(newest)) = (
            draft.dedupe_key.as_ref(),
            self.items.first().and_then(|item| item.dedupe_key.as_ref()),
            self.items.first(),
        ) {
            if key == newest_key && now.duration_since(newest.created_at) < DEDUPE_WINDOW {
                return None;
            }
        }

        let id = self.next_id;
        self.next_id += 1;
        self.items.insert(
            0,
            NotificationRecord {
                id,
                kind: draft.kind,
                title: draft.title,
                summary: draft.summary,
                details: draft.details,
                source: draft.source,
                actions: draft.actions,
                dedupe_key: draft.dedupe_key,
                created_at: now,
                toast_until: draft.toast_duration.map(|duration| now + duration),
                read: false,
            },
        );
        let mut visible_toasts = 0;
        for item in &mut self.items {
            if item.toast_until.is_some() {
                visible_toasts += 1;
                if visible_toasts > MAX_TOASTS {
                    item.toast_until = None;
                }
            }
        }
        self.items.truncate(MAX_ITEMS);
        Some(id)
    }

    pub fn list(&self) -> &[NotificationRecord] {
        &self.items
    }

    pub fn unread_count(&self) -> usize {
        self.items.iter().filter(|item| !item.read).count()
    }

    pub fn mark_read(&mut self, id: u64) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        let changed = !item.read;
        item.read = true;
        changed
    }

    pub fn action(&self, id: u64, action_id: &str) -> Option<&NotificationAction> {
        self.items
            .iter()
            .find(|item| item.id == id)?
            .actions
            .iter()
            .find(|action| action.id == action_id)
    }

    pub fn dismiss_toast(&mut self, id: u64) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        item.toast_until.take().is_some()
    }

    pub fn expire_toasts(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for item in &mut self.items {
            if item.toast_until.is_some_and(|deadline| deadline <= now) {
                item.toast_until = None;
                changed = true;
            }
        }
        changed
    }

    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        before != self.items.len()
    }

    pub fn clear(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        self.items.clear();
        true
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(summary: &str) -> NotificationDraft {
        NotificationDraft::new(NotificationKind::Info, "Title", summary)
            .source("tests")
            .dedupe_key(format!("tests:Title:{summary}"))
    }

    #[test]
    fn retains_newest_first_and_tracks_unread() {
        let mut registry = NotificationRegistry::new();
        registry.publish(draft("one"));
        registry.publish(draft("two"));
        assert_eq!(registry.list()[0].summary, "two");
        assert_eq!(registry.unread_count(), 2);
        assert!(registry.mark_read(registry.list()[0].id));
        assert_eq!(registry.unread_count(), 1);
    }

    #[test]
    fn deduplicates_only_equivalent_recent_events() {
        let mut registry = NotificationRegistry::new();
        let now = Instant::now();
        assert!(registry.insert(draft("one"), now).is_some());
        assert!(registry
            .insert(draft("one"), now + Duration::from_millis(500))
            .is_none());
        assert!(registry
            .insert(draft("two"), now + Duration::from_millis(500))
            .is_some());
        assert!(registry
            .insert(draft("one"), now + Duration::from_secs(3))
            .is_some());
    }

    #[test]
    fn details_and_actions_are_retained_as_data() {
        let mut registry = NotificationRegistry::new();
        registry.publish(
            NotificationDraft::new(NotificationKind::Error, "Failed", "Could not connect")
                .details("The server rejected the connection.")
                .action(NotificationAction::new("open-logs", "Open logs")),
        );
        assert_eq!(
            registry.list()[0].details.as_deref(),
            Some("The server rejected the connection.")
        );
        assert_eq!(registry.list()[0].actions[0].id, "open-logs");
    }
}
