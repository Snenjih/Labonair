// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! UI-independent registry of in-flight background activity.
//!
//! Feature modules announce work that is running (a git fetch, an SSH
//! connect, a language-server start, a download) or a failure the user should
//! look at. The statusbar activity indicator renders the single most
//! important entry and lists the rest. This crate owns identity, priority
//! ordering, progress, and the sticky-failure lifecycle; GPUI, popovers, and
//! feature behavior belong to adapters and consumers.

use std::time::{Duration, Instant};

/// How an activity is presented. Variants are ordered by display priority,
/// with the highest-priority activity first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActivityKind {
    /// Informational and self-clearing, e.g. a partial explorer index.
    Notice,
    /// A package-like operation: theme install, update check/download.
    Package,
    /// A language-server health warning that is not a failure.
    Health,
    /// Something failed; stays until dismissed or acted on.
    Failure,
    /// A filesystem or remote-listing job.
    FileJob,
    /// A git command.
    Git,
    /// A language server starting up or reporting progress.
    LanguageServer,
    /// An SSH connect / reconnect.
    Connection,
}

impl ActivityKind {
    /// Whether the entry is rendered with a spinner (running work) rather
    /// than a static state icon.
    pub fn is_running(self) -> bool {
        !matches!(self, Self::Notice | Self::Health | Self::Failure)
    }
}

/// Data submitted by a feature module.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityDraft {
    pub kind: ActivityKind,
    /// Stable identity for work several views may announce at once (one
    /// language server shared by many editors). Beginning a draft whose key
    /// is already registered updates that entry instead of adding a second.
    pub key: Option<String>,
    /// Owner label, e.g. `"git"`, `"ssh"`, `"editor"`.
    pub source: String,
    pub message: String,
    pub tooltip: Option<String>,
    /// `0.0..=1.0`, `None` for indeterminate work.
    pub progress: Option<f32>,
    /// Owner-interpreted id of the cancel action, if the work can be stopped.
    pub cancel_action: Option<String>,
    /// Owner-interpreted id activated when the user clicks the entry.
    pub click_action: Option<String>,
    /// Runs shorter than this never reach the indicator, so a 20ms `git
    /// status` does not flash the bar. Zero shows immediately.
    pub show_after: Duration,
}

impl ActivityDraft {
    pub fn new(kind: ActivityKind, source: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            key: None,
            source: source.into(),
            message: message.into(),
            tooltip: None,
            progress: None,
            cancel_action: None,
            click_action: None,
            show_after: Duration::ZERO,
        }
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn progress(mut self, progress: f32) -> Self {
        self.progress = Some(progress.clamp(0.0, 1.0));
        self
    }

    pub fn cancellable(mut self, action: impl Into<String>) -> Self {
        self.cancel_action = Some(action.into());
        self
    }

    pub fn on_click(mut self, action: impl Into<String>) -> Self {
        self.click_action = Some(action.into());
        self
    }

    pub fn show_after(mut self, delay: Duration) -> Self {
        self.show_after = delay;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    pub id: u64,
    pub draft: ActivityDraft,
    pub started: Instant,
    pub updated: Instant,
}

impl Activity {
    fn visible(&self, now: Instant) -> bool {
        now.duration_since(self.started) >= self.draft.show_after
    }
}

#[derive(Debug, Default)]
pub struct ActivityRegistry {
    next_id: u64,
    entries: Vec<Activity>,
}

impl ActivityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin(&mut self, draft: ActivityDraft, now: Instant) -> u64 {
        if let Some(key) = &draft.key {
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.draft.key.as_ref() == Some(key))
            {
                entry.draft = draft;
                entry.updated = now;
                return entry.id;
            }
        }
        self.next_id += 1;
        let id = self.next_id;
        self.entries.push(Activity {
            id,
            draft,
            started: now,
            updated: now,
        });
        id
    }

    /// Replace the message and progress of a running activity. Returns
    /// `false` if it already finished.
    pub fn update(
        &mut self,
        id: u64,
        message: Option<String>,
        progress: Option<f32>,
        now: Instant,
    ) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        if let Some(message) = message {
            entry.draft.message = message;
        }
        if let Some(progress) = progress {
            entry.draft.progress = Some(progress.clamp(0.0, 1.0));
        }
        entry.updated = now;
        true
    }

    /// Remove an activity (success, cancel, or dismissed failure).
    pub fn finish(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        before != self.entries.len()
    }

    /// Remove the entry registered under `key`, if any.
    pub fn finish_key(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|entry| entry.draft.key.as_deref() != Some(key));
        before != self.entries.len()
    }

    /// Turn a running activity into a sticky failure the user can click.
    pub fn fail(
        &mut self,
        id: u64,
        message: impl Into<String>,
        tooltip: Option<String>,
        click_action: Option<String>,
        now: Instant,
    ) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        entry.draft.kind = ActivityKind::Failure;
        entry.draft.message = message.into();
        entry.draft.tooltip = tooltip;
        entry.draft.click_action = click_action;
        entry.draft.cancel_action = None;
        entry.draft.progress = None;
        entry.draft.show_after = Duration::ZERO;
        entry.updated = now;
        true
    }

    /// Visible entries, most important first: by kind, then most recently
    /// updated. The first element is what the statusbar shows inline.
    pub fn snapshot(&self, now: Instant) -> Vec<Activity> {
        let mut visible: Vec<Activity> = self
            .entries
            .iter()
            .filter(|entry| entry.visible(now))
            .cloned()
            .collect();
        visible.sort_by(|a, b| {
            b.draft
                .kind
                .cmp(&a.draft.kind)
                .then(b.updated.cmp(&a.updated))
        });
        visible
    }

    /// Earliest instant at which a currently hidden entry becomes visible,
    /// so the adapter can schedule exactly one re-render.
    pub fn next_reveal(&self, now: Instant) -> Option<Instant> {
        self.entries
            .iter()
            .filter(|entry| !entry.visible(now))
            .map(|entry| entry.started + entry.draft.show_after)
            .min()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(kind: ActivityKind, message: &str) -> ActivityDraft {
        ActivityDraft::new(kind, "test", message)
    }

    #[test]
    fn highest_kind_wins_then_most_recent() {
        let start = Instant::now();
        let mut registry = ActivityRegistry::new();
        registry.begin(draft(ActivityKind::Git, "fetch"), start);
        let connect = registry.begin(draft(ActivityKind::Connection, "ssh"), start);
        let later = registry.begin(
            draft(ActivityKind::Git, "pull"),
            start + Duration::from_secs(1),
        );
        let snapshot = registry.snapshot(start + Duration::from_secs(2));
        assert_eq!(snapshot[0].id, connect);
        assert_eq!(snapshot[1].id, later);
        assert_eq!(snapshot.len(), 3);
    }

    #[test]
    fn short_runs_stay_hidden_until_delay() {
        let start = Instant::now();
        let mut registry = ActivityRegistry::new();
        let id = registry.begin(
            draft(ActivityKind::Git, "status").show_after(Duration::from_millis(300)),
            start,
        );
        assert!(registry
            .snapshot(start + Duration::from_millis(100))
            .is_empty());
        assert_eq!(
            registry.next_reveal(start + Duration::from_millis(100)),
            Some(start + Duration::from_millis(300))
        );
        assert_eq!(
            registry.snapshot(start + Duration::from_millis(300)).len(),
            1
        );
        registry.finish(id);
        assert!(registry.is_empty());
    }

    #[test]
    fn fail_keeps_entry_as_sticky_failure() {
        let start = Instant::now();
        let mut registry = ActivityRegistry::new();
        let id = registry.begin(
            draft(ActivityKind::Connection, "connecting").cancellable("cancel"),
            start,
        );
        assert!(registry.fail(id, "refused", None, Some("show".into()), start));
        let entry = &registry.snapshot(start)[0];
        assert_eq!(entry.draft.kind, ActivityKind::Failure);
        assert!(entry.draft.cancel_action.is_none());
        assert!(!entry.draft.kind.is_running());
        assert!(!registry.update(99, None, None, start));
    }

    #[test]
    fn keyed_begin_updates_instead_of_duplicating() {
        let start = Instant::now();
        let mut registry = ActivityRegistry::new();
        let first = registry.begin(draft(ActivityKind::LanguageServer, "a").key("lsp:x"), start);
        let second = registry.begin(draft(ActivityKind::Failure, "b").key("lsp:x"), start);
        assert_eq!(first, second);
        let snapshot = registry.snapshot(start);
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].draft.kind, ActivityKind::Failure);
        assert!(registry.finish_key("lsp:x"));
        assert!(!registry.finish_key("lsp:x"));
    }

    #[test]
    fn progress_is_clamped() {
        let start = Instant::now();
        let mut registry = ActivityRegistry::new();
        let id = registry.begin(draft(ActivityKind::Package, "dl"), start);
        registry.update(id, None, Some(4.0), start);
        assert_eq!(registry.snapshot(start)[0].draft.progress, Some(1.0));
    }
}
