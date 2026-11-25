// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! SSH, SFTP and transfers inside the detached session host (ADR 0009).
//!
//! The daemon runs the concrete service stack ([`HostServices`]); the app
//! talks to it through [`RpcCall`]s (request/response, answered out of order)
//! and receives [`Message::BusEvent`]s. PTY bytes of hosted SSH shells use the
//! same data frames and the same attach/snapshot/resync machinery as local
//! terminals ([`Source::Ssh`]).
//!
//! * **Credentials** arrive with the connect request ([`SshHostSnapshot`]) and
//!   are handed to the stack through [`HostServices::install_host`]; the
//!   daemon never opens the app's database or secret store and never logs
//!   them.
//! * **Host-key prompts** are events to subscribed clients. With no client
//!   subscribed (or when the last one goes away) every pending prompt is
//!   rejected: fail closed.
//! * **Transfers** keep running while no client is connected. A conflict or
//!   per-file error parks the job (the worker waits for [`TransferService::resolve`]);
//!   the daemon never resolves on its own, and a configured `overwrite`
//!   default is downgraded to `ask` while nobody is connected.
//! * **Connection loss** turns the hosted session `Exited`-like with the
//!   humanized reason; the daemon never reconnects silently.
//!
//! Lock order: hub -> (nothing else). Session/outbox locks are taken only
//! after the hub guard is dropped, except `Outbox::push_control`, which is a
//! leaf lock.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use labonair_events::{EventBus, RawEvent};
use labonair_sftp::{SftpBrowserService, SftpSessionHandle, SftpSessionService};
use labonair_ssh::{
    ForwardId, ForwardInfo, ForwardOrigin, SharedSshEventSink, SshConnectRequest,
    SshConnectionService, SshEventSink, SshExecService, SshForwardService, SshPtyService,
    SshRemoteCommandService, SshRemoteFileService, SshSessionEvent, SshSessionId,
};
use labonair_terminal_core::{TermDimensions, TerminalColors, TerminalEmulator};
use labonair_transfers::{
    TransferEvent, TransferRegistry, TransferRequest, TransferService, TransferSettings,
    TransferStatus,
};
use serde_json::{json, Value};

use super::{lock, Attachments, Outbox, Session, Source, SshCmd, SshSource, State, Status};
use crate::protocol::{
    Message, RpcCall, RpcValue, SshConnectParams, SshHostSnapshot, SshSessionInfo, TransferConfig,
    WireError, WireTransferSnapshot, FORWARDS_PROTO, FORWARD_CHANGED, FORWARD_REMOVED,
    FORWARD_RESET,
};

/// Keepalive defaults when the connect request leaves them open: on.
const DEFAULT_KEEPALIVE_SECS: u64 = 25;
const DEFAULT_KEEPALIVE_MAX_FAILURES: u32 = 3;
/// `ssh_connect` defaults to this PTY size when the request has none.
const DEFAULT_SSH_COLS: u32 = 220;
const DEFAULT_SSH_ROWS: u32 = 50;
/// Bus events forwarded to subscribed clients.
const FORWARDED_EVENTS: &[&str] = &[
    "ssh_connect_log",
    "known_hosts_warning",
    "hop_prompt",
    "auth_required",
    "passphrase_required",
    "session_established",
    "ssh_connection_lost",
    "sftp_ready",
    "transfer_progress",
    "transfer_step",
    "file_conflict",
    "file_error",
];

/// See [`HostServices::install_host`].
pub type InstallHost = Arc<dyn Fn(&SshHostSnapshot) -> Result<(), String> + Send + Sync>;

/// The SSH / SFTP / transfer stack the daemon hosts. The `labonair-host`
/// binary injects the concrete one ([`crate::stack`]); tests inject fakes.
/// All services report through `events` (the in-process bus the concrete
/// transport already emits on).
pub struct HostServices {
    pub events: EventBus,
    pub connection: Arc<dyn SshConnectionService>,
    pub pty: Arc<dyn SshPtyService>,
    pub remote_command: Arc<dyn SshRemoteCommandService>,
    pub remote_file: Arc<dyn SshRemoteFileService>,
    pub exec: Arc<dyn SshExecService>,
    pub forwards: Arc<dyn SshForwardService>,
    pub sftp_session: Arc<dyn SftpSessionService>,
    pub sftp_browser: Arc<dyn SftpBrowserService>,
    pub transfers: Arc<dyn TransferService>,
    /// Shared settings of the transfer worker.
    pub transfer_settings: Arc<TransferSettings>,
    /// Make the host rows and secrets of a request available to the stack,
    /// in RAM only.
    pub install_host: InstallHost,
}

#[derive(Default)]
struct ConnState {
    /// Payload of the last `session_established` (replayed on subscribe).
    established: Option<Value>,
    /// Humanized reason of the last `ssh_connection_lost`.
    lost: Option<String>,
}

struct PendingTrust {
    owner_conn: u64,
    request_id: labonair_ssh::PromptRequestId,
}

struct TransferPrompt {
    owner_conn: u64,
    request_id: labonair_transfers::TransferPromptId,
    in_flight: bool,
}

#[derive(Default)]
struct Hub {
    registry: TransferRegistry,
    /// Ids of transfer jobs that are queued, running, paused or parked on a
    /// decision: they keep the daemon alive.
    live_jobs: HashSet<String>,
    /// `(connection id, protocol version, outbox)` for event subscribers.
    subscribers: Vec<(u64, u32, Arc<Outbox>)>,
    /// App connection that initiated each live SSH connect/retry.
    prompt_owners: HashMap<String, u64>,
    /// App connection allowed to answer each live transfer decision.
    transfer_owners: HashMap<String, u64>,
    /// One-use decision IDs, fenced by the owning socket connection.
    transfer_prompts: HashMap<String, TransferPrompt>,
    /// Enqueue calls whose worker job id has not been returned to the app yet.
    pending_transfer_enqueues: usize,
    /// Decisions observed while enqueue ownership was still being assigned.
    unannounced_transfer_decisions: HashSet<String>,
    /// Prevent takeover while a previously authorized resolution is executing.
    pending_transfer_resolves: HashSet<String>,
    /// SSH sessions with a host-key prompt awaiting its one owner.
    pending_trust: HashMap<String, PendingTrust>,
    connections: HashMap<String, ConnState>,
    /// The conflict policy the user configured (before the unattended clamp).
    configured_policy: Option<String>,
    /// Per-connection forward diff pushers (v3 subscribers).
    forward_pushers: HashMap<u64, tokio::task::JoinHandle<()>>,
}

pub(super) struct SshRuntime {
    services: HostServices,
    hub: Mutex<Hub>,
}

impl SshRuntime {
    pub(super) fn new(services: HostServices) -> Self {
        Self {
            services,
            hub: Mutex::new(Hub::default()),
        }
    }

    pub(super) fn jobs_active(&self) -> bool {
        !lock(&self.hub).live_jobs.is_empty()
            // A forward that is starting, up, reconnecting or waiting for an
            // answer keeps the daemon alive like a running job.
            || self.services.forwards.list().iter().any(ForwardInfo::is_running)
    }

    pub(super) fn forget_session(&self, ssh_id: &str) {
        let mut hub = lock(&self.hub);
        hub.connections.remove(ssh_id);
        hub.prompt_owners.remove(ssh_id);
        hub.pending_trust.remove(ssh_id);
    }

    /// Follow the service event bus for the daemon's lifetime.
    pub(super) fn start(self: &Arc<Self>, state: Weak<State>) {
        let runtime = Arc::clone(self);
        let mut events = self.services.events.subscribe();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(raw) => {
                        let Some(state) = state.upgrade() else { return };
                        runtime.on_event(&state, raw);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        // Progress is periodic; conflicts and errors would be
                        // lost for good, so re-announce them to everyone.
                        tracing::warn!(skipped, "service event bus lagged");
                        runtime.replay_to_subscribers();
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    fn on_event(self: &Arc<Self>, state: &Arc<State>, raw: RawEvent) {
        let name = raw.name.as_str();
        if !FORWARDED_EVENTS.contains(&name) {
            return;
        }
        let sid = raw
            .payload
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        match name {
            "ssh_connection_lost" => {
                let reason = raw
                    .payload
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("Connection lost")
                    .to_string();
                if let Some(sid) = &sid {
                    lock(&self.hub)
                        .connections
                        .entry(sid.clone())
                        .or_default()
                        .lost = Some(reason.clone());
                    if let Some(session) = hosted(state, sid) {
                        session.mark_ssh_lost(reason);
                        state.changed.notify_one();
                    }
                }
            }
            "session_established" => {
                if let Some(sid) = &sid {
                    let mut hub = lock(&self.hub);
                    let entry = hub.connections.entry(sid.clone()).or_default();
                    entry.established = Some(raw.payload.clone());
                    entry.lost = None;
                }
            }
            "known_hosts_warning" => {
                let Some(sid) = sid else { return };
                let owner = self.owner_outbox(&sid);
                let Some((owner_conn, outbox, request_id)) = owner else {
                    tracing::warn!(session = %sid, "host key prompt has no connected owner; rejecting");
                    self.reject_trust(vec![sid]);
                    return;
                };
                let mut payload = raw.payload;
                if let Value::Object(fields) = &mut payload {
                    fields.insert("prompt_request_id".into(), json!(request_id.as_str()));
                }
                let _ = owner_conn;
                outbox.push_control(&Message::BusEvent {
                    name: raw.name,
                    payload,
                });
                return;
            }
            "auth_required" | "passphrase_required" | "hop_prompt" => {
                let Some(sid) = sid else { return };
                if let Some((_, outbox)) = self.owner_subscriber(&sid) {
                    outbox.push_control(&Message::BusEvent {
                        name: raw.name,
                        payload: raw.payload,
                    });
                }
                return;
            }
            _ => {
                if let Some(event) = TransferEvent::from_raw(name, &raw.payload) {
                    // The registry's own auto-resolve answer (sticky "apply to
                    // all" choices) is deliberately ignored: with the UI gone
                    // nothing may resolve a conflict on its own.
                    let mut hub = lock(&self.hub);
                    match &event {
                        TransferEvent::Progress(job) => {
                            if matches!(
                                job.status,
                                TransferStatus::Queued
                                    | TransferStatus::Running
                                    | TransferStatus::Paused
                            ) {
                                hub.live_jobs.insert(job.id.clone());
                            } else {
                                hub.live_jobs.remove(&job.id);
                            }
                        }
                        TransferEvent::Conflict { job_id, .. }
                        | TransferEvent::FileError { job_id, .. } => {
                            hub.live_jobs.insert(job_id.clone());
                        }
                        TransferEvent::Step { .. } => {}
                    }
                    let decision_job = match &event {
                        TransferEvent::Conflict { job_id, .. }
                        | TransferEvent::FileError { job_id, .. } => Some(job_id.clone()),
                        TransferEvent::Progress(job)
                            if matches!(
                                job.status,
                                TransferStatus::Completed
                                    | TransferStatus::Cancelled
                                    | TransferStatus::Failed(_)
                            ) =>
                        {
                            hub.transfer_owners.remove(&job.id);
                            hub.transfer_prompts.remove(&job.id);
                            None
                        }
                        _ => None,
                    };
                    let registry_update = hub.registry.apply(event);
                    if let labonair_transfers::RegistryUpdate::Resolve(requests) = registry_update {
                        // This is only produced by a sticky choice previously
                        // made by a client that owned the prompt.
                        let transfers = Arc::clone(&self.services.transfers);
                        drop(hub);
                        tokio::spawn(async move {
                            for request in requests {
                                if let Err(error) =
                                    transfers.resolve(request.job_id, request.resolution).await
                                {
                                    tracing::warn!(%error, "sticky transfer resolution failed");
                                }
                            }
                        });
                        state.changed.notify_one();
                        return;
                    }
                    if let Some(job_id) = &decision_job {
                        if let Some(owner_conn) = hub.transfer_owners.get(job_id).copied() {
                            Self::ensure_transfer_prompt(&mut hub, job_id, owner_conn);
                        }
                    }
                    let defer_decision = decision_job.as_ref().is_some_and(|job_id| {
                        if hub.transfer_owners.contains_key(job_id) {
                            false
                        } else if hub.pending_transfer_enqueues > 0 {
                            hub.unannounced_transfer_decisions.insert(job_id.clone());
                            true
                        } else {
                            false
                        }
                    });
                    drop(hub);
                    state.changed.notify_one();
                    if let Some(job_id) = decision_job {
                        if !defer_decision {
                            self.announce_transfer_decision(&job_id);
                        }
                        return;
                    }
                }
            }
        }
        self.broadcast(raw.name, raw.payload);
    }

    fn broadcast(&self, name: String, payload: Value) {
        let message = Message::BusEvent { name, payload };
        for (_, _, outbox) in &lock(&self.hub).subscribers {
            outbox.push_control(&message);
        }
    }

    fn announce_transfer_decision(&self, job_id: &str) {
        let deliveries = {
            let hub = lock(&self.hub);
            let Some(snapshot) = hub
                .registry
                .snapshot()
                .into_iter()
                .find(|snapshot| snapshot.job.id == job_id)
            else {
                return;
            };
            let owner = hub.transfer_owners.get(job_id).copied();
            let prompt_request_id = hub
                .transfer_prompts
                .get(job_id)
                .filter(|prompt| Some(prompt.owner_conn) == owner)
                .map(|prompt| prompt.request_id.as_str());
            let (name, payload) = if let Some(conflict) = snapshot.conflict {
                (
                    "file_conflict",
                    json!({
                        "job_id": job_id,
                        "src_path": conflict.source_path,
                        "dest_path": conflict.destination_path,
                        "prompt_request_id": prompt_request_id,
                    }),
                )
            } else if let Some(error) = snapshot.file_error {
                (
                    "file_error",
                    json!({
                        "job_id": job_id,
                        "path": error.path,
                        "error": error.error,
                        "prompt_request_id": prompt_request_id,
                    }),
                )
            } else {
                return;
            };
            hub.subscribers
                .iter()
                .map(|(conn, proto_version, outbox)| {
                    (
                        *conn,
                        *proto_version,
                        Arc::clone(outbox),
                        name,
                        payload.clone(),
                        owner,
                    )
                })
                .collect::<Vec<_>>()
        };
        for (conn, proto_version, outbox, name, mut payload, owner) in deliveries {
            if proto_version < 2 && owner != Some(conn) {
                continue;
            }
            if let Value::Object(fields) = &mut payload {
                fields.insert("actionable".into(), Value::Bool(owner == Some(conn)));
                fields.insert("recoverable".into(), Value::Bool(owner.is_none()));
            }
            outbox.push_control(&Message::BusEvent {
                name: name.to_string(),
                payload,
            });
        }
    }

    fn ensure_transfer_prompt(
        hub: &mut Hub,
        job_id: &str,
        owner_conn: u64,
    ) -> labonair_transfers::TransferPromptId {
        if let Some(prompt) = hub
            .transfer_prompts
            .get(job_id)
            .filter(|prompt| prompt.owner_conn == owner_conn)
        {
            return prompt.request_id.clone();
        }
        let request_id =
            labonair_transfers::TransferPromptId::new(uuid::Uuid::new_v4().to_string());
        hub.transfer_prompts.insert(
            job_id.to_string(),
            TransferPrompt {
                owner_conn,
                request_id: request_id.clone(),
                in_flight: false,
            },
        );
        request_id
    }

    fn begin_transfer_enqueue(&self) {
        lock(&self.hub).pending_transfer_enqueues += 1;
    }

    fn finish_transfer_enqueue(&self, conn: u64, job_id: Option<&str>) {
        let announce = {
            let mut hub = lock(&self.hub);
            if let Some(job_id) = job_id {
                // Ownership belongs to a live event subscriber. If its
                // connection vanished while enqueue was running, leave the
                // parked decision available for a later explicit takeover.
                if hub.subscribers.iter().any(|(id, _, _)| *id == conn) {
                    hub.transfer_owners.insert(job_id.to_string(), conn);
                }
            }
            hub.pending_transfer_enqueues = hub.pending_transfer_enqueues.saturating_sub(1);
            let ready = hub
                .unannounced_transfer_decisions
                .iter()
                .filter(|id| {
                    hub.transfer_owners.contains_key(*id) || hub.pending_transfer_enqueues == 0
                })
                .cloned()
                .collect::<Vec<_>>();
            for id in &ready {
                hub.unannounced_transfer_decisions.remove(id);
                if let Some(owner_conn) = hub.transfer_owners.get(id).copied() {
                    Self::ensure_transfer_prompt(&mut hub, id, owner_conn);
                }
            }
            ready
        };
        // The worker may have parked before enqueue returned. Publish the
        // now-authorized decision, or make an unrelated orphan recoverable.
        for job_id in announce {
            self.announce_transfer_decision(&job_id);
        }
    }

    fn recover_transfer_conflict(&self, conn: u64, job_id: &str) -> Result<(), String> {
        {
            let mut hub = lock(&self.hub);
            let pending = hub.registry.snapshot().into_iter().any(|snapshot| {
                snapshot.job.id == job_id
                    && (snapshot.conflict.is_some() || snapshot.file_error.is_some())
            });
            if !pending {
                return Err("transfer has no pending conflict or file error".into());
            }
            if hub.pending_transfer_resolves.contains(job_id) {
                return Err("the transfer decision is already being resolved".into());
            }
            if hub
                .transfer_owners
                .get(job_id)
                .is_some_and(|owner| *owner != conn)
            {
                return Err("another connected client owns this transfer decision".into());
            }
            hub.transfer_owners.insert(job_id.to_string(), conn);
            Self::ensure_transfer_prompt(&mut hub, job_id, conn);
        }
        self.announce_transfer_decision(job_id);
        Ok(())
    }

    async fn resolve_transfer(
        &self,
        conn: u64,
        proto_version: u32,
        job_id: String,
        resolution: labonair_transfers::TransferResolution,
        prompt_request_id: Option<labonair_transfers::TransferPromptId>,
    ) -> Result<(), String> {
        let request_id = {
            let mut hub = lock(&self.hub);
            if hub.transfer_owners.get(&job_id) != Some(&conn) {
                return Err("only the transfer decision owner may resolve this prompt".into());
            }
            if proto_version >= 2 && prompt_request_id.is_none() {
                return Err("the transfer decision request ID is required".into());
            }
            let prompt = hub
                .transfer_prompts
                .get_mut(&job_id)
                .filter(|prompt| prompt.owner_conn == conn)
                .ok_or_else(|| "there is no pending transfer decision".to_string())?;
            if prompt_request_id
                .as_ref()
                .is_some_and(|request_id| request_id != &prompt.request_id)
            {
                return Err(
                    "transfer decision request is stale or belongs to another client".into(),
                );
            }
            if prompt.in_flight {
                return Err("transfer decision is already being resolved".into());
            }
            prompt.in_flight = true;
            let request_id = prompt.request_id.clone();
            hub.pending_transfer_resolves.insert(job_id.clone());
            request_id
        };
        if let Err(error) = self
            .services
            .transfers
            .resolve(job_id.clone(), resolution.clone())
            .await
        {
            let mut hub = lock(&self.hub);
            hub.pending_transfer_resolves.remove(&job_id);
            if let Some(prompt) = hub.transfer_prompts.get_mut(&job_id) {
                if prompt.owner_conn == conn && prompt.request_id == request_id {
                    prompt.in_flight = false;
                }
            }
            return Err(error);
        }
        let mut hub = lock(&self.hub);
        hub.pending_transfer_resolves.remove(&job_id);
        if hub
            .transfer_prompts
            .get(&job_id)
            .is_some_and(|prompt| prompt.owner_conn == conn && prompt.request_id == request_id)
        {
            hub.transfer_prompts.remove(&job_id);
        }
        let _ = hub.registry.resolve(&job_id, resolution);
        Ok(())
    }

    fn owner_subscriber(&self, ssh_id: &str) -> Option<(u64, Arc<Outbox>)> {
        let hub = lock(&self.hub);
        let conn = *hub.prompt_owners.get(ssh_id)?;
        let outbox = hub
            .subscribers
            .iter()
            .find(|(subscribed_conn, _, _)| *subscribed_conn == conn)
            .map(|(_, _, outbox)| Arc::clone(outbox))?;
        Some((conn, outbox))
    }

    fn owner_outbox(
        &self,
        ssh_id: &str,
    ) -> Option<(u64, Arc<Outbox>, labonair_ssh::PromptRequestId)> {
        let mut hub = lock(&self.hub);
        let conn = *hub.prompt_owners.get(ssh_id)?;
        let outbox = hub
            .subscribers
            .iter()
            .find(|(subscribed_conn, _, _)| *subscribed_conn == conn)
            .map(|(_, _, outbox)| Arc::clone(outbox))?;
        let request_id = hub
            .pending_trust
            .entry(ssh_id.to_string())
            .or_insert_with(|| PendingTrust {
                owner_conn: conn,
                request_id: labonair_ssh::PromptRequestId::new(uuid::Uuid::new_v4().to_string()),
            });
        if request_id.owner_conn != conn {
            return None;
        }
        Some((conn, outbox, request_id.request_id.clone()))
    }

    fn trust_host(
        &self,
        conn: u64,
        ssh_id: String,
        prompt_request_id: Option<labonair_ssh::PromptRequestId>,
        accepted: bool,
    ) -> Result<(), String> {
        let mut hub = lock(&self.hub);
        if hub.prompt_owners.get(&ssh_id) != Some(&conn) {
            return Err("only the SSH connection owner may answer this prompt".into());
        }
        let pending = hub
            .pending_trust
            .get(&ssh_id)
            .ok_or_else(|| "there is no pending host-key prompt".to_string())?;
        if pending.owner_conn != conn
            || prompt_request_id
                .as_ref()
                .is_some_and(|request_id| request_id != &pending.request_id)
        {
            return Err("host-key prompt request is stale or belongs to another client".into());
        }
        let request_id = pending.request_id.clone();
        hub.pending_trust.remove(&ssh_id);
        drop(hub);
        let connection = Arc::clone(&self.services.connection);
        tokio::spawn(async move {
            if let Err(error) = connection
                .trust_host(SshSessionId::new(ssh_id), Some(request_id), accepted)
                .await
            {
                tracing::warn!(%error, "host-key prompt response failed");
            }
        });
        Ok(())
    }

    /// Reject host-key prompts: `trust_host(.., false)`. Retried briefly
    /// because the transport registers its wait a moment after emitting the
    /// event; the call is idempotent.
    fn reject_trust(&self, sessions: Vec<String>) {
        let connection = Arc::clone(&self.services.connection);
        tokio::spawn(async move {
            for attempt in 0..3 {
                for sid in &sessions {
                    let _ = connection
                        .trust_host(SshSessionId::new(sid.clone()), None, false)
                        .await;
                }
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        });
    }

    /// Replay the daemon's current state to one outbox as bus events (see
    /// [`RpcCall::SubscribeEvents`]).
    fn replay(hub: &Hub, conn: u64, proto_version: u32, outbox: &Outbox, with_steps: bool) {
        let push = |name: &str, payload: Value| {
            outbox.push_control(&Message::BusEvent {
                name: name.to_string(),
                payload,
            })
        };
        let mut ids: Vec<_> = hub.connections.keys().collect();
        ids.sort();
        for sid in ids {
            let state = &hub.connections[sid];
            if let Some(reason) = &state.lost {
                push(
                    "ssh_connection_lost",
                    json!({ "session_id": sid, "reason": reason }),
                );
            } else if let Some(payload) = &state.established {
                push("session_established", payload.clone());
            }
        }
        // Registry order is newest first; replay oldest first.
        for snapshot in hub.registry.snapshot().into_iter().rev() {
            let job_id = snapshot.job.id.clone();
            push(
                "transfer_progress",
                serde_json::to_value(&snapshot.job).unwrap_or_default(),
            );
            if with_steps {
                for step in &snapshot.steps {
                    push(
                        "transfer_step",
                        json!({ "job_id": job_id, "ts": step.timestamp, "message": step.message }),
                    );
                }
            }
            if let Some(conflict) = &snapshot.conflict {
                if proto_version < 2 && hub.transfer_owners.get(&job_id) != Some(&conn) {
                    continue;
                }
                push(
                    "file_conflict",
                    json!({
                        "job_id": job_id,
                        "src_path": conflict.source_path,
                        "dest_path": conflict.destination_path,
                        "actionable": hub.transfer_owners.get(&job_id) == Some(&conn),
                        "recoverable": !hub.transfer_owners.contains_key(&job_id),
                        "prompt_request_id": hub
                            .transfer_prompts
                            .get(&job_id)
                            .filter(|prompt| prompt.owner_conn == conn)
                            .map(|prompt| prompt.request_id.as_str()),
                    }),
                );
            }
            if let Some(error) = &snapshot.file_error {
                if proto_version < 2 && hub.transfer_owners.get(&job_id) != Some(&conn) {
                    continue;
                }
                push(
                    "file_error",
                    json!({
                        "job_id": job_id,
                        "path": error.path,
                        "error": error.error,
                        "actionable": hub.transfer_owners.get(&job_id) == Some(&conn),
                        "recoverable": !hub.transfer_owners.contains_key(&job_id),
                        "prompt_request_id": hub
                            .transfer_prompts
                            .get(&job_id)
                            .filter(|prompt| prompt.owner_conn == conn)
                            .map(|prompt| prompt.request_id.as_str()),
                    }),
                );
            }
        }
    }

    fn replay_to_subscribers(&self) {
        let hub = lock(&self.hub);
        for (conn, proto_version, outbox) in &hub.subscribers {
            Self::replay(&hub, *conn, *proto_version, outbox, false);
        }
    }

    /// Subscribe `conn`: replay state, then stream live events. Atomic with
    /// respect to [`Self::on_event`] (both hold the hub).
    fn subscribe(&self, conn: u64, proto_version: u32, outbox: &Arc<Outbox>) {
        let mut hub = lock(&self.hub);
        if let Some((_, version, _)) = hub.subscribers.iter_mut().find(|(id, _, _)| *id == conn) {
            *version = proto_version;
        } else {
            hub.subscribers
                .push((conn, proto_version, Arc::clone(outbox)));
        }
        Self::replay(&hub, conn, proto_version, outbox, true);
        // Finished jobs were handed to the UI; do not replay them forever.
        hub.registry.clear_completed();
        if let Some(old) = hub.forward_pushers.remove(&conn) {
            old.abort();
        }
        if proto_version >= FORWARDS_PROTO {
            // Replay and live pushes share one task start so no change is
            // lost in between (the receiver is marked before the first read).
            let forwards = Arc::clone(&self.services.forwards);
            let mut rx = forwards.revision();
            rx.borrow_and_update();
            outbox.push_control(&Message::BusEvent {
                name: FORWARD_RESET.into(),
                payload: Value::Null,
            });
            let mut last = HashMap::new();
            push_forward_diff(&*forwards, outbox, &mut last);
            let outbox = Arc::clone(outbox);
            hub.forward_pushers.insert(
                conn,
                tokio::spawn(async move {
                    while rx.changed().await.is_ok() {
                        push_forward_diff(&*forwards, &outbox, &mut last);
                    }
                }),
            );
        }
        drop(hub);
        self.apply_policy();
    }

    /// A client went away (explicitly or by disconnecting). The last one
    /// leaving rejects every pending host-key prompt.
    pub(super) fn unsubscribe(&self, conn: u64) {
        let mut hub = lock(&self.hub);
        hub.subscribers.retain(|(id, _, _)| *id != conn);
        if let Some(pusher) = hub.forward_pushers.remove(&conn) {
            pusher.abort();
        }
        let orphaned = hub
            .pending_trust
            .iter()
            .filter(|(_, pending)| pending.owner_conn == conn)
            .map(|(ssh_id, _)| ssh_id.clone())
            .collect::<Vec<_>>();
        for ssh_id in &orphaned {
            hub.pending_trust.remove(ssh_id);
        }
        hub.prompt_owners.retain(|_, owner| *owner != conn);
        hub.transfer_owners.retain(|_, owner| *owner != conn);
        hub.transfer_prompts
            .retain(|_, prompt| prompt.owner_conn != conn);
        drop(hub);
        if !orphaned.is_empty() {
            self.reject_trust(orphaned);
        }
        self.apply_policy();
    }

    /// The conflict policy the worker sees: unattended, `overwrite` becomes
    /// `ask` so a parked job can never be overwritten without a decision.
    fn apply_policy(&self) {
        let hub = lock(&self.hub);
        let configured = hub.configured_policy.as_deref().unwrap_or("ask");
        let effective = if hub.subscribers.is_empty() && configured == "overwrite" {
            "ask"
        } else {
            configured
        };
        *lock(&self.services.transfer_settings.default_conflict_resolution) = effective.to_string();
    }

    fn configure_transfers(&self, config: TransferConfig) {
        use std::sync::atomic::Ordering::Relaxed;
        let settings = &self.services.transfer_settings;
        settings
            .max_concurrent
            .store(config.max_concurrent.max(1), Relaxed);
        settings.chunk_size.store(config.chunk_size.max(1), Relaxed);
        *lock(&settings.on_folder_file_error) = config.on_folder_file_error;
        lock(&self.hub).configured_policy = Some(config.default_conflict_resolution);
        self.apply_policy();
    }
}

/// Send only what changed since `last` (the forwards this connection already
/// knows). `origin` is `Daemon` on the wire whatever the service says.
fn push_forward_diff(
    service: &dyn SshForwardService,
    outbox: &Outbox,
    last: &mut HashMap<ForwardId, ForwardInfo>,
) {
    let now = wire_forwards(service);
    for id in last.keys().filter(|id| !now.contains_key(*id)) {
        outbox.push_control(&Message::BusEvent {
            name: FORWARD_REMOVED.into(),
            payload: json!({ "id": id.0 }),
        });
    }
    for (id, info) in &now {
        if last.get(id) != Some(info) {
            outbox.push_control(&Message::BusEvent {
                name: FORWARD_CHANGED.into(),
                payload: serde_json::to_value(info).unwrap_or_default(),
            });
        }
    }
    *last = now;
}

fn wire_forwards(service: &dyn SshForwardService) -> HashMap<ForwardId, ForwardInfo> {
    service
        .list()
        .into_iter()
        .map(|mut info| {
            info.origin = ForwardOrigin::Daemon;
            (info.spec.id.clone(), info)
        })
        .collect()
}

fn hosted(state: &State, ssh_id: &str) -> Option<Arc<Session>> {
    let wire = *lock(&state.ssh_index).get(ssh_id)?;
    state.session(wire)
}

// ── Hosted SSH sessions ──────────────────────────────────────────────────────

/// The output sink handed to `SshConnectionService::connect`. It never fails
/// while the session lives (an `Err` would end the transport's reader).
struct HostedSink {
    session: Weak<Session>,
}

impl SshEventSink for HostedSink {
    fn send(&self, event: SshSessionEvent) -> Result<(), String> {
        let session = self.session.upgrade().ok_or("session closed")?;
        match event {
            SshSessionEvent::Output { data } => session.feed_ssh(data.as_bytes()),
        }
        Ok(())
    }
}

/// Write the PTY input of one SSH session, in order, coalescing bursts. The
/// transport takes `String`s, so an incomplete UTF-8 tail waits for its rest.
fn spawn_pty_task(
    pty: Arc<dyn SshPtyService>,
    ssh_id: String,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<SshCmd>,
) {
    async fn flush(pty: &dyn SshPtyService, id: &SshSessionId, pending: &mut Vec<u8>) {
        let cut = match std::str::from_utf8(pending) {
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            _ => pending.len(),
        };
        if cut == 0 {
            return;
        }
        let data = String::from_utf8_lossy(&pending[..cut]).into_owned();
        pending.drain(..cut);
        let _ = pty.write(id.clone(), data).await;
    }
    tokio::spawn(async move {
        let id = SshSessionId::new(ssh_id);
        let mut pending = Vec::new();
        while let Some(first) = rx.recv().await {
            let mut next = Some(first);
            while let Some(cmd) = next.take() {
                match cmd {
                    SshCmd::Write(bytes) => pending.extend_from_slice(&bytes),
                    SshCmd::Resize(cols, rows) => {
                        flush(pty.as_ref(), &id, &mut pending).await;
                        let _ = pty
                            .resize(id.clone(), u32::from(cols), u32::from(rows))
                            .await;
                    }
                }
                next = rx.try_recv().ok();
            }
            flush(pty.as_ref(), &id, &mut pending).await;
        }
    });
}

impl State {
    /// Create the hosted session that will carry an SSH shell. Registered
    /// before the connect starts so no output is lost.
    fn create_ssh_session(
        self: &Arc<Self>,
        runtime: &SshRuntime,
        params: &SshConnectParams,
    ) -> Result<Arc<Session>, String> {
        let cols = params.initial_cols.unwrap_or(DEFAULT_SSH_COLS);
        let rows = params.initial_rows.unwrap_or(DEFAULT_SSH_ROWS);
        let id = match params.id {
            Some(id) if lock(&self.sessions).contains_key(&id) => {
                return Err(format!("session {id} already exists"));
            }
            Some(id) => id,
            None => self.next_id.fetch_add(1, Ordering::SeqCst),
        };
        let scrollback_lines = params
            .scrollback
            .map_or_else(
                || self.scrollback_lines.load(Ordering::SeqCst),
                |n| n as usize,
            )
            .max(1);
        let colors = params
            .colors
            .as_ref()
            .map_or_else(TerminalColors::xterm_default, Into::into);
        let (event_tx, events) = std::sync::mpsc::sync_channel(65_536);
        let config = labonair_terminal_core::EmulatorConfig {
            scrollback: scrollback_lines,
            ..Default::default()
        };
        let emulator = TerminalEmulator::new_with(
            colors,
            TermDimensions::new(cols as usize, rows as usize),
            event_tx,
            config,
        );
        let (cmd, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_pty_task(
            Arc::clone(&runtime.services.pty),
            params.session_id.clone(),
            cmd_rx,
        );
        let session = Arc::new(Session {
            id,
            pane_tag: params.pane_tag.clone(),
            label: params.label.clone(),
            persist_key: params
                .persist_key
                .clone()
                .unwrap_or_else(|| super::persist_key_for(id)),
            scrollback_lines,
            source: Source::Ssh(Box::new(SshSource {
                emulator: Mutex::new(emulator),
                events: Mutex::new(events),
                cmd,
                ssh_id: params.session_id.clone(),
                host_id: params.host_id.clone(),
                connection: Arc::clone(&runtime.services.connection),
                lost: Mutex::new(None),
                connected: AtomicBool::new(false),
            })),
            attachments: Arc::new(Mutex::new(Attachments::default())),
            input_order: Mutex::new(()),
            status: Mutex::new(Status::Running),
            killed: AtomicBool::new(false),
            notify: Arc::new(tokio::sync::Notify::new()),
        });
        lock(&self.sessions).insert(id, Arc::clone(&session));
        lock(&self.ssh_index).insert(params.session_id.clone(), id);
        self.changed.notify_one();
        Ok(session)
    }
}

fn invalid(message: &str) -> WireError {
    WireError {
        code: Some("InvalidInput".into()),
        message: message.into(),
    }
}

/// Local file paths are trusted (same user, 0600 socket) but must be
/// absolute: the daemon's working directory means nothing to the app.
fn require_local_path(path: &str) -> Result<(), WireError> {
    if path.is_empty() {
        return Err(invalid("local path is empty"));
    }
    if path.starts_with('/') || path == "~" || path.starts_with("~/") {
        Ok(())
    } else {
        Err(invalid("local path must be absolute"))
    }
}

async fn connect(
    state: &Arc<State>,
    runtime: &SshRuntime,
    owner_conn: u64,
    params: SshConnectParams,
) -> Result<RpcValue, WireError> {
    if params.session_id.is_empty() || params.host_id.is_empty() {
        return Err(invalid("session_id and host_id are required"));
    }
    (runtime.services.install_host)(&params.host).map_err(WireError::plain)?;
    // The app re-connecting a tab: the old connection (usually lost) goes.
    let previous = lock(&state.ssh_index).get(&params.session_id).copied();
    if let Some(old) = previous.and_then(|id| state.remove_session(id)) {
        super::retire(old);
    }
    {
        let mut hub = lock(&runtime.hub);
        hub.prompt_owners
            .insert(params.session_id.clone(), owner_conn);
        hub.pending_trust.remove(&params.session_id);
    }
    let session = state
        .create_ssh_session(runtime, &params)
        .map_err(WireError::plain)?;
    let id = session.id;
    let sink: SharedSshEventSink = Arc::new(HostedSink {
        session: Arc::downgrade(&session),
    });
    let request = SshConnectRequest {
        session_id: SshSessionId::new(params.session_id),
        host_id: params.host_id,
        passphrase: params.passphrase,
        password: params.password,
        initial_cols: params.initial_cols,
        initial_rows: params.initial_rows,
        blocks: params.blocks,
        connect_timeout_secs: params.connect_timeout_secs,
        keepalive_interval_secs: params
            .keepalive_interval_secs
            .or(Some(DEFAULT_KEEPALIVE_SECS)),
        keepalive_max_failures: params
            .keepalive_max_failures
            .or(Some(DEFAULT_KEEPALIVE_MAX_FAILURES)),
        route_override: params.route_override,
    };
    drop(session);
    match runtime.services.connection.connect(request, sink).await {
        Ok(()) => {
            if let Some(session) = state.session(id) {
                if let Source::Ssh(ssh) = &session.source {
                    ssh.connected
                        .store(lock(&ssh.lost).is_none(), Ordering::SeqCst);
                }
            }
            Ok(RpcValue::SshConnected { session: id })
        }
        Err(error) => {
            if let Some(session) = state.remove_session(id) {
                super::retire(session);
            }
            Err(WireError::from(error))
        }
    }
}

fn ssh_sessions(state: &State) -> Vec<SshSessionInfo> {
    let mut sessions: Vec<_> = lock(&state.sessions).values().cloned().collect();
    sessions.sort_by_key(|s| s.id);
    sessions
        .iter()
        .filter_map(|session| {
            let Source::Ssh(ssh) = &session.source else {
                return None;
            };
            let info = session.info();
            Some(SshSessionInfo {
                session: session.id,
                ssh_session_id: ssh.ssh_id.clone(),
                host_id: ssh.host_id.clone(),
                connected: ssh.connected.load(Ordering::SeqCst),
                lost_reason: lock(&ssh.lost).clone(),
                pane_tag: info.pane_tag,
                label: info.label,
                cols: info.cols,
                rows: info.rows,
            })
        })
        .collect()
}

fn transfer_snapshots(runtime: &SshRuntime) -> Vec<WireTransferSnapshot> {
    lock(&runtime.hub)
        .registry
        .snapshot()
        .into_iter()
        .map(|snapshot| WireTransferSnapshot {
            job: snapshot.job,
            steps: snapshot
                .steps
                .into_iter()
                .map(|step| (step.timestamp, step.message))
                .collect(),
            conflict: snapshot
                .conflict
                .map(|c| (c.source_path, c.destination_path)),
            file_error: snapshot.file_error.map(|e| (e.path, e.error)),
        })
        .collect()
}

// ── RPC dispatch ─────────────────────────────────────────────────────────────

/// Run `call` on its own task and queue the answer. Every call gets its own
/// task: a connect can wait minutes for a host-key decision that arrives on the
/// same connection, and a panic must not leave the caller hanging.
pub(super) fn spawn_rpc(
    state: &Arc<State>,
    conn: u64,
    proto_version: u32,
    outbox: &Arc<Outbox>,
    req_id: u64,
    call: RpcCall,
) {
    let state = Arc::clone(state);
    let outbox = Arc::clone(outbox);
    tokio::spawn(async move {
        let work = tokio::spawn({
            let state = Arc::clone(&state);
            let outbox = Arc::clone(&outbox);
            async move { dispatch(&state, conn, proto_version, &outbox, call).await }
        });
        let reply = match work.await {
            Ok(Ok(value)) => Message::RpcOk { req_id, value },
            Ok(Err(error)) => Message::RpcErr { req_id, error },
            Err(_) => Message::RpcErr {
                req_id,
                error: WireError::plain("internal error"),
            },
        };
        outbox.push_control(&reply);
    });
}

async fn dispatch(
    state: &Arc<State>,
    conn: u64,
    proto_version: u32,
    outbox: &Arc<Outbox>,
    call: RpcCall,
) -> Result<RpcValue, WireError> {
    let Some(runtime) = state.ssh.clone() else {
        return Err(WireError::plain(
            "this host has no SSH, SFTP or transfer services",
        ));
    };
    let services = &runtime.services;
    let sid = |s: String| SshSessionId::new(s);
    let handle = |s: &str| SftpSessionHandle::from_ssh_session(SshSessionId::new(s));
    let unit = |r: Result<(), String>| r.map(|()| RpcValue::Unit).map_err(WireError::plain);
    let unit_typed = |r: Result<(), labonair_errors::LabonairError>| {
        r.map(|()| RpcValue::Unit).map_err(WireError::from)
    };

    match call {
        RpcCall::SubscribeEvents => {
            runtime.subscribe(conn, proto_version, outbox);
            Ok(RpcValue::Unit)
        }
        RpcCall::UnsubscribeEvents => {
            runtime.unsubscribe(conn);
            Ok(RpcValue::Unit)
        }

        RpcCall::SshConnect(params) => connect(state, &runtime, conn, params).await,
        RpcCall::SshDisconnect { session_id } => {
            let wire = lock(&state.ssh_index).get(&session_id).copied();
            match wire.and_then(|id| state.remove_session(id)) {
                // `retire` closes the connection.
                Some(session) => {
                    super::retire(session);
                    Ok(RpcValue::Unit)
                }
                None => unit(services.connection.disconnect(sid(session_id)).await),
            }
        }
        RpcCall::SshTrustHost {
            session_id,
            prompt_request_id,
            accepted,
        } => runtime
            .trust_host(conn, session_id, prompt_request_id, accepted)
            .map(|()| RpcValue::Unit)
            .map_err(WireError::plain),

        RpcCall::SshAnswerPrompt {
            session_id,
            prompt_request_id,
            answer,
        } => {
            if lock(&runtime.hub).prompt_owners.get(&session_id) != Some(&conn) {
                return Err(WireError::plain(
                    "only the SSH connection owner may answer this prompt".to_string(),
                ));
            }
            unit(
                services
                    .connection
                    .answer_prompt(sid(session_id), prompt_request_id, answer)
                    .await,
            )
        }

        RpcCall::SshForwardOpen { request, host } => {
            (services.install_host)(&host).map_err(WireError::plain)?;
            services
                .forwards
                .open(request)
                .await
                .map(|id| RpcValue::Text(id.0))
                .map_err(WireError::plain)
        }
        RpcCall::SshForwardClose { id } => unit(services.forwards.close(id).await),
        RpcCall::SshForwardRestart { id } => unit(services.forwards.restart(id).await),
        RpcCall::SshForwardAnswerPrompt {
            id,
            prompt_request_id,
            answer,
        } => unit(
            services
                .forwards
                .answer_prompt(id, prompt_request_id, answer)
                .await,
        ),
        RpcCall::SshForwardAttachHost { host_id, host } => {
            (services.install_host)(&host).map_err(WireError::plain)?;
            unit(services.forwards.attach_host(host_id).await)
        }
        RpcCall::SshForwardDetachHost { host_id } => {
            unit(services.forwards.detach_host(host_id).await)
        }
        RpcCall::SshForwardSetPolicy { auto_reconnect } => {
            services.forwards.set_reconnect_policy(auto_reconnect);
            Ok(RpcValue::Unit)
        }
        RpcCall::SshForwardList => Ok(RpcValue::Forwards(
            wire_forwards(&*services.forwards).into_values().collect(),
        )),

        RpcCall::SshPtyWrite { session_id, data } => match hosted(state, &session_id) {
            Some(session) => session
                .send_external_input(data.into_bytes())
                .map(|()| RpcValue::Unit)
                .map_err(|message| invalid(&message)),
            None => unit(services.pty.write(sid(session_id), data).await),
        },
        RpcCall::SshPtyResize {
            session_id,
            cols,
            rows,
        } => match hosted(state, &session_id) {
            Some(session) => session
                .resize_external(
                    cols.min(u32::from(u16::MAX)) as u16,
                    rows.min(u32::from(u16::MAX)) as u16,
                )
                .map(|()| RpcValue::Unit)
                .map_err(|message| invalid(&message)),
            None => unit(services.pty.resize(sid(session_id), cols, rows).await),
        },

        RpcCall::SshExec {
            session_id,
            command,
            stdin,
        } => {
            use base64::Engine as _;
            let b64 = base64::engine::general_purpose::STANDARD;
            let stdin = stdin
                .map(|text| b64.decode(text))
                .transpose()
                .map_err(|_| invalid("stdin is not valid base64"))?;
            let out = services
                .exec
                .exec(sid(session_id), command, stdin)
                .await
                .map_err(WireError::plain)?;
            Ok(RpcValue::Exec {
                stdout: b64.encode(out.stdout),
                stderr: b64.encode(out.stderr),
                exit_code: out.exit_code,
            })
        }
        RpcCall::SshChown {
            session_id,
            path,
            owner,
            group,
        } => unit(
            services
                .remote_command
                .chown(sid(session_id), path, owner, group)
                .await,
        ),
        RpcCall::SshCalculateSize { session_id, path } => services
            .remote_command
            .calculate_size(sid(session_id), path)
            .await
            .map(RpcValue::Text)
            .map_err(WireError::plain),

        RpcCall::SshPrepareRemoteEdit {
            session_id,
            remote_path,
            max_bytes,
        } => services
            .remote_file
            .prepare_remote_edit(sid(session_id), remote_path, max_bytes)
            .await
            .map(RpcValue::Text)
            .map_err(WireError::plain),
        RpcCall::SshSaveRemoteEdit {
            session_id,
            remote_path,
            local_temp_path,
        } => {
            require_local_path(&local_temp_path)?;
            unit(
                services
                    .remote_file
                    .save_remote_edit(sid(session_id), remote_path, local_temp_path)
                    .await,
            )
        }
        RpcCall::SshCleanupRemoteEditTemp { local_temp_path } => {
            require_local_path(&local_temp_path)?;
            unit(
                services
                    .remote_file
                    .cleanup_remote_edit_temp(local_temp_path)
                    .await,
            )
        }

        RpcCall::SftpOpen { ssh_session } => services
            .sftp_session
            .open(sid(ssh_session))
            .await
            .map(|handle| RpcValue::SftpHandle(handle.as_str().to_string()))
            .map_err(WireError::from),
        RpcCall::SftpClose { session } => unit(services.sftp_session.close(handle(&session)).await),
        RpcCall::SftpReadDir { session, path } => services
            .sftp_browser
            .read_dir(handle(&session), path)
            .await
            .map(RpcValue::Entries)
            .map_err(WireError::from),
        RpcCall::SftpRename {
            session,
            old_path,
            new_path,
        } => unit_typed(
            services
                .sftp_browser
                .rename(handle(&session), old_path, new_path)
                .await,
        ),
        RpcCall::SftpDelete { session, paths } => {
            unit_typed(services.sftp_browser.delete(handle(&session), paths).await)
        }
        RpcCall::SftpMkdir {
            session,
            path,
            recursive,
        } => unit_typed(
            services
                .sftp_browser
                .mkdir(handle(&session), path, recursive)
                .await,
        ),
        RpcCall::SftpCreateFile { session, path } => unit_typed(
            services
                .sftp_browser
                .create_file(handle(&session), path)
                .await,
        ),
        RpcCall::SftpChmod {
            session,
            path,
            mode,
        } => unit_typed(
            services
                .sftp_browser
                .chmod(handle(&session), path, mode)
                .await,
        ),

        RpcCall::TransferEnqueue {
            session_id,
            src_path,
            dest_path,
            direction,
        } => {
            let (local, remote) = match direction {
                labonair_transfers::TransferDirection::Upload => (&src_path, &dest_path),
                labonair_transfers::TransferDirection::Download => (&dest_path, &src_path),
            };
            require_local_path(local)?;
            if remote.is_empty() {
                return Err(invalid("remote path is empty"));
            }
            runtime.begin_transfer_enqueue();
            let result = services
                .transfers
                .enqueue(TransferRequest {
                    session_id,
                    src_path,
                    dest_path,
                    direction,
                })
                .await;
            let job_id = match result {
                Ok(job_id) => {
                    runtime.finish_transfer_enqueue(conn, Some(&job_id));
                    job_id
                }
                Err(error) => {
                    runtime.finish_transfer_enqueue(conn, None);
                    return Err(WireError::plain(error));
                }
            };
            Ok(RpcValue::JobId(job_id))
        }
        RpcCall::TransferCancel { job_id } => unit(services.transfers.cancel(job_id).await),
        RpcCall::TransferResolve {
            job_id,
            resolution,
            prompt_request_id,
        } => unit(
            runtime
                .resolve_transfer(
                    conn,
                    proto_version,
                    job_id,
                    resolution.into(),
                    prompt_request_id,
                )
                .await,
        ),
        RpcCall::TransferRecoverConflict { job_id } => runtime
            .recover_transfer_conflict(conn, &job_id)
            .map(|()| RpcValue::Unit)
            .map_err(WireError::plain),
        RpcCall::TransferConfigure(config) => {
            runtime.configure_transfers(config);
            Ok(RpcValue::Unit)
        }
        RpcCall::ListTransfers => Ok(RpcValue::Transfers(transfer_snapshots(&runtime))),
        RpcCall::ListSshSessions => Ok(RpcValue::SshSessions(ssh_sessions(state))),
        RpcCall::Unknown => Err(WireError::plain("unsupported call")),
    }
}
