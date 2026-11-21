// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Multi-process Workspace bootstrap and revision synchronization.
//!
//! SQLite and legacy-file I/O run on GPUI's background executor. The owner
//! converts the existing session format here, then passes only stable typed
//! Workspace data across the UI boundary.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use labonair_settings::Settings as _;
use labonair_workspace_storage::{
    ImportedSpace, SpaceSummary, StoredSpace, StoredTab, WorkspaceChange, WorkspaceInstanceId,
    WorkspaceLease, WorkspaceStorage,
};

use crate::shared_session::SharedTabPayload;

const LEASE_TTL: Duration = Duration::from_secs(15);
const SYNC_INTERVAL: Duration = Duration::from_millis(200);
const LEASE_RENEW_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) struct BootstrapState {
    pub storage: Arc<WorkspaceStorage>,
    pub lease: WorkspaceLease,
    pub space: StoredSpace,
    pub spaces: Vec<SpaceSummary>,
    pub restore_enabled: bool,
}

pub(crate) struct JoinState {
    pub target_local_id: crate::spaces::SpaceId,
    pub lease: WorkspaceLease,
    pub space: StoredSpace,
    pub summaries: Vec<SpaceSummary>,
    pub conflict: bool,
}

struct SyncPacket {
    storage: Arc<WorkspaceStorage>,
    lease: WorkspaceLease,
    base: StoredSpace,
    local: StoredSpace,
    renew_lease: bool,
}

struct SyncResult {
    lease: WorkspaceLease,
    space: StoredSpace,
    summaries: Vec<SpaceSummary>,
    conflict: bool,
    resync: bool,
}

pub(crate) fn bootstrap(
    data_dir: std::path::PathBuf,
    instance_id: WorkspaceInstanceId,
    restore_enabled: bool,
) -> Result<BootstrapState, String> {
    let storage = Arc::new(
        WorkspaceStorage::open(data_dir.join("workspace").join("workspace.db"))
            .map_err(|error| error.to_string())?,
    );
    let now = now_seconds();

    if restore_enabled {
        let legacy = crate::session::load_snapshot();
        let mut imported = Vec::new();
        let active_index = legacy.as_ref().map_or(0, |snapshot| {
            snapshot
                .active_space_index
                .min(snapshot.spaces.len().saturating_sub(1))
        });
        if let Some(snapshot) = legacy {
            for entry in snapshot.spaces {
                let tabs = entry
                    .session
                    .tabs
                    .iter()
                    .map(|group| {
                        let payload = SharedTabPayload::from_legacy(group);
                        Ok(StoredTab {
                            id: labonair_workspace_storage::TabId::new(),
                            scrollback_keys: payload.scrollback_keys(),
                            payload: payload.into_value().map_err(|error| error.to_string())?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                imported.push(ImportedSpace {
                    id: labonair_workspace_storage::SpaceId::new(),
                    name: entry.name,
                    identity: serde_json::to_value(entry.session.identity)
                        .map_err(|error| error.to_string())?,
                    tabs,
                });
            }
        }
        storage
            .import_legacy_once(&imported, active_index)
            .map_err(|error| error.to_string())?;
    }

    let (lease, space) = storage
        .claim_startup(instance_id, now, LEASE_TTL, restore_enabled)
        .map_err(|error| error.to_string())?;
    let spaces = storage
        .list_spaces(now)
        .map_err(|error| error.to_string())?;
    Ok(BootstrapState {
        storage,
        lease,
        space,
        spaces,
        restore_enabled,
    })
}

pub(crate) fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

impl crate::Workspace {
    pub(crate) fn apply_shared_bootstrap(
        &mut self,
        state: BootstrapState,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let local_active = self.spaces.update(cx, |spaces, cx| {
            spaces.hydrate_shared(&state.spaces, state.lease.space_id, cx)
        });
        let Some(local_active) = local_active else {
            self.pending_shared_error =
                Some("the claimed Space was missing from the catalog".into());
            return;
        };

        self.shared_storage = Some(state.storage);
        self.shared_lease = Some(state.lease.clone());
        self.shared_revision = state.space.revision;
        self.shared_base = Some(state.space.clone());
        self.shared_restore_enabled = state.restore_enabled;
        self.shared_ready = true;
        self.tabs
            .update(cx, |tabs, _| tabs.set_current_space(local_active));

        let restored = self.restore_shared_space_tabs(&state.space, window, cx);
        if let Some(id) = restored.first().copied() {
            self.select_tab(id, window, cx);
        } else if labonair_settings::GeneralSettings::try_get(cx)
            .map(|settings| {
                settings.default_startup_tab()
                    == labonair_settings::content::general::StartupTab::Terminal
            })
            .unwrap_or(true)
        {
            self.open_terminal_tab(window, cx);
        }

        self.apply_space_switch(local_active, cx);
        self.cleanup_scrollback(cx);
        self.announce_background_sessions(cx);
        self.start_shared_sync(cx);
        cx.notify();
    }

    pub(crate) fn notify_shared_startup_error(
        &mut self,
        message: String,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let was_ready = self.shared_ready;
        self.shared_ready = false;
        labonair_notifications::notification_center(cx).update(cx, |center, cx| {
            center.push(
                labonair_notifications::Notification::error(
                    "Shared Workspace unavailable",
                    "The local Workspace could not connect to its shared state store.",
                )
                .source("workspace")
                .details(message),
                cx,
            );
        });
        if !was_ready
            && labonair_settings::GeneralSettings::try_get(cx)
                .map(|settings| {
                    settings.default_startup_tab()
                        == labonair_settings::content::general::StartupTab::Terminal
                })
                .unwrap_or(true)
        {
            self.open_terminal_tab(window, cx);
        }
    }

    fn restore_shared_space_tabs(
        &mut self,
        space: &StoredSpace,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<u64> {
        let mut restored = Vec::with_capacity(space.tabs.len());
        let mut unopened = Vec::new();
        for tab in &space.tabs {
            if let Some(id) = self.restore_shared_tab(tab, window, cx) {
                restored.push(id);
            } else {
                unopened.push(tab.clone());
            }
        }
        self.remember_unopened_shared_tabs(space.id, unopened, cx);
        restored
    }

    fn remember_unopened_shared_tabs(
        &mut self,
        space_id: labonair_workspace_storage::SpaceId,
        tabs: Vec<labonair_workspace_storage::StoredTab>,
        cx: &mut gpui::Context<Self>,
    ) {
        let previous = self
            .shared_unopened_tabs
            .get(&space_id)
            .map(|rows| {
                rows.iter()
                    .map(|tab| tab.id)
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();
        let new_ids = tabs
            .iter()
            .map(|tab| tab.id)
            .filter(|id| !previous.contains(id))
            .collect::<Vec<_>>();
        if tabs.is_empty() {
            self.shared_unopened_tabs.remove(&space_id);
        } else {
            self.shared_unopened_tabs.insert(space_id, tabs);
        }
        if !new_ids.is_empty() {
            labonair_notifications::notification_center(cx).update(cx, |center, cx| {
                center.push(
                    labonair_notifications::Notification::warning(
                        "Some shared tabs could not be opened",
                        "Their saved state is preserved in the shared Space. Check that the required project files and SSH hosts are available, then rejoin the Space.",
                    )
                    .source("workspace"),
                    cx,
                );
            });
        }
    }

    pub(crate) fn apply_pending_shared_state(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if let Some(state) = self.pending_shared_bootstrap.take() {
            self.apply_shared_bootstrap(state, window, cx);
        }
        if let Some(error) = self.pending_shared_error.take() {
            self.notify_shared_startup_error(error, window, cx);
        }
        if let Some((space, summaries, conflict, resync)) = self.pending_shared_space.take() {
            self.apply_shared_space(space, summaries, conflict, resync, window, cx);
        }
        if let Some(join) = self.pending_shared_join.take() {
            self.apply_shared_join(join, window, cx);
        }
    }

    fn capture_shared_sync_packet(
        &mut self,
        renew_lease: bool,
        cx: &gpui::App,
    ) -> Option<SyncPacket> {
        let storage = self.shared_storage.clone()?;
        let lease = self.shared_lease.clone()?;
        let base = self.shared_base.clone()?;
        let active_local_id = self.spaces.read(cx).active_id();
        let active = self.spaces.read(cx).get(active_local_id)?.clone();
        if !active.loaded || active.shared_id != lease.space_id {
            return None;
        }

        let tabs = self
            .tabs
            .read(cx)
            .tabs_in_space(active_local_id)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let mut stored_tabs = Vec::with_capacity(tabs.len());
        for tab in &tabs {
            if let Some(stored) = self.capture_shared_tab(tab, cx) {
                stored_tabs.push(stored);
            }
        }
        if let Some(unopened) = self.shared_unopened_tabs.get(&active.shared_id) {
            merge_unopened_tabs(&mut stored_tabs, unopened, &base);
        }
        let identity = serde_json::to_value(active.identity).ok()?;
        Some(SyncPacket {
            storage,
            lease,
            base: base.clone(),
            local: StoredSpace {
                id: active.shared_id,
                name: active.name,
                identity,
                revision: base.revision,
                last_used_sequence: base.last_used_sequence,
                tabs: stored_tabs,
                participant_count: base.participant_count,
            },
            renew_lease,
        })
    }

    fn capture_shared_tab(&mut self, tab: &crate::tabs::Tab, cx: &gpui::App) -> Option<StoredTab> {
        let captured = std::cell::RefCell::new(Vec::new());
        let group = self.snapshot_tab(tab, cx, &captured, false)?;
        let local_tab_id = tab.id;
        let payload = SharedTabPayload::capture(&group, |pane| {
            *self
                .shared_pane_ids
                .entry((local_tab_id, pane))
                .or_default()
        });
        Some(StoredTab {
            id: tab.shared_id,
            scrollback_keys: payload.scrollback_keys(),
            payload: payload.into_value().ok()?,
        })
    }

    fn start_shared_sync(&mut self, cx: &mut gpui::Context<Self>) {
        let mut last_lease_renew = Instant::now();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(SYNC_INTERVAL).await;
            let renew_lease = last_lease_renew.elapsed() >= LEASE_RENEW_INTERVAL;
            let packet = match this.update(cx, |this, cx| {
                this.capture_shared_sync_packet(renew_lease, cx)
            }) {
                Ok(Some(packet)) => packet,
                Ok(None) => continue,
                Err(_) => break,
            };
            let renew_requested = packet.renew_lease;
            let request_space = packet.lease.space_id;
            let request_generation = packet.lease.generation;
            let result = cx
                .background_executor()
                .spawn(async move { synchronize(packet) })
                .await;
            match result {
                Ok(result) => {
                    if renew_requested {
                        last_lease_renew = Instant::now();
                    }
                    if this
                        .update(cx, |this, cx| {
                            if this.shared_lease.as_ref().is_none_or(|lease| {
                                lease.space_id != request_space
                                    || lease.generation != request_generation
                            }) {
                                return;
                            }
                            this.shared_lease = Some(result.lease.clone());
                            let current_participants =
                                this.shared_base.as_ref().map(|base| base.participant_count);
                            let catalog_changed = this
                                .spaces
                                .read(cx)
                                .shared_catalog_changed(&result.summaries);
                            let changed = result.space.revision != this.shared_revision
                                || current_participants != Some(result.space.participant_count)
                                || catalog_changed;
                            if changed || result.conflict || result.resync {
                                this.pending_shared_space = Some((
                                    result.space,
                                    result.summaries,
                                    result.conflict,
                                    result.resync,
                                ));
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(labonair_workspace_storage::StoreError::StaleRevision { .. }) => {
                    // Another writer committed between the read and the CAS.
                    // Keep the local base unchanged and retry next poll.
                }
                Err(error) => {
                    tracing::warn!(%error, "shared Workspace synchronization failed");
                }
            }
        })
        .detach();
    }

    pub(crate) fn request_shared_space_join(
        &mut self,
        target_local_id: crate::spaces::SpaceId,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.pending_shared_join.is_some() || self.shared_transition_pending {
            return;
        }
        let Some(target) = self
            .spaces
            .read(cx)
            .get(target_local_id)
            .map(|space| space.shared_id)
        else {
            return;
        };
        let Some(storage) = self.shared_storage.clone() else {
            return;
        };
        let Some(current_lease) = self.shared_lease.clone() else {
            return;
        };
        let packet = self.capture_shared_sync_packet(true, cx);
        let instance_id = self.shared_instance_id;
        self.shared_transition_pending = true;
        let work = cx.background_executor().spawn(async move {
            let (mut lease, conflict) = if let Some(packet) = packet {
                let result = synchronize(packet)?;
                (result.lease, result.conflict)
            } else {
                (current_lease, false)
            };
            let now = now_seconds();
            if lease.space_id != target {
                lease = match storage.switch_space(&lease, target, now, LEASE_TTL) {
                    Ok((lease, _)) => lease,
                    Err(labonair_workspace_storage::StoreError::LeaseLost) => {
                        storage.join_space(instance_id, target, now, LEASE_TTL)?.0
                    }
                    Err(error) => return Err(error),
                };
            }
            let space = storage.load_space(target, now)?.ok_or(
                labonair_workspace_storage::StoreError::SpaceNotFound(target),
            )?;
            let summaries = storage.list_spaces(now)?;
            Ok(JoinState {
                target_local_id,
                lease,
                space,
                summaries,
                conflict,
            })
        });
        cx.spawn(async move |this, cx| match work.await {
            Ok(join) => {
                let _ = this.update(cx, |this, cx| {
                    this.shared_transition_pending = false;
                    this.pending_shared_join = Some(join);
                    cx.notify();
                });
            }
            Err(error) => {
                let _ = this.update(cx, |this, cx| {
                    this.shared_transition_pending = false;
                    this.pending_shared_error = Some(format!(
                        "Could not join the selected Workspace Space: {error}"
                    ));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn request_shared_space_create(
        &mut self,
        target_local_id: crate::spaces::SpaceId,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.pending_shared_join.is_some() || self.shared_transition_pending {
            return;
        }
        let Some(target_space) = self.spaces.read(cx).get(target_local_id).cloned() else {
            return;
        };
        let Some(storage) = self.shared_storage.clone() else {
            return;
        };
        let Some(current_lease) = self.shared_lease.clone() else {
            return;
        };
        let packet = self.capture_shared_sync_packet(true, cx);
        let instance_id = self.shared_instance_id;
        let name = target_space.name.clone();
        let identity = serde_json::to_value(target_space.identity).unwrap_or_default();
        let shared_id = target_space.shared_id;
        self.shared_transition_pending = true;
        let work = cx.background_executor().spawn(async move {
            let (mut lease, conflict) = if let Some(packet) = packet {
                let result = synchronize(packet)?;
                (result.lease, result.conflict)
            } else {
                (current_lease, false)
            };
            let now = now_seconds();
            if lease.expires_at <= now {
                lease = storage
                    .join_space(instance_id, lease.space_id, now, LEASE_TTL)?
                    .0;
            }
            storage.create_space(&lease, shared_id, &name, &identity, now)?;
            lease = match storage.switch_space(&lease, shared_id, now, LEASE_TTL) {
                Ok((lease, _)) => lease,
                Err(labonair_workspace_storage::StoreError::LeaseLost) => {
                    storage
                        .join_space(instance_id, shared_id, now, LEASE_TTL)?
                        .0
                }
                Err(error) => return Err(error),
            };
            let space = storage.load_space(shared_id, now)?.ok_or(
                labonair_workspace_storage::StoreError::SpaceNotFound(shared_id),
            )?;
            let summaries = storage.list_spaces(now)?;
            Ok(JoinState {
                target_local_id,
                lease,
                space,
                summaries,
                conflict,
            })
        });
        cx.spawn(async move |this, cx| match work.await {
            Ok(join) => {
                let _ = this.update(cx, |this, cx| {
                    this.shared_transition_pending = false;
                    this.pending_shared_join = Some(join);
                    cx.notify();
                });
            }
            Err(error) => {
                let _ = this.update(cx, |this, cx| {
                    this.shared_transition_pending = false;
                    this.pending_shared_error = Some(format!(
                        "Could not create the selected Workspace Space: {error}"
                    ));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn release_shared_lease(&mut self, cx: &mut gpui::Context<Self>) {
        if self.shared_release_started {
            return;
        }
        self.shared_release_started = true;
        let (Some(storage), Some(lease)) = (self.shared_storage.clone(), self.shared_lease.clone())
        else {
            return;
        };
        let restore_enabled = self.shared_restore_enabled;
        let packet = self.capture_shared_sync_packet(true, cx);
        cx.background_executor()
            .spawn(async move {
                let lease = if let Some(packet) = packet {
                    match synchronize(packet) {
                        Ok(result) => result.lease,
                        Err(error) => {
                            tracing::warn!(%error, "final shared Workspace save failed before release");
                            lease
                        }
                    }
                } else {
                    lease
                };
                if let Err(error) = storage.release(&lease, now_seconds(), restore_enabled) {
                    tracing::warn!(%error, "failed to release shared Workspace lease");
                }
            })
            .detach();
    }

    fn apply_shared_space(
        &mut self,
        space: StoredSpace,
        summaries: Vec<SpaceSummary>,
        conflict: bool,
        resync: bool,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let revision_changed = space.revision != self.shared_revision;
        for summary in &summaries {
            self.spaces
                .update(cx, |spaces, cx| spaces.upsert_shared_summary(summary, cx));
        }
        let Some(local_space_id) = self
            .spaces
            .read(cx)
            .find_shared(space.id)
            .map(|space| space.id)
        else {
            return;
        };
        let active_stable_tab = self
            .tabs
            .read(cx)
            .active()
            .filter(|tab| tab.space_id == local_space_id)
            .map(|tab| tab.shared_id);
        let remote_by_id: std::collections::HashMap<_, _> =
            space.tabs.iter().map(|tab| (tab.id, tab)).collect();
        let removed_local = self
            .tabs
            .read(cx)
            .tabs_in_space(local_space_id)
            .into_iter()
            .filter(|tab| !remote_by_id.contains_key(&tab.shared_id))
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        for tab_id in removed_local {
            if let Some(tab) = self.tabs.update(cx, |tabs, cx| tabs.close(tab_id, cx)) {
                self.retire_tab(&tab, cx);
            }
        }

        let mut unopened = Vec::new();
        for stored in &space.tabs {
            let local = self
                .tabs
                .read(cx)
                .tabs_in_space(local_space_id)
                .into_iter()
                .find(|tab| tab.shared_id == stored.id)
                .cloned();
            if local
                .as_ref()
                .and_then(|tab| self.capture_shared_tab(tab, cx))
                .is_some_and(|current| current == *stored)
            {
                continue;
            }
            if let Some(local_id) = local.map(|tab| tab.id) {
                if let Some(tab) = self.tabs.update(cx, |tabs, cx| tabs.close(local_id, cx)) {
                    self.retire_tab(&tab, cx);
                }
            }
            self.tabs
                .update(cx, |tabs, _| tabs.set_current_space(local_space_id));
            if self.restore_shared_tab(stored, window, cx).is_none() {
                unopened.push(stored.clone());
            }
        }
        self.remember_unopened_shared_tabs(space.id, unopened, cx);
        self.tabs.update(cx, |tabs, cx| {
            tabs.reorder_shared_space(
                local_space_id,
                &space.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
                cx,
            )
        });

        self.spaces.update(cx, |spaces, cx| {
            spaces.set_shared_loaded(space.id, true, space.participant_count, cx);
            spaces.update_shared_summary(
                &SpaceSummary {
                    id: space.id,
                    name: space.name.clone(),
                    identity: space.identity.clone(),
                    last_used_sequence: space.last_used_sequence,
                    participant_count: space.participant_count,
                    tab_count: space.tabs.len() as u32,
                },
                cx,
            );
        });

        let restored_active = active_stable_tab.and_then(|shared_id| {
            self.tabs
                .read(cx)
                .tabs_in_space(local_space_id)
                .into_iter()
                .find(|tab| tab.shared_id == shared_id)
                .map(|tab| tab.id)
        });
        if let Some(tab_id) = restored_active {
            self.tabs.update(cx, |tabs, cx| tabs.set_active(tab_id, cx));
        }
        if self.spaces.read(cx).active_id() == local_space_id {
            self.apply_space_switch(local_space_id, cx);
        }

        self.shared_revision = space.revision;
        self.shared_base = Some(space.clone());
        if conflict {
            labonair_notifications::notification_center(cx).update(cx, |center, cx| {
                center.push(
                    labonair_notifications::Notification::warning(
                        "Workspace changed elsewhere",
                        "A concurrent edit touched the same tab or ordering. The latest shared state was loaded; repeat the local change if needed.",
                    )
                    .source("workspace"),
                    cx,
                );
            });
        } else if resync {
            tracing::info!(space_id = %space.id, "reloaded shared Workspace after change-log gap");
        }
        if revision_changed {
            self.cleanup_scrollback(cx);
        }
        cx.notify();
    }

    fn apply_shared_join(
        &mut self,
        join: JoinState,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let old_local_id = self.spaces.read(cx).active_id();
        for tab in self
            .tabs
            .update(cx, |tabs, cx| tabs.close_by_space(old_local_id, cx))
        {
            self.retire_tab(&tab, cx);
        }
        if let Some(old) = self.spaces.read(cx).get(old_local_id).cloned() {
            self.spaces.update(cx, |spaces, cx| {
                spaces.set_shared_loaded(
                    old.shared_id,
                    false,
                    old.participants.saturating_sub(1),
                    cx,
                )
            });
        }

        for summary in &join.summaries {
            self.spaces
                .update(cx, |spaces, cx| spaces.upsert_shared_summary(summary, cx));
        }
        self.shared_lease = Some(join.lease);
        self.shared_revision = join.space.revision;
        self.shared_base = Some(join.space.clone());
        self.tabs
            .update(cx, |tabs, _| tabs.set_current_space(join.target_local_id));
        self.spaces.update(cx, |spaces, cx| {
            spaces.set_shared_loaded(join.space.id, true, join.space.participant_count, cx);
            spaces.update_shared_summary(
                &SpaceSummary {
                    id: join.space.id,
                    name: join.space.name.clone(),
                    identity: join.space.identity.clone(),
                    last_used_sequence: join.space.last_used_sequence,
                    participant_count: join.space.participant_count,
                    tab_count: join.space.tabs.len() as u32,
                },
                cx,
            );
        });
        let restored = self.restore_shared_space_tabs(&join.space, window, cx);
        if let Some(id) = restored.first().copied() {
            self.select_tab(id, window, cx);
        }
        self.apply_space_switch(join.target_local_id, cx);
        self.announce_background_sessions(cx);
        if join.conflict {
            labonair_notifications::notification_center(cx).update(cx, |center, cx| {
                center.push(
                    labonair_notifications::Notification::warning(
                        "Workspace changed elsewhere",
                        "A concurrent edit touched the same tab or ordering. The latest shared state was loaded; repeat the local change if needed.",
                    )
                    .source("workspace"),
                    cx,
                );
            });
        }
        cx.notify();
    }
}

fn merge_unopened_tabs(
    visible: &mut Vec<labonair_workspace_storage::StoredTab>,
    unopened: &[labonair_workspace_storage::StoredTab],
    base: &StoredSpace,
) {
    for tab in unopened {
        if visible.iter().any(|present| present.id == tab.id) {
            continue;
        }
        let Some(base_position) = base
            .tabs
            .iter()
            .position(|candidate| candidate.id == tab.id)
        else {
            visible.push(tab.clone());
            continue;
        };
        let previous = base.tabs[..base_position]
            .iter()
            .rev()
            .find_map(|candidate| {
                visible
                    .iter()
                    .position(|present| present.id == candidate.id)
            });
        let next = base.tabs[base_position + 1..].iter().find_map(|candidate| {
            visible
                .iter()
                .position(|present| present.id == candidate.id)
        });
        let position = previous
            .map(|position| position + 1)
            .or(next)
            .unwrap_or(visible.len());
        visible.insert(position, tab.clone());
    }
}

fn synchronize(packet: SyncPacket) -> Result<SyncResult, labonair_workspace_storage::StoreError> {
    let now = now_seconds();
    let lease = if packet.renew_lease {
        match packet.storage.renew(&packet.lease, now, LEASE_TTL) {
            Ok(lease) => lease,
            Err(labonair_workspace_storage::StoreError::LeaseLost) => {
                packet
                    .storage
                    .join_space(
                        packet.lease.instance_id,
                        packet.lease.space_id,
                        now,
                        LEASE_TTL,
                    )?
                    .0
            }
            Err(error) => return Err(error),
        }
    } else {
        packet.lease
    };
    let remote = packet.storage.load_space(lease.space_id, now)?.ok_or(
        labonair_workspace_storage::StoreError::SpaceNotFound(lease.space_id),
    )?;
    let changes = packet
        .storage
        .changes_since(lease.space_id, packet.base.revision)?;
    let resync = changes.requires_snapshot;
    let conflict = has_conflict(&packet.base, &packet.local, &remote);
    let final_space = if conflict {
        remote
    } else {
        let desired = merge_local_changes(&packet.base, &packet.local, &remote);
        let mut revision = remote.revision;
        revision = persist_desired(&packet.storage, &lease, &remote, &desired, revision, now)?;
        let _ = revision;
        packet.storage.load_space(lease.space_id, now)?.ok_or(
            labonair_workspace_storage::StoreError::SpaceNotFound(lease.space_id),
        )?
    };
    let summaries = packet.storage.list_spaces(now)?;
    Ok(SyncResult {
        lease,
        space: final_space,
        summaries,
        conflict,
        resync,
    })
}

struct DesiredSpace {
    name: String,
    identity: serde_json::Value,
    tabs: Vec<StoredTab>,
}

fn has_conflict(base: &StoredSpace, local: &StoredSpace, remote: &StoredSpace) -> bool {
    if local.name != base.name && remote.name != base.name && local.name != remote.name {
        return true;
    }
    if local.identity != base.identity
        && remote.identity != base.identity
        && local.identity != remote.identity
    {
        return true;
    }

    let base_tabs = base
        .tabs
        .iter()
        .map(|tab| (tab.id, tab))
        .collect::<std::collections::HashMap<_, _>>();
    let local_tabs = local
        .tabs
        .iter()
        .map(|tab| (tab.id, tab))
        .collect::<std::collections::HashMap<_, _>>();
    let remote_tabs = remote
        .tabs
        .iter()
        .map(|tab| (tab.id, tab))
        .collect::<std::collections::HashMap<_, _>>();
    for id in base_tabs
        .keys()
        .chain(local_tabs.keys())
        .chain(remote_tabs.keys())
        .copied()
        .collect::<std::collections::HashSet<_>>()
    {
        let before = base_tabs.get(&id).copied();
        let ours = local_tabs.get(&id).copied();
        let theirs = remote_tabs.get(&id).copied();
        if ours != before && theirs != before && ours != theirs {
            return true;
        }
    }

    let base_order = base.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
    let local_order = local.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
    let remote_order = remote.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
    let base_ids = base_order
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let local_ids = local_order
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let remote_ids = remote_order
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let common = base_ids
        .intersection(&local_ids)
        .filter(|id| remote_ids.contains(id))
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let filter_common = |order: &[labonair_workspace_storage::TabId]| {
        order
            .iter()
            .copied()
            .filter(|id| common.contains(id))
            .collect::<Vec<_>>()
    };
    let base_common = filter_common(&base_order);
    let local_common = filter_common(&local_order);
    let remote_common = filter_common(&remote_order);
    let local_reordered = local_common != base_common;
    let remote_reordered = remote_common != base_common;
    local_reordered && remote_reordered && local_common != remote_common
}

fn merge_local_changes(
    base: &StoredSpace,
    local: &StoredSpace,
    remote: &StoredSpace,
) -> DesiredSpace {
    let base_tabs = base
        .tabs
        .iter()
        .map(|tab| (tab.id, tab.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    let local_tabs = local
        .tabs
        .iter()
        .map(|tab| (tab.id, tab.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    let mut merged = remote
        .tabs
        .iter()
        .map(|tab| (tab.id, tab.clone()))
        .collect::<std::collections::HashMap<_, _>>();

    for id in base_tabs
        .keys()
        .chain(local_tabs.keys())
        .copied()
        .collect::<std::collections::HashSet<_>>()
    {
        let before = base_tabs.get(&id);
        let ours = local_tabs.get(&id);
        let theirs = merged.get(&id);
        if ours != before && theirs == before {
            if let Some(tab) = ours {
                merged.insert(id, tab.clone());
            } else {
                merged.remove(&id);
            }
        }
    }

    let base_order = base.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
    let local_order = local.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
    let remote_order = remote.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
    let base_ids = base_order
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let common = base_ids
        .intersection(&local_order.iter().copied().collect())
        .filter(|id| remote_order.contains(id))
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let common_order = |order: &[labonair_workspace_storage::TabId]| {
        order
            .iter()
            .copied()
            .filter(|id| common.contains(id))
            .collect::<Vec<_>>()
    };
    let local_reordered = common_order(&local_order) != common_order(&base_order);
    let remote_reordered = common_order(&remote_order) != common_order(&base_order);
    let mut order = if local_reordered && !remote_reordered {
        local_order.clone()
    } else {
        remote_order.clone()
    };
    for id in local_order.iter().chain(remote_order.iter()).copied() {
        if merged.contains_key(&id) && !order.contains(&id) {
            order.push(id);
        }
    }
    order.retain(|id| merged.contains_key(id));
    let tabs = order
        .into_iter()
        .filter_map(|id| merged.remove(&id))
        .collect();
    DesiredSpace {
        name: if local.name != base.name {
            local.name.clone()
        } else {
            remote.name.clone()
        },
        identity: if local.identity != base.identity {
            local.identity.clone()
        } else {
            remote.identity.clone()
        },
        tabs,
    }
}

fn persist_desired(
    storage: &WorkspaceStorage,
    lease: &WorkspaceLease,
    remote: &StoredSpace,
    desired: &DesiredSpace,
    mut revision: u64,
    now: i64,
) -> Result<u64, labonair_workspace_storage::StoreError> {
    let desired_tabs = desired
        .tabs
        .iter()
        .map(|tab| (tab.id, tab))
        .collect::<std::collections::HashMap<_, _>>();
    let mut current_ids = remote.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();

    for current in &remote.tabs {
        if !desired_tabs.contains_key(&current.id) {
            revision = storage.apply_change(
                lease,
                revision,
                WorkspaceChange::RemoveTab { tab_id: current.id },
                now,
            )?;
            current_ids.retain(|id| *id != current.id);
        }
    }

    for wanted in &desired.tabs {
        if let Some(current) = remote.tabs.iter().find(|tab| tab.id == wanted.id) {
            if current != wanted {
                revision = storage.apply_change(
                    lease,
                    revision,
                    WorkspaceChange::UpdateTab {
                        tab: (*wanted).clone(),
                    },
                    now,
                )?;
            }
        }
    }

    for (position, wanted) in desired.tabs.iter().enumerate() {
        if !current_ids.contains(&wanted.id) {
            let position = position.min(current_ids.len());
            revision = storage.apply_change(
                lease,
                revision,
                WorkspaceChange::AddTab {
                    tab: (*wanted).clone(),
                    position: position as u32,
                },
                now,
            )?;
            current_ids.insert(position, wanted.id);
        }
    }

    for (position, wanted) in desired.tabs.iter().enumerate() {
        if current_ids.get(position) != Some(&wanted.id) {
            revision = storage.apply_change(
                lease,
                revision,
                WorkspaceChange::MoveTab {
                    tab_id: wanted.id,
                    position: position as u32,
                },
                now,
            )?;
            if let Some(old) = current_ids.iter().position(|id| *id == wanted.id) {
                current_ids.remove(old);
            }
            current_ids.insert(position, wanted.id);
        }
    }

    if desired.name != remote.name {
        revision = storage.apply_change(
            lease,
            revision,
            WorkspaceChange::RenameSpace {
                name: desired.name.clone(),
            },
            now,
        )?;
    }
    if desired.identity != remote.identity {
        revision = storage.apply_change(
            lease,
            revision,
            WorkspaceChange::SetIdentity {
                identity: desired.identity.clone(),
            },
            now,
        )?;
    }
    Ok(revision)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(payload: &str) -> StoredTab {
        StoredTab {
            id: labonair_workspace_storage::TabId::new(),
            payload: serde_json::json!({"payload": payload}),
            scrollback_keys: Vec::new(),
        }
    }

    fn space(tabs: Vec<StoredTab>) -> StoredSpace {
        StoredSpace {
            id: labonair_workspace_storage::SpaceId::new(),
            name: "Default".into(),
            identity: serde_json::json!({"kind":"standalone"}),
            revision: 1,
            last_used_sequence: 1,
            tabs,
            participant_count: 1,
        }
    }

    #[test]
    fn independent_tab_additions_merge_without_losing_either_writer() {
        let base = space(Vec::new());
        let local_tab = tab("local");
        let remote_tab = tab("remote");
        let local = space(vec![local_tab.clone()]);
        let remote = space(vec![remote_tab.clone()]);

        assert!(!has_conflict(&base, &local, &remote));
        let merged = merge_local_changes(&base, &local, &remote);
        assert_eq!(
            merged
                .tabs
                .iter()
                .map(|tab| tab.id)
                .collect::<std::collections::HashSet<_>>(),
            [local_tab.id, remote_tab.id].into_iter().collect()
        );
    }

    #[test]
    fn concurrent_edits_to_the_same_tab_are_a_visible_conflict() {
        let original = tab("base");
        let base = space(vec![original.clone()]);
        let mut local_tab = original.clone();
        local_tab.payload = serde_json::json!({"payload":"local edit"});
        let mut remote_tab = original.clone();
        remote_tab.payload = serde_json::json!({"payload":"remote edit"});
        let local = space(vec![local_tab]);
        let remote = space(vec![remote_tab.clone()]);

        assert!(has_conflict(&base, &local, &remote));
        assert_eq!(remote.tabs[0], remote_tab);
    }

    #[test]
    fn independent_tab_update_and_remote_addition_both_survive() {
        let original = tab("base");
        let base = space(vec![original.clone()]);
        let mut local_edit = original.clone();
        local_edit.payload = serde_json::json!({"payload":"local edit"});
        let local = space(vec![local_edit.clone()]);
        let remote_add = tab("remote add");
        let remote = space(vec![original, remote_add.clone()]);

        assert!(!has_conflict(&base, &local, &remote));
        let merged = merge_local_changes(&base, &local, &remote);
        assert_eq!(merged.tabs, vec![local_edit, remote_add]);
    }
}
