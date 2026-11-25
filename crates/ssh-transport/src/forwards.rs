// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Port-forward engine (ADR 0016 §3): one shared SSH session per host,
//! per-forward cancellation, Local and Dynamic (SOCKS5) forwards, health
//! monitoring with reconnect, Remote forwards (`tcpip-forward`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::prompts::{HopPrompts, PromptBroker, PromptSink};
use labonair_ssh::{
    BoxFuture, ForwardId, ForwardInfo, ForwardKind, ForwardLifetime, ForwardOpenRequest,
    ForwardOrigin, ForwardSpec, ForwardState, PendingPrompt, PromptAnswer, PromptRequestId,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// Counter-only revision bumps are coalesced to at most one per this interval.
const COUNTER_BUMP_INTERVAL: Duration = Duration::from_millis(250);

pub trait DuplexStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> DuplexStream for T {}
pub type BoxedDuplexStream = Box<dyn DuplexStream>;

/// One authenticated SSH session shared by all forwards of a host.
pub trait ForwardSession: Send + Sync {
    fn open_direct_tcpip<'a>(
        &'a self,
        host: &'a str,
        port: u16,
    ) -> BoxFuture<'a, Result<BoxedDuplexStream, String>>;
    /// Resolves when the transport is gone (drives reconnect).
    fn closed(&self) -> BoxFuture<'static, ()>;
    /// Incoming `forwarded-tcpip` channels of this session are routed here.
    fn remote_dispatch(&self) -> &RemoteForwardDispatch;
    /// Asks the server to listen; returns the bound port (the server's pick for 0).
    fn request_remote_forward<'a>(
        &'a self,
        listen_host: &'a str,
        listen_port: u16,
    ) -> BoxFuture<'a, Result<u16, String>>;
    fn cancel_remote_forward<'a>(
        &'a self,
        listen_host: &'a str,
        bound_port: u16,
    ) -> BoxFuture<'a, ()>;
}

const REMOTE_REFUSED: &str =
    "The server refused the remote forward (AllowTcpForwarding / GatewayPorts?)";
const LOCAL_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Where a remote forward delivers its incoming channels.
#[derive(Clone)]
pub struct RemoteTarget {
    host: String,
    port: u16,
    counters: Arc<Counters>,
    touch: Arc<dyn Fn() + Send + Sync>,
}

/// `(address, port)` the server reports for a `forwarded-tcpip` channel -> target.
#[derive(Clone, Default)]
pub struct RemoteForwardDispatch(Arc<Mutex<HashMap<(String, u32), RemoteTarget>>>);

impl RemoteForwardDispatch {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<(String, u32), RemoteTarget>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn insert(&self, addr: &str, port: u32, t: RemoteTarget) {
        self.map().insert((addr.to_string(), port), t);
    }

    fn remove(&self, addr: &str, port: u32) {
        self.map().remove(&(addr.to_string(), port));
    }

    /// Exact match first; servers may report another spelling of the bind
    /// address (`127.0.0.1` for `localhost`), so fall back to a unique port match.
    fn lookup(&self, addr: &str, port: u32) -> Option<RemoteTarget> {
        let m = self.map();
        if let Some(t) = m.get(&(addr.to_string(), port)) {
            return Some(t.clone());
        }
        let mut by_port = m.iter().filter(|((_, p), _)| *p == port).map(|(_, t)| t);
        let first = by_port.next()?;
        by_port.next().is_none().then(|| first.clone())
    }

    /// Bridges an incoming channel to its local target; connect failure only
    /// closes the channel.
    pub(crate) fn serve(&self, addr: &str, port: u32, mut stream: BoxedDuplexStream) {
        let Some(t) = self.lookup(addr, port) else {
            return log::debug!("remote forward: no target for {addr}:{port}");
        };
        tokio::spawn(async move {
            let connect = TcpStream::connect((t.host.as_str(), t.port));
            let mut tcp = match tokio::time::timeout(LOCAL_CONNECT_TIMEOUT, connect).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    return log::debug!("remote forward: connect {}:{}: {e}", t.host, t.port)
                }
                Err(_) => {
                    return log::debug!("remote forward: connect {}:{} timed out", t.host, t.port)
                }
            };
            t.counters.connections.fetch_add(1, Ordering::Relaxed);
            (t.touch)();
            if let Ok((to_local, to_remote)) =
                tokio::io::copy_bidirectional(&mut stream, &mut tcp).await
            {
                t.counters.bytes_in.fetch_add(to_local, Ordering::Relaxed);
                t.counters.bytes_out.fetch_add(to_remote, Ordering::Relaxed);
            }
            t.counters.connections.fetch_sub(1, Ordering::Relaxed);
            (t.touch)();
        });
    }
}

/// Prompt access for a host connect: ask via `broker`, published through `sink`.
pub type ForwardPrompts = HopPrompts;

/// Failure reason when a prompt timed out or was cancelled.
pub const NO_ANSWER: &str = "No answer to the prompt";

pub trait HostSessionFactory: Send + Sync {
    /// Connects the host session; may block on `prompts` (host key, password, passphrase).
    fn connect<'a>(
        &'a self,
        host_id: &'a str,
        prompts: ForwardPrompts,
    ) -> BoxFuture<'a, Result<Arc<dyn ForwardSession>, String>>;
    /// Saved forwards of the host (`hosts.tunnels`), for `attach_host`.
    fn saved(&self, host_id: &str) -> Vec<labonair_forwards::SavedForward>;
}

// ---- production factory -------------------------------------------------

pub struct SshSessionFactory {
    pub hosts_db: labonair_persistence::Database,
    pub secrets: Arc<labonair_secrets::SecretsState>,
    pub trust_state: super::TrustState,
    pub events: labonair_events::EventBus,
    pub connect_timeout_secs: Option<u64>,
}

struct RusshSession(
    Arc<russh::client::Handle<super::client::ClientHandler>>,
    Arc<tokio::sync::Notify>,
    RemoteForwardDispatch,
);

impl ForwardSession for RusshSession {
    fn open_direct_tcpip<'a>(
        &'a self,
        host: &'a str,
        port: u16,
    ) -> BoxFuture<'a, Result<BoxedDuplexStream, String>> {
        Box::pin(async move {
            let ch = self
                .0
                .channel_open_direct_tcpip(host.to_string(), port as u32, "127.0.0.1", 0)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Box::new(ch.into_stream()) as BoxedDuplexStream)
        })
    }

    fn closed(&self) -> BoxFuture<'static, ()> {
        let notify = self.1.clone();
        let handle = self.0.clone();
        Box::pin(async move {
            // The permit is stored, so a disconnect before this call is not missed.
            if !handle.is_closed() {
                notify.notified().await;
            }
        })
    }

    fn remote_dispatch(&self) -> &RemoteForwardDispatch {
        &self.2
    }

    fn request_remote_forward<'a>(
        &'a self,
        listen_host: &'a str,
        listen_port: u16,
    ) -> BoxFuture<'a, Result<u16, String>> {
        Box::pin(async move {
            let port = self
                .0
                .tcpip_forward(listen_host, listen_port as u32)
                .await
                .map_err(|e| match e {
                    russh::Error::RequestDenied => REMOTE_REFUSED.to_string(),
                    e => e.to_string(),
                })?;
            // The server answers 0 when it bound the requested non-zero port.
            Ok(if listen_port == 0 {
                port as u16
            } else {
                listen_port
            })
        })
    }

    fn cancel_remote_forward<'a>(
        &'a self,
        listen_host: &'a str,
        bound_port: u16,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let _ = self
                .0
                .cancel_tcpip_forward(listen_host, bound_port as u32)
                .await;
        })
    }
}

impl HostSessionFactory for SshSessionFactory {
    fn connect<'a>(
        &'a self,
        host_id: &'a str,
        prompts: ForwardPrompts,
    ) -> BoxFuture<'a, Result<Arc<dyn ForwardSession>, String>> {
        Box::pin(async move {
            let (address, port, username, auth, key_path) = {
                let conn = self.hosts_db.0.lock().map_err(|e| e.to_string())?;
                conn.query_row(
                    "SELECT host_address, port, username, auth_method, private_key_path \
                     FROM hosts WHERE id = ?1",
                    rusqlite::params![host_id],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .map_err(|e| e.to_string())?
            };
            // Stored secret (password, or key passphrase) is tried first; prompts only on miss.
            let password = if auth == "password" || auth == "key" {
                labonair_secrets::get_password(&self.secrets, "labonair-app", host_id)
                    .ok()
                    .flatten()
            } else {
                None
            };
            let jump =
                super::client::resolve_jump_chain(&self.hosts_db, &self.secrets, host_id, None)
                    .map_err(|e| e.to_string())?;
            let notify = Arc::new(tokio::sync::Notify::new());
            let dispatch = RemoteForwardDispatch::default();
            let handle = super::client::establish_authenticated_session_notify(
                &format!("forward_{host_id}"),
                &address,
                port,
                &username,
                &auth,
                key_path.as_deref(),
                None,
                None,
                if auth == "password" {
                    password.as_deref()
                } else {
                    None
                },
                if auth == "key" {
                    password.as_deref()
                } else {
                    None
                },
                &self.trust_state,
                &self.events,
                true,
                jump,
                self.connect_timeout_secs,
                Some(notify.clone()),
                Some(dispatch.clone()),
                Some(prompts),
            )
            .await
            .map_err(|e| {
                if e.ends_with("prompt cancelled") {
                    NO_ANSWER.to_string()
                } else {
                    e
                }
            })?;
            Ok(Arc::new(RusshSession(handle, notify, dispatch)) as Arc<dyn ForwardSession>)
        })
    }

    fn saved(&self, host_id: &str) -> Vec<labonair_forwards::SavedForward> {
        let Ok(conn) = self.hosts_db.0.lock() else {
            return Vec::new();
        };
        let raw: Option<String> = conn
            .query_row(
                "SELECT tunnels FROM hosts WHERE id = ?1",
                rusqlite::params![host_id],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        labonair_forwards::parse_saved(&raw)
    }
}

// ---- registry -----------------------------------------------------------

#[derive(Default)]
pub(crate) struct Counters {
    connections: AtomicU32,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
}

struct Entry {
    spec: ForwardSpec,
    lifetime: ForwardLifetime,
    state: ForwardState,
    saved: bool,
    bound_port: Option<u16>,
    counters: Arc<Counters>,
    cancel: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}

type SharedSession = Option<Arc<dyn ForwardSession>>;

/// Per-host session cell. `None` while (re)connecting.
struct HostSlot {
    connect: tokio::sync::Mutex<()>,
    tx: watch::Sender<SharedSession>,
    monitor: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// The monitor is re-establishing the session; nobody else may connect.
    reconnecting: AtomicBool,
}

impl HostSlot {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            connect: tokio::sync::Mutex::new(()),
            tx: watch::channel(None).0,
            monitor: Mutex::new(None),
            reconnecting: AtomicBool::new(false),
        })
    }

    /// Stops health monitoring and drops the session.
    fn shutdown(&self) {
        if let Some(m) = self
            .monitor
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            m.abort();
        }
        self.tx.send_replace(None);
    }
}

type SessionSlot = Arc<HostSlot>;

/// Auto-reconnect behaviour after an unexpected session loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectPolicy {
    pub enabled: bool,
    pub max_attempts: u32,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 10,
        }
    }
}

/// Delay before reconnect attempt `attempt` (1-based): 1 s, 2 s, 4 s … capped at 30 s.
pub fn backoff(attempt: u32) -> Duration {
    Duration::from_secs((1u64 << attempt.saturating_sub(1).min(5)).min(30))
}

/// How long an inbound connection waits for a reconnecting session.
const SESSION_WAIT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct State {
    forwards: HashMap<ForwardId, Entry>,
    /// attach_host ref counts.
    attached: HashMap<String, usize>,
    sessions: HashMap<String, SessionSlot>,
}

struct Inner {
    factory: Arc<dyn HostSessionFactory>,
    policy: Mutex<ReconnectPolicy>,
    state: Mutex<State>,
    broker: PromptBroker,
    revision: watch::Sender<u64>,
    /// A trailing counter bump is already scheduled.
    bump_pending: AtomicBool,
}

#[derive(Clone)]
pub struct ForwardRegistry(Arc<Inner>);

impl Inner {
    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn bump(&self) {
        self.revision.send_modify(|v| *v += 1);
    }

    /// Coalesced bump for counter changes: the first change schedules one
    /// trailing bump after 250 ms (no standing ticker task, nothing runs when idle).
    fn touch(self: &Arc<Self>) {
        if !self.bump_pending.swap(true, Ordering::AcqRel) {
            let inner = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(COUNTER_BUMP_INTERVAL).await;
                inner.bump_pending.store(false, Ordering::Release);
                inner.bump();
            });
        }
    }

    fn set_state(&self, id: &ForwardId, state: ForwardState) {
        if let Some(e) = self.st().forwards.get_mut(id) {
            e.state = state;
        }
        self.bump();
    }

    /// Drops the host session when none of its forwards is running any more.
    fn release_if_idle(&self, host_id: &str) {
        let mut st = self.st();
        let busy = st.forwards.values().any(|e| {
            e.spec.host_id == host_id
                && matches!(
                    e.state,
                    ForwardState::Starting
                        | ForwardState::Active
                        | ForwardState::Reconnecting { .. }
                        | ForwardState::AwaitingInput(_)
                )
        });
        if !busy {
            if let Some(slot) = st.sessions.remove(host_id) {
                slot.shutdown();
            }
        }
    }

    /// Connects (or reuses) the host session; returns a receiver that always
    /// holds the current session (`None` while reconnecting).
    async fn session_for(
        self: &Arc<Self>,
        id: &ForwardId,
        host_id: &str,
    ) -> Result<watch::Receiver<SharedSession>, String> {
        let slot = self
            .st()
            .sessions
            .entry(host_id.to_string())
            .or_insert_with(HostSlot::new)
            .clone();
        // Single-flight per host: this lock serialises first connects; the
        // monitor's reconnect is covered by `reconnecting` (checked before `tx`,
        // since the monitor clears the flag last).
        let _guard = slot.connect.lock().await;
        if slot.reconnecting.load(Ordering::Acquire) || slot.tx.borrow().is_some() {
            return Ok(slot.tx.subscribe());
        }
        // Another forward's failed connect may already have failed this one.
        let starting = self.st().forwards.get(id).is_some_and(|e| {
            matches!(
                e.state,
                ForwardState::Starting | ForwardState::AwaitingInput(_)
            )
        });
        if !starting {
            return Err("cancelled".into());
        }
        let s = self.factory.connect(host_id, self.prompts(host_id)).await?;
        let closed = s.closed();
        slot.tx.send_replace(Some(s));
        let task = tokio::spawn(monitor(
            self.clone(),
            host_id.to_string(),
            slot.clone(),
            closed,
        ));
        *slot.monitor.lock().unwrap_or_else(|p| p.into_inner()) = Some(task);
        Ok(slot.tx.subscribe())
    }

    fn prompts(self: &Arc<Self>, host_id: &str) -> ForwardPrompts {
        HopPrompts {
            broker: self.broker.clone(),
            sink: Arc::new(HostPromptSink {
                inner: self.clone(),
                host_id: host_id.to_string(),
            }),
        }
    }

    /// Sets `to` on every forward of the host currently in a state accepted by `from`.
    fn set_host_state(
        &self,
        host_id: &str,
        from: impl Fn(&ForwardState) -> bool,
        to: ForwardState,
        cancel: bool,
    ) {
        for e in self.st().forwards.values_mut() {
            if e.spec.host_id == host_id && from(&e.state) {
                e.state = to.clone();
                if cancel {
                    e.cancel.cancel();
                }
            }
        }
        self.bump();
    }
}

/// Records a pending prompt on every forward of the host that is waiting for its session.
struct HostPromptSink {
    inner: Arc<Inner>,
    host_id: String,
}

impl PromptSink for HostPromptSink {
    fn publish(&self, _session_id: &str, pending: PendingPrompt) {
        for e in self.inner.st().forwards.values_mut() {
            if e.spec.host_id == self.host_id
                && matches!(
                    e.state,
                    ForwardState::Starting
                        | ForwardState::Reconnecting { .. }
                        | ForwardState::AwaitingInput(_)
                )
            {
                e.state = ForwardState::AwaitingInput(pending.clone());
            }
        }
        self.inner.bump();
    }
}

/// Watches the host session; on unexpected loss reconnects with backoff.
/// Aborted (via `HostSlot::shutdown`) on deliberate close.
async fn monitor(
    inner: Arc<Inner>,
    host: String,
    slot: Arc<HostSlot>,
    mut closed: BoxFuture<'static, ()>,
) {
    loop {
        closed.await;
        slot.reconnecting.store(true, Ordering::Release);
        slot.tx.send_replace(None);
        let policy = *inner.policy.lock().unwrap_or_else(|p| p.into_inner());
        let is_up = |s: &ForwardState| matches!(s, ForwardState::Active);
        if !policy.enabled {
            inner.set_host_state(
                &host,
                is_up,
                ForwardState::Failed {
                    reason: "Connection lost".into(),
                },
                true,
            );
            slot.reconnecting.store(false, Ordering::Release);
            return;
        }
        let mut last = String::from("Connection lost");
        let mut next = None;
        for attempt in 1..=policy.max_attempts {
            inner.set_host_state(
                &host,
                |s| {
                    matches!(
                        s,
                        ForwardState::Active
                            | ForwardState::Reconnecting { .. }
                            | ForwardState::Starting
                    )
                },
                ForwardState::Reconnecting { attempt },
                false,
            );
            tokio::time::sleep(backoff(attempt)).await;
            match inner.factory.connect(&host, inner.prompts(&host)).await {
                Ok(s) => {
                    next = Some(s);
                    break;
                }
                // Nobody answered: retrying would just ask again for minutes.
                Err(e) if e == NO_ANSWER => {
                    last = e;
                    break;
                }
                Err(e) => last = e,
            }
        }
        let Some(s) = next else {
            let reason = format!("Connection lost: {last}");
            inner.set_host_state(
                &host,
                |s| {
                    matches!(
                        s,
                        ForwardState::Reconnecting { .. }
                            | ForwardState::Starting
                            | ForwardState::AwaitingInput(_)
                    )
                },
                ForwardState::Failed { reason },
                true,
            );
            slot.reconnecting.store(false, Ordering::Release);
            return;
        };
        closed = s.closed();
        slot.tx.send_replace(Some(s));
        inner.set_host_state(
            &host,
            |s| {
                matches!(
                    s,
                    ForwardState::Reconnecting { .. } | ForwardState::Starting
                )
            },
            ForwardState::Active,
            false,
        );
        slot.reconnecting.store(false, Ordering::Release);
    }
}

/// Binds the listener(s) for a Local forward. `localhost` binds both loopbacks.
async fn bind_listeners(host: &str, port: u16) -> Result<Vec<TcpListener>, String> {
    let describe = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::AddrInUse => format!("Port {port} is already in use"),
        std::io::ErrorKind::PermissionDenied if port < 1024 => {
            "Ports below 1024 need elevated rights".to_string()
        }
        _ => e.to_string(),
    };
    if host.eq_ignore_ascii_case("localhost") {
        let v4 = TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(describe)?;
        let port = v4.local_addr().map_err(|e| e.to_string())?.port();
        let mut out = vec![v4];
        if let Ok(v6) = TcpListener::bind(("::1", port)).await {
            out.push(v6);
        }
        return Ok(out);
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    Ok(vec![TcpListener::bind((host, port))
        .await
        .map_err(describe)?])
}

struct ConnGuard(Arc<Counters>, Arc<Inner>);
impl ConnGuard {
    fn new(c: Arc<Counters>, inner: Arc<Inner>) -> Self {
        c.connections.fetch_add(1, Ordering::Relaxed);
        inner.touch();
        Self(c, inner)
    }
}
impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::Relaxed);
        self.1.touch();
    }
}

// ---- SOCKS5 (Dynamic forwards) ---------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum SocksError {
    Version,
    NoAcceptableMethod,
    Command,
    AddressType,
    BadAddress,
}

impl SocksError {
    /// SOCKS5 reply code for the failure.
    fn reply(&self) -> u8 {
        match self {
            Self::Command => 0x07,
            Self::AddressType => 0x08,
            _ => 0x01,
        }
    }
}

/// Parses the method-selection message. `Ok(None)` = need more bytes;
/// `Ok(Some(consumed))` = no-auth is offered.
fn parse_greeting(buf: &[u8]) -> Result<Option<usize>, SocksError> {
    let [ver, n, rest @ ..] = buf else {
        return if buf.first().is_some_and(|v| *v != 5) {
            Err(SocksError::Version)
        } else {
            Ok(None)
        };
    };
    if *ver != 5 {
        return Err(SocksError::Version);
    }
    let n = *n as usize;
    if rest.len() < n {
        return Ok(None);
    }
    if rest[..n].contains(&0) {
        Ok(Some(2 + n))
    } else {
        Err(SocksError::NoAcceptableMethod)
    }
}

/// Parses a request. `Ok(None)` = need more bytes; otherwise (host, port).
fn parse_request(buf: &[u8]) -> Result<Option<(String, u16)>, SocksError> {
    if buf.first().is_some_and(|v| *v != 5) {
        return Err(SocksError::Version);
    }
    if buf.len() < 4 {
        return Ok(None);
    }
    if buf[1] != 1 {
        return Err(SocksError::Command);
    }
    let (host, end) = match buf[3] {
        1 => match buf.get(4..8) {
            Some(b) => (
                std::net::Ipv4Addr::new(b[0], b[1], b[2], b[3]).to_string(),
                8,
            ),
            None => return Ok(None),
        },
        4 => match buf.get(4..20) {
            Some(b) => {
                let o: [u8; 16] = b.try_into().map_err(|_| SocksError::BadAddress)?;
                (std::net::Ipv6Addr::from(o).to_string(), 20)
            }
            None => return Ok(None),
        },
        3 => {
            let Some(&len) = buf.get(4) else {
                return Ok(None);
            };
            if len == 0 {
                return Err(SocksError::BadAddress);
            }
            let end = 5 + len as usize;
            match buf.get(5..end) {
                Some(b) => (
                    String::from_utf8(b.to_vec()).map_err(|_| SocksError::BadAddress)?,
                    end,
                ),
                None => return Ok(None),
            }
        }
        _ => return Err(SocksError::AddressType),
    };
    match buf.get(end..end + 2) {
        Some(p) => Ok(Some((host, u16::from_be_bytes([p[0], p[1]])))),
        None => Ok(None),
    }
}

/// Maps a `direct-tcpip` open error to a SOCKS5 reply code.
fn socks_open_reply(err: &str) -> u8 {
    let e = err.to_ascii_lowercase();
    if e.contains("refused") {
        0x05
    } else if e.contains("unreachable") || e.contains("no route") || e.contains("resolve") {
        0x04
    } else {
        0x01
    }
}

fn socks_reply(code: u8) -> [u8; 10] {
    [5, code, 0, 1, 0, 0, 0, 0, 0, 0]
}

async fn read_until<S, T>(
    s: &mut S,
    buf: &mut Vec<u8>,
    mut parse: impl FnMut(&[u8]) -> Result<Option<T>, SocksError>,
) -> Result<T, Result<SocksError, String>>
where
    S: AsyncRead + Unpin,
{
    loop {
        match parse(buf) {
            Ok(Some(v)) => return Ok(v),
            Ok(None) => {}
            Err(e) => return Err(Ok(e)),
        }
        let mut chunk = [0u8; 300];
        let n = s.read(&mut chunk).await.map_err(|e| Err(e.to_string()))?;
        if n == 0 {
            return Err(Err("truncated SOCKS5 handshake".into()));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Runs the SOCKS5 negotiation up to (not including) the success reply.
async fn socks5_handshake<S>(s: &mut S) -> Result<(String, u16), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buf = Vec::new();
    let used = match read_until(s, &mut buf, parse_greeting).await {
        Ok(n) => n,
        Err(e) => {
            if e == Ok(SocksError::NoAcceptableMethod) {
                let _ = s.write_all(&[5, 0xFF]).await;
            }
            return Err(format!("{e:?}"));
        }
    };
    buf.drain(..used);
    s.write_all(&[5, 0]).await.map_err(|e| e.to_string())?;
    match read_until(s, &mut buf, parse_request).await {
        Ok(t) => Ok(t),
        Err(e) => {
            if let Ok(se) = &e {
                let _ = s.write_all(&socks_reply(se.reply())).await;
            }
            Err(format!("{e:?}"))
        }
    }
}

/// Waits for a live session (a reconnect may be running); `None` after 10 s.
async fn current_session(
    mut rx: watch::Receiver<SharedSession>,
) -> Option<Arc<dyn ForwardSession>> {
    let s = tokio::time::timeout(SESSION_WAIT, rx.wait_for(|s| s.is_some()))
        .await
        .ok()?
        .ok()?;
    s.clone()
}

/// Bridges one accepted connection. `target` is fixed for Local forwards;
/// `None` means a SOCKS5 handshake decides it (Dynamic).
async fn bridge(
    inner: Arc<Inner>,
    counters: Arc<Counters>,
    rx: watch::Receiver<SharedSession>,
    mut tcp: TcpStream,
    target: Option<(String, u16)>,
) {
    let _guard = ConnGuard::new(counters.clone(), inner.clone());
    let socks = target.is_none();
    let (host, port) = match target {
        Some(t) => t,
        None => match tokio::time::timeout(SESSION_WAIT, socks5_handshake(&mut tcp)).await {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => return log::debug!("forward: SOCKS5 handshake failed: {e}"),
            Err(_) => return log::debug!("forward: SOCKS5 handshake timed out"),
        },
    };
    let Some(session) = current_session(rx).await else {
        if socks {
            let _ = tcp.write_all(&socks_reply(0x01)).await;
        }
        return log::debug!("forward: no session for {host}:{port}");
    };
    let mut remote = match session.open_direct_tcpip(&host, port).await {
        Ok(s) => s,
        Err(e) => {
            log::debug!("forward: open {host}:{port} failed: {e}");
            if socks {
                let _ = tcp.write_all(&socks_reply(socks_open_reply(&e))).await;
            }
            return;
        }
    };
    drop(session);
    if socks && tcp.write_all(&socks_reply(0)).await.is_err() {
        return;
    }
    match tokio::io::copy_bidirectional(&mut tcp, &mut remote).await {
        Ok((out, back)) => {
            counters.bytes_out.fetch_add(out, Ordering::Relaxed);
            counters.bytes_in.fetch_add(back, Ordering::Relaxed);
        }
        Err(e) => log::debug!("forward: connection closed: {e}"),
    }
}

/// Remote forward runner: requests the listener on every (re)established
/// session, then waits for cancellation or loss of that session.
async fn run_remote(
    inner: Arc<Inner>,
    id: ForwardId,
    spec: ForwardSpec,
    counters: Arc<Counters>,
    cancel: CancellationToken,
    mut rx: watch::Receiver<SharedSession>,
) {
    let target = RemoteTarget {
        host: spec.target_host.clone(),
        port: spec.target_port,
        counters,
        touch: {
            let inner = inner.clone();
            Arc::new(move || inner.touch())
        },
    };
    let mut registered: Option<u16> = None;
    loop {
        let session = tokio::select! {
            s = async { rx.wait_for(|s| s.is_some()).await.ok().and_then(|s| s.clone()) } => s,
            _ = cancel.cancelled() => None,
        };
        let Some(session) = session else { break };
        match session
            .request_remote_forward(&spec.listen_host, spec.listen_port)
            .await
        {
            Ok(port) => {
                session
                    .remote_dispatch()
                    .insert(&spec.listen_host, port as u32, target.clone());
                registered = Some(port);
                if let Some(e) = inner.st().forwards.get_mut(&id) {
                    e.bound_port = Some(port);
                }
                inner.set_state(&id, ForwardState::Active);
            }
            Err(reason) => {
                inner.set_state(&id, ForwardState::Failed { reason });
                break;
            }
        }
        drop(session);
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = async { let _ = rx.wait_for(|s| s.is_none()).await; } => {}
        }
    }
    // Best-effort cleanup on the current session (gone if the link was lost).
    let current = rx.borrow().clone();
    if let (Some(session), Some(port)) = (current, registered) {
        session
            .remote_dispatch()
            .remove(&spec.listen_host, port as u32);
        session.cancel_remote_forward(&spec.listen_host, port).await;
    }
    inner.release_if_idle(&spec.host_id);
    inner.bump();
}

async fn run_forward(
    inner: Arc<Inner>,
    id: ForwardId,
    spec: ForwardSpec,
    counters: Arc<Counters>,
    cancel: CancellationToken,
) {
    let fail = |reason: String| inner.set_state(&id, ForwardState::Failed { reason });
    let remote = spec.kind == ForwardKind::Remote;

    // Preflight: bind before dialing so the user gets the real reason fast.
    let listeners = if remote {
        Vec::new()
    } else {
        match bind_listeners(&spec.listen_host, spec.listen_port).await {
            Ok(l) => l,
            Err(reason) => return fail(reason),
        }
    };
    let bound = listeners
        .first()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port());
    if let Some(e) = inner.st().forwards.get_mut(&id) {
        e.bound_port = bound;
    }

    let session = tokio::select! {
        r = inner.session_for(&id, &spec.host_id) => r,
        _ = cancel.cancelled() => Err("cancelled".into()),
    };
    let session = match session {
        Ok(s) => s,
        Err(reason) => {
            // Fail every forward of the host that is still waiting for the session.
            let mut st = inner.st();
            if !cancel.is_cancelled() {
                for e in st.forwards.values_mut() {
                    if e.spec.host_id == spec.host_id
                        && matches!(
                            e.state,
                            ForwardState::Starting | ForwardState::AwaitingInput(_)
                        )
                    {
                        e.state = ForwardState::Failed {
                            reason: reason.clone(),
                        };
                    }
                }
            }
            drop(st);
            inner.bump();
            inner.release_if_idle(&spec.host_id);
            return;
        }
    };
    if remote {
        return run_remote(inner, id, spec, counters, cancel, session).await;
    }
    // During a reconnect the monitor flips this forward to Active with the session.
    if session.borrow().is_some() {
        inner.set_state(&id, ForwardState::Active);
    }

    let target =
        (spec.kind == ForwardKind::Local).then(|| (spec.target_host.clone(), spec.target_port));
    let mut accepts = tokio::task::JoinSet::new();
    for listener in listeners {
        let (inner, counters, session, cancel, target) = (
            inner.clone(),
            counters.clone(),
            session.clone(),
            cancel.clone(),
            target.clone(),
        );
        accepts.spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    accepted = listener.accept() => match accepted {
                        Ok((tcp, _)) => {
                            let work = bridge(inner.clone(), counters.clone(), session.clone(), tcp, target.clone());
                            let cancel = cancel.clone();
                            tokio::spawn(async move {
                                tokio::select! { _ = work => {}, _ = cancel.cancelled() => {} }
                            });
                        }
                        Err(e) => { log::debug!("forward: accept failed: {e}"); break; }
                    }
                }
            }
        });
    }
    drop(session);
    // Runs until cancelled (or every accept loop died).
    tokio::select! {
        _ = cancel.cancelled() => {}
        _ = async { while accepts.join_next().await.is_some() {} } => {
            if !cancel.is_cancelled() {
                fail("Listener stopped unexpectedly".into());
            }
        }
    }
    accepts.shutdown().await; // listeners are closed before we report back
    inner.release_if_idle(&spec.host_id);
    inner.bump();
}

impl ForwardRegistry {
    pub fn new(factory: Arc<dyn HostSessionFactory>) -> Self {
        Self(Arc::new(Inner {
            factory,
            policy: Mutex::new(ReconnectPolicy::default()),
            state: Mutex::new(State::default()),
            broker: PromptBroker::default(),
            revision: watch::channel(0).0,
            bump_pending: AtomicBool::new(false),
        }))
    }

    pub fn set_reconnect_policy(&self, policy: ReconnectPolicy) {
        *self.0.policy.lock().unwrap_or_else(|p| p.into_inner()) = policy;
    }

    pub fn revision(&self) -> watch::Receiver<u64> {
        self.0.revision.subscribe()
    }

    /// Sorted by (host id, listen port, id).
    pub fn list(&self) -> Vec<ForwardInfo> {
        let st = self.0.st();
        let mut out: Vec<ForwardInfo> = st
            .forwards
            .values()
            .map(|e| ForwardInfo {
                spec: e.spec.clone(),
                state: e.state.clone(),
                lifetime: e.lifetime,
                origin: ForwardOrigin::App,
                bound_port: e.bound_port,
                connections: e.counters.connections.load(Ordering::Relaxed),
                bytes_in: e.counters.bytes_in.load(Ordering::Relaxed),
                bytes_out: e.counters.bytes_out.load(Ordering::Relaxed),
                saved: e.saved,
            })
            .collect();
        out.sort_by(|a, b| {
            (&a.spec.host_id, a.spec.listen_port, &a.spec.id).cmp(&(
                &b.spec.host_id,
                b.spec.listen_port,
                &b.spec.id,
            ))
        });
        out
    }

    pub async fn open(&self, request: ForwardOpenRequest) -> Result<ForwardId, String> {
        self.open_inner(request, false)
    }

    fn open_inner(&self, request: ForwardOpenRequest, saved: bool) -> Result<ForwardId, String> {
        let ForwardOpenRequest { spec, lifetime } = request;
        let id = spec.id.clone();
        {
            let mut st = self.0.st();
            if st.forwards.get(&id).is_some_and(|e| e.task.is_some()) {
                return Err(format!("Forward {} already exists", id.0));
            }
            st.forwards.insert(
                id.clone(),
                Entry {
                    spec,
                    lifetime,
                    state: ForwardState::Starting,
                    saved,
                    bound_port: None,
                    counters: Arc::default(),
                    cancel: CancellationToken::new(),
                    task: None,
                },
            );
        }
        self.spawn(&id);
        Ok(id)
    }

    /// (Re)starts the runner task of an existing entry in `Starting`.
    fn spawn(&self, id: &ForwardId) {
        let (spec, counters, cancel) = {
            let st = self.0.st();
            let Some(e) = st.forwards.get(id) else { return };
            (e.spec.clone(), e.counters.clone(), e.cancel.clone())
        };
        let task = tokio::spawn(run_forward(
            self.0.clone(),
            id.clone(),
            spec,
            counters,
            cancel,
        ));
        if let Some(e) = self.0.st().forwards.get_mut(id) {
            e.task = Some(task);
        }
        self.0.bump();
    }

    /// Cancels the runner and waits until its listeners are gone.
    async fn stop_task(&self, id: &ForwardId) -> Option<String> {
        let (cancel, task, host) = {
            let mut st = self.0.st();
            let e = st.forwards.get_mut(id)?;
            (e.cancel.clone(), e.task.take(), e.spec.host_id.clone())
        };
        cancel.cancel();
        if let Some(t) = task {
            let _ = t.await;
        }
        Some(host)
    }

    pub async fn close(&self, id: ForwardId) -> Result<(), String> {
        let host = self.stop_task(&id).await.ok_or("Unknown forward")?;
        self.0.st().forwards.remove(&id);
        self.0.release_if_idle(&host);
        self.0.bump();
        Ok(())
    }

    pub async fn restart(&self, id: ForwardId) -> Result<(), String> {
        self.stop_task(&id).await.ok_or("Unknown forward")?;
        {
            let mut st = self.0.st();
            let e = st.forwards.get_mut(&id).ok_or("Unknown forward")?;
            e.state = ForwardState::Starting;
            e.bound_port = None;
            e.counters = Arc::default();
            e.cancel = CancellationToken::new();
        }
        self.spawn(&id);
        Ok(())
    }

    /// Delivers the user's answer to a pending prompt; the forwards waiting on
    /// it go back to `Starting`.
    pub fn answer_prompt(
        &self,
        id: ForwardId,
        request_id: PromptRequestId,
        answer: PromptAnswer,
    ) -> Result<(), String> {
        let host = {
            let st = self.0.st();
            let e = st.forwards.get(&id).ok_or("Unknown forward")?;
            match &e.state {
                ForwardState::AwaitingInput(p) if p.request_id == request_id => {
                    e.spec.host_id.clone()
                }
                _ => return Err("No pending prompt".into()),
            }
        };
        if !self.0.broker.answer(&request_id, answer) {
            return Err("The prompt is no longer pending".into());
        }
        self.0.set_host_state(
            &host,
            |s| matches!(s, ForwardState::AwaitingInput(p) if p.request_id == request_id),
            ForwardState::Starting,
            false,
        );
        Ok(())
    }

    pub async fn attach_host(&self, host_id: String) -> Result<(), String> {
        *self.0.st().attached.entry(host_id.clone()).or_insert(0) += 1;
        for saved in self.0.factory.saved(&host_id) {
            if !saved.auto_start {
                continue;
            }
            let spec = saved.into_spec(&host_id);
            let exists = self.0.st().forwards.contains_key(&spec.id);
            if !exists {
                self.open_inner(
                    ForwardOpenRequest {
                        spec,
                        lifetime: ForwardLifetime::WithConnection,
                    },
                    true,
                )?;
            }
        }
        Ok(())
    }

    pub async fn detach_host(&self, host_id: String) -> Result<(), String> {
        let ids: Vec<ForwardId> = {
            let mut st = self.0.st();
            let Some(n) = st.attached.get_mut(&host_id) else {
                return Ok(());
            };
            *n = n.saturating_sub(1);
            if *n > 0 {
                return Ok(());
            }
            st.attached.remove(&host_id);
            st.forwards
                .values()
                .filter(|e| {
                    e.spec.host_id == host_id && e.lifetime == ForwardLifetime::WithConnection
                })
                .map(|e| e.spec.id.clone())
                .collect()
        };
        for id in ids {
            let _ = self.close(id).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Default)]
    struct MockFactory {
        connects: AtomicUsize,
        live: Arc<AtomicUsize>,
        fail: Mutex<Option<String>>,
        saved: Mutex<Vec<labonair_forwards::SavedForward>>,
        links: Mutex<Vec<Arc<tokio::sync::Notify>>>,
        refuse_remote: AtomicBool,
        sessions: Mutex<Vec<std::sync::Weak<MockSession>>>,
        /// Simulated connect latency.
        delay: Mutex<Duration>,
        /// Ask this prompt during every connect.
        prompt: Mutex<Option<labonair_ssh::PromptKind>>,
    }
    impl MockFactory {
        /// Simulates the transport of the newest session going down.
        fn session(&self, i: usize) -> Arc<MockSession> {
            self.sessions.lock().unwrap()[i]
                .upgrade()
                .expect("session alive")
        }
        fn drop_link(&self) {
            self.links.lock().unwrap().last().unwrap().notify_one();
        }
    }
    struct MockSession {
        live: Arc<AtomicUsize>,
        link: Arc<tokio::sync::Notify>,
        dispatch: RemoteForwardDispatch,
        refuse: bool,
        requests: Mutex<Vec<(String, u16)>>,
        cancelled: Mutex<Vec<(String, u16)>>,
    }
    impl Drop for MockSession {
        fn drop(&mut self) {
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }
    impl ForwardSession for MockSession {
        fn open_direct_tcpip<'a>(
            &'a self,
            _h: &'a str,
            _p: u16,
        ) -> BoxFuture<'a, Result<BoxedDuplexStream, String>> {
            Box::pin(async {
                // Echo server on the far end of an in-memory pipe.
                let (near, mut far) = tokio::io::duplex(1024);
                tokio::spawn(async move {
                    let mut buf = [0u8; 256];
                    while let Ok(n) = far.read(&mut buf).await {
                        if n == 0 || far.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                });
                Ok(Box::new(near) as BoxedDuplexStream)
            })
        }
        fn closed(&self) -> BoxFuture<'static, ()> {
            let n = self.link.clone();
            Box::pin(async move { n.notified().await })
        }
        fn remote_dispatch(&self) -> &RemoteForwardDispatch {
            &self.dispatch
        }
        fn request_remote_forward<'a>(
            &'a self,
            h: &'a str,
            p: u16,
        ) -> BoxFuture<'a, Result<u16, String>> {
            Box::pin(async move {
                if self.refuse {
                    return Err(REMOTE_REFUSED.to_string());
                }
                let bound = if p == 0 { 40_000 } else { p };
                self.requests.lock().unwrap().push((h.to_string(), bound));
                Ok(bound)
            })
        }
        fn cancel_remote_forward<'a>(&'a self, h: &'a str, p: u16) -> BoxFuture<'a, ()> {
            Box::pin(async move {
                self.cancelled.lock().unwrap().push((h.to_string(), p));
            })
        }
    }
    impl HostSessionFactory for MockFactory {
        fn connect<'a>(
            &'a self,
            _h: &'a str,
            prompts: ForwardPrompts,
        ) -> BoxFuture<'a, Result<Arc<dyn ForwardSession>, String>> {
            Box::pin(async move {
                self.connects.fetch_add(1, Ordering::SeqCst);
                let delay = *self.delay.lock().unwrap();
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                let kind = self.prompt.lock().unwrap().clone();
                if let Some(kind) = kind {
                    match prompts.broker.ask(&*prompts.sink, "mock", kind, None).await {
                        Some(PromptAnswer::Trust(true) | PromptAnswer::Secret(Some(_))) => {}
                        Some(_) => return Err("host key rejected".into()),
                        None => return Err(NO_ANSWER.into()),
                    }
                }
                if let Some(e) = self.fail.lock().unwrap().clone() {
                    return Err(e);
                }
                self.live.fetch_add(1, Ordering::SeqCst);
                let n = Arc::new(tokio::sync::Notify::new());
                self.links.lock().unwrap().push(n.clone());
                let s = Arc::new(MockSession {
                    live: self.live.clone(),
                    link: n,
                    dispatch: RemoteForwardDispatch::default(),
                    refuse: self.refuse_remote.load(Ordering::SeqCst),
                    requests: Mutex::default(),
                    cancelled: Mutex::default(),
                });
                self.sessions.lock().unwrap().push(Arc::downgrade(&s));
                Ok(s as Arc<dyn ForwardSession>)
            })
        }
        fn saved(&self, _h: &str) -> Vec<labonair_forwards::SavedForward> {
            self.saved.lock().unwrap().clone()
        }
    }

    fn req(id: &str, host: &str, port: u16, lifetime: ForwardLifetime) -> ForwardOpenRequest {
        ForwardOpenRequest {
            spec: ForwardSpec {
                id: ForwardId(id.into()),
                host_id: host.into(),
                kind: ForwardKind::Local,
                name: None,
                listen_host: "127.0.0.1".into(),
                listen_port: port,
                target_host: "db".into(),
                target_port: 5432,
            },
            lifetime,
        }
    }

    fn setup() -> (Arc<MockFactory>, ForwardRegistry) {
        let f = Arc::new(MockFactory::default());
        (f.clone(), ForwardRegistry::new(f))
    }

    async fn wait_state(
        r: &ForwardRegistry,
        id: &str,
        f: impl Fn(&ForwardState) -> bool,
    ) -> ForwardInfo {
        for _ in 0..200 {
            if let Some(i) = r.list().into_iter().find(|i| i.spec.id.0 == id) {
                if f(&i.state) {
                    return i;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("state not reached for {id}: {:?}", r.list());
    }

    fn is_active(s: &ForwardState) -> bool {
        matches!(s, ForwardState::Active)
    }

    #[tokio::test]
    async fn port_zero_reports_bound_port_and_moves_bytes() {
        let (_f, r) = setup();
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        let info = wait_state(&r, "a", is_active).await;
        let port = info.bound_port.expect("bound port");
        assert_ne!(port, 0);

        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        c.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
        let live = wait_state(&r, "a", |_| true).await;
        assert_eq!(live.connections, 1);
        drop(c);
        // counters are published through the coalesced revision, but readable at once
        for _ in 0..100 {
            let i = &r.list()[0];
            if i.connections == 0 && i.bytes_out == 5 && i.bytes_in == 5 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("counters not updated: {:?}", r.list());
    }

    #[tokio::test]
    async fn port_in_use_fails_with_reason() {
        let (f, r) = setup();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        r.open(req("a", "h", port, ForwardLifetime::Pinned))
            .await
            .unwrap();
        let i = wait_state(&r, "a", |s| matches!(s, ForwardState::Failed { .. })).await;
        assert_eq!(
            i.state,
            ForwardState::Failed {
                reason: format!("Port {port} is already in use")
            }
        );
        assert_eq!(f.connects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn session_is_shared_and_dropped_with_last_forward() {
        let (f, r) = setup();
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        r.open(req("b", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_state(&r, "a", is_active).await;
        wait_state(&r, "b", is_active).await;
        assert_eq!(f.connects.load(Ordering::SeqCst), 1);
        assert_eq!(f.live.load(Ordering::SeqCst), 1);
        r.close(ForwardId("a".into())).await.unwrap();
        assert_eq!(f.live.load(Ordering::SeqCst), 1);
        r.close(ForwardId("b".into())).await.unwrap();
        assert_eq!(f.live.load(Ordering::SeqCst), 0);
        assert!(r.list().is_empty());
    }

    #[tokio::test]
    async fn attach_detach_ref_counts_and_pinned_survive() {
        let (f, r) = setup();
        f.saved.lock().unwrap().extend(labonair_forwards::parse_saved(&Some(
            r#"[{"id":"s1","listen_host":"127.0.0.1","listen_port":0,"target_host":"x","target_port":1},
                {"id":"s2","listen_port":0,"target_host":"x","target_port":1,"auto_start":false}]"#
                .into(),
        )));
        r.open(req("p", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        r.attach_host("h".into()).await.unwrap();
        r.attach_host("h".into()).await.unwrap();
        wait_state(&r, "s1", is_active).await;
        assert!(r.list().iter().all(|i| i.spec.id.0 != "s2"));
        assert!(r.list().iter().find(|i| i.spec.id.0 == "s1").unwrap().saved);
        r.detach_host("h".into()).await.unwrap();
        assert!(r.list().iter().any(|i| i.spec.id.0 == "s1"));
        r.detach_host("h".into()).await.unwrap();
        let ids: Vec<_> = r.list().into_iter().map(|i| i.spec.id.0).collect();
        assert_eq!(ids, vec!["p"]);
        wait_state(&r, "p", is_active).await;
        assert_eq!(f.live.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn restart_rebinds_and_connect_failure_fails_all() {
        let (f, r) = setup();
        *f.fail.lock().unwrap() = Some("auth denied".into());
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        r.open(req("b", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        for id in ["a", "b"] {
            let i = wait_state(&r, id, |s| matches!(s, ForwardState::Failed { .. })).await;
            assert_eq!(
                i.state,
                ForwardState::Failed {
                    reason: "auth denied".into()
                }
            );
        }
        *f.fail.lock().unwrap() = None;
        r.restart(ForwardId("a".into())).await.unwrap();
        wait_state(&r, "a", is_active).await;
        r.restart(ForwardId("a".into())).await.unwrap();
        wait_state(&r, "a", is_active).await;
        assert!(r.restart(ForwardId("zz".into())).await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn counter_bumps_are_coalesced_state_bumps_are_immediate() {
        let (_f, r) = setup();
        let mut rev = r.revision();
        let start = *rev.borrow_and_update();
        r.0.touch();
        r.0.touch();
        r.0.touch();
        tokio::task::yield_now().await;
        assert_eq!(*rev.borrow(), start);
        tokio::time::sleep(Duration::from_millis(260)).await;
        assert_eq!(*rev.borrow(), start + 1);
        r.0.bump();
        assert_eq!(*rev.borrow(), start + 2);
    }

    #[tokio::test]
    async fn list_is_sorted() {
        let (_f, r) = setup();
        r.open(req("d", "b", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        r.open(req("m", "a", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_state(&r, "d", is_active).await;
        wait_state(&r, "m", is_active).await;
        let ids: Vec<_> = r.list().into_iter().map(|i| i.spec.id.0).collect();
        assert_eq!(ids, vec!["m", "d"]);
    }

    // ---- SOCKS5 ----

    #[test]
    fn socks_greeting_parser() {
        assert_eq!(parse_greeting(&[]), Ok(None));
        assert_eq!(parse_greeting(&[5]), Ok(None));
        assert_eq!(parse_greeting(&[5, 2, 0]), Ok(None)); // truncated
        assert_eq!(parse_greeting(&[5, 1, 0]), Ok(Some(3)));
        assert_eq!(parse_greeting(&[5, 2, 2, 0]), Ok(Some(4)));
        assert_eq!(
            parse_greeting(&[5, 1, 2]),
            Err(SocksError::NoAcceptableMethod)
        );
        assert_eq!(parse_greeting(&[5, 0]), Err(SocksError::NoAcceptableMethod));
        assert_eq!(parse_greeting(&[4, 1, 0]), Err(SocksError::Version));
        assert_eq!(parse_greeting(&[4]), Err(SocksError::Version));
    }

    #[test]
    fn socks_request_parser() {
        let v4 = [5, 1, 0, 1, 10, 0, 0, 7, 0x1F, 0x90];
        assert_eq!(parse_request(&v4), Ok(Some(("10.0.0.7".into(), 8080))));
        let mut v6 = vec![5, 1, 0, 4];
        v6.extend(std::net::Ipv6Addr::LOCALHOST.octets());
        v6.extend([0, 80]);
        assert_eq!(parse_request(&v6), Ok(Some(("::1".into(), 80))));
        let dom = [5, 1, 0, 3, 2, b'd', b'b', 0x15, 0x38];
        assert_eq!(parse_request(&dom), Ok(Some(("db".into(), 5432))));
        // every strict prefix is incomplete, never an error or a panic
        for n in 0..dom.len() {
            assert_eq!(parse_request(&dom[..n]), Ok(None), "prefix {n}");
        }
        for n in 0..v6.len() {
            assert_eq!(parse_request(&v6[..n]), Ok(None), "prefix {n}");
        }
        assert_eq!(
            parse_request(&[5, 1, 0, 3, 0, 0, 80]),
            Err(SocksError::BadAddress)
        );
        let mut max = vec![5, 1, 0, 3, 255];
        max.extend(vec![b'a'; 255]);
        assert_eq!(parse_request(&max), Ok(None)); // port still missing
        max.extend([0, 1]);
        assert_eq!(parse_request(&max).unwrap().unwrap().0.len(), 255);
        assert_eq!(
            parse_request(&[5, 1, 0, 3, 2, 0xff, 0xfe, 0, 1]),
            Err(SocksError::BadAddress)
        );
        assert_eq!(parse_request(&[5, 2, 0, 1]), Err(SocksError::Command)); // BIND
        assert_eq!(parse_request(&[5, 3, 0, 1]), Err(SocksError::Command)); // UDP
        assert_eq!(
            parse_request(&[5, 1, 0, 9, 0]),
            Err(SocksError::AddressType)
        );
        assert_eq!(parse_request(&[4, 1, 0, 1]), Err(SocksError::Version));
        assert_eq!(SocksError::Command.reply(), 7);
        assert_eq!(socks_open_reply("Connection refused"), 5);
        assert_eq!(socks_open_reply("No route to host"), 4);
        assert_eq!(socks_open_reply("boom"), 1);
    }

    async fn socks_client(port: u16) -> TcpStream {
        TcpStream::connect(("127.0.0.1", port)).await.unwrap()
    }

    #[tokio::test]
    async fn socks_connect_round_trip_and_rejections() {
        let (_f, r) = setup();
        let mut d = req("d", "h", 0, ForwardLifetime::Pinned);
        d.spec.kind = ForwardKind::Dynamic;
        r.open(d).await.unwrap();
        let port = wait_state(&r, "d", is_active).await.bound_port.unwrap();

        let mut c = socks_client(port).await;
        c.write_all(&[5, 1, 0]).await.unwrap();
        let mut m = [0u8; 2];
        c.read_exact(&mut m).await.unwrap();
        assert_eq!(m, [5, 0]);
        c.write_all(&[5, 1, 0, 3, 2, b'd', b'b', 0x15, 0x38])
            .await
            .unwrap();
        let mut rep = [0u8; 10];
        c.read_exact(&mut rep).await.unwrap();
        assert_eq!(rep[..2], [5, 0]);
        c.write_all(b"ping").await.unwrap();
        let mut echo = [0u8; 4];
        c.read_exact(&mut echo).await.unwrap();
        assert_eq!(&echo, b"ping");
        drop(c);

        // no acceptable method
        let mut c = socks_client(port).await;
        c.write_all(&[5, 1, 2]).await.unwrap();
        c.read_exact(&mut m).await.unwrap();
        assert_eq!(m, [5, 0xFF]);

        // BIND is rejected with 0x07
        let mut c = socks_client(port).await;
        c.write_all(&[5, 1, 0]).await.unwrap();
        c.read_exact(&mut m).await.unwrap();
        c.write_all(&[5, 2, 0, 1, 1, 2, 3, 4, 0, 80]).await.unwrap();
        c.read_exact(&mut rep).await.unwrap();
        assert_eq!(rep[1], 7);
    }

    // ---- health & reconnect ----

    /// Paused-clock variant of `wait_state` (backoff sleeps are virtual).
    async fn wait_long(
        r: &ForwardRegistry,
        id: &str,
        f: impl Fn(&ForwardState) -> bool,
    ) -> ForwardInfo {
        for _ in 0..2000 {
            if let Some(i) = r.list().into_iter().find(|i| i.spec.id.0 == id) {
                if f(&i.state) {
                    return i;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("state not reached for {id}: {:?}", r.list());
    }

    #[test]
    fn backoff_schedule() {
        let secs: Vec<u64> = (1..=8).map(|a| backoff(a).as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(backoff(0), Duration::from_secs(1));
        assert_eq!(backoff(u32::MAX), Duration::from_secs(30));
    }

    #[tokio::test(start_paused = true)]
    async fn lost_session_reconnects_and_goes_active_again() {
        let (f, r) = setup();
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_long(&r, "a", is_active).await;
        *f.fail.lock().unwrap() = Some("down".into());
        f.drop_link();
        wait_long(&r, "a", |s| matches!(s, ForwardState::Reconnecting { .. })).await;
        let port = r.list()[0].bound_port.unwrap();
        // Clear the failure only once an attempt has actually failed. The real
        // connect below lets the paused clock auto-advance, so without this the
        // first attempt could run after the clear and succeed (connects == 2).
        for _ in 0..200 {
            if f.connects.load(Ordering::SeqCst) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            TcpStream::connect(("127.0.0.1", port)).await.is_ok(),
            "listener stays bound"
        );
        *f.fail.lock().unwrap() = None;
        wait_long(&r, "a", is_active).await;
        assert!(f.connects.load(Ordering::SeqCst) >= 3);
        // traffic flows through the new session
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(b"x").await.unwrap();
        let mut b = [0u8; 1];
        c.read_exact(&mut b).await.unwrap();
        // a second loss is handled too
        f.drop_link();
        wait_long(&r, "a", |s| matches!(s, ForwardState::Reconnecting { .. })).await;
        wait_long(&r, "a", is_active).await;
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_max_attempts() {
        let (f, r) = setup();
        r.set_reconnect_policy(ReconnectPolicy {
            enabled: true,
            max_attempts: 3,
        });
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_long(&r, "a", is_active).await;
        *f.fail.lock().unwrap() = Some("down".into());
        f.drop_link();
        let i = wait_long(&r, "a", |s| matches!(s, ForwardState::Failed { .. })).await;
        assert_eq!(
            i.state,
            ForwardState::Failed {
                reason: "Connection lost: down".into()
            }
        );
        assert_eq!(f.connects.load(Ordering::SeqCst), 1 + 3);
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(f.live.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_policy_fails_immediately() {
        let (f, r) = setup();
        r.set_reconnect_policy(ReconnectPolicy {
            enabled: false,
            max_attempts: 10,
        });
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_long(&r, "a", is_active).await;
        f.drop_link();
        let i = wait_long(&r, "a", |s| matches!(s, ForwardState::Failed { .. })).await;
        assert_eq!(
            i.state,
            ForwardState::Failed {
                reason: "Connection lost".into()
            }
        );
        assert_eq!(f.connects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn deliberate_close_does_not_reconnect() {
        let (f, r) = setup();
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_long(&r, "a", is_active).await;
        r.close(ForwardId("a".into())).await.unwrap();
        f.drop_link();
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert_eq!(f.connects.load(Ordering::SeqCst), 1);
        assert!(r.list().is_empty());
    }

    // ---- single-flight connect ----

    #[tokio::test(start_paused = true)]
    async fn concurrent_opens_connect_once() {
        let (f, r) = setup();
        *f.delay.lock().unwrap() = Duration::from_secs(5);
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        r.open(req("b", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_long(&r, "a", is_active).await;
        wait_long(&r, "b", is_active).await;
        assert_eq!(f.connects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn open_during_reconnect_does_not_connect_a_second_session() {
        let (f, r) = setup();
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_long(&r, "a", is_active).await;
        *f.fail.lock().unwrap() = Some("down".into());
        f.drop_link();
        wait_long(&r, "a", |s| matches!(s, ForwardState::Reconnecting { .. })).await;
        r.open(req("b", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        let during = f.connects.load(Ordering::SeqCst);
        *f.fail.lock().unwrap() = None;
        wait_long(&r, "a", is_active).await;
        wait_long(&r, "b", is_active).await;
        // Only the monitor connected: initial + its attempts, one live session.
        assert!(f.connects.load(Ordering::SeqCst) <= during + 1);
        assert_eq!(f.live.load(Ordering::SeqCst), 1);
    }

    // ---- prompts ----

    fn pending_of(r: &ForwardRegistry, id: &str) -> PendingPrompt {
        match r
            .list()
            .into_iter()
            .find(|i| i.spec.id.0 == id)
            .unwrap()
            .state
        {
            ForwardState::AwaitingInput(p) => p,
            s => panic!("not awaiting input: {s:?}"),
        }
    }

    fn is_awaiting(s: &ForwardState) -> bool {
        matches!(s, ForwardState::AwaitingInput(_))
    }

    #[tokio::test(start_paused = true)]
    async fn prompt_marks_all_forwards_and_answer_resumes() {
        let (f, r) = setup();
        *f.prompt.lock().unwrap() = Some(labonair_ssh::PromptKind::Password);
        let rev = r.revision();
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        r.open(req("b", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_long(&r, "a", is_awaiting).await;
        wait_long(&r, "b", is_awaiting).await;
        assert!(rev.has_changed().unwrap());
        let p = pending_of(&r, "a");
        assert_eq!(p, pending_of(&r, "b"));
        assert!(r
            .answer_prompt(
                ForwardId("a".into()),
                PromptRequestId::new("nope".to_string()),
                PromptAnswer::Secret(Some("x".into()))
            )
            .is_err());
        r.answer_prompt(
            ForwardId("a".into()),
            p.request_id.clone(),
            PromptAnswer::Secret(Some("x".into())),
        )
        .unwrap();
        // Back to Starting for the whole host right away, then Active.
        assert!(r
            .list()
            .iter()
            .all(|i| matches!(i.state, ForwardState::Starting)));
        wait_long(&r, "a", is_active).await;
        wait_long(&r, "b", is_active).await;
        assert!(r
            .answer_prompt(
                ForwardId("a".into()),
                p.request_id,
                PromptAnswer::Secret(None)
            )
            .is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn unanswered_prompt_times_out_to_failed() {
        let (f, r) = setup();
        *f.prompt.lock().unwrap() = Some(labonair_ssh::PromptKind::Passphrase);
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_long(&r, "a", is_awaiting).await;
        tokio::time::sleep(Duration::from_secs(301)).await;
        let i = wait_long(&r, "a", |s| matches!(s, ForwardState::Failed { .. })).await;
        assert_eq!(
            i.state,
            ForwardState::Failed {
                reason: NO_ANSWER.into()
            }
        );
    }

    #[tokio::test(start_paused = true)]
    async fn rejected_prompt_fails_and_restart_retries() {
        let (f, r) = setup();
        *f.prompt.lock().unwrap() = Some(labonair_ssh::PromptKind::HostKey {
            host: "h".into(),
            fingerprint: "fp".into(),
            is_mismatch: false,
        });
        r.open(req("a", "h", 0, ForwardLifetime::Pinned))
            .await
            .unwrap();
        wait_long(&r, "a", is_awaiting).await;
        let p = pending_of(&r, "a");
        r.answer_prompt(
            ForwardId("a".into()),
            p.request_id,
            PromptAnswer::Trust(false),
        )
        .unwrap();
        let i = wait_long(&r, "a", |s| matches!(s, ForwardState::Failed { .. })).await;
        assert_eq!(
            i.state,
            ForwardState::Failed {
                reason: "host key rejected".into()
            }
        );
        // Restart asks again; this time it is trusted.
        r.restart(ForwardId("a".into())).await.unwrap();
        wait_long(&r, "a", is_awaiting).await;
        let p = pending_of(&r, "a");
        r.answer_prompt(
            ForwardId("a".into()),
            p.request_id,
            PromptAnswer::Trust(true),
        )
        .unwrap();
        wait_long(&r, "a", is_active).await;
        assert_eq!(f.connects.load(Ordering::SeqCst), 2);
    }

    // ---- remote forwards ----

    fn remote_req(id: &str, listen_port: u16, target_port: u16) -> ForwardOpenRequest {
        let mut r = req(id, "h", listen_port, ForwardLifetime::Pinned);
        r.spec.kind = ForwardKind::Remote;
        r.spec.listen_host = "0.0.0.0".into();
        r.spec.target_host = "127.0.0.1".into();
        r.spec.target_port = target_port;
        r
    }

    fn target(port: u16) -> RemoteTarget {
        RemoteTarget {
            host: "127.0.0.1".into(),
            port,
            counters: Arc::default(),
            touch: Arc::new(|| {}),
        }
    }

    #[test]
    fn dispatch_lookup_exact_then_unique_port() {
        let d = RemoteForwardDispatch::default();
        d.insert("localhost", 2222, target(1));
        d.insert("0.0.0.0", 3333, target(2));
        assert_eq!(d.lookup("localhost", 2222).unwrap().port, 1);
        assert_eq!(d.lookup("127.0.0.1", 2222).unwrap().port, 1);
        assert!(d.lookup("x", 9).is_none());
        d.insert("::", 3333, target(3));
        assert_eq!(d.lookup("0.0.0.0", 3333).unwrap().port, 2);
        assert!(d.lookup("other", 3333).is_none(), "ambiguous port");
        d.remove("localhost", 2222);
        assert!(d.lookup("localhost", 2222).is_none());
    }

    #[tokio::test]
    async fn remote_forward_bridges_injected_channel() {
        let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_port = echo.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = echo.accept().await {
                tokio::spawn(async move {
                    let mut b = [0u8; 64];
                    while let Ok(n) = s.read(&mut b).await {
                        if n == 0 || s.write_all(&b[..n]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        let (f, r) = setup();
        r.open(remote_req("r", 0, echo_port)).await.unwrap();
        let info = wait_state(&r, "r", is_active).await;
        assert_eq!(
            info.bound_port,
            Some(40_000),
            "port 0 reports the server's pick"
        );
        let session = f.session(0);
        assert_eq!(
            *session.requests.lock().unwrap(),
            vec![("0.0.0.0".to_string(), 40_000)]
        );

        let (mut far, near) = tokio::io::duplex(1024);
        session.dispatch.serve("0.0.0.0", 40_000, Box::new(near));
        far.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        far.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
        drop(far);
        for _ in 0..100 {
            let i = &r.list()[0];
            if i.connections == 0 && i.bytes_in == 4 && i.bytes_out == 4 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("counters not updated: {:?}", r.list());
    }

    #[tokio::test]
    async fn remote_connect_failure_closes_channel_only() {
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_port = dead.local_addr().unwrap().port();
        drop(dead);
        let (f, r) = setup();
        r.open(remote_req("r", 7000, dead_port)).await.unwrap();
        wait_state(&r, "r", is_active).await;
        let session = f.session(0);
        let (mut far, near) = tokio::io::duplex(64);
        session.dispatch.serve("0.0.0.0", 7000, Box::new(near));
        let mut b = [0u8; 1];
        assert_eq!(far.read(&mut b).await.unwrap(), 0, "channel closed");
        assert!(is_active(&r.list()[0].state));
    }

    #[tokio::test]
    async fn remote_refusal_fails_with_reason() {
        let (f, r) = setup();
        f.refuse_remote.store(true, Ordering::SeqCst);
        r.open(remote_req("r", 7000, 1)).await.unwrap();
        let i = wait_state(&r, "r", |s| matches!(s, ForwardState::Failed { .. })).await;
        assert_eq!(
            i.state,
            ForwardState::Failed {
                reason: REMOTE_REFUSED.into()
            }
        );
    }

    #[tokio::test]
    async fn remote_close_cancels_and_unregisters() {
        let (f, r) = setup();
        r.open(remote_req("r", 7000, 1)).await.unwrap();
        wait_state(&r, "r", is_active).await;
        let session = f.session(0);
        assert!(session.dispatch.lookup("0.0.0.0", 7000).is_some());
        r.close(ForwardId("r".into())).await.unwrap();
        assert_eq!(
            *session.cancelled.lock().unwrap(),
            vec![("0.0.0.0".to_string(), 7000)]
        );
        assert!(session.dispatch.lookup("0.0.0.0", 7000).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn remote_forward_is_rerequested_after_reconnect() {
        let (f, r) = setup();
        r.open(remote_req("r", 7000, 1)).await.unwrap();
        wait_long(&r, "r", is_active).await;
        f.drop_link();
        wait_long(&r, "r", |s| matches!(s, ForwardState::Reconnecting { .. })).await;
        wait_long(&r, "r", is_active).await;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if f.sessions.lock().unwrap().len() == 2
                && !f.session(1).requests.lock().unwrap().is_empty()
            {
                break;
            }
        }
        let second = f.session(1);
        assert_eq!(
            *second.requests.lock().unwrap(),
            vec![("0.0.0.0".to_string(), 7000)]
        );
        assert!(second.dispatch.lookup("0.0.0.0", 7000).is_some());
    }
}
