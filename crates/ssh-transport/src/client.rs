// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

use super::prompts::{HopPromptCtx, HopPrompts};
use labonair_errors::LabonairError;
use labonair_events::{EventBus, EventChannel};
use labonair_ssh::{HopInfo, PromptAnswer, PromptKind};
use std::sync::Arc;
use std::time::Duration;

macro_rules! log_step {
    ($events:expr, $session_id:expr, $msg:expr) => {
        let _ = $events.emit(
            "ssh_connect_log",
            serde_json::json!({ "session_id": $session_id, "message": $msg }),
        );
    };
}

/// Object-safety helper so a direct `tokio::net::TcpStream` and a
/// jump-bridged `russh::ChannelStream` (different concrete types) can both be
/// handed to `russh::client::connect_stream` through one boxed value.
trait AsyncStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> AsyncStream for T {}

/// Error type for `ClientHandler`. Wraps `russh::Error` (satisfying the
/// `Handler::Error: From<russh::Error>` bound) plus an `Other` variant for
/// our own host-trust/rejection messages, so `check_server_key` can surface a
/// precise, user-facing reason instead of the generic error russh would
/// otherwise produce when a check simply returns `Ok(false)`.
#[derive(Debug)]
pub enum ClientHandlerError {
    Russh(russh::Error),
    Other(String),
}

impl std::fmt::Display for ClientHandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientHandlerError::Russh(e) => write!(f, "{e}"),
            ClientHandlerError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for ClientHandlerError {}

impl From<russh::Error> for ClientHandlerError {
    fn from(e: russh::Error) -> Self {
        ClientHandlerError::Russh(e)
    }
}

/// Result of checking a server's host key against `known_hosts`. Direct
/// analog of the previous SSH library's host-key check-result type, minus
/// the `Failure` variant (folded into `NotFound` here, matching how this
/// code already treated the two identically before this migration).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnownHostStatus {
    Match,
    Mismatch,
    NotFound,
}

/// Checks `pubkey` against `known_hosts_path` for `host_address:port`.
/// Preserves the pre-migration lookup convention: check the port-qualified name
/// first (bare hostname if port==22, else `[host]:port` — this is exactly
/// what `russh::keys::check_known_hosts_path` does internally), then fall
/// back to the bare hostname if that lookup finds nothing and port != 22
/// (forced by re-querying with port=22, which makes the helper look up the
/// bare host). A `KeyChanged` error at either step means a mismatch; any
/// other error is treated as not-found, same as today.
fn check_known_host(
    known_hosts_path: &Option<std::path::PathBuf>,
    host_address: &str,
    port: i64,
    pubkey: &russh::keys::PublicKey,
) -> KnownHostStatus {
    let Some(path) = known_hosts_path else {
        return KnownHostStatus::NotFound;
    };
    if !path.exists() {
        return KnownHostStatus::NotFound;
    }
    match russh::keys::check_known_hosts_path(host_address, port as u16, pubkey, path) {
        Ok(true) => KnownHostStatus::Match,
        Ok(false) => {
            if port != 22 {
                match russh::keys::check_known_hosts_path(host_address, 22, pubkey, path) {
                    Ok(true) => KnownHostStatus::Match,
                    Ok(false) => KnownHostStatus::NotFound,
                    Err(russh::keys::Error::KeyChanged { .. }) => KnownHostStatus::Mismatch,
                    Err(_) => KnownHostStatus::NotFound,
                }
            } else {
                KnownHostStatus::NotFound
            }
        }
        Err(russh::keys::Error::KeyChanged { .. }) => KnownHostStatus::Mismatch,
        Err(_) => KnownHostStatus::NotFound,
    }
}

/// MD5 hex-colon fingerprint over a public key's encoded wire bytes — the
/// same format the previous SSH library's MD5 host-key hash produced.
/// `ssh_key`'s `HashAlg` has no MD5 variant, so this is computed manually
/// (the `md5` crate is already a dependency) to preserve the exact string
/// format users see today.
fn md5_fingerprint(key_bytes: &[u8]) -> String {
    let digest = md5::compute(key_bytes);
    digest
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":")
}

/// Implements `russh::client::Handler` — the host-key verification hook,
/// plus everything else the trait requires via its no-op defaults. One
/// instance is constructed per connection attempt (main session or jump
/// session), carrying just enough context to run the known-hosts/trust flow
/// for that one connection.
pub struct ClientHandler {
    session_id: String,
    events: EventBus,
    trust_state: super::TrustState,
    fail_fast_untrusted_host: bool,
    host_address: String,
    port: i64,
    /// Shared with the owning `RushSession` (when one exists) so the real
    /// disconnect cause is available to whoever is blocked reading from this
    /// session's channels — see `RushSession::disconnect_reason`'s doc
    /// comment. Connections that have no such consumer (jump-host hops,
    /// tunnels) pass a throwaway `Arc` that nothing ever reads.
    disconnect_reason: Arc<std::sync::Mutex<Option<String>>>,
    /// Set for jump hops of an interactive connect: unknown/mismatched host
    /// keys are asked through the prompt broker instead of failing fast.
    interactive_hop: Option<HopPromptCtx>,
    /// Woken when the transport goes down (port-forward reconnect).
    disconnect_notify: Option<Arc<tokio::sync::Notify>>,
    /// Routes server-initiated `forwarded-tcpip` channels (remote forwards).
    remote_dispatch: Option<super::forwards::RemoteForwardDispatch>,
}

impl ClientHandler {
    /// Asks the user whether to trust `fingerprint`. Interactive hops use the
    /// prompt broker; main hops use the `known_hosts_warning` + `TrustState` flow.
    async fn confirm_host_key(
        &self,
        fingerprint: &str,
        is_mismatch: bool,
    ) -> Result<bool, ClientHandlerError> {
        if let Some(ctx) = &self.interactive_hop {
            let answer = ctx
                .ask(PromptKind::HostKey {
                    host: self.host_address.clone(),
                    fingerprint: fingerprint.to_string(),
                    is_mismatch,
                })
                .await;
            return Ok(matches!(answer, Some(PromptAnswer::Trust(true))));
        }
        self.events
            .emit(
                "known_hosts_warning",
                serde_json::json!({
                    "session_id": self.session_id, "fingerprint": fingerprint,
                    "host": self.host_address, "is_mismatch": is_mismatch
                }),
            )
            .map_err(|e| ClientHandlerError::Other(e.to_string()))?;
        Ok(wait_for_trust(&self.session_id, &self.trust_state).await)
    }
}

impl russh::client::Handler for ClientHandler {
    type Error = ClientHandlerError;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        log_step!(self.events, self.session_id, "Verifying host fingerprint…");

        let key_bytes = server_public_key
            .to_bytes()
            .map_err(|e| ClientHandlerError::Other(format!("failed to encode host key: {e}")))?;
        let fingerprint = md5_fingerprint(&key_bytes);

        let known_hosts_path = dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts"));
        // Only needed for the mismatch-cleanup path below — the actual
        // check/learn calls let russh's known_hosts helpers derive this
        // lookup name themselves.
        let known_host_check_name = if self.port == 22 {
            self.host_address.clone()
        } else {
            format!("[{}]:{}", self.host_address, self.port)
        };

        let status = check_known_host(
            &known_hosts_path,
            &self.host_address,
            self.port,
            server_public_key,
        );

        match status {
            KnownHostStatus::Match => {
                log_step!(self.events, self.session_id, "Host fingerprint verified ✓");
                Ok(true)
            }
            KnownHostStatus::Mismatch => {
                log_step!(
                    self.events,
                    self.session_id,
                    format!("Host key mismatch! Fingerprint: {}", fingerprint)
                );
                if self.fail_fast_untrusted_host && self.interactive_hop.is_none() {
                    return Err(ClientHandlerError::Other(format!(
                        "host key mismatch for {} — fingerprint: {} — open a terminal tab to this host to verify and trust its new fingerprint",
                        self.host_address, fingerprint
                    )));
                }
                if !self.confirm_host_key(&fingerprint, true).await? {
                    return Err(ClientHandlerError::Other(
                        if self.interactive_hop.is_some() {
                            "host key rejected"
                        } else {
                            "User rejected host"
                        }
                        .to_string(),
                    ));
                }
                if let Some(ref path) = known_hosts_path {
                    drop_known_host_entry(path, &known_host_check_name);
                    drop_known_host_entry(path, &self.host_address);
                    let _ = russh::keys::known_hosts::learn_known_hosts_path(
                        &self.host_address,
                        self.port as u16,
                        server_public_key,
                        path,
                    );
                }
                log_step!(
                    self.events,
                    self.session_id,
                    "Host key accepted and updated in known_hosts ✓"
                );
                Ok(true)
            }
            KnownHostStatus::NotFound => {
                log_step!(
                    self.events,
                    self.session_id,
                    format!("Unknown host — fingerprint: {}", fingerprint)
                );
                if self.fail_fast_untrusted_host && self.interactive_hop.is_none() {
                    return Err(ClientHandlerError::Other(format!(
                        "host key not yet trusted for {} — fingerprint: {} — open a terminal tab to this host to verify and trust its fingerprint",
                        self.host_address, fingerprint
                    )));
                }
                if !self.confirm_host_key(&fingerprint, false).await? {
                    return Err(ClientHandlerError::Other(
                        if self.interactive_hop.is_some() {
                            "host key rejected"
                        } else {
                            "User rejected host"
                        }
                        .to_string(),
                    ));
                }
                if let Some(ref path) = known_hosts_path {
                    let _ = russh::keys::known_hosts::learn_known_hosts_path(
                        &self.host_address,
                        self.port as u16,
                        server_public_key,
                        path,
                    );
                }
                log_step!(
                    self.events,
                    self.session_id,
                    "Host trusted and added to known_hosts ✓"
                );
                Ok(true)
            }
        }
    }

    /// Captures the real reason the transport went down — a server-sent
    /// SSH_MSG_DISCONNECT, or the underlying I/O/protocol error — into the
    /// shared `disconnect_reason` slot so a blocked `ChannelReadHalf::wait()`
    /// caller (which otherwise only sees `None`/`Eof` with no error info) can
    /// report something more useful than a generic fallback string.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        if let Some(d) = &self.remote_dispatch {
            d.serve(
                connected_address,
                connected_port,
                Box::new(channel.into_stream()),
            );
        }
        Ok(())
    }

    async fn disconnected(
        &mut self,
        reason: russh::client::DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        let text = match &reason {
            russh::client::DisconnectReason::ReceivedDisconnect(info) => {
                if info.message.is_empty() {
                    format!("server disconnected: {:?}", info.reason_code)
                } else {
                    format!("server disconnected: {}", info.message)
                }
            }
            russh::client::DisconnectReason::Error(e) => e.to_string(),
        };
        if let Ok(mut slot) = self.disconnect_reason.lock() {
            *slot = Some(text);
        }
        if let Some(n) = &self.disconnect_notify {
            // A stored permit, so a late `closed()` still resolves.
            n.notify_one();
        }
        match reason {
            russh::client::DisconnectReason::ReceivedDisconnect(_) => Ok(()),
            russh::client::DisconnectReason::Error(e) => Err(e),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn ssh_connect(
    session_id: String,
    host_id: String,
    passphrase: Option<String>,
    password_override: Option<String>,
    initial_cols: Option<u32>,
    initial_rows: Option<u32>,
    blocks: bool,
    on_event: EventChannel<super::pty::SshPtyEvent>,
    state: &super::SshState,
    trust_state: &super::TrustState,
    hosts_db: &labonair_persistence::Database,
    secrets: &labonair_secrets::SecretsState,
    events: EventBus,
    connect_timeout_secs: Option<u64>,
    default_keepalive_interval_secs: Option<u64>,
    default_keepalive_max_failures: Option<u32>,
    prompts: Option<HopPrompts>,
    route_override: Option<Vec<String>>,
) -> Result<(), LabonairError> {
    // Step 1: Fetch host from SQLite (fast, sync — do before spawn_blocking)
    log_step!(events, session_id, "Reading host configuration…");
    let (
        host_address,
        port,
        username,
        auth_method,
        private_key_path,
        keep_alive_interval,
        keep_alive_tries,
        default_path_ssh,
        credential_id,
        jump_host_id,
    ) = {
        let conn = hosts_db
            .0
            .lock()
            .map_err(|e| LabonairError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT host_address, port, username, auth_method, private_key_path, \
                 keep_alive_interval, keep_alive_tries, default_path_ssh, credential_id, jump_host_id \
                 FROM hosts WHERE id = ?1",
            )?;
        stmt.query_row(rusqlite::params![host_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
            ))
        })?
    };

    // Step 1b: Resolve credential — if the host references a credential, override auth fields.
    let (auth_method, private_key_path) = if let Some(cid) = &credential_id {
        log_step!(events, session_id, "Resolving credential…");
        let (cred_type, cred_key_path, cred_has_secret): (String, Option<String>, bool) = {
            let conn = hosts_db
                .0
                .lock()
                .map_err(|e| LabonairError::Internal(e.to_string()))?;
            conn.query_row(
                "SELECT cred_type, key_path, has_secret FROM credentials WHERE id=?1",
                rusqlite::params![cid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2).map(|v| v != 0).unwrap_or(false))),
            )
            .map_err(|_| LabonairError::Internal(format!("Credential '{}' not found — it may have been deleted. Please update the host's auth settings.", cid)))?
        };
        let _ = cred_has_secret; // used below for password fetch
        (cred_type, cred_key_path)
    } else {
        (auth_method, private_key_path)
    };

    // Step 2: Fetch password — use override if provided, else local store.
    // When using a credential, fetch the credential's secret instead of the host's.
    let password: Option<String> = if auth_method == "password" {
        if password_override.is_some() {
            password_override.clone()
        } else {
            log_step!(
                events,
                session_id,
                "Retrieving credentials from local store…"
            );
            if let Some(cid) = &credential_id {
                labonair_secrets::get_password(secrets, "labonair-cred", cid)
                    .ok()
                    .flatten()
            } else {
                labonair_secrets::get_password(secrets, "labonair-app", &host_id)
                    .ok()
                    .flatten()
            }
        }
    } else {
        None
    };

    // For key auth via credential, the passphrase may be stored in the credential's secret.
    let passphrase = if credential_id.is_some() && auth_method == "key" && passphrase.is_none() {
        if let Some(cid) = &credential_id {
            labonair_secrets::get_password(secrets, "labonair-cred", cid)
                .ok()
                .flatten()
        } else {
            passphrase
        }
    } else {
        passphrase
    };

    // Step 3: Resolve jump host fields (if any).
    let jump = if jump_host_id.is_some() || route_override.is_some() {
        log_step!(events, session_id, "Resolving jump route…");
        resolve_jump_chain(hosts_db, secrets, &host_id, route_override.as_deref())?
    } else {
        Vec::new()
    };

    // A host record's own keep-alive fields win; a global Settings default
    // (`ConnectionsSettings`) only fills in what the host record left unset.
    let keep_alive_interval =
        keep_alive_interval.or(default_keepalive_interval_secs.map(|v| v as i64));
    let keep_alive_tries = keep_alive_tries.or(default_keepalive_max_failures.map(|v| v as i64));

    let state_inner = state.clone();
    let trust_inner = trust_state.clone();
    let cols = initial_cols.unwrap_or(220);
    let rows = initial_rows.unwrap_or(50);
    let result = ssh_connect_async(
        session_id,
        host_id.clone(),
        passphrase,
        host_address,
        port,
        username,
        auth_method,
        private_key_path,
        keep_alive_interval,
        keep_alive_tries,
        default_path_ssh,
        password,
        cols,
        rows,
        jump,
        blocks,
        state_inner,
        trust_inner,
        events.clone(),
        on_event,
        connect_timeout_secs,
        prompts,
    )
    .await;

    if result.is_ok() {
        // Update last_connected_at on the DB thread after successful connect.
        let conn = hosts_db
            .0
            .lock()
            .map_err(|e| LabonairError::Internal(e.to_string()))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let _ = conn.execute(
            "UPDATE hosts SET last_connected_at = ?1 WHERE id = ?2",
            rusqlite::params![now, host_id],
        );
    }

    result.map_err(classify_ssh_error)
}

/// Maps a string error from ssh_connect_async to a structured LabonairError
/// variant. Delegates to the shared [`LabonairError::classify`] catalog.
fn classify_ssh_error(s: String) -> LabonairError {
    LabonairError::classify(s)
}

/// Quick-connect: initiate an SSH connection without a saved host record.
/// Password is provided directly; no keyring lookup.
#[allow(clippy::too_many_arguments)]
pub async fn ssh_connect_quick(
    session_id: String,
    username: String,
    host_address: String,
    port: u16,
    password: String,
    passphrase: Option<String>,
    initial_cols: Option<u32>,
    initial_rows: Option<u32>,
    blocks: bool,
    on_event: EventChannel<super::pty::SshPtyEvent>,
    state: &super::SshState,
    trust_state: &super::TrustState,
    events: EventBus,
    connect_timeout_secs: Option<u64>,
) -> Result<(), String> {
    let state_inner = state.clone();
    let trust_inner = trust_state.clone();
    let cols = initial_cols.unwrap_or(220);
    let rows = initial_rows.unwrap_or(50);
    ssh_connect_async(
        session_id,
        String::new(), // no host_id — quick connect has no DB record
        passphrase,
        host_address,
        port as i64,
        username,
        "password".to_string(),
        None,
        None,
        None,
        None,
        Some(password),
        cols,
        rows,
        // No jump host for quick connect — no host_id/DB record to resolve one from.
        Vec::new(),
        blocks,
        state_inner,
        trust_inner,
        events,
        on_event,
        connect_timeout_secs,
        None,
    )
    .await
}

/// Signal the backend to accept or reject a host's fingerprint.
/// Called by the frontend after the user acts on the trust dialog.
pub async fn ssh_trust_host(
    session_id: String,
    accepted: bool,
    trust_state: &super::TrustState,
) -> Result<(), String> {
    let sender = {
        let mut map = trust_state.0.lock().map_err(|e| e.to_string())?;
        map.remove(&session_id)
    };
    if let Some(tx) = sender {
        let _ = tx.send(accepted);
    }
    Ok(())
}

/// Remove all known_hosts entries for a given host address.
/// Used when a host key mismatch is confirmed by the user.
pub async fn ssh_remove_known_host(host_address: String) -> Result<(), String> {
    let path = dirs::home_dir()
        .map(|h| h.join(".ssh").join("known_hosts"))
        .ok_or_else(|| "Cannot determine home directory".to_string())?;

    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                return true;
            }
            // Each line: "hostname[,hostname2] key-type base64-key [comment]"
            // Reject lines whose first field matches the host address.
            let first_field = trimmed.split_whitespace().next().unwrap_or("");
            !first_field.split(',').any(|h| h == host_address)
        })
        .collect();
    let new_content = filtered.join("\n") + "\n";
    std::fs::write(&path, new_content).map_err(|e| e.to_string())?;
    Ok(())
}

/// Waits for the user to accept or reject an unknown/changed host key, or for
/// the 5-minute dialog timeout to expire. Always removes the pending entry
/// from `TrustState` before returning, so the map never leaks even if the
/// dialog is dismissed without a response. Returns `true` = trusted, `false`
/// = rejected or timed out.
async fn wait_for_trust(session_id: &str, trust_state: &super::TrustState) -> bool {
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    {
        let Ok(mut map) = trust_state.0.lock() else {
            return false;
        };
        map.insert(session_id.to_string(), tx);
    }

    let trusted = matches!(
        tokio::time::timeout(Duration::from_secs(300), rx).await,
        Ok(Ok(true))
    );

    // Always clean up — no leaks regardless of how we exit.
    let _ = trust_state.0.lock().map(|mut m| m.remove(session_id));

    trusted
}

/// Like `establish_authenticated_session` but accepts an already-connected
/// async stream (e.g. a jump-host bridge's `ChannelStream`).
///
/// `fail_fast_untrusted_host`: when true, an unknown/mismatched host key
/// returns an error immediately instead of waiting on the trust dialog.
/// Used by background sessions that have no trust-prompt UI of their own
/// (the sidebar Explorer's lazy SFTP session) — without this they'd hang
/// for up to 5 minutes on first connect to a not-yet-trusted host. The
/// interactive terminal/SFTP-tab connect flow always passes `false` for its
/// *main* hop since it owns a real trust dialog (`SshLoadingScreen`). A jump
/// hop (see `connect_via_jump`) always passes `true` regardless of the main
/// hop's policy — it has no trust-prompt UI of its own either way.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn establish_authenticated_session_from_stream<R>(
    session_id: &str,
    tcp: R,
    host_address: &str,
    port: i64,
    username: &str,
    auth_method: &str,
    private_key_path: Option<&str>,
    keep_alive_interval: Option<i64>,
    keep_alive_tries: Option<i64>,
    password: Option<&str>,
    passphrase: Option<&str>,
    trust_state: &super::TrustState,
    events: &EventBus,
    fail_fast_untrusted_host: bool,
    disconnect_reason: Arc<std::sync::Mutex<Option<String>>>,
    interactive_hop: Option<HopPromptCtx>,
    disconnect_notify: Option<Arc<tokio::sync::Notify>>,
    remote_dispatch: Option<super::forwards::RemoteForwardDispatch>,
) -> Result<Arc<russh::client::Handle<ClientHandler>>, String>
where
    R: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    log_step!(events, session_id, "TCP connection established.");

    // SSH handshake
    log_step!(events, session_id, "Starting SSH handshake…");
    let effective_interval = keep_alive_interval.unwrap_or(25i64) as u64;
    let effective_tries = keep_alive_tries.unwrap_or(3).max(0) as usize;
    let config = Arc::new(russh::client::Config {
        keepalive_interval: Some(Duration::from_secs(effective_interval)),
        keepalive_max: effective_tries,
        ..Default::default()
    });

    let handler = ClientHandler {
        session_id: session_id.to_string(),
        events: events.clone(),
        trust_state: trust_state.clone(),
        fail_fast_untrusted_host,
        host_address: host_address.to_string(),
        port,
        disconnect_reason,
        interactive_hop: interactive_hop.clone(),
        disconnect_notify,
        remote_dispatch,
    };

    let mut handle = russh::client::connect_stream(config, tcp, handler)
        .await
        .map_err(|e| e.to_string())?;
    log_step!(events, session_id, "SSH handshake complete.");

    // Authentication
    log_step!(events, session_id, "Authenticating…");
    let authenticated = if auth_method == "key" {
        let key_path = private_key_path
            .map(std::path::Path::new)
            .ok_or("private_key_path not set for key auth")?;
        if !key_path.exists() {
            return Err(format!(
                "Private key file not found: {}",
                key_path.display()
            ));
        }

        let agent_ok = try_agent_auth(&mut handle, username, session_id, events).await;
        if agent_ok {
            true
        } else {
            let pem = std::fs::read_to_string(key_path)
                .map_err(|e| format!("Failed to read private key file: {e}"))?;
            log_step!(events, session_id, "Authenticating with public key file…");
            // `russh::keys::Error::KeyIsEncrypted` is only ever raised by the
            // legacy OpenSSH-format and PKCS#5 ("-----BEGIN RSA PRIVATE
            // KEY-----" with a DEK-Info header) decoders — never by the
            // PKCS#8 path, which is what `credential_generate_keypair`
            // actually produces (`-----BEGIN ENCRYPTED PRIVATE KEY-----`).
            // Detect that format directly from the PEM header so a missing
            // or wrong passphrase on one of this app's own generated keys
            // still routes through the passphrase prompt instead of a
            // generic auth-failed error.
            let is_pkcs8_encrypted = pem.contains("-----BEGIN ENCRYPTED PRIVATE KEY-----");
            // Interactive hops: stored passphrase first, then up to 3 prompts.
            let mut pass = passphrase.map(str::to_string);
            if let Some(ctx) = &interactive_hop {
                let mut asked = 0;
                while let Err(e) = russh::keys::decode_secret_key(&pem, pass.as_deref()) {
                    if !(matches!(e, russh::keys::Error::KeyIsEncrypted) || is_pkcs8_encrypted) {
                        break; // not a passphrase problem; the match below reports it
                    }
                    if asked == 3 {
                        return Err("passphrase rejected".to_string());
                    }
                    asked += 1;
                    log_step!(events, session_id, "Key is passphrase-protected, asking…");
                    match ctx.ask(PromptKind::Passphrase).await {
                        Some(PromptAnswer::Secret(Some(s))) => pass = Some(s),
                        _ => return Err("passphrase prompt cancelled".to_string()),
                    }
                }
            }
            match russh::keys::decode_secret_key(&pem, pass.as_deref()) {
                Ok(key_pair) => {
                    let hash_alg = handle
                        .best_supported_rsa_hash()
                        .await
                        .ok()
                        .flatten()
                        .flatten();
                    let key = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg);
                    match handle.authenticate_publickey(username, key).await {
                        Ok(res) if res.success() => true,
                        other => {
                            let msg = match other {
                                Err(e) => e.to_string(),
                                Ok(_) => "authentication failed".to_string(),
                            };
                            log_step!(events, session_id, format!("Key auth failed: {}", msg));
                            events.emit("auth_required", serde_json::json!({
                                "session_id": session_id, "prompt_message": msg, "is_2fa": false
                            })).map_err(|e| e.to_string())?;
                            return Err(format!("authentication failed: {}", msg));
                        }
                    }
                }
                // The key is encrypted and no passphrase was supplied — prompt
                // for one instead of surfacing a generic auth failure. If a
                // passphrase WAS supplied and is simply wrong, russh reports
                // this the same way (it can't distinguish "wrong passphrase"
                // from "encrypted, no passphrase given" at decode time), so
                // it also lands here — the frontend re-prompts either way.
                // The PKCS#8-encrypted case (see `is_pkcs8_encrypted` above)
                // gets the same treatment for any decode failure, since it
                // never surfaces as `KeyIsEncrypted` at all.
                Err(russh::keys::Error::KeyIsEncrypted) => {
                    log_step!(
                        events,
                        session_id,
                        "Key is passphrase-protected, prompting…"
                    );
                    events
                        .emit(
                            "passphrase_required",
                            serde_json::json!({ "session_id": session_id }),
                        )
                        .map_err(|e| e.to_string())?;
                    return Err("passphrase_required".to_string());
                }
                Err(e) if is_pkcs8_encrypted => {
                    log_step!(
                        events,
                        session_id,
                        "Key is passphrase-protected, prompting…"
                    );
                    let _ = e; // decode failure on an encrypted PKCS#8 key means missing/wrong passphrase
                    events
                        .emit(
                            "passphrase_required",
                            serde_json::json!({ "session_id": session_id }),
                        )
                        .map_err(|e| e.to_string())?;
                    return Err("passphrase_required".to_string());
                }
                Err(e) => {
                    let msg = e.to_string();
                    log_step!(events, session_id, format!("Key auth failed: {}", msg));
                    events
                        .emit(
                            "auth_required",
                            serde_json::json!({
                                "session_id": session_id, "prompt_message": msg, "is_2fa": false
                            }),
                        )
                        .map_err(|e| e.to_string())?;
                    return Err(format!("authentication failed: {}", msg));
                }
            }
        }
    } else if let Some(ctx) = &interactive_hop {
        // Stored password first; only a missing/wrong one prompts (max 3).
        let mut pw = password.map(str::to_string);
        let mut asked = 0;
        loop {
            if let Some(p) = pw.take() {
                match handle.authenticate_password(username, &p).await {
                    Ok(res) if res.success() => break true,
                    Ok(_) => {}
                    Err(e) => return Err(format!("authentication failed: {e}")),
                }
            }
            if asked == 3 {
                return Err("authentication failed: password rejected".to_string());
            }
            asked += 1;
            match ctx.ask(PromptKind::Password).await {
                Some(PromptAnswer::Secret(Some(s))) => pw = Some(s),
                _ => return Err("password prompt cancelled".to_string()),
            }
        }
    } else {
        let pw = password.unwrap_or("");
        match handle.authenticate_password(username, pw).await {
            Ok(res) if res.success() => true,
            other => {
                let msg = match other {
                    Err(e) => e.to_string(),
                    Ok(_) => "authentication failed".to_string(),
                };
                log_step!(events, session_id, format!("Password auth failed: {}", msg));
                events
                    .emit(
                        "auth_required",
                        serde_json::json!({
                            "session_id": session_id, "prompt_message": msg, "is_2fa": false
                        }),
                    )
                    .map_err(|e| e.to_string())?;
                return Err(format!("authentication failed: {}", msg));
            }
        }
    };

    if !authenticated {
        log_step!(events, session_id, "Authentication failed.");
        events.emit(
            "auth_required",
            serde_json::json!({
                "session_id": session_id, "prompt_message": "Authentication failed", "is_2fa": false
            }),
        )
        .map_err(|e| e.to_string())?;
        return Err("not authenticated".to_string());
    }

    log_step!(events, session_id, "Authenticated ✓");
    Ok(Arc::new(handle))
}

/// Maximum number of jump hops in a route (ADR 0016).
pub const MAX_JUMP_HOPS: usize = 8;

/// Why a jump route could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JumpRouteError {
    Cycle,
    TooDeep,
    Missing(String),
    SelfJump,
}

impl std::fmt::Display for JumpRouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cycle => write!(f, "Jump route contains a cycle"),
            Self::TooDeep => write!(f, "Jump route is deeper than {MAX_JUMP_HOPS} hops"),
            Self::Missing(id) => write!(f, "Jump host '{id}' not found"),
            Self::SelfJump => write!(f, "Jump host cannot be the same as the target host"),
        }
    }
}

impl std::error::Error for JumpRouteError {}

/// Follows `parent_of` from `start` and returns the ancestor ids outermost
/// first, excluding `start`.
pub fn route_ids(
    start: &str,
    parent_of: impl Fn(&str) -> Option<String>,
) -> Result<Vec<String>, JumpRouteError> {
    let mut chain: Vec<String> = Vec::new();
    let mut cur = start.to_string();
    while let Some(parent) = parent_of(&cur) {
        if parent == start {
            return Err(if chain.is_empty() {
                JumpRouteError::SelfJump
            } else {
                JumpRouteError::Cycle
            });
        }
        if chain.contains(&parent) {
            return Err(JumpRouteError::Cycle);
        }
        if chain.len() >= MAX_JUMP_HOPS {
            return Err(JumpRouteError::TooDeep);
        }
        chain.push(parent.clone());
        cur = parent;
    }
    chain.reverse();
    Ok(chain)
}

/// Fully-resolved connection parameters for one jump hop.
pub struct JumpHostParams {
    pub host_id: String,
    /// Host `name`, falling back to `host_address`.
    pub label: String,
    pub address: String,
    pub port: i64,
    pub username: String,
    pub auth_method: String,
    pub private_key_path: Option<String>,
    pub password: Option<String>,
    pub keep_alive_interval: Option<i64>,
}

/// Validates a one-off route (outermost hop first) for `host_id`: no hop may be
/// the target (`SelfJump`) or repeat (`Cycle`), at most [`MAX_JUMP_HOPS`].
pub fn override_route_ids(host_id: &str, route: &[String]) -> Result<Vec<String>, JumpRouteError> {
    if route.len() > MAX_JUMP_HOPS {
        return Err(JumpRouteError::TooDeep);
    }
    if route.iter().any(|id| id == host_id) {
        return Err(JumpRouteError::SelfJump);
    }
    for (i, id) in route.iter().enumerate() {
        if route[..i].contains(id) {
            return Err(JumpRouteError::Cycle);
        }
    }
    Ok(route.to_vec())
}

/// Resolves the whole jump chain of `host_id` (outermost hop first; empty =
/// direct) by following `hosts.jump_host_id`, then resolves each hop's
/// credential and keyring password. `route_override` REPLACES the stored chain
/// (`Some(&[])` = direct); it is never persisted.
pub fn resolve_jump_chain(
    hosts_db: &labonair_persistence::Database,
    secrets: &labonair_secrets::SecretsState,
    host_id: &str,
    route_override: Option<&[String]>,
) -> Result<Vec<JumpHostParams>, LabonairError> {
    let parent_of = |id: &str| -> Option<String> {
        let conn = hosts_db.0.lock().ok()?;
        conn.query_row(
            "SELECT jump_host_id FROM hosts WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
    };
    let ids = match route_override {
        Some(route) => override_route_ids(host_id, route),
        None => route_ids(host_id, parent_of),
    }
    .map_err(|e| LabonairError::Internal(e.to_string()))?;
    ids.iter()
        .map(|id| resolve_jump_hop(hosts_db, secrets, id))
        .collect()
}

#[allow(clippy::type_complexity)]
fn resolve_jump_hop(
    hosts_db: &labonair_persistence::Database,
    secrets: &labonair_secrets::SecretsState,
    jump_host_id: &str,
) -> Result<JumpHostParams, LabonairError> {
    let (jh_name, jh_addr, jh_port, jh_user, jh_auth, jh_key, jh_kai, jh_cred_id): (
        Option<String>,
        String,
        i64,
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
    ) = {
        let conn = hosts_db
            .0
            .lock()
            .map_err(|e| LabonairError::Internal(e.to_string()))?;
        conn.query_row(
            "SELECT name, host_address, port, username, auth_method, private_key_path, \
             keep_alive_interval, credential_id FROM hosts WHERE id = ?1",
            rusqlite::params![jump_host_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .map_err(|_| JumpRouteError::Missing(jump_host_id.to_string()))
        .map_err(|e| LabonairError::Internal(e.to_string()))?
    };
    let label = jh_name
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| jh_addr.clone());

    // Resolve credential for jump host if needed
    let (jh_auth, jh_key) = if let Some(ref jcid) = jh_cred_id {
        let (cred_type, cred_key_path): (String, Option<String>) = {
            let conn = hosts_db
                .0
                .lock()
                .map_err(|e| LabonairError::Internal(e.to_string()))?;
            conn.query_row(
                "SELECT cred_type, key_path FROM credentials WHERE id=?1",
                rusqlite::params![jcid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| {
                LabonairError::Internal(format!("Jump host credential '{}' not found", jcid))
            })?
        };
        (cred_type, cred_key_path)
    } else {
        (jh_auth, jh_key)
    };

    // Fetch jump host password from keyring
    let jh_pw: Option<String> = if jh_auth == "password" {
        if let Some(ref jcid) = jh_cred_id {
            labonair_secrets::get_password(secrets, "labonair-cred", jcid)
                .ok()
                .flatten()
        } else {
            labonair_secrets::get_password(secrets, "labonair-app", jump_host_id)
                .ok()
                .flatten()
        }
    } else {
        None
    };

    Ok(JumpHostParams {
        host_id: jump_host_id.to_string(),
        label,
        address: jh_addr,
        port: jh_port,
        username: jh_user,
        auth_method: jh_auth,
        private_key_path: jh_key,
        password: jh_pw,
        keep_alive_interval: jh_kai,
    })
}

/// A jump-bridged stream that owns every intermediate hop `Handle`. Dropping
/// an intermediate handle would tear down the channel carrying the next hop,
/// so they must live exactly as long as the final stream.
struct ChainedStream {
    inner: russh::ChannelStream<russh::client::Msg>,
    _hops: Vec<Arc<russh::client::Handle<ClientHandler>>>,
}

impl tokio::io::AsyncRead for ChainedStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for ChainedStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Opens the transport stream for a connection — either a direct TCP socket,
/// or one bridged through the jump chain via `connect_via_jump`. Shared by the
/// terminal path (`ssh_connect_async`) and the SFTP/tunnels path
/// (`establish_authenticated_session`).
#[allow(clippy::too_many_arguments)]
async fn connect_transport_maybe_via_jump(
    session_id: &str,
    host_address: &str,
    port: i64,
    jump: &[JumpHostParams],
    trust_state: &super::TrustState,
    events: &EventBus,
    connect_timeout_secs: u64,
    prompts: Option<&HopPrompts>,
) -> Result<Box<dyn AsyncStream>, String> {
    if jump.is_empty() {
        return Ok(Box::new(
            tcp_connect_async(host_address, port, connect_timeout_secs).await?,
        ));
    }
    // Circular reference guard
    if jump
        .iter()
        .any(|jh| jh.address.as_str() == host_address && jh.port == port)
    {
        return Err(JumpRouteError::SelfJump.to_string());
    }
    let stream = connect_via_jump(
        session_id,
        jump,
        host_address,
        port,
        trust_state,
        events,
        connect_timeout_secs,
        prompts,
    )
    .await?;
    Ok(Box::new(stream))
}

/// Performs TCP connect (direct, or bridged through a jump host) → SSH
/// handshake → host-key check → authentication. Returns the fully
/// authenticated `Handle`. Thin wrapper around
/// `establish_authenticated_session_from_stream` that first opens the
/// transport. Used by SFTP connect and tunnels.
#[allow(clippy::too_many_arguments)]
pub async fn establish_authenticated_session_notify(
    session_id: &str,
    host_address: &str,
    port: i64,
    username: &str,
    auth_method: &str,
    private_key_path: Option<&str>,
    keep_alive_interval: Option<i64>,
    keep_alive_tries: Option<i64>,
    password: Option<&str>,
    passphrase: Option<&str>,
    trust_state: &super::TrustState,
    events: &EventBus,
    fail_fast_untrusted_host: bool,
    jump: Vec<JumpHostParams>,
    connect_timeout_secs: Option<u64>,
    disconnect_notify: Option<Arc<tokio::sync::Notify>>,
    remote_dispatch: Option<super::forwards::RemoteForwardDispatch>,
    prompts: Option<HopPrompts>,
) -> Result<Arc<russh::client::Handle<ClientHandler>>, String> {
    log_step!(
        events,
        session_id,
        format!("TCP connecting to {}:{}…", host_address, port)
    );
    let tcp = connect_transport_maybe_via_jump(
        session_id,
        host_address,
        port,
        &jump,
        trust_state,
        events,
        connect_timeout_secs.unwrap_or(10),
        prompts.as_ref(),
    )
    .await?;
    // Neither of this wrapper's callers (SFTP-only connect, tunnels) currently
    // consume a rich disconnect reason, so a throwaway slot is passed here —
    // only the interactive terminal path (which calls the `_from_stream`
    // variant directly) shares its own `RushSession.disconnect_reason`.
    establish_authenticated_session_from_stream(
        session_id,
        tcp,
        host_address,
        port,
        username,
        auth_method,
        private_key_path,
        keep_alive_interval,
        keep_alive_tries,
        password,
        passphrase,
        trust_state,
        events,
        fail_fast_untrusted_host,
        Arc::new(std::sync::Mutex::new(None)),
        // Main hop of a prompting connect (forward sessions) asks like a hop, unlabelled.
        prompts.map(|p| HopPromptCtx {
            prompts: p,
            base_session_id: session_id.to_string(),
            info: None,
        }),
        disconnect_notify,
        remote_dispatch,
    )
    .await
}

/// `establish_authenticated_session_notify` without a disconnect notifier.
#[allow(clippy::too_many_arguments)]
pub async fn establish_authenticated_session(
    session_id: &str,
    host_address: &str,
    port: i64,
    username: &str,
    auth_method: &str,
    private_key_path: Option<&str>,
    keep_alive_interval: Option<i64>,
    keep_alive_tries: Option<i64>,
    password: Option<&str>,
    passphrase: Option<&str>,
    trust_state: &super::TrustState,
    events: &EventBus,
    fail_fast_untrusted_host: bool,
    jump: Vec<JumpHostParams>,
    connect_timeout_secs: Option<u64>,
) -> Result<Arc<russh::client::Handle<ClientHandler>>, String> {
    establish_authenticated_session_notify(
        session_id,
        host_address,
        port,
        username,
        auth_method,
        private_key_path,
        keep_alive_interval,
        keep_alive_tries,
        password,
        passphrase,
        trust_state,
        events,
        fail_fast_untrusted_host,
        jump,
        connect_timeout_secs,
        None,
        None,
        None,
    )
    .await
}

/// Outcome of `ssh_test_connection`. An unknown/changed host key is reported
/// as an informational variant rather than an error — testing a connection
/// must never silently trust-on-first-use (or otherwise auto-accept) a host
/// key as a side effect; that's a deliberate, visible action the user takes
/// via the real connect flow's trust dialog.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TestConnectionResult {
    /// Transport connected and authentication succeeded.
    Success,
    /// First-time connection — this host's key isn't in `known_hosts` yet.
    UnknownHostKey { fingerprint: String },
    /// The host's key differs from the one stored in `known_hosts`.
    HostKeyChanged { fingerprint: String },
}

/// Pulls the `fingerprint: <fp>` token out of one of `check_server_key`'s
/// fail-fast error messages (see its `fail_fast_untrusted_host` branches
/// above). The fingerprint itself is colon-separated hex with no whitespace,
/// so the first whitespace-delimited token after the marker is the value.
fn extract_fingerprint(msg: &str) -> Option<String> {
    msg.split("fingerprint: ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map(|s| s.to_string())
}

/// Pure connectivity + authentication check for a saved host. Reuses the
/// exact same building blocks `ssh_connect_async` uses *before* it ever
/// opens a shell channel or registers a session — `connect_transport_maybe_via_jump`
/// then `establish_authenticated_session_from_stream` — called directly here
/// instead of through the full connect flow, so this never opens a PTY
/// channel and never inserts anything into `SshState`. On success the
/// authenticated handle is disconnected immediately; nothing usable is left
/// behind.
///
/// Always runs with `fail_fast_untrusted_host = true`: there is no
/// interactive tab/trust dialog backing a test call, so an unknown or
/// changed host key is reported back as an informational
/// `TestConnectionResult` variant instead of hanging on (or silently
/// resolving) a trust prompt.
#[allow(clippy::too_many_arguments)]
pub async fn ssh_test_connection(
    host_id: String,
    passphrase: Option<String>,
    password_override: Option<String>,
    trust_state: &super::TrustState,
    hosts_db: &labonair_persistence::Database,
    secrets: &labonair_secrets::SecretsState,
    events: EventBus,
    connect_timeout_secs: Option<u64>,
) -> Result<TestConnectionResult, LabonairError> {
    // Step 1: fetch host configuration (same shape as `ssh_connect`'s step 1).
    let (
        host_address,
        port,
        username,
        auth_method,
        private_key_path,
        keep_alive_interval,
        keep_alive_tries,
        credential_id,
        jump_host_id,
    ) = {
        let conn = hosts_db
            .0
            .lock()
            .map_err(|e| LabonairError::Internal(e.to_string()))?;
        conn.query_row(
            "SELECT host_address, port, username, auth_method, private_key_path, \
             keep_alive_interval, keep_alive_tries, credential_id, jump_host_id \
             FROM hosts WHERE id = ?1",
            rusqlite::params![host_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )?
    };

    // Step 1b: resolve credential — same override as `ssh_connect`.
    let (auth_method, private_key_path) = if let Some(cid) = &credential_id {
        let (cred_type, cred_key_path): (String, Option<String>) = {
            let conn = hosts_db
                .0
                .lock()
                .map_err(|e| LabonairError::Internal(e.to_string()))?;
            conn.query_row(
                "SELECT cred_type, key_path FROM credentials WHERE id=?1",
                rusqlite::params![cid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| LabonairError::Internal(format!("Credential '{}' not found — it may have been deleted. Please update the host's auth settings.", cid)))?
        };
        (cred_type, cred_key_path)
    } else {
        (auth_method, private_key_path)
    };

    // Step 2: password — explicit override wins, else keyring lookup.
    let password: Option<String> = if auth_method == "password" {
        if password_override.is_some() {
            password_override
        } else if let Some(cid) = &credential_id {
            labonair_secrets::get_password(secrets, "labonair-cred", cid)
                .ok()
                .flatten()
        } else {
            labonair_secrets::get_password(secrets, "labonair-app", &host_id)
                .ok()
                .flatten()
        }
    } else {
        None
    };

    // Same credential-sourced-passphrase fallback as `ssh_connect`.
    let passphrase = if credential_id.is_some() && auth_method == "key" && passphrase.is_none() {
        if let Some(cid) = &credential_id {
            labonair_secrets::get_password(secrets, "labonair-cred", cid)
                .ok()
                .flatten()
        } else {
            passphrase
        }
    } else {
        passphrase
    };

    // Step 3: jump host, if any.
    let jump = if jump_host_id.is_some() {
        resolve_jump_chain(hosts_db, secrets, &host_id, None)?
    } else {
        Vec::new()
    };

    // Synthetic session id — this never becomes a real session, so it just
    // needs to be unique enough not to collide with a live tab's session_id
    // for the log-step/event emits along the way.
    let session_id = format!("test-{}", uuid::Uuid::new_v4());
    let timeout = connect_timeout_secs.unwrap_or(10);

    let attempt: Result<Arc<russh::client::Handle<ClientHandler>>, String> = async {
        let tcp = connect_transport_maybe_via_jump(
            &session_id,
            &host_address,
            port,
            &jump,
            trust_state,
            &events,
            timeout,
            None,
        )
        .await?;
        establish_authenticated_session_from_stream(
            &session_id,
            tcp,
            &host_address,
            port,
            &username,
            &auth_method,
            private_key_path.as_deref(),
            keep_alive_interval,
            keep_alive_tries,
            password.as_deref(),
            passphrase.as_deref(),
            trust_state,
            &events,
            true, // fail_fast_untrusted_host — never waits on, or auto-accepts, a trust dialog
            Arc::new(std::sync::Mutex::new(None)),
            None,
            None,
            None,
        )
        .await
    }
    .await;

    match attempt {
        Ok(handle) => {
            // This is a connectivity check, not a real session — no PTY
            // channel was opened and nothing was inserted into `SshState`.
            // Close the transport immediately rather than leaving it dangling.
            let _ = handle
                .disconnect(
                    russh::Disconnect::ByApplication,
                    "Connection test complete",
                    "",
                )
                .await;
            Ok(TestConnectionResult::Success)
        }
        Err(msg) => {
            let lower = msg.to_lowercase();
            let fingerprint = extract_fingerprint(&msg).unwrap_or_default();
            if lower.contains("host key mismatch") {
                Ok(TestConnectionResult::HostKeyChanged { fingerprint })
            } else if lower.contains("not yet trusted") {
                Ok(TestConnectionResult::UnknownHostKey { fingerprint })
            } else {
                Err(classify_ssh_error(msg))
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn ssh_connect_async(
    session_id: String,
    _host_id: String,
    passphrase: Option<String>,
    host_address: String,
    port: i64,
    username: String,
    auth_method: String,
    private_key_path: Option<String>,
    keep_alive_interval: Option<i64>,
    keep_alive_tries: Option<i64>,
    default_path_ssh: Option<String>,
    password: Option<String>,
    initial_cols: u32,
    initial_rows: u32,
    jump: Vec<JumpHostParams>,
    blocks: bool,
    state: super::SshState,
    trust_state: super::TrustState,
    events: EventBus,
    on_event: EventChannel<super::pty::SshPtyEvent>,
    connect_timeout_secs: Option<u64>,
    prompts: Option<HopPrompts>,
) -> Result<(), String> {
    // Establish the transport stream — either a direct TCP connection or a
    // channel bridged through a jump host.
    let tcp = connect_transport_maybe_via_jump(
        &session_id,
        &host_address,
        port,
        &jump,
        &trust_state,
        &events,
        connect_timeout_secs.unwrap_or(10),
        prompts.as_ref(),
    )
    .await?;

    let disconnect_reason = Arc::new(std::sync::Mutex::new(None));
    let handle = establish_authenticated_session_from_stream(
        &session_id,
        tcp,
        &host_address,
        port,
        &username,
        &auth_method,
        private_key_path.as_deref(),
        keep_alive_interval,
        keep_alive_tries,
        password.as_deref(),
        passphrase.as_deref(),
        &trust_state,
        &events,
        false,
        disconnect_reason.clone(),
        None,
        None,
        None,
    )
    .await?;

    // Store session in SshState *before* opening the shell channel — this
    // function runs sequentially on one async task with no `.await` between
    // building the session and inserting it, so the reader task spawned by
    // `open_shell_channel` can never race ahead of registration (see that
    // function's doc comment for why the old thread-rendezvous is no longer
    // needed).
    let session = Arc::new(super::RushSession {
        handle,
        pty: tokio::sync::Mutex::new(None),
        sftp: tokio::sync::OnceCell::new(),
        shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        disconnect_reason,
        agent_tap: tokio::sync::broadcast::channel(256).0,
    });
    {
        let mut map = state.0.lock().map_err(|e| e.to_string())?;
        map.insert(session_id.clone(), session.clone());
    }

    log_step!(events, session_id, "Opening shell channel…");
    if let Err(e) = super::pty::open_shell_channel(
        session,
        session_id.clone(),
        events.clone(),
        state.clone(),
        initial_cols,
        initial_rows,
        blocks,
        on_event,
    )
    .await
    {
        // Roll back the just-inserted entry — a session with an authenticated
        // handle but no working PTY is not a usable connection.
        let mut map = state.0.lock().map_err(|e| e.to_string())?;
        map.remove(&session_id);
        return Err(format!("failed to open shell channel: {e}"));
    }

    log_step!(events, session_id, "Session established ✓");
    events
        .emit(
            "session_established",
            serde_json::json!({ "session_id": session_id, "default_path_ssh": default_path_ssh }),
        )
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Dials hop 1 over TCP, authenticates, opens `direct-tcpip` to hop 2 on that
/// session and authenticates over the channel stream, and so on; finally
/// opens `direct-tcpip` to the target and returns that stream. Hops fail fast
/// on an unknown/mismatched host key unless `prompts` is set, in which case
/// host keys and missing/wrong credentials are asked through the broker.
/// Errors read `Hop i/n (label): cause`.
#[allow(clippy::too_many_arguments)]
async fn connect_via_jump(
    session_id: &str,
    jump: &[JumpHostParams],
    target_host: &str,
    target_port: i64,
    trust_state: &super::TrustState,
    events: &EventBus,
    connect_timeout_secs: u64,
    prompts: Option<&HopPrompts>,
) -> Result<ChainedStream, String> {
    let n = jump.len();
    let mut handles: Vec<Arc<russh::client::Handle<ClientHandler>>> = Vec::with_capacity(n);
    let mut prev_stream: Option<russh::ChannelStream<russh::client::Msg>> = None;
    for (idx, jh) in jump.iter().enumerate() {
        let i = idx + 1;
        let err = |cause: String| format!("Hop {i}/{n} ({}): {cause}", jh.label);
        log_step!(
            events,
            session_id,
            format!("Hop {i}/{n} ({}): connecting…", jh.label)
        );
        let hop_id = labonair_ssh::hop_session_id(session_id, i);
        let ctx = prompts.map(|p| HopPromptCtx {
            prompts: p.clone(),
            base_session_id: session_id.to_string(),
            info: Some(HopInfo {
                index: i,
                total: n,
                label: jh.label.clone(),
            }),
        });
        let hop_handle = if let Some(stream) = prev_stream.take() {
            establish_hop(&hop_id, stream, jh, trust_state, events, ctx).await
        } else {
            let tcp = tcp_connect_async(&jh.address, jh.port, connect_timeout_secs)
                .await
                .map_err(|e| err(format!("TCP connect failed: {e}")))?;
            establish_hop(&hop_id, tcp, jh, trust_state, events, ctx).await
        }
        .map_err(err)?;
        log_step!(
            events,
            session_id,
            format!("Hop {i}/{n} ({}): authenticated ✓", jh.label)
        );
        // Open the tunnel to the next hop (or the target) on this session.
        let (next_host, next_port, next_label) = match jump.get(idx + 1) {
            Some(next) => (next.address.as_str(), next.port, next.label.as_str()),
            None => (target_host, target_port, "target"),
        };
        let channel = hop_handle
            .channel_open_direct_tcpip(next_host, next_port as u32, "127.0.0.1", 0)
            .await
            .map_err(|e| err(format!("failed to open tunnel to {next_label}: {e}")))?;
        prev_stream = Some(channel.into_stream());
        handles.push(hop_handle);
    }
    log_step!(events, session_id, "Jump tunnel established ✓");
    let inner = prev_stream.ok_or_else(|| "Empty jump route".to_string())?;
    Ok(ChainedStream {
        inner,
        _hops: handles,
    })
}

async fn establish_hop<R>(
    hop_id: &str,
    stream: R,
    jh: &JumpHostParams,
    trust_state: &super::TrustState,
    events: &EventBus,
    interactive: Option<HopPromptCtx>,
) -> Result<Arc<russh::client::Handle<ClientHandler>>, String>
where
    R: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    establish_authenticated_session_from_stream(
        hop_id,
        stream,
        &jh.address,
        jh.port,
        &jh.username,
        &jh.auth_method,
        jh.private_key_path.as_deref(),
        jh.keep_alive_interval,
        None,
        jh.password.as_deref(),
        None,
        trust_state,
        events,
        // Fail fast on an untrusted host key unless `interactive` can ask.
        true,
        Arc::new(std::sync::Mutex::new(None)),
        interactive,
        None,
        None,
    )
    .await
}

/// Opens a TCP connection to host:port with a configurable timeout
/// (`sshConnectTimeoutSecs`, default 10s — see `tcp_connect_async`).
/// Uses socket2 for explicit OS-level socket control and IPv4-only filtering.
/// This fixes "No route to host" errors on macOS with local network addresses.
fn tcp_connect(host: &str, port: i64, timeout_secs: u64) -> Result<std::net::TcpStream, String> {
    use socket2::{Domain, Socket, TcpKeepalive, Type};
    use std::net::{IpAddr, ToSocketAddrs};
    use std::time::Duration;

    let addr_str = format!("{}:{}", host, port);
    let addrs: Vec<std::net::SocketAddr> = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("Failed to resolve {}: {}", addr_str, e))?
        .filter(|addr| matches!(addr.ip(), IpAddr::V4(_)))
        .collect();

    if addrs.is_empty() {
        return Err(format!("No IPv4 addresses resolved for {}", addr_str));
    }

    let mut last_err = String::new();
    for addr in &addrs {
        match Socket::new(Domain::IPV4, Type::STREAM, None) {
            Ok(socket) => {
                let _ = socket.set_nonblocking(false);
                let sock_addr: socket2::SockAddr = (*addr).into();
                match socket.connect_timeout(&sock_addr, Duration::from_secs(timeout_secs)) {
                    Ok(_) => {
                        let ka = TcpKeepalive::new()
                            .with_time(Duration::from_secs(60))
                            .with_interval(Duration::from_secs(15));
                        socket.set_tcp_keepalive(&ka).ok();
                        let tcp: std::net::TcpStream = socket.into();
                        tcp.set_nodelay(true).ok();
                        return Ok(tcp);
                    }
                    Err(e) => last_err = e.to_string(),
                }
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(last_err)
}

/// Runs the blocking `tcp_connect` helper and converts its result into a
/// `tokio::net::TcpStream` — the stream must be switched to non-blocking mode
/// before tokio can register it with the reactor.
async fn tcp_connect_async(
    host: &str,
    port: i64,
    timeout_secs: u64,
) -> Result<tokio::net::TcpStream, String> {
    let host_owned = host.to_string();
    let std_tcp = tcp_connect(&host_owned, port, timeout_secs)
        .map_err(|e| format!("TCP connect to {}:{} failed: {}", host_owned, port, e))?;
    std_tcp.set_nonblocking(true).map_err(|e| e.to_string())?;
    tokio::net::TcpStream::from_std(std_tcp).map_err(|e| e.to_string())
}

/// Tries to authenticate via the running ssh-agent (SSH_AUTH_SOCK).
/// Returns true if authentication succeeded, false if agent unavailable or all
/// identities were rejected — never panics. Certificates offered by the agent
/// are skipped (not supported via this path, matching the pre-migration
/// behavior — only plain public-key identities are tried).
#[cfg(unix)]
async fn try_agent_auth(
    handle: &mut russh::client::Handle<ClientHandler>,
    username: &str,
    session_id: &str,
    events: &EventBus,
) -> bool {
    if std::env::var("SSH_AUTH_SOCK").is_err() {
        return false;
    }
    let mut agent = match russh::keys::agent::client::AgentClient::connect_env().await {
        Ok(a) => a,
        Err(_) => return false,
    };
    let identities = match agent.request_identities().await {
        Ok(ids) => ids,
        Err(_) => return false,
    };
    for identity in identities {
        let russh::keys::agent::AgentIdentity::PublicKey { key, .. } = identity else {
            continue;
        };
        // For RSA identities, negotiate the signature hash the server actually
        // accepts (`rsa-sha2-256`/`-512`) rather than always requesting the
        // legacy `ssh-rsa`/SHA-1 combination, which OpenSSH >= 8.8 rejects by
        // default. Irrelevant (ignored) for non-RSA key types.
        let hash_alg = handle
            .best_supported_rsa_hash()
            .await
            .ok()
            .flatten()
            .flatten();
        if let Ok(res) = handle
            .authenticate_publickey_with(username, key, hash_alg, &mut agent)
            .await
        {
            if res.success() {
                log_step!(events, session_id, "Authenticated via ssh-agent ✓");
                return true;
            }
        }
    }
    false
}

#[cfg(not(unix))]
async fn try_agent_auth(
    _handle: &mut russh::client::Handle<ClientHandler>,
    _username: &str,
    _session_id: &str,
    _events: &EventBus,
) -> bool {
    false
}

/// Remove all known_hosts lines matching `host_address` via file rewrite.
pub fn drop_known_host_entry(path: &std::path::Path, host_address: &str) {
    if !path.exists() {
        return;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                return true;
            }
            let first_field = trimmed.split_whitespace().next().unwrap_or("");
            !first_field.split(',').any(|h| h == host_address)
        })
        .collect();
    let new_content = filtered.join("\n") + "\n";
    let _ = std::fs::write(path, new_content);
}

pub async fn ssh_disconnect(session_id: String, state: &super::SshState) -> Result<(), String> {
    let sess = {
        let mut map = state.0.lock().map_err(|e| e.to_string())?;
        map.remove(&session_id)
    };
    if let Some(sess) = sess {
        // Signal any dependent reader task to exit before tearing down the transport.
        sess.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = sess
            .handle
            .disconnect(russh::Disconnect::ByApplication, "User disconnected", "")
            .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    fn parents(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        move |id| m.get(id).cloned()
    }

    fn route_db() -> (
        labonair_persistence::Database,
        labonair_secrets::SecretsState,
    ) {
        let db = labonair_persistence::Database(Arc::new(std::sync::Mutex::new(
            labonair_persistence::initialize_in_memory_database().unwrap(),
        )));
        {
            let conn = db.0.lock().unwrap();
            // t jumps via old; a and b are free hosts.
            for (id, jump) in [("old", None), ("a", None), ("b", None), ("t", Some("old"))] {
                conn.execute(
                    "INSERT INTO hosts (id, name, host_address, port, username, auth_method, \
                     jump_host_id, created_at) VALUES (?1, ?1, ?1, 22, 'u', 'password', ?2, 0)",
                    rusqlite::params![id, jump],
                )
                .unwrap();
            }
        }
        (db, labonair_secrets::SecretsState::in_memory())
    }

    #[test]
    fn route_override_replaces_the_stored_chain() {
        let (db, secrets) = route_db();
        let ids = |r: Option<&[String]>| -> Vec<String> {
            resolve_jump_chain(&db, &secrets, "t", r)
                .unwrap()
                .into_iter()
                .map(|h| h.host_id)
                .collect()
        };
        assert_eq!(ids(None), ["old"]);
        assert_eq!(ids(Some(&["a".into(), "b".into()])), ["a", "b"]);
        // An empty override means direct, even though a chain is stored.
        assert!(ids(Some(&[])).is_empty());
    }

    #[test]
    fn route_override_rejects_self_cycle_and_depth() {
        let t = "t";
        let r = |v: &[&str]| {
            override_route_ids(t, &v.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        };
        assert_eq!(r(&["a", "t"]), Err(JumpRouteError::SelfJump));
        assert_eq!(r(&["a", "b", "a"]), Err(JumpRouteError::Cycle));
        assert_eq!(
            r(&["1", "2", "3", "4", "5", "6", "7", "8", "9"]),
            Err(JumpRouteError::TooDeep)
        );
        assert_eq!(r(&["a", "b"]).unwrap(), ["a", "b"]);
        let (db, secrets) = route_db();
        assert!(resolve_jump_chain(&db, &secrets, "t", Some(&["t".to_string()])).is_err());
    }

    #[test]
    fn route_ids_orders_outermost_first() {
        assert_eq!(route_ids("t", parents(&[])).unwrap(), Vec::<String>::new());
        assert_eq!(route_ids("t", parents(&[("t", "a")])).unwrap(), ["a"]);
        let p = parents(&[("t", "c"), ("c", "b"), ("b", "a")]);
        assert_eq!(route_ids("t", p).unwrap(), ["a", "b", "c"]);
    }

    #[test]
    fn route_ids_rejects_bad_routes() {
        assert_eq!(
            route_ids("a", parents(&[("a", "a")])),
            Err(JumpRouteError::SelfJump)
        );
        assert_eq!(
            route_ids("a", parents(&[("a", "b"), ("b", "a")])),
            Err(JumpRouteError::Cycle)
        );
        assert_eq!(
            route_ids("a", parents(&[("a", "b"), ("b", "c"), ("c", "b")])),
            Err(JumpRouteError::Cycle)
        );
        let deep = |id: &str| -> Option<String> {
            let n: usize = id.trim_start_matches('h').parse().ok()?;
            Some(format!("h{}", n + 1))
        };
        assert_eq!(route_ids("h0", deep), Err(JumpRouteError::TooDeep));
    }
    use super::*;
    use labonair_events::EventChannel;

    struct TestCapabilities {
        ssh: super::super::SshState,
        trust: super::super::TrustState,
        db: labonair_persistence::Database,
        secrets: Arc<labonair_secrets::SecretsState>,
        events: EventBus,
    }

    fn test_capabilities() -> TestCapabilities {
        let dir = std::env::temp_dir().join(format!("labonair-ssh-test-{}", uuid::Uuid::new_v4()));
        let connection = labonair_persistence::initialize_database(dir.clone()).expect("database");
        TestCapabilities {
            ssh: super::super::SshState::default(),
            trust: super::super::TrustState::default(),
            db: labonair_persistence::Database(Arc::new(std::sync::Mutex::new(connection))),
            secrets: Arc::new(labonair_secrets::SecretsState::new(dir)),
            events: EventBus::new(),
        }
    }

    #[tokio::test]
    async fn trust_host_releases_the_pending_known_hosts_warning() {
        // Mirrors the security flow: ClientHandler parks on a oneshot after
        // emitting `known_hosts_warning`; the UI's accept/reject resolves it.
        let ts = super::super::TrustState::default();
        let (tx, rx) = tokio::sync::oneshot::channel();
        ts.0.lock().unwrap().insert("s1".to_string(), tx);

        ssh_trust_host("s1".to_string(), true, &ts).await.unwrap();

        assert!(rx.await.unwrap(), "accept must be delivered to the waiter");
        assert!(
            ts.0.lock().unwrap().is_empty(),
            "the trust slot must not leak"
        );
    }

    #[tokio::test]
    async fn trust_host_rejects_unknown_session_without_panicking() {
        let ts = super::super::TrustState::default();
        assert!(ssh_trust_host("nope".to_string(), false, &ts).await.is_ok());
    }

    #[tokio::test]
    async fn connect_to_unknown_host_id_fails_cleanly() {
        let app = test_capabilities();
        let on_event: EventChannel<super::super::pty::SshPtyEvent> = EventChannel::null();
        let res = ssh_connect(
            "sess-x".to_string(),
            "does-not-exist".to_string(),
            None,
            None,
            Some(80),
            Some(24),
            false,
            on_event,
            &app.ssh,
            &app.trust,
            &app.db,
            &app.secrets,
            app.events.clone(),
            Some(1),
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(
            res.is_err(),
            "connecting to a missing host must error, not hang"
        );
    }
}
