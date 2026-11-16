//! GPUI adapter and statusbar presentation for the activity registry.
//!
//! The data model lives in [`labonair_activity`]. This crate adds GPUI
//! invalidation, the delayed reveal timer, the app-wide global, and the
//! statusbar item. Actions are data-only ids: activating one emits an
//! [`ActivityActionEvent`] and the owning module interprets it.

use std::time::Instant;

use gpui::{App, AppContext, Context, Entity, EventEmitter, Global};
pub use labonair_activity::{Activity, ActivityDraft, ActivityKind};
use labonair_activity::ActivityRegistry;

mod status_item;

pub use status_item::{registration, ActivityStatusItem};

/// Emitted when the user clicks an entry or its cancel button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityActionEvent {
    pub activity_id: u64,
    pub source: String,
    pub action_id: String,
}

pub struct ActivityCenter {
    registry: ActivityRegistry,
}

impl EventEmitter<ActivityActionEvent> for ActivityCenter {}

impl ActivityCenter {
    fn new() -> Self {
        Self {
            registry: ActivityRegistry::new(),
        }
    }

    /// Announce running work. Pair every call with [`Self::finish`] or
    /// [`Self::fail`]; the registry never times an entry out on its own.
    pub fn begin(&mut self, draft: ActivityDraft, cx: &mut Context<Self>) -> u64 {
        let now = Instant::now();
        let delay = draft.show_after;
        let id = self.registry.begin(draft, now);
        if delay.is_zero() {
            cx.notify();
        } else {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
        }
        id
    }

    pub fn finish_key(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.registry.finish_key(key) {
            cx.notify();
        }
    }

    pub fn update(
        &mut self,
        id: u64,
        message: Option<String>,
        progress: Option<f32>,
        cx: &mut Context<Self>,
    ) {
        if self.registry.update(id, message, progress, Instant::now()) {
            cx.notify();
        }
    }

    pub fn finish(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.registry.finish(id) {
            cx.notify();
        }
    }

    /// Keep the entry as a sticky failure. `click_action` is interpreted by
    /// the owner (e.g. "show the error log").
    pub fn fail(
        &mut self,
        id: u64,
        message: impl Into<String>,
        tooltip: Option<String>,
        click_action: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self
            .registry
            .fail(id, message, tooltip, click_action, Instant::now())
        {
            cx.notify();
        }
    }

    pub fn snapshot(&self) -> Vec<Activity> {
        self.registry.snapshot(Instant::now())
    }

    /// Hand an entry's action to its owner. A failure is cleared once its
    /// click action ran, matching Zed's "click to show error" behavior.
    pub fn activate(&mut self, id: u64, cancel: bool, cx: &mut Context<Self>) {
        let Some(activity) = self.snapshot().into_iter().find(|entry| entry.id == id) else {
            return;
        };
        let action = if cancel {
            activity.draft.cancel_action.clone()
        } else {
            activity.draft.click_action.clone()
        };
        let Some(action_id) = action else {
            return;
        };
        cx.emit(ActivityActionEvent {
            activity_id: id,
            source: activity.draft.source.clone(),
            action_id,
        });
        if !cancel && activity.draft.kind == ActivityKind::Failure {
            self.finish(id, cx);
        }
    }

    pub fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.finish(id, cx);
    }
}

pub struct GlobalActivityCenter(pub Entity<ActivityCenter>);

impl Global for GlobalActivityCenter {}

/// Install the app-wide center once; later calls (one per window) return the
/// existing entity so every window's status item shares one registry.
pub fn init(cx: &mut App) -> Entity<ActivityCenter> {
    if let Some(existing) = try_activity_center(cx) {
        return existing;
    }
    let center = cx.new(|_| ActivityCenter::new());
    cx.set_global(GlobalActivityCenter(center.clone()));
    center
}

pub fn activity_center(cx: &App) -> Entity<ActivityCenter> {
    cx.global::<GlobalActivityCenter>().0.clone()
}

/// `None` when no composition root installed the center (tests, examples), so
/// producers degrade to "no indicator" instead of panicking.
pub fn try_activity_center(cx: &App) -> Option<Entity<ActivityCenter>> {
    cx.try_global::<GlobalActivityCenter>()
        .map(|global| global.0.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::sync::{Arc, Mutex};

    #[gpui::test]
    fn actions_reach_the_owner_and_failures_clear_after_click(cx: &mut TestAppContext) {
        let center = cx.update(init);
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = received.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&center, move |_, event: &ActivityActionEvent, _| {
                sink.lock().unwrap().push(event.action_id.clone());
            })
        });
        let (running, failed) = cx.update(|cx| {
            center.update(cx, |center, cx| {
                let running = center.begin(
                    ActivityDraft::new(ActivityKind::Git, "git", "Pushing").cancellable("cancel"),
                    cx,
                );
                let failed = center.begin(
                    ActivityDraft::new(ActivityKind::Failure, "ssh", "Failed").on_click("show"),
                    cx,
                );
                (running, failed)
            })
        });
        cx.update(|cx| {
            center.update(cx, |center, cx| {
                center.activate(running, true, cx);
                center.activate(failed, false, cx);
            })
        });
        assert_eq!(*received.lock().unwrap(), vec!["cancel", "show"]);
        let left = cx.update(|cx| center.read(cx).snapshot());
        assert_eq!(left.len(), 1, "cancel keeps the run, a clicked failure clears");
        assert_eq!(left[0].id, running);
    }

    #[gpui::test]
    fn init_is_shared_across_windows(cx: &mut TestAppContext) {
        let (first, second) = cx.update(|cx| (init(cx), init(cx)));
        assert_eq!(first.entity_id(), second.entity_id());
    }
}
