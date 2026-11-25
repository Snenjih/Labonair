// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! UI-free SSH capability contracts.
//!
//! Transport implementations live in `labonair-ssh-transport`. Feature crates
//! use these focused contracts instead of importing russh or transport state. The
//! traits are split by responsibility so a consumer that only needs PTY I/O
//! does not depend on host configuration or tunnel management.

use labonair_errors::LabonairError;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SshSessionId(String);

impl SshSessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SshSessionId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<SshSessionId> for String {
    fn from(value: SshSessionId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshSessionEvent {
    Output { data: String },
}

pub trait SshEventSink: Send + Sync {
    fn send(&self, event: SshSessionEvent) -> Result<(), String>;
}

pub type SharedSshEventSink = Arc<dyn SshEventSink>;

/// Unforgeable-in-practice, single-use identity for an actionable SSH prompt.
/// The Detached Host creates IDs; app-mode transports may leave them absent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PromptRequestId(String);

impl PromptRequestId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a hop prompt asks the user for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptKind {
    HostKey {
        host: String,
        fingerprint: String,
        is_mismatch: bool,
    },
    Password,
    Passphrase,
}

/// Position of a jump hop within its route (`index` is 1-based).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HopInfo {
    pub index: usize,
    pub total: usize,
    pub label: String,
}

/// An actionable prompt raised by a hop (or background) session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingPrompt {
    pub request_id: PromptRequestId,
    pub kind: PromptKind,
    pub hop: Option<HopInfo>,
}

/// The user's answer to a [`PendingPrompt`]. `Debug` never prints a secret.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptAnswer {
    Trust(bool),
    Secret(Option<String>),
}

impl std::fmt::Debug for PromptAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trust(v) => f.debug_tuple("Trust").field(v).finish(),
            Self::Secret(Some(_)) => f.write_str("Secret(<redacted>)"),
            Self::Secret(None) => f.write_str("Secret(None)"),
        }
    }
}

/// Connection-lifecycle events consumed by the SSH surface.
///
/// These events are deliberately separate from PTY output: the connection
/// flow owns authentication, host trust, progress, and disconnect state, while
/// the terminal session owns streamed bytes through [`SshSessionEvent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshConnectionEvent {
    ConnectLog {
        session_id: String,
        message: String,
    },
    KnownHostsWarning {
        session_id: String,
        prompt_request_id: Option<PromptRequestId>,
        fingerprint: String,
        host: String,
        is_mismatch: bool,
    },
    AuthRequired {
        session_id: String,
        prompt_message: String,
        is_2fa: bool,
    },
    PassphraseRequired {
        session_id: String,
    },
    /// A jump hop needs input; `session_id` is the BASE (tab) session id.
    HopPrompt {
        session_id: String,
        pending: PendingPrompt,
    },
    SessionEstablished {
        session_id: String,
        default_path: Option<String>,
    },
    ConnectionLost {
        session_id: String,
    },
}

/// Asynchronous receiver for typed SSH connection events.
pub trait SshEventReceiver: Send {
    fn recv<'a>(&'a mut self) -> BoxFuture<'a, Option<SshConnectionEvent>>;
}

/// Source of SSH connection-lifecycle events.
///
/// The application shell injects this at composition time. Implementations
/// may translate a platform event stream, but consumers never depend on the
/// transport's global event bus or aggregate application state.
pub trait SshEventSource: Send + Sync {
    fn subscribe(&self) -> Box<dyn SshEventReceiver>;
}

#[derive(Debug, Clone)]
pub struct SshConnectRequest {
    pub session_id: SshSessionId,
    pub host_id: String,
    pub passphrase: Option<String>,
    pub password: Option<String>,
    pub initial_cols: Option<u32>,
    pub initial_rows: Option<u32>,
    pub blocks: bool,
    pub connect_timeout_secs: Option<u64>,
    /// Fallback keep-alive ping interval, in seconds, applied when the host
    /// record doesn't set its own (`None` = use the transport's built-in
    /// default).
    pub keepalive_interval_secs: Option<u64>,
    /// Fallback missed-ping tolerance applied when the host record doesn't
    /// set its own (`None` = use the transport's built-in default).
    pub keepalive_max_failures: Option<u32>,
    /// One-off jump route (outermost hop first) that REPLACES the host's
    /// stored chain for this connection; `Some(vec![])` = direct. Never
    /// persisted (ADR 0016).
    pub route_override: Option<Vec<String>>,
}

pub trait SshConnectionService: Send + Sync {
    fn connect<'a>(
        &'a self,
        request: SshConnectRequest,
        events: SharedSshEventSink,
    ) -> BoxFuture<'a, Result<(), LabonairError>>;

    fn disconnect<'a>(&'a self, session_id: SshSessionId) -> BoxFuture<'a, Result<(), String>>;

    fn trust_host<'a>(
        &'a self,
        session_id: SshSessionId,
        prompt_request_id: Option<PromptRequestId>,
        accepted: bool,
    ) -> BoxFuture<'a, Result<(), String>>;

    /// Answers a [`SshConnectionEvent::HopPrompt`] for the base `session_id`.
    fn answer_prompt<'a>(
        &'a self,
        session_id: SshSessionId,
        request_id: PromptRequestId,
        answer: PromptAnswer,
    ) -> BoxFuture<'a, Result<(), String>>;
}

pub trait SshPtyService: Send + Sync {
    fn write<'a>(
        &'a self,
        session_id: SshSessionId,
        data: String,
    ) -> BoxFuture<'a, Result<(), String>>;

    fn resize<'a>(
        &'a self,
        session_id: SshSessionId,
        cols: u32,
        rows: u32,
    ) -> BoxFuture<'a, Result<(), String>>;
}

/// Typed boundary for remote commands that are not SFTP protocol operations.
/// `chown`, size calculation, search, and checksums stay separate from the
/// SFTP browser contract even though they are surfaced by remote-file UI.
pub trait SshRemoteCommandService: Send + Sync {
    fn chown<'a>(
        &'a self,
        session_id: SshSessionId,
        path: String,
        owner: String,
        group: String,
    ) -> BoxFuture<'a, Result<(), String>>;

    fn calculate_size<'a>(
        &'a self,
        session_id: SshSessionId,
        path: String,
    ) -> BoxFuture<'a, Result<String, String>>;
}

/// Transitional remote-file lifecycle contract used by the editor bridge.
/// The implementation owns the temporary-file mechanics; the workspace only
/// receives paths and coordinates editor tabs. This keeps transport
/// implementation details out of the workspace until streaming file I/O is
/// introduced.
pub trait SshRemoteFileService: Send + Sync {
    fn prepare_remote_edit<'a>(
        &'a self,
        session_id: SshSessionId,
        remote_path: String,
        max_bytes: Option<u64>,
    ) -> BoxFuture<'a, Result<String, String>>;

    fn save_remote_edit<'a>(
        &'a self,
        session_id: SshSessionId,
        remote_path: String,
        local_temp_path: String,
    ) -> BoxFuture<'a, Result<(), String>>;

    fn cleanup_remote_edit_temp<'a>(
        &'a self,
        local_temp_path: String,
    ) -> BoxFuture<'a, Result<(), String>>;
}

/// Output of a one-shot remote command (a fresh exec channel; the interactive
/// PTY is not disturbed).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SshExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `-1` when the server never reported an exit status.
    pub exit_code: i32,
}

/// Run a one-shot command on an existing SSH session, optionally feeding
/// `stdin` first. Implemented in-process by `labonair-ssh-transport` and
/// served by the detached session host for sessions it owns.
pub trait SshExecService: Send + Sync {
    fn exec<'a>(
        &'a self,
        session_id: SshSessionId,
        command: String,
        stdin: Option<Vec<u8>>,
    ) -> BoxFuture<'a, Result<SshExecOutput, String>>;
}

/// Access to SSH sessions that live outside this process (the detached session
/// host), for consumers that otherwise read the in-process session map:
/// remote git, snippet runs and the MCP bridge. A consumer asks this first and
/// falls back to its in-process path when [`Self::is_hosted`] is `false`, so
/// app-bound sessions behave exactly as before.
pub trait SshHostedAccess: Send + Sync {
    /// Whether `session_id` is owned by the detached session host.
    fn is_hosted(&self, session_id: &str) -> bool;

    fn exec<'a>(
        &'a self,
        session_id: SshSessionId,
        command: String,
        stdin: Option<Vec<u8>>,
    ) -> BoxFuture<'a, Result<SshExecOutput, String>>;

    /// Type `data` into the interactive PTY of a hosted session (the MCP bridge).
    fn write_pty<'a>(
        &'a self,
        session_id: SshSessionId,
        data: String,
    ) -> BoxFuture<'a, Result<(), String>>;

    /// Live PTY output of a hosted session (the MCP `agent_tap`).
    fn agent_tap(&self, session_id: &str) -> Option<tokio::sync::broadcast::Receiver<String>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshTestResult {
    Success,
    UnknownHostKey { fingerprint: String },
    HostKeyChanged { fingerprint: String },
}

pub trait SshConnectionTester: Send + Sync {
    fn test<'a>(
        &'a self,
        host_id: String,
        passphrase: Option<String>,
        password_override: Option<String>,
        connect_timeout_secs: Option<u64>,
    ) -> BoxFuture<'a, Result<SshTestResult, LabonairError>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshConfigEntry {
    pub alias: String,
    pub host_address: String,
    pub port: i64,
    pub username: Option<String>,
    pub auth_method: String,
    pub private_key_path: Option<String>,
    pub proxy_jump: Option<String>,
    /// Raw `LocalForward` / `RemoteForward` / `DynamicForward` lines.
    #[serde(default)]
    pub forwards: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportConflict {
    #[default]
    Skip,
    Overwrite,
    Rename,
}

pub trait SshConfigService: Send + Sync {
    fn parse<'a>(&'a self) -> BoxFuture<'a, Result<Vec<SshConfigEntry>, String>>;
    fn import<'a>(
        &'a self,
        entries: Vec<SshConfigEntry>,
        conflict: ImportConflict,
    ) -> BoxFuture<'a, Result<Vec<String>, String>>;
    fn export<'a>(&'a self, host_ids: Vec<String>) -> BoxFuture<'a, Result<String, String>>;
    fn write_export<'a>(
        &'a self,
        block: String,
        append: bool,
    ) -> BoxFuture<'a, Result<String, String>>;
}

/// Opaque id of a port forward.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ForwardId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForwardKind {
    #[default]
    Local,
    Remote,
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardSpec {
    pub id: ForwardId,
    pub host_id: String,
    pub kind: ForwardKind,
    pub name: Option<String>,
    /// `listen_port == 0` picks a free port.
    pub listen_host: String,
    pub listen_port: u16,
    /// Dynamic: empty / 0.
    pub target_host: String,
    pub target_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForwardLifetime {
    WithConnection,
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForwardOrigin {
    App,
    Daemon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForwardState {
    Starting,
    Active,
    Reconnecting { attempt: u32 },
    AwaitingInput(PendingPrompt),
    Failed { reason: String },
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardInfo {
    pub spec: ForwardSpec,
    pub state: ForwardState,
    pub lifetime: ForwardLifetime,
    pub origin: ForwardOrigin,
    pub bound_port: Option<u16>,
    pub connections: u32,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub saved: bool,
}

impl ForwardInfo {
    /// True while the forward holds or is acquiring resources (not failed/stopped).
    pub fn is_running(&self) -> bool {
        matches!(
            self.state,
            ForwardState::Starting
                | ForwardState::Active
                | ForwardState::Reconnecting { .. }
                | ForwardState::AwaitingInput(_)
        )
    }

    /// `host:port` the forward listens on, using the bound port when known.
    /// Remote forwards listen on the server, hence no scheme either way.
    pub fn address_label(&self) -> String {
        let port = self.bound_port.unwrap_or(self.spec.listen_port);
        let host = &self.spec.listen_host;
        if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardOpenRequest {
    pub spec: ForwardSpec,
    pub lifetime: ForwardLifetime,
}

pub trait SshForwardService: Send + Sync {
    fn open<'a>(&'a self, request: ForwardOpenRequest) -> BoxFuture<'a, Result<ForwardId, String>>;
    fn close<'a>(&'a self, id: ForwardId) -> BoxFuture<'a, Result<(), String>>;
    fn restart<'a>(&'a self, id: ForwardId) -> BoxFuture<'a, Result<(), String>>;
    fn answer_prompt<'a>(
        &'a self,
        id: ForwardId,
        request_id: PromptRequestId,
        answer: PromptAnswer,
    ) -> BoxFuture<'a, Result<(), String>>;
    /// A connection for the host started: refcount++, start saved auto-start forwards.
    fn attach_host<'a>(&'a self, host_id: String) -> BoxFuture<'a, Result<(), String>>;
    /// A connection ended: refcount--; at 0 close `WithConnection` forwards.
    fn detach_host<'a>(&'a self, host_id: String) -> BoxFuture<'a, Result<(), String>>;
    /// Sync, cheap, cached for the daemon proxy.
    fn list(&self) -> Vec<ForwardInfo>;
    /// Bumps on every change.
    fn revision(&self) -> tokio::sync::watch::Receiver<u64>;
    /// Enable or disable automatic reconnect of dropped forwards
    /// (`forwards.autoReconnect`). Default no-op for services without a policy.
    fn set_reconnect_policy(&self, _enabled: bool) {}
}

/// Session id of the `n`-th (1-based, outermost = 1) jump hop of `base`.
pub fn hop_session_id(base: &str, n: usize) -> String {
    format!("{base}_jump{n}")
}

/// Splits a trailing `_jump<digits>` hop suffix off `id` (last suffix only).
pub fn split_hop_session_id(id: &str) -> (&str, Option<usize>) {
    if let Some(pos) = id.rfind("_jump") {
        let digits = &id[pos + 5..];
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(n) = digits.parse() {
                return (&id[..pos], Some(n));
            }
        }
    }
    (id, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_info_helpers() {
        let mut info = ForwardInfo {
            spec: ForwardSpec {
                id: ForwardId("f".into()),
                host_id: "h".into(),
                kind: ForwardKind::Local,
                name: None,
                listen_host: "127.0.0.1".into(),
                listen_port: 0,
                target_host: "db".into(),
                target_port: 5432,
            },
            state: ForwardState::Active,
            lifetime: ForwardLifetime::Pinned,
            origin: ForwardOrigin::App,
            bound_port: Some(4000),
            connections: 0,
            bytes_in: 0,
            bytes_out: 0,
            saved: false,
        };
        assert!(info.is_running());
        assert_eq!(info.address_label(), "127.0.0.1:4000");
        info.spec.listen_host = "::1".into();
        assert_eq!(info.address_label(), "[::1]:4000");
        info.state = ForwardState::Failed { reason: "x".into() };
        assert!(!info.is_running());
        assert_eq!(
            serde_json::to_string(&ForwardKind::Dynamic).unwrap(),
            "\"dynamic\""
        );
    }

    #[test]
    fn session_ids_are_opaque_and_round_trip() {
        let id = SshSessionId::new("tab-1");
        assert_eq!(id.as_str(), "tab-1");
        assert_eq!(String::from(id), "tab-1");
    }

    #[test]
    fn config_conflict_defaults_to_skip() {
        assert_eq!(ImportConflict::default(), ImportConflict::Skip);
    }

    #[test]
    fn connection_events_keep_authentication_payloads_typed() {
        let event = SshConnectionEvent::AuthRequired {
            session_id: "session-1".to_string(),
            prompt_message: "Password".to_string(),
            is_2fa: false,
        };
        assert_eq!(
            event,
            SshConnectionEvent::AuthRequired {
                session_id: "session-1".to_string(),
                prompt_message: "Password".to_string(),
                is_2fa: false,
            }
        );
    }

    #[test]
    fn prompt_answer_debug_is_redacted() {
        let dbg = format!("{:?}", PromptAnswer::Secret(Some("hunter2".into())));
        assert!(!dbg.contains("hunter2"));
        assert_eq!(dbg, "Secret(<redacted>)");
        assert_eq!(format!("{:?}", PromptAnswer::Trust(true)), "Trust(true)");
    }

    #[test]
    fn hop_session_ids_round_trip() {
        assert_eq!(hop_session_id("tab", 2), "tab_jump2");
        assert_eq!(split_hop_session_id("tab_jump2"), ("tab", Some(2)));
        assert_eq!(split_hop_session_id("tab"), ("tab", None));
        assert_eq!(split_hop_session_id("tab_jump"), ("tab_jump", None));
        assert_eq!(split_hop_session_id("tab_jumpx"), ("tab_jumpx", None));
        assert_eq!(split_hop_session_id("a_jump1_jump2"), ("a_jump1", Some(2)));
    }
}
