// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Host-view contract between `labonair-workspace` and the Hosts UI (R08-012).
//!
//! The workspace needs to read the host catalog (ids, names, jump-host labels,
//! picker rows) and push two live snapshots back (connection status, active
//! forward states). Before this crate existed it held an `Entity<HostManagerView>`
//! and called that view's methods directly, coupling the workspace to the
//! Hosts *UI* crate. [`HostView`] replaces that entity with a narrow set of
//! injected callbacks, mirroring `labonair-explorer-host` /
//! `labonair-snippets-host`. The composition root builds the host from the
//! active `HostManagerView`; the workspace never sees a Hosts-UI type.

use std::rc::Rc;

use std::collections::HashMap;

use gpui::App;
use labonair_forwards::SavedForward;
use labonair_hosts::HostPickerRow;
use labonair_ssh::{ForwardId, ForwardState};

/// Live state of one running forward, as the Hosts forwards tab shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardLive {
    pub state: ForwardState,
    /// Actual listen port (differs from the saved one for port 0).
    pub bound_port: Option<u16>,
}

/// Live connection status for a saved host, tracked off the SSH event stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Failed,
}

impl HostStatus {
    /// Short label for the host-list item's status pill.
    pub fn label(self) -> &'static str {
        match self {
            HostStatus::Disconnected => "offline",
            HostStatus::Connecting => "connecting\u{2026}",
            HostStatus::Connected => "connected",
            HostStatus::Failed => "failed",
        }
    }
}

type HostIdsFn = Rc<dyn Fn(&App) -> Vec<String>>;
type RouteLabelsFn = Rc<dyn Fn(&str, &App) -> Vec<String>>;
type HostNameFn = Rc<dyn Fn(&str, &App) -> Option<String>>;
type PickerRowsFn = Rc<dyn Fn(&App) -> Vec<HostPickerRow>>;
type RecentPickerRowsFn = Rc<dyn Fn(usize, &App) -> Vec<HostPickerRow>>;
type SetStatusFn = Rc<dyn Fn(&str, HostStatus, &mut App)>;
type SetForwardStatesFn = Rc<dyn Fn(HashMap<ForwardId, ForwardLive>, &mut App)>;
type SavedForwardsFn = Rc<dyn Fn(&str, &App) -> Vec<SavedForward>>;
type SaveForwardFn = Rc<dyn Fn(&str, SavedForward, &mut App)>;

/// Narrow composition contract between the workspace and the Hosts UI.
#[derive(Clone)]
pub struct HostView {
    host_ids: HostIdsFn,
    host_name: HostNameFn,
    route_labels: RouteLabelsFn,
    picker_rows: PickerRowsFn,
    recent_picker_rows: RecentPickerRowsFn,
    set_status: SetStatusFn,
    set_forward_states: SetForwardStatesFn,
    saved_forwards: SavedForwardsFn,
    save_forward: SaveForwardFn,
}

impl HostView {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host_ids: impl Fn(&App) -> Vec<String> + 'static,
        host_name: impl Fn(&str, &App) -> Option<String> + 'static,
        route_labels: impl Fn(&str, &App) -> Vec<String> + 'static,
        picker_rows: impl Fn(&App) -> Vec<HostPickerRow> + 'static,
        recent_picker_rows: impl Fn(usize, &App) -> Vec<HostPickerRow> + 'static,
        set_status: impl Fn(&str, HostStatus, &mut App) + 'static,
        set_forward_states: impl Fn(HashMap<ForwardId, ForwardLive>, &mut App) + 'static,
        saved_forwards: impl Fn(&str, &App) -> Vec<SavedForward> + 'static,
        save_forward: impl Fn(&str, SavedForward, &mut App) + 'static,
    ) -> Self {
        Self {
            host_ids: Rc::new(host_ids),
            host_name: Rc::new(host_name),
            route_labels: Rc::new(route_labels),
            picker_rows: Rc::new(picker_rows),
            recent_picker_rows: Rc::new(recent_picker_rows),
            set_status: Rc::new(set_status),
            set_forward_states: Rc::new(set_forward_states),
            saved_forwards: Rc::new(saved_forwards),
            save_forward: Rc::new(save_forward),
        }
    }

    /// A host that knows nothing. For headless views and tests.
    pub fn disconnected() -> Self {
        Self::new(
            |_| Vec::new(),
            |_, _| None,
            |_, _| Vec::new(),
            |_| Vec::new(),
            |_, _| Vec::new(),
            |_, _, _| {},
            |_, _| {},
            |_, _| Vec::new(),
            |_, _, _| {},
        )
    }

    /// All known host ids (session restore).
    pub fn host_ids(&self, cx: &App) -> Vec<String> {
        (self.host_ids)(cx)
    }

    /// Display name for a host id, if known.
    pub fn host_name(&self, host_id: &str, cx: &App) -> Option<String> {
        (self.host_name)(host_id, cx)
    }

    /// Jump-host names for a host id, outermost first (empty = direct).
    pub fn route_labels(&self, host_id: &str, cx: &App) -> Vec<String> {
        (self.route_labels)(host_id, cx)
    }

    /// Canonical host rows for command-palette / new-tab pickers.
    pub fn picker_rows(&self, cx: &App) -> Vec<HostPickerRow> {
        (self.picker_rows)(cx)
    }

    /// Canonical recent-host rows.
    pub fn recent_picker_rows(&self, n: usize, cx: &App) -> Vec<HostPickerRow> {
        (self.recent_picker_rows)(n, cx)
    }

    /// Push a host's live connection status into the host manager panel.
    pub fn set_status(&self, host_id: &str, status: HostStatus, cx: &mut App) {
        (self.set_status)(host_id, status, cx);
    }

    /// Upsert a saved forward into a host's `tunnels` column (Hosts owns the write).
    pub fn save_forward(&self, host_id: &str, forward: SavedForward, cx: &mut App) {
        (self.save_forward)(host_id, forward, cx);
    }

    /// Replace the live forward-state snapshot (running forwards by id).
    pub fn set_forward_states(&self, states: HashMap<ForwardId, ForwardLive>, cx: &mut App) {
        (self.set_forward_states)(states, cx);
    }

    /// Typed saved forwards of a host (from the hosts catalog).
    pub fn saved_forwards(&self, host_id: &str, cx: &App) -> Vec<SavedForward> {
        (self.saved_forwards)(host_id, cx)
    }
}
