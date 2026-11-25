// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! In-process tests of the SSH / SFTP / transfer side of the detached session
//! host: the real server on a temp socket with fake services injected through
//! the `HostServices` seam (plus one run against the concrete stack).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use labonair_errors::LabonairError;
use labonair_events::EventBus;
use labonair_sftp::{
    BoxFuture, RemoteEntry, SftpBrowserService, SftpSessionHandle, SftpSessionService,
};
use labonair_ssh::{
    ForwardId, ForwardInfo, ForwardOpenRequest, ForwardState, PromptAnswer, PromptRequestId,
    SharedSshEventSink, SshConnectRequest, SshConnectionService, SshExecOutput, SshExecService,
    SshForwardService, SshPtyService, SshRemoteCommandService, SshRemoteFileService,
    SshSessionEvent, SshSessionId,
};
use labonair_terminal_core::{TermDimensions, TerminalColors, TerminalEmulator, TerminalEvent};
use labonair_terminal_host::protocol::*;
use labonair_terminal_host::server::*;
use labonair_transfers::{
    TransferDirection, TransferJob, TransferPromptId, TransferRequest, TransferResolution,
    TransferService, TransferSettings, TransferStatus,
};
use tokio::io::AsyncWriteExt;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::sync::{oneshot, Notify};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

static COUNTER: AtomicUsize = AtomicUsize::new(0);
const WAIT: Duration = Duration::from_secs(20);

// ── Fake services ────────────────────────────────────────────────────────────

#[derive(Default)]
struct Fake {
    events: EventBus,
    sinks: Mutex<HashMap<String, SharedSshEventSink>>,
    writes: Mutex<Vec<String>>,
    resizes: Mutex<Vec<(u32, u32)>>,
    trust_calls: Mutex<Vec<(String, bool)>>,
    trust_waiters: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    disconnects: Mutex<Vec<String>>,
    answers: Mutex<Vec<(String, String, labonair_ssh::PromptAnswer)>>,
    answer_waiters: Mutex<HashMap<String, oneshot::Sender<()>>>,
    installed: Mutex<Vec<SshHostSnapshot>>,
    resolutions: Mutex<Vec<(String, TransferResolution)>>,
    resolve_waiters: Mutex<HashMap<String, oneshot::Sender<()>>>,
    finish: Arc<Notify>,
    jobs: AtomicUsize,
    settings: Arc<TransferSettings>,
    fwd: Mutex<Vec<ForwardInfo>>,
    fwd_rev: Rev,
    fwd_calls: Mutex<Vec<String>>,
    policies: Mutex<Vec<bool>>,
}

struct Rev(tokio::sync::watch::Sender<u64>);

impl Default for Rev {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(0).0)
    }
}

impl Fake {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn services(self: &Arc<Self>) -> HostServices {
        let install = {
            let fake = Arc::clone(self);
            Arc::new(move |snapshot: &SshHostSnapshot| {
                fake.installed.lock().unwrap().push(snapshot.clone());
                Ok(())
            })
        };
        HostServices {
            events: self.events.clone(),
            connection: self.clone(),
            pty: self.clone(),
            remote_command: self.clone(),
            remote_file: self.clone(),
            exec: self.clone(),
            forwards: self.clone(),
            sftp_session: self.clone(),
            sftp_browser: self.clone(),
            transfers: self.clone(),
            transfer_settings: Arc::clone(&self.settings),
            install_host: install,
        }
    }

    /// Remote output for an SSH session (as the transport reader would send).
    fn push_output(&self, ssh_id: &str, text: &str) {
        let sink = self.sinks.lock().unwrap().get(ssh_id).cloned().unwrap();
        sink.send(SshSessionEvent::Output { data: text.into() })
            .unwrap();
    }

    /// Add or replace a forward (as the engine would) and bump the revision.
    fn set_forward(&self, info: ForwardInfo) {
        {
            let mut all = self.fwd.lock().unwrap();
            all.retain(|f| f.spec.id != info.spec.id);
            all.push(info);
        }
        self.fwd_rev.0.send_modify(|rev| *rev += 1);
    }

    fn remove_forward(&self, id: &str) {
        self.fwd.lock().unwrap().retain(|f| f.spec.id.0 != id);
        self.fwd_rev.0.send_modify(|rev| *rev += 1);
    }

    fn job(id: &str, status: TransferStatus) -> TransferJob {
        TransferJob {
            id: id.into(),
            session_id: "tab-1".into(),
            src_path: "/tmp/src".into(),
            dest_path: "/remote/dest".into(),
            direction: TransferDirection::Upload,
            status,
            bytes_total: 10,
            bytes_transferred: 0,
            speed_bps: 0.0,
            skipped_count: 0,
        }
    }
}

impl SshConnectionService for Fake {
    fn connect<'a>(
        &'a self,
        request: SshConnectRequest,
        events: SharedSshEventSink,
    ) -> BoxFuture<'a, Result<(), LabonairError>> {
        Box::pin(async move {
            let sid = request.session_id.as_str().to_string();
            if request.host_id == "hop-host" {
                let (tx, rx) = oneshot::channel();
                self.answer_waiters.lock().unwrap().insert(sid.clone(), tx);
                self.events
                    .emit(
                        "hop_prompt",
                        serde_json::json!({
                            "session_id": sid,
                            "pending": {
                                "request_id": "req-1",
                                "kind": "Password",
                                "hop": {"index": 1, "total": 2, "label": "bastion"}
                            }
                        }),
                    )
                    .unwrap();
                let _ = rx.await;
            }
            if request.host_id == "prompt-host" {
                let (tx, rx) = oneshot::channel();
                self.trust_waiters.lock().unwrap().insert(sid.clone(), tx);
                self.events
                    .emit(
                        "known_hosts_warning",
                        serde_json::json!({
                            "session_id": sid, "fingerprint": "aa:bb",
                            "host": "prompt-host", "is_mismatch": false
                        }),
                    )
                    .unwrap();
                if !matches!(rx.await, Ok(true)) {
                    return Err(LabonairError::HostKeyMismatch("User rejected host".into()));
                }
            }
            self.sinks
                .lock()
                .unwrap()
                .insert(sid.clone(), events.clone());
            let _ = self.events.emit(
                "session_established",
                serde_json::json!({ "session_id": sid, "default_path_ssh": "/home/me" }),
            );
            events
                .send(SshSessionEvent::Output {
                    data: "ready\r\n".into(),
                })
                .unwrap();
            Ok(())
        })
    }

    fn disconnect<'a>(&'a self, session_id: SshSessionId) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.disconnects
                .lock()
                .unwrap()
                .push(session_id.as_str().to_string());
            Ok(())
        })
    }

    fn answer_prompt<'a>(
        &'a self,
        session_id: SshSessionId,
        request_id: labonair_ssh::PromptRequestId,
        answer: labonair_ssh::PromptAnswer,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let sid = session_id.as_str().to_string();
            self.answers.lock().unwrap().push((
                sid.clone(),
                request_id.as_str().to_string(),
                answer,
            ));
            if let Some(tx) = self.answer_waiters.lock().unwrap().remove(&sid) {
                let _ = tx.send(());
            }
            Ok(())
        })
    }

    fn trust_host<'a>(
        &'a self,
        session_id: SshSessionId,
        _prompt_request_id: Option<labonair_ssh::PromptRequestId>,
        accepted: bool,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let sid = session_id.as_str().to_string();
            self.trust_calls
                .lock()
                .unwrap()
                .push((sid.clone(), accepted));
            if let Some(tx) = self.trust_waiters.lock().unwrap().remove(&sid) {
                let _ = tx.send(accepted);
            }
            Ok(())
        })
    }
}

impl SshPtyService for Fake {
    fn write<'a>(
        &'a self,
        session_id: SshSessionId,
        data: String,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.writes.lock().unwrap().push(data.clone());
            // The remote shell echoes what it is typed.
            let sink = self.sinks.lock().unwrap().get(session_id.as_str()).cloned();
            if let Some(sink) = sink {
                let _ = sink.send(SshSessionEvent::Output { data });
            }
            Ok(())
        })
    }

    fn resize<'a>(
        &'a self,
        _session_id: SshSessionId,
        cols: u32,
        rows: u32,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.resizes.lock().unwrap().push((cols, rows));
            Ok(())
        })
    }
}

impl SshRemoteCommandService for Fake {
    fn chown<'a>(
        &'a self,
        _: SshSessionId,
        _: String,
        _: String,
        _: String,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }

    fn calculate_size<'a>(
        &'a self,
        _: SshSessionId,
        path: String,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("size of {path}")) })
    }
}

impl SshRemoteFileService for Fake {
    fn prepare_remote_edit<'a>(
        &'a self,
        _: SshSessionId,
        _: String,
        _: Option<u64>,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("/tmp/edit".into()) })
    }

    fn save_remote_edit<'a>(
        &'a self,
        _: SshSessionId,
        _: String,
        _: String,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }

    fn cleanup_remote_edit_temp<'a>(&'a self, _: String) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

impl SshExecService for Fake {
    fn exec<'a>(
        &'a self,
        session_id: SshSessionId,
        command: String,
        stdin: Option<Vec<u8>>,
    ) -> BoxFuture<'a, Result<SshExecOutput, String>> {
        Box::pin(async move {
            if session_id.as_str() == "gone" {
                return Err("no SSH session for tab gone".into());
            }
            Ok(SshExecOutput {
                stdout: [command.as_bytes(), b"|", &stdin.unwrap_or_default()].concat(),
                stderr: vec![0xff, b'e'],
                exit_code: 3,
            })
        })
    }
}

impl SshForwardService for Fake {
    fn set_reconnect_policy(&self, enabled: bool) {
        self.policies.lock().unwrap().push(enabled);
    }

    fn open<'a>(&'a self, request: ForwardOpenRequest) -> BoxFuture<'a, Result<ForwardId, String>> {
        Box::pin(async move {
            let id = request.spec.id.clone();
            self.fwd_calls
                .lock()
                .unwrap()
                .push(format!("open:{}", id.0));
            self.set_forward(forward(&id.0, "h", ForwardState::Active));
            Ok(id)
        })
    }

    fn close<'a>(&'a self, id: ForwardId) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.fwd_calls
                .lock()
                .unwrap()
                .push(format!("close:{}", id.0));
            self.remove_forward(&id.0);
            Ok(())
        })
    }

    fn restart<'a>(&'a self, id: ForwardId) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.fwd_calls
                .lock()
                .unwrap()
                .push(format!("restart:{}", id.0));
            Ok(())
        })
    }

    fn answer_prompt<'a>(
        &'a self,
        id: ForwardId,
        _: PromptRequestId,
        _: PromptAnswer,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.fwd_calls
                .lock()
                .unwrap()
                .push(format!("answer:{}", id.0));
            Ok(())
        })
    }

    fn attach_host<'a>(&'a self, host_id: String) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.fwd_calls
                .lock()
                .unwrap()
                .push(format!("attach:{host_id}"));
            Ok(())
        })
    }

    fn detach_host<'a>(&'a self, host_id: String) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.fwd_calls
                .lock()
                .unwrap()
                .push(format!("detach:{host_id}"));
            Ok(())
        })
    }

    fn list(&self) -> Vec<ForwardInfo> {
        self.fwd.lock().unwrap().clone()
    }

    fn revision(&self) -> tokio::sync::watch::Receiver<u64> {
        self.fwd_rev.0.subscribe()
    }
}

/// A forward as the engine reports it (origin `App`: the daemon must
/// overwrite it on the wire).
fn forward(id: &str, host: &str, state: ForwardState) -> ForwardInfo {
    ForwardInfo {
        spec: labonair_ssh::ForwardSpec {
            id: ForwardId(id.into()),
            host_id: host.into(),
            kind: labonair_ssh::ForwardKind::Local,
            name: None,
            listen_host: "127.0.0.1".into(),
            listen_port: 8080,
            target_host: "db".into(),
            target_port: 5432,
        },
        state,
        lifetime: labonair_ssh::ForwardLifetime::Pinned,
        origin: labonair_ssh::ForwardOrigin::App,
        bound_port: Some(8080),
        connections: 0,
        bytes_in: 0,
        bytes_out: 0,
        saved: false,
    }
}

impl SftpSessionService for Fake {
    fn open<'a>(
        &'a self,
        ssh_session: SshSessionId,
    ) -> BoxFuture<'a, Result<SftpSessionHandle, LabonairError>> {
        Box::pin(async move { Ok(SftpSessionHandle::from_ssh_session(ssh_session)) })
    }

    fn close<'a>(&'a self, _: SftpSessionHandle) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

impl SftpBrowserService for Fake {
    fn read_dir<'a>(
        &'a self,
        _: SftpSessionHandle,
        path: String,
    ) -> BoxFuture<'a, Result<Vec<RemoteEntry>, LabonairError>> {
        Box::pin(async move {
            if path == "/missing" {
                return Err(LabonairError::NotFound("no such directory".into()));
            }
            Ok(vec![RemoteEntry {
                name: "f".into(),
                path: format!("{path}/f"),
                size: 3,
                modified_at: 4,
                is_dir: false,
                is_symlink: false,
                symlink_target: None,
                permissions: "rw-r--r--".into(),
            }])
        })
    }

    fn rename<'a>(
        &'a self,
        _: SftpSessionHandle,
        _: String,
        _: String,
    ) -> BoxFuture<'a, Result<(), LabonairError>> {
        Box::pin(async { Ok(()) })
    }

    fn delete<'a>(
        &'a self,
        _: SftpSessionHandle,
        _: Vec<String>,
    ) -> BoxFuture<'a, Result<(), LabonairError>> {
        Box::pin(async { Ok(()) })
    }

    fn mkdir<'a>(
        &'a self,
        _: SftpSessionHandle,
        _: String,
        _: bool,
    ) -> BoxFuture<'a, Result<(), LabonairError>> {
        Box::pin(async { Ok(()) })
    }

    fn create_file<'a>(
        &'a self,
        _: SftpSessionHandle,
        _: String,
    ) -> BoxFuture<'a, Result<(), LabonairError>> {
        Box::pin(async { Ok(()) })
    }

    fn chmod<'a>(
        &'a self,
        _: SftpSessionHandle,
        _: String,
        _: u32,
    ) -> BoxFuture<'a, Result<(), LabonairError>> {
        Box::pin(async { Ok(()) })
    }
}

impl TransferService for Fake {
    /// A job that runs until the test calls `finish`. A source path containing
    /// "conflict" asks about a conflict first and waits for `resolve`.
    fn enqueue<'a>(&'a self, request: TransferRequest) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let id = format!("job-{}", self.jobs.fetch_add(1, Ordering::SeqCst) + 1);
            let events = self.events.clone();
            let job_id = id.clone();
            let (tx, rx) = oneshot::channel();
            self.resolve_waiters.lock().unwrap().insert(id.clone(), tx);
            let finish = Arc::clone(&self.finish);
            let conflict = request.src_path.contains("conflict");
            tokio::spawn(async move {
                let mut job = Fake::job(&job_id, TransferStatus::Running);
                job.src_path = request.src_path;
                job.dest_path = request.dest_path;
                let _ = events.emit("transfer_progress", &job);
                if conflict {
                    let _ = events.emit(
                        "file_conflict",
                        serde_json::json!({
                            "job_id": job_id, "src_path": job.src_path, "dest_path": job.dest_path
                        }),
                    );
                    let _ = rx.await;
                    let _ = events.emit("transfer_progress", &job);
                }
                finish.notified().await;
                job.status = TransferStatus::Completed;
                let _ = events.emit("transfer_progress", &job);
            });
            Ok(id)
        })
    }

    fn cancel<'a>(&'a self, _: String) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }

    fn resolve<'a>(
        &'a self,
        job_id: String,
        resolution: TransferResolution,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.resolutions
                .lock()
                .unwrap()
                .push((job_id.clone(), resolution));
            if let Some(tx) = self.resolve_waiters.lock().unwrap().remove(&job_id) {
                let _ = tx.send(());
            }
            Ok(())
        })
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Harness {
    dir: PathBuf,
    socket: PathBuf,
    handle: ServerHandle,
    task: JoinHandle<ExitReason>,
    _cleanup: DirGuard,
}

impl Harness {
    async fn start(services: Option<HostServices>) -> Self {
        Self::start_with(services, |_| {}).await
    }

    async fn start_with(
        services: Option<HostServices>,
        tweak: impl FnOnce(&mut ServerConfig),
    ) -> Self {
        let dir = PathBuf::from(format!(
            "/tmp/lhs-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let mut cfg = ServerConfig::new(&dir);
        if let Some(services) = services {
            cfg = cfg.with_services(services);
        }
        tweak(&mut cfg);
        let server = Server::bind(cfg).expect("bind");
        let socket = server.paths().socket.clone();
        let handle = server.handle();
        let task = tokio::spawn(server.run());
        Self {
            _cleanup: DirGuard(dir.clone()),
            dir,
            socket,
            handle,
            task,
        }
    }

    async fn client(&self) -> Client {
        self.client_with_protocol(PROTO_VERSION).await
    }

    async fn client_with_protocol(&self, proto_version: u32) -> Client {
        let stream = UnixStream::connect(&self.socket).await.expect("connect");
        let (read, write) = stream.into_split();
        let mut client = Client {
            read,
            write,
            screen: Screen::new(80, 24),
            msgs: Vec::new(),
            events: Vec::new(),
            next_req: 1,
        };
        let mut hello = Hello::current("test", None);
        hello.proto_version = proto_version;
        hello.min_compat = proto_version;
        client.send(Message::Hello(hello)).await;
        client.wait_msg(|m| matches!(m, Message::HelloAck(_))).await;
        client
    }

    /// A client that already subscribed to service events.
    async fn ui_client(&self) -> Client {
        let mut client = self.client().await;
        client.rpc(RpcCall::SubscribeEvents).await.unwrap();
        client
    }

    async fn shutdown(self) -> ExitReason {
        self.handle.stop(ExitReason::Shutdown);
        timeout(WAIT, self.task).await.unwrap().unwrap()
    }
}

struct Screen {
    emulator: TerminalEmulator,
    events: Receiver<TerminalEvent>,
}

impl Screen {
    fn new(cols: u16, rows: u16) -> Self {
        let (tx, events) = sync_channel(4096);
        let emulator = TerminalEmulator::new(
            TerminalColors::xterm_default(),
            TermDimensions::new(usize::from(cols), usize::from(rows)),
            tx,
        );
        Self { emulator, events }
    }

    fn feed(&mut self, bytes: &[u8]) {
        let _ = self.emulator.feed(bytes);
        let _ = self.emulator.take_pty_output();
        while self.events.try_recv().is_ok() {}
    }

    fn text(&self) -> String {
        self.emulator.render().to_text()
    }
}

struct Client {
    read: OwnedReadHalf,
    write: OwnedWriteHalf,
    screen: Screen,
    msgs: Vec<Message>,
    /// Service events (`BusEvent`s) in arrival order: `(name, payload)`.
    events: Vec<(String, serde_json::Value)>,
    next_req: u64,
}

impl Client {
    async fn send(&mut self, message: Message) {
        let bytes = control_frame(&message).unwrap();
        self.write.write_all(&bytes).await.unwrap();
    }

    async fn input(&mut self, session: SessionId, text: &str) {
        let bytes = data_frame(DataKind::Input, session, text.as_bytes());
        self.write.write_all(&bytes).await.unwrap();
    }

    async fn step(&mut self) {
        let frame = timeout(WAIT, read_frame(&mut self.read))
            .await
            .expect("timed out waiting for a frame")
            .expect("protocol error")
            .expect("host closed the connection");
        match frame {
            Frame::Data {
                kind: DataKind::Output | DataKind::Snapshot,
                data,
                ..
            } => self.screen.feed(&data),
            Frame::Data { .. } => {}
            Frame::Control(Message::BusEvent { name, payload }) => {
                self.events.push((name, payload));
            }
            Frame::Control(message) => {
                if let Message::Attached { cols, rows, .. } = message {
                    self.screen = Screen::new(cols, rows);
                }
                self.msgs.push(message);
            }
        }
    }

    async fn wait_msg(&mut self, pred: impl Fn(&Message) -> bool) -> Message {
        loop {
            if let Some(i) = self.msgs.iter().position(&pred) {
                return self.msgs.remove(i);
            }
            self.step().await;
        }
    }

    async fn wait_event(&mut self, name: &str) -> serde_json::Value {
        loop {
            if let Some(i) = self.events.iter().position(|(n, _)| n == name) {
                return self.events.remove(i).1;
            }
            self.step().await;
        }
    }

    async fn wait_decision(&mut self, name: &str, actionable: bool) -> serde_json::Value {
        loop {
            let payload = self.wait_event(name).await;
            if payload
                .get("actionable")
                .and_then(serde_json::Value::as_bool)
                == Some(actionable)
            {
                return payload;
            }
        }
    }

    async fn wait_text(&mut self, needle: &str) {
        let result = timeout(WAIT, async {
            while !self.screen.text().contains(needle) {
                self.step().await;
            }
        })
        .await;
        assert!(
            result.is_ok(),
            "{needle:?} never appeared; screen:\n{}",
            self.screen.text()
        );
    }

    async fn send_rpc(&mut self, call: RpcCall) -> u64 {
        let req_id = self.next_req;
        self.next_req += 1;
        self.send(Message::Rpc { req_id, call }).await;
        req_id
    }

    async fn reply(&mut self, req_id: u64) -> Result<RpcValue, WireError> {
        let message = self
            .wait_msg(|m| match m {
                Message::RpcOk { req_id: r, .. } | Message::RpcErr { req_id: r, .. } => {
                    *r == req_id
                }
                _ => false,
            })
            .await;
        match message {
            Message::RpcOk { value, .. } => Ok(value),
            Message::RpcErr { error, .. } => Err(error),
            _ => unreachable!(),
        }
    }

    async fn rpc(&mut self, call: RpcCall) -> Result<RpcValue, WireError> {
        let req_id = self.send_rpc(call).await;
        self.reply(req_id).await
    }

    async fn connect_ssh(&mut self, tab: &str, host: &str) -> Result<SessionId, WireError> {
        match self.rpc(connect_call(tab, host)).await? {
            RpcValue::SshConnected { session } => Ok(session),
            other => panic!("unexpected {other:?}"),
        }
    }

    async fn sessions(&mut self) -> Vec<SessionInfo> {
        self.send(Message::ListSessions).await;
        match self
            .wait_msg(|m| matches!(m, Message::Sessions { .. }))
            .await
        {
            Message::Sessions { sessions } => sessions,
            _ => unreachable!(),
        }
    }
}

fn connect_call(tab: &str, host: &str) -> RpcCall {
    RpcCall::SshConnect(SshConnectParams {
        session_id: tab.into(),
        host_id: host.into(),
        password: Some("hunter2-password".into()),
        // A pane owns the session: only those are listed by `ListSessions`.
        pane_tag: Some("pane-1".into()),
        host: SshHostSnapshot {
            hosts: vec![HostRecord {
                id: host.into(),
                host_address: "127.0.0.1".into(),
                port: 1,
                username: "me".into(),
                auth_method: "password".into(),
                ..Default::default()
            }],
            secrets: vec![SecretEntry {
                service: "labonair-app".into(),
                account: host.into(),
                value: "hunter2-secret".into(),
            }],
            ..Default::default()
        },
        ..Default::default()
    })
}

async fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    let result = timeout(WAIT, async {
        while !cond() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(result.is_ok(), "timed out waiting for {what}");
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn ssh_shell_uses_the_attach_snapshot_and_live_machinery() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let mut ui = host.ui_client().await;

    let session = ui.connect_ssh("tab-1", "host-1").await.unwrap();
    // The request's host rows and secrets reached the stack, in RAM.
    {
        let installed = fake.installed.lock().unwrap();
        assert_eq!(installed[0].hosts[0].id, "host-1");
        assert_eq!(installed[0].secrets[0].value, "hunter2-secret");
    }
    let established = ui.wait_event("session_established").await;
    assert_eq!(established["session_id"], "tab-1");

    // Output that arrived before the attach is in the snapshot.
    ui.send(Message::Attach {
        session,
        cols: 100,
        rows: 30,
        attachment_id: None,
    })
    .await;
    ui.wait_msg(|m| matches!(m, Message::Attached { .. })).await;
    ui.wait_text("ready").await;
    // ... and the remote PTY was resized to the attach size (wiggle included).
    eventually("PTY resize to the attach size", || {
        fake.resizes.lock().unwrap().last() == Some(&(100, 30))
    })
    .await;

    // Input goes to the remote shell, its echo comes back live.
    ui.input(session, "abc").await;
    ui.wait_text("abc").await;
    assert_eq!(fake.writes.lock().unwrap().concat(), "abc");

    // Output produced while detached shows up in the next snapshot.
    ui.send(Message::Detach { session }).await;
    fake.push_output("tab-1", "while away\r\n");
    ui.send(Message::Attach {
        session,
        cols: 100,
        rows: 30,
        attachment_id: None,
    })
    .await;
    ui.wait_msg(|m| matches!(m, Message::Attached { .. })).await;
    ui.wait_text("while away").await;

    // The session is listed like any other and by `ListSshSessions`.
    let listed = ui.sessions().await;
    assert!(listed
        .iter()
        .any(|s| s.id == session && s.status == SessionStatus::Running));
    let Ok(RpcValue::SshSessions(ssh)) = ui.rpc(RpcCall::ListSshSessions).await else {
        panic!("ssh sessions");
    };
    assert_eq!(ssh[0].ssh_session_id, "tab-1");
    assert!(ssh[0].connected);

    // Kill disconnects the transport.
    ui.send(Message::Kill { session }).await;
    eventually("transport disconnect", || {
        fake.disconnects
            .lock()
            .unwrap()
            .contains(&"tab-1".to_string())
    })
    .await;
    host.shutdown().await;
}

#[tokio::test]
async fn connection_loss_is_reported_and_the_session_waits_for_ack() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let mut ui = host.ui_client().await;
    let session = ui.connect_ssh("tab-1", "host-1").await.unwrap();
    ui.send(Message::Attach {
        session,
        cols: 80,
        rows: 24,
        attachment_id: None,
    })
    .await;
    ui.wait_msg(|m| matches!(m, Message::Attached { .. })).await;

    fake.events
        .emit(
            "ssh_connection_lost",
            serde_json::json!({ "session_id": "tab-1", "reason": "Connection timed out" }),
        )
        .unwrap();
    let lost = ui
        .wait_msg(|m| matches!(m, Message::ConnectionLost { .. }))
        .await;
    assert_eq!(
        lost,
        Message::ConnectionLost {
            session,
            reason: "Connection timed out".into()
        }
    );
    let listed = ui.sessions().await;
    let info = listed.iter().find(|s| s.id == session).unwrap();
    assert_eq!(info.status, SessionStatus::Exited);

    // No silent reconnect: nothing new was connected, the old entry stays.
    let Ok(RpcValue::SshSessions(ssh)) = ui.rpc(RpcCall::ListSshSessions).await else {
        panic!("ssh sessions");
    };
    assert!(!ssh[0].connected);
    assert_eq!(ssh[0].lost_reason.as_deref(), Some("Connection timed out"));

    // A fresh client attaching later still learns why.
    let mut later = host.client().await;
    later
        .send(Message::Attach {
            session,
            cols: 80,
            rows: 24,
            attachment_id: None,
        })
        .await;
    later
        .wait_msg(|m| matches!(m, Message::ConnectionLost { .. }))
        .await;

    ui.send(Message::Ack { session }).await;
    eventually("ack removes the session", || {
        host.handle.session_count() == 0
    })
    .await;
    host.shutdown().await;
}

#[tokio::test]
async fn host_key_prompt_goes_to_the_ui_and_fails_closed_without_one() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;

    // A subscribed client sees the prompt and answers it.
    let mut ui = host.ui_client().await;
    let req = ui.send_rpc(connect_call("tab-1", "prompt-host")).await;
    let prompt = ui.wait_event("known_hosts_warning").await;
    assert_eq!(prompt["session_id"], "tab-1");
    let request_id = prompt["prompt_request_id"]
        .as_str()
        .expect("prompt has unique request id")
        .to_string();
    let mut intruder = host.client().await;
    assert!(intruder
        .rpc(RpcCall::SshTrustHost {
            session_id: "tab-1".into(),
            prompt_request_id: Some(labonair_ssh::PromptRequestId::new(request_id.clone())),
            accepted: true,
        })
        .await
        .is_err());
    ui.rpc(RpcCall::SshTrustHost {
        session_id: "tab-1".into(),
        prompt_request_id: Some(labonair_ssh::PromptRequestId::new(request_id)),
        accepted: true,
    })
    .await
    .unwrap();
    assert!(matches!(
        ui.reply(req).await,
        Ok(RpcValue::SshConnected { .. })
    ));

    // No client subscribed: rejected immediately, nothing connects.
    ui.rpc(RpcCall::UnsubscribeEvents).await.unwrap();
    let mut headless = host.client().await;
    let err = headless
        .rpc(connect_call("tab-2", "prompt-host"))
        .await
        .unwrap_err();
    assert_eq!(err.code.as_deref(), Some("HostKeyMismatch"));
    assert!(fake
        .trust_calls
        .lock()
        .unwrap()
        .contains(&("tab-2".to_string(), false)));

    // A prompt belongs to the connection that started the SSH request. A
    // different subscribed UI does not receive it or inherit its authority.
    ui.rpc(RpcCall::SubscribeEvents).await.unwrap();
    let mut doomed = host.ui_client().await;
    doomed.send_rpc(connect_call("tab-3", "prompt-host")).await;
    doomed.wait_event("known_hosts_warning").await;
    drop(doomed);
    sleep(Duration::from_millis(300)).await;
    assert!(fake
        .trust_calls
        .lock()
        .unwrap()
        .contains(&("tab-3".to_string(), false)));
    ui.rpc(RpcCall::UnsubscribeEvents).await.unwrap();
    eventually("prompt rejected after the last client left", || {
        fake.trust_calls
            .lock()
            .unwrap()
            .contains(&("tab-3".to_string(), false))
    })
    .await;
    eventually("failed connects leave no session", || {
        host.handle.session_count() == 1 // only tab-1
    })
    .await;
    host.shutdown().await;
}

#[tokio::test]
async fn transfers_run_unattended_park_on_conflicts_and_hold_the_daemon_open() {
    let fake = Fake::new();
    let host = Harness::start_with(Some(fake.services()), |cfg| {
        cfg.idle_timeout = Duration::from_millis(300);
    })
    .await;

    let mut ui = host.ui_client().await;
    ui.rpc(RpcCall::TransferConfigure(TransferConfig {
        max_concurrent: 2,
        chunk_size: 65536,
        default_conflict_resolution: "overwrite".into(),
        on_folder_file_error: "ask".into(),
    }))
    .await
    .unwrap();
    // Subscribed: the user's explicit setting is honoured ...
    assert_eq!(
        *fake.settings.default_conflict_resolution.lock().unwrap(),
        "overwrite"
    );

    // Relative and empty local paths are refused.
    for (src, dest) in [("relative/file", "/remote/x"), ("", "/remote/x")] {
        let err = ui
            .rpc(RpcCall::TransferEnqueue {
                session_id: "tab-1".into(),
                src_path: src.into(),
                dest_path: dest.into(),
                direction: TransferDirection::Upload,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code.as_deref(), Some("InvalidInput"));
    }
    let Ok(RpcValue::JobId(job)) = ui
        .rpc(RpcCall::TransferEnqueue {
            session_id: "tab-1".into(),
            src_path: "/tmp/conflict-file".into(),
            dest_path: "/remote/x".into(),
            direction: TransferDirection::Upload,
        })
        .await
    else {
        panic!("job id");
    };
    let _original_prompt = ui.wait_decision("file_conflict", true).await;

    // The UI goes away. The job is parked, nothing resolves it, and the
    // daemon (idle timeout 300 ms) stays up because the job is not finished.
    drop(ui);
    eventually("unattended policy", || {
        *fake.settings.default_conflict_resolution.lock().unwrap() == "ask"
    })
    .await;
    sleep(Duration::from_millis(1200)).await;
    assert!(!host.task.is_finished(), "idle exit must wait for the job");
    assert!(fake.resolutions.lock().unwrap().is_empty());

    // A fresh UI sees the parked job and its pending conflict.
    let mut ui = host.ui_client().await;
    let progress = ui.wait_event("transfer_progress").await;
    assert_eq!(progress["id"], job.as_str());
    let conflict = ui.wait_event("file_conflict").await;
    assert_eq!(conflict["job_id"], job.as_str());
    assert_eq!(conflict["actionable"], false);
    assert_eq!(conflict["recoverable"], true);
    let Ok(RpcValue::Transfers(snapshots)) = ui.rpc(RpcCall::ListTransfers).await else {
        panic!("snapshots");
    };
    assert!(snapshots[0].conflict.is_some());
    assert_eq!(snapshots[0].job.status, TransferStatus::Paused);

    // The parked decision needs an explicit takeover after its original
    // owner disconnects.
    ui.rpc(RpcCall::TransferRecoverConflict {
        job_id: job.clone(),
    })
    .await
    .unwrap();
    let prompt = ui.wait_decision("file_conflict", true).await;
    assert_ne!(
        _original_prompt.get("prompt_request_id"),
        prompt.get("prompt_request_id")
    );

    // Only an explicit decision resumes it; then it finishes and the daemon
    // may idle out.
    ui.rpc(RpcCall::TransferResolve {
        job_id: job.clone(),
        resolution: WireResolution::Skip,
        prompt_request_id: prompt
            .get("prompt_request_id")
            .and_then(serde_json::Value::as_str)
            .map(|id| TransferPromptId::new(id.to_string())),
    })
    .await
    .unwrap();
    assert!(ui
        .rpc(RpcCall::TransferResolve {
            job_id: job.clone(),
            resolution: WireResolution::Skip,
            prompt_request_id: prompt
                .get("prompt_request_id")
                .and_then(serde_json::Value::as_str)
                .map(|id| TransferPromptId::new(id.to_string())),
        })
        .await
        .is_err());
    assert_eq!(
        fake.resolutions.lock().unwrap().as_slice(),
        &[(job, TransferResolution::Skip)]
    );
    drop(ui);
    fake.finish.notify_one();
    let reason = timeout(WAIT, host.task).await.expect("idle exit").unwrap();
    assert_eq!(reason, ExitReason::Idle);
}

#[tokio::test]
async fn only_the_transfer_owner_can_resolve_until_disconnect_and_takeover() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let mut owner = host.ui_client().await;
    let mut observer = host.ui_client().await;
    let Ok(RpcValue::JobId(job)) = owner
        .rpc(RpcCall::TransferEnqueue {
            session_id: "tab-1".into(),
            src_path: "/tmp/conflict-file".into(),
            dest_path: "/remote/x".into(),
            direction: TransferDirection::Upload,
        })
        .await
    else {
        panic!("job id")
    };
    let _owner_prompt = owner.wait_decision("file_conflict", true).await;
    let observer_event = observer.wait_decision("file_conflict", false).await;
    assert_eq!(observer_event["job_id"], job);
    assert_eq!(observer_event["recoverable"], false);

    assert!(observer
        .rpc(RpcCall::TransferResolve {
            job_id: job.clone(),
            resolution: WireResolution::Skip,
            prompt_request_id: None,
        })
        .await
        .is_err());
    assert!(observer
        .rpc(RpcCall::TransferRecoverConflict {
            job_id: job.clone(),
        })
        .await
        .is_err());

    drop(owner);
    observer
        .rpc(RpcCall::TransferRecoverConflict {
            job_id: job.clone(),
        })
        .await
        .unwrap();
    let prompt = observer.wait_decision("file_conflict", true).await;
    assert_ne!(
        _owner_prompt.get("prompt_request_id"),
        prompt.get("prompt_request_id")
    );
    observer
        .rpc(RpcCall::TransferResolve {
            job_id: job.clone(),
            resolution: WireResolution::Skip,
            prompt_request_id: prompt
                .get("prompt_request_id")
                .and_then(serde_json::Value::as_str)
                .map(|id| TransferPromptId::new(id.to_string())),
        })
        .await
        .unwrap();
    assert_eq!(
        fake.resolutions.lock().unwrap().as_slice(),
        &[(job, TransferResolution::Skip)]
    );
    fake.finish.notify_waiters();
    host.shutdown().await;
}

#[tokio::test]
async fn v1_transfer_clients_only_receive_their_own_conflict_prompt() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let mut owner = host.client_with_protocol(1).await;
    let mut observer = host.client_with_protocol(1).await;
    owner.rpc(RpcCall::SubscribeEvents).await.unwrap();
    observer.rpc(RpcCall::SubscribeEvents).await.unwrap();

    let Ok(RpcValue::JobId(job)) = owner
        .rpc(RpcCall::TransferEnqueue {
            session_id: "tab-1".into(),
            src_path: "/tmp/conflict-file".into(),
            dest_path: "/remote/x".into(),
            direction: TransferDirection::Upload,
        })
        .await
    else {
        panic!("job id")
    };
    let prompt = owner.wait_event("file_conflict").await;
    assert_eq!(prompt["job_id"], job);
    assert!(prompt["prompt_request_id"].is_string());
    assert!(timeout(
        Duration::from_millis(100),
        observer.wait_event("file_conflict")
    )
    .await
    .is_err());

    // A v1 response may omit the new token, but is still fenced to the
    // originating connection and consumed once by the host.
    owner
        .rpc(RpcCall::TransferResolve {
            job_id: job.clone(),
            resolution: WireResolution::Skip,
            prompt_request_id: None,
        })
        .await
        .unwrap();
    assert!(owner
        .rpc(RpcCall::TransferResolve {
            job_id: job,
            resolution: WireResolution::Skip,
            prompt_request_id: None,
        })
        .await
        .is_err());
    fake.finish.notify_waiters();
    host.shutdown().await;
}

#[tokio::test]
async fn a_download_to_a_relative_local_path_is_refused() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let mut ui = host.ui_client().await;
    let err = ui
        .rpc(RpcCall::TransferEnqueue {
            session_id: "tab-1".into(),
            src_path: "/remote/x".into(),
            dest_path: "downloads/x".into(),
            direction: TransferDirection::Download,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code.as_deref(), Some("InvalidInput"));
    host.shutdown().await;
}

#[tokio::test]
async fn sftp_and_remote_calls_map_one_to_one_with_typed_errors() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let mut ui = host.ui_client().await;

    let Ok(RpcValue::SftpHandle(handle)) = ui
        .rpc(RpcCall::SftpOpen {
            ssh_session: "tab-1".into(),
        })
        .await
    else {
        panic!("handle");
    };
    assert_eq!(handle, "tab-1");
    let Ok(RpcValue::Entries(entries)) = ui
        .rpc(RpcCall::SftpReadDir {
            session: handle.clone(),
            path: "/srv".into(),
        })
        .await
    else {
        panic!("entries");
    };
    assert_eq!(entries[0].path, "/srv/f");
    let err = ui
        .rpc(RpcCall::SftpReadDir {
            session: handle,
            path: "/missing".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(
        err.into_labonair(),
        LabonairError::NotFound("no such directory".into())
    );
    let Ok(RpcValue::Text(size)) = ui
        .rpc(RpcCall::SshCalculateSize {
            session_id: "tab-1".into(),
            path: "/x".into(),
        })
        .await
    else {
        panic!("size");
    };
    assert_eq!(size, "size of /x");
    // The cleanup call deletes a local file: relative paths are refused.
    let err = ui
        .rpc(RpcCall::SshCleanupRemoteEditTemp {
            local_temp_path: "tmp/edit".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code.as_deref(), Some("InvalidInput"));
    host.shutdown().await;
}

#[tokio::test]
async fn a_terminal_only_host_refuses_service_calls() {
    let host = Harness::start(None).await;
    let mut client = host.client().await;
    let err = client.rpc(RpcCall::ListTransfers).await.unwrap_err();
    assert!(err.message.contains("no SSH"));
    host.shutdown().await;
}

/// The concrete stack: an unreachable host fails with a typed error, leaves no
/// session behind, and the credentials of the request never touch the disk.
#[tokio::test]
async fn concrete_stack_keeps_credentials_in_ram_only() {
    let services = labonair_terminal_host::stack::concrete_services().expect("stack");
    let host = Harness::start(Some(services)).await;
    let mut ui = host.ui_client().await;
    let err = ui.connect_ssh("tab-1", "host-1").await.unwrap_err();
    assert!(!err.message.is_empty());
    assert_eq!(host.handle.session_count(), 0);

    fn files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut all = Vec::new();
    files(&host.dir, &mut all);
    for path in all {
        if path.extension().is_some_and(|e| e == "sock") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("hunter2"),
            "secret found in {}",
            path.display()
        );
    }
    host.shutdown().await;
}

// ── App-side proxies over a real client ──────────────────────────────────────

use labonair_ssh::SshHostedAccess;
use labonair_terminal_host::client::HostClient;
use labonair_terminal_host::proxy::router::{DaemonAccess, LocalServices, RoutedServices};
use labonair_terminal_host::proxy::{DaemonServices, ProxyContext};

/// Collects what a connect call's output sink receives.
#[derive(Default)]
struct Collect(Mutex<String>);

impl labonair_ssh::SshEventSink for Collect {
    fn send(&self, event: SshSessionEvent) -> Result<(), String> {
        let SshSessionEvent::Output { data } = event;
        self.0.lock().unwrap().push_str(&data);
        Ok(())
    }
}

fn snapshot_for(host: &str) -> SshHostSnapshot {
    SshHostSnapshot {
        hosts: vec![HostRecord {
            id: host.into(),
            host_address: "127.0.0.1".into(),
            port: 1,
            username: "me".into(),
            auth_method: "password".into(),
            ..Default::default()
        }],
        secrets: vec![SecretEntry {
            service: "labonair-app".into(),
            account: host.into(),
            value: "hunter2-secret".into(),
        }],
        ..Default::default()
    }
}

/// A proxy on its own client connection to `host`, with a fresh app bus.
async fn proxy(host: &Harness) -> (DaemonServices, EventBus) {
    let client = HostClient::new(host.dir.clone(), Hello::current("proxy-test", None));
    let c = client.clone();
    tokio::task::spawn_blocking(move || c.reconnect())
        .await
        .unwrap()
        .expect("client connects");
    let bus = EventBus::new();
    let ctx = ProxyContext {
        bus: bus.clone(),
        hosts: Arc::new(|id, _route| Ok(snapshot_for(id))),
        transfer_config: Arc::new(|| TransferConfig {
            max_concurrent: 3,
            chunk_size: 4096,
            default_conflict_resolution: "ask".into(),
            on_folder_file_error: "ask".into(),
        }),
        connected: Arc::new(|_| {}),
    };
    (DaemonServices::new(client, ctx), bus)
}

fn request(tab: &str, host: &str) -> SshConnectRequest {
    SshConnectRequest {
        session_id: SshSessionId::new(tab),
        host_id: host.into(),
        passphrase: None,
        password: None,
        initial_cols: Some(80),
        initial_rows: Some(24),
        blocks: false,
        connect_timeout_secs: None,
        keepalive_interval_secs: None,
        keepalive_max_failures: None,
        route_override: None,
    }
}

async fn bus_event(
    rx: &mut tokio::sync::broadcast::Receiver<labonair_events::RawEvent>,
    name: &str,
) -> serde_json::Value {
    timeout(WAIT, async {
        loop {
            let event = rx.recv().await.expect("bus open");
            if event.name == name {
                return event.payload;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {name} event"))
}

async fn bus_actionable_decision(
    rx: &mut tokio::sync::broadcast::Receiver<labonair_events::RawEvent>,
    name: &str,
) -> serde_json::Value {
    timeout(WAIT, async {
        loop {
            let event = rx.recv().await.expect("bus open");
            if event.name == name && event.payload["actionable"] == true {
                return event.payload;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no actionable {name} event"))
}

async fn eventually_text(sink: &Collect, needle: &str) {
    let result = timeout(WAIT, async {
        while !sink.0.lock().unwrap().contains(needle) {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(result.is_ok(), "{needle:?} never arrived");
}

#[tokio::test(flavor = "multi_thread")]
async fn proxy_connects_through_the_trust_prompt_streams_output_and_disconnects() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let (daemon, bus) = proxy(&host).await;
    let mut events = bus.subscribe();
    let sink = Arc::new(Collect::default());

    // The connect waits for the host-key decision while other calls go on.
    let connect = {
        let (daemon, sink) = (daemon.clone(), sink.clone());
        tokio::spawn(async move { daemon.connect(request("tab-1", "prompt-host"), sink).await })
    };
    let warning = bus_event(&mut events, "known_hosts_warning").await;
    assert_eq!(warning["session_id"], "tab-1");
    assert_eq!(
        daemon
            .calculate_size(SshSessionId::new("tab-1"), "/x".into())
            .await
            .unwrap(),
        "size of /x"
    );
    daemon
        .trust_host(SshSessionId::new("tab-1"), None, true)
        .await
        .unwrap();
    connect.await.unwrap().expect("connect");

    // Credentials came from the app's snapshot; transfers were configured.
    assert_eq!(
        fake.installed.lock().unwrap()[0].secrets[0].value,
        "hunter2-secret"
    );
    assert_eq!(
        fake.settings
            .max_concurrent
            .load(std::sync::atomic::Ordering::SeqCst),
        3
    );
    assert_eq!(
        bus_event(&mut events, "session_established").await["session_id"],
        "tab-1"
    );
    assert!(daemon.owns("tab-1"));

    // Output (incl. what arrived before the attach) and the input echo.
    eventually_text(&sink, "ready").await;
    daemon
        .write(SshSessionId::new("tab-1"), "abc".into())
        .await
        .unwrap();
    eventually_text(&sink, "abc").await;
    let tap_session = daemon.agent_tap("tab-1");
    assert!(tap_session.is_some());

    // Disconnect ends the transport and forgets the session.
    daemon.disconnect(SshSessionId::new("tab-1")).await.unwrap();
    assert!(fake
        .disconnects
        .lock()
        .unwrap()
        .contains(&"tab-1".to_string()));
    assert!(!daemon.owns("tab-1"));
    host.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_host_key_fails_the_connect_with_a_typed_error() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let (daemon, bus) = proxy(&host).await;
    let mut events = bus.subscribe();
    let connect = {
        let daemon = daemon.clone();
        tokio::spawn(async move {
            daemon
                .connect(
                    request("tab-1", "prompt-host"),
                    Arc::new(Collect::default()),
                )
                .await
        })
    };
    bus_event(&mut events, "known_hosts_warning").await;
    daemon
        .trust_host(SshSessionId::new("tab-1"), None, false)
        .await
        .unwrap();
    let err = connect.await.unwrap().unwrap_err();
    assert_eq!(
        err,
        LabonairError::HostKeyMismatch("User rejected host".into())
    );
    assert!(!daemon.owns("tab-1"));
    host.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn exec_rpc_carries_binary_stdin_and_output() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let (daemon, _bus) = proxy(&host).await;
    let out = SshHostedAccess::exec(
        &daemon,
        SshSessionId::new("tab-1"),
        "git status".into(),
        Some(vec![0, 159, 146, 150]),
    )
    .await
    .unwrap();
    assert_eq!(
        out.stdout,
        [b"git status|".as_slice(), &[0, 159, 146, 150]].concat()
    );
    assert_eq!(out.stderr, vec![0xff, b'e']);
    assert_eq!(out.exit_code, 3);
    let err = SshHostedAccess::exec(&daemon, SshSessionId::new("gone"), "x".into(), None)
        .await
        .unwrap_err();
    assert!(err.contains("no SSH session"));
    host.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_transfer_keeps_running_while_the_app_is_away_and_its_conflict_is_replayed() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let (daemon, bus) = proxy(&host).await;
    let mut events = bus.subscribe();
    let job = TransferService::enqueue(
        &daemon,
        TransferRequest {
            session_id: "tab-1".into(),
            src_path: "/tmp/conflict-file".into(),
            dest_path: "/remote/x".into(),
            direction: TransferDirection::Upload,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        bus_event(&mut events, "file_conflict").await["job_id"],
        job.as_str()
    );

    // The app goes away: the job stays parked in the daemon.
    daemon.client().disconnect();
    sleep(Duration::from_millis(200)).await;
    assert!(fake.resolutions.lock().unwrap().is_empty());

    // A restarted app subscribes and sees the live job and its conflict.
    let (again, bus2) = proxy(&host).await;
    let mut events2 = bus2.subscribe();
    // Subscribed after construction: the replay may already be on the bus,
    // so ask the daemon's registry as well.
    let listed = again.client().rpc(RpcCall::ListTransfers).await.unwrap();
    let RpcValue::Transfers(jobs) = listed else {
        panic!("jobs")
    };
    assert_eq!(jobs[0].job.id, job);
    assert!(jobs[0].conflict.is_some());
    TransferService::recover_conflict(&again, job.clone())
        .await
        .unwrap();
    let prompt = bus_actionable_decision(&mut events2, "file_conflict").await;
    TransferService::resolve_prompt(
        &again,
        job.clone(),
        TransferResolution::Skip,
        prompt
            .get("prompt_request_id")
            .and_then(serde_json::Value::as_str)
            .map(|id| TransferPromptId::new(id.to_string())),
    )
    .await
    .unwrap();
    assert_eq!(
        fake.resolutions.lock().unwrap().as_slice(),
        &[(job, TransferResolution::Skip)]
    );
    fake.finish.notify_waiters();
    host.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn routed_services_keep_app_and_daemon_sessions_apart_and_fall_back_on_failure() {
    let remote = Fake::new();
    let local = Fake::new();
    local.jobs.store(10, Ordering::SeqCst);
    let host = Harness::start(Some(remote.services())).await;
    let (daemon, bus) = proxy(&host).await;
    let mut transfer_events = bus.subscribe();

    let detached = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let daemon_ok = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let fallbacks = Arc::new(Mutex::new(Vec::<String>::new()));
    let access = {
        let (d1, d2, ok) = (daemon.clone(), daemon.clone(), daemon_ok.clone());
        DaemonAccess {
            peek: Arc::new(move || Some(d1.clone())),
            connect: Arc::new(move |_| {
                if ok.load(Ordering::SeqCst) {
                    Ok(d2.clone())
                } else {
                    Err("helper missing".to_string())
                }
            }),
        }
    };
    let routed = RoutedServices::new(
        LocalServices {
            connection: local.clone(),
            pty: local.clone(),
            remote_command: local.clone(),
            remote_file: local.clone(),
            forwards: local.clone(),
            sftp_session: local.clone(),
            sftp_browser: local.clone(),
            transfers: local.clone(),
        },
        access,
        {
            let detached = detached.clone();
            Arc::new(move || detached.load(Ordering::SeqCst))
        },
        {
            let fallbacks = fallbacks.clone();
            Arc::new(move |why| fallbacks.lock().unwrap().push(why))
        },
    );

    // App mode: the daemon is never asked.
    let sink = Arc::new(Collect::default());
    routed
        .connect(request("tab-app", "host-1"), sink.clone())
        .await
        .unwrap();
    assert!(local.sinks.lock().unwrap().contains_key("tab-app"));
    assert!(remote.sinks.lock().unwrap().is_empty());

    // Detached mode: the next connection lives in the daemon; the first
    // stays where it was created.
    detached.store(true, Ordering::SeqCst);
    let sink2 = Arc::new(Collect::default());
    routed
        .connect(request("tab-det", "host-2"), sink2.clone())
        .await
        .unwrap();
    assert!(remote.sinks.lock().unwrap().contains_key("tab-det"));
    assert!(!local.sinks.lock().unwrap().contains_key("tab-det"));
    eventually_text(&sink2, "ready").await;

    routed
        .write(SshSessionId::new("tab-app"), "L".into())
        .await
        .unwrap();
    routed
        .write(SshSessionId::new("tab-det"), "D".into())
        .await
        .unwrap();
    assert_eq!(local.writes.lock().unwrap().concat(), "L");
    eventually("daemon write", || {
        remote.writes.lock().unwrap().concat() == "D"
    })
    .await;
    assert!(routed.is_hosted("tab-det") && !routed.is_hosted("tab-app"));

    // Transfer jobs follow the host of their ssh session.
    let request_for = |tab: &str| TransferRequest {
        session_id: tab.into(),
        src_path: "/tmp/a".into(),
        dest_path: "/remote/a".into(),
        direction: TransferDirection::Upload,
    };
    let app_job = routed.enqueue(request_for("tab-app")).await.unwrap();
    let mut detached_request = request_for("tab-det");
    detached_request.src_path = "/tmp/conflict-a".into();
    let det_job = routed.enqueue(detached_request).await.unwrap();
    assert_eq!((app_job.as_str(), det_job.as_str()), ("job-11", "job-1"));
    let conflict = bus_actionable_decision(&mut transfer_events, "file_conflict").await;
    let prompt_request_id = conflict
        .get("prompt_request_id")
        .and_then(serde_json::Value::as_str)
        .map(|id| TransferPromptId::new(id.to_string()));
    assert!(prompt_request_id.is_some());
    routed
        .resolve_prompt(det_job.clone(), TransferResolution::Skip, prompt_request_id)
        .await
        .unwrap();
    routed
        .resolve(app_job.clone(), TransferResolution::Overwrite)
        .await
        .unwrap();
    assert_eq!(
        remote.resolutions.lock().unwrap().as_slice(),
        &[(det_job, TransferResolution::Skip)]
    );
    assert_eq!(
        local.resolutions.lock().unwrap().as_slice(),
        &[(app_job, TransferResolution::Overwrite)]
    );

    // An unusable daemon: reported once, the connection is app-bound.
    daemon_ok.store(false, Ordering::SeqCst);
    // `peek` still sees the daemon, but a new session id is not owned by it.
    routed
        .connect(request("tab-fb", "host-3"), Arc::new(Collect::default()))
        .await
        .unwrap();
    assert!(local.sinks.lock().unwrap().contains_key("tab-fb"));
    assert_eq!(fallbacks.lock().unwrap().as_slice(), ["helper missing"]);
    host.shutdown().await;
}

#[tokio::test]
async fn hop_prompt_reaches_the_client_and_answer_reaches_the_service() {
    let fake = Fake::new();
    let host = Harness::start(Some(fake.services())).await;
    let mut ui = host.ui_client().await;
    let req = ui.send_rpc(connect_call("tab-h", "hop-host")).await;
    let prompt = ui.wait_event("hop_prompt").await;
    assert_eq!(prompt["session_id"], "tab-h");

    let answer = labonair_ssh::PromptAnswer::Secret(Some("hunter2".into()));
    let call = RpcCall::SshAnswerPrompt {
        session_id: "tab-h".into(),
        prompt_request_id: labonair_ssh::PromptRequestId::new("req-1"),
        answer: answer.clone(),
    };
    assert!(!format!("{call:?}").contains("hunter2"));
    assert!(!format!(
        "{:?}",
        Message::Rpc {
            req_id: 1,
            call: call.clone()
        }
    )
    .contains("hunter2"));

    // Only the connection owner may answer.
    let mut intruder = host.client().await;
    assert!(intruder.rpc(call.clone()).await.is_err());
    ui.rpc(call).await.unwrap();
    assert!(matches!(
        ui.reply(req).await,
        Ok(RpcValue::SshConnected { .. })
    ));
    {
        let answers = fake.answers.lock().unwrap();
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].0, "tab-h");
        assert_eq!(answers[0].1, "req-1");
        assert_eq!(answers[0].2, answer);
    }
    host.shutdown().await;
}

// ── Port forwards (R12 F6) ───────────────────────────────────────────────────

/// Wait for the next `forward_changed` of `id` and return it.
async fn changed_forward(client: &mut Client, id: &str) -> ForwardInfo {
    loop {
        let payload = client.wait_event("forward_changed").await;
        let info: ForwardInfo = serde_json::from_value(payload).unwrap();
        if info.spec.id.0 == id {
            return info;
        }
    }
}

fn forward_events(client: &Client, name: &str) -> usize {
    client.events.iter().filter(|(n, _)| n == name).count()
}

#[tokio::test]
async fn forwards_are_replayed_then_only_changes_are_pushed_with_daemon_origin() {
    let fake = Fake::new();
    fake.set_forward(forward("f1", "h", ForwardState::Active));
    fake.set_forward(forward("f2", "h", ForwardState::Starting));
    let host = Harness::start(Some(fake.services())).await;

    // Replay on subscribe: reset, then every forward once, origin Daemon.
    let mut ui = host.ui_client().await;
    assert_eq!(forward_events(&ui, FORWARD_RESET), 1);
    assert_eq!(forward_events(&ui, FORWARD_CHANGED), 2);
    for (name, payload) in &ui.events {
        if name == FORWARD_CHANGED {
            assert_eq!(payload["origin"], "Daemon");
        }
    }
    ui.events.clear();

    // A second client gets the same replay.
    let late = host.ui_client().await;
    assert_eq!(forward_events(&late, FORWARD_CHANGED), 2);

    // Diff: change f1 only, then remove f2. f2 is never re-sent.
    let mut busy = forward("f1", "h", ForwardState::Active);
    busy.connections = 3;
    fake.set_forward(busy);
    let info = changed_forward(&mut ui, "f1").await;
    assert_eq!(info.connections, 3);
    assert_eq!(info.origin, labonair_ssh::ForwardOrigin::Daemon);
    fake.remove_forward("f2");
    let removed = ui.wait_event(FORWARD_REMOVED).await;
    assert_eq!(removed["id"], "f2");
    assert_eq!(
        forward_events(&ui, FORWARD_CHANGED),
        0,
        "only changes are pushed"
    );

    // RPCs dispatch to the service with the host snapshot installed.
    let host_snapshot = snapshot_for("h");
    let request = labonair_ssh::ForwardOpenRequest {
        spec: forward("f3", "h", ForwardState::Active).spec,
        lifetime: labonair_ssh::ForwardLifetime::Pinned,
    };
    let Ok(RpcValue::Text(id)) = ui
        .rpc(RpcCall::SshForwardOpen {
            request,
            host: host_snapshot.clone(),
        })
        .await
    else {
        panic!("open");
    };
    assert_eq!(id, "f3");
    assert_eq!(fake.installed.lock().unwrap().last(), Some(&host_snapshot));
    let Ok(RpcValue::Forwards(all)) = ui.rpc(RpcCall::SshForwardList).await else {
        panic!("list");
    };
    assert_eq!(all.len(), 2);
    assert!(all
        .iter()
        .all(|f| f.origin == labonair_ssh::ForwardOrigin::Daemon));
    ui.rpc(RpcCall::SshForwardClose {
        id: ForwardId("f3".into()),
    })
    .await
    .unwrap();
    assert_eq!(ui.wait_event(FORWARD_REMOVED).await["id"], "f3");
    ui.rpc(RpcCall::SshForwardAttachHost {
        host_id: "h".into(),
        host: host_snapshot,
    })
    .await
    .unwrap();
    ui.rpc(RpcCall::SshForwardDetachHost {
        host_id: "h".into(),
    })
    .await
    .unwrap();
    assert!(fake
        .fwd_calls
        .lock()
        .unwrap()
        .ends_with(&["attach:h".to_string(), "detach:h".to_string()]));

    // A v2 subscriber is left alone.
    let mut old = host.client_with_protocol(2).await;
    old.rpc(RpcCall::SubscribeEvents).await.unwrap();
    assert_eq!(
        forward_events(&old, FORWARD_RESET) + forward_events(&old, FORWARD_CHANGED),
        0
    );
    host.shutdown().await;
}

#[tokio::test]
async fn a_running_forward_keeps_the_daemon_alive_until_it_stops() {
    let fake = Fake::new();
    fake.set_forward(forward(
        "f1",
        "h",
        ForwardState::Reconnecting { attempt: 1 },
    ));
    let mut host = Harness::start_with(Some(fake.services()), |cfg| {
        cfg.idle_timeout = Duration::from_millis(200);
    })
    .await;
    sleep(Duration::from_millis(900)).await;
    assert!(!host.task.is_finished(), "a live forward is busy");

    // Failed / Stopped forwards do not count.
    fake.set_forward(forward("f1", "h", ForwardState::Stopped));
    let reason = timeout(WAIT, &mut host.task)
        .await
        .expect("idle exit")
        .unwrap();
    assert_eq!(reason, ExitReason::Idle);
}

fn routed_over(
    local: &Arc<Fake>,
    daemon: &DaemonServices,
    detached: &Arc<std::sync::atomic::AtomicBool>,
) -> RoutedServices {
    let (d1, d2) = (daemon.clone(), daemon.clone());
    RoutedServices::new(
        LocalServices {
            connection: local.clone(),
            pty: local.clone(),
            remote_command: local.clone(),
            remote_file: local.clone(),
            forwards: local.clone(),
            sftp_session: local.clone(),
            sftp_browser: local.clone(),
            transfers: local.clone(),
        },
        DaemonAccess {
            peek: Arc::new(move || Some(d1.clone())),
            connect: Arc::new(move |_| Ok(d2.clone())),
        },
        {
            let detached = detached.clone();
            Arc::new(move || detached.load(Ordering::SeqCst))
        },
        Arc::new(|_| {}),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn routed_forwards_follow_origin_and_the_session_host_policy() {
    let remote = Fake::new();
    remote.set_forward(forward("f-daemon", "h", ForwardState::Active));
    let host = Harness::start(Some(remote.services())).await;
    let (daemon, _bus) = proxy(&host).await;
    // The replay fills the proxy cache.
    eventually("replayed forward", || daemon.list().len() == 1).await;
    assert_eq!(daemon.list()[0].origin, labonair_ssh::ForwardOrigin::Daemon);

    // A brand-new router (app restart) never saw the id but finds it by origin.
    let local = Fake::new();
    local.set_forward(forward("f-local", "h", ForwardState::Active));
    let detached = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let routed = routed_over(&local, &daemon, &detached);
    let mut revision = routed.revision();
    let listed = routed.list();
    assert_eq!(listed.len(), 2);
    let origin = |id: &str| {
        listed
            .iter()
            .find(|f| f.spec.id.0 == id)
            .map(|f| f.origin)
            .unwrap()
    };
    assert_eq!(origin("f-local"), labonair_ssh::ForwardOrigin::App);
    assert_eq!(origin("f-daemon"), labonair_ssh::ForwardOrigin::Daemon);

    SshForwardService::close(&routed, ForwardId("f-daemon".into()))
        .await
        .unwrap();
    assert_eq!(
        remote.fwd_calls.lock().unwrap().as_slice(),
        ["close:f-daemon"]
    );
    assert!(local.fwd_calls.lock().unwrap().is_empty());
    // The merged revision wakes up on the daemon's change, and the row is gone.
    timeout(WAIT, revision.changed()).await.unwrap().unwrap();
    eventually("daemon forward removed", || routed.list().len() == 1).await;
    routed.restart(ForwardId("f-local".into())).await.unwrap();
    assert_eq!(
        local.fwd_calls.lock().unwrap().as_slice(),
        ["restart:f-local"]
    );
    // The local engine's changes reach the same receiver.
    revision.borrow_and_update();
    local.set_forward(forward("f-local", "h", ForwardState::Stopped));
    timeout(WAIT, revision.changed()).await.unwrap().unwrap();

    // Policy: app mode opens locally; detached mode opens in the daemon and
    // detach follows the attach.
    let open = |id: &str| labonair_ssh::ForwardOpenRequest {
        spec: forward(id, "h", ForwardState::Active).spec,
        lifetime: labonair_ssh::ForwardLifetime::Pinned,
    };
    routed.attach_host("h".into()).await.unwrap();
    SshForwardService::open(&routed, open("n-local"))
        .await
        .unwrap();
    detached.store(true, Ordering::SeqCst);
    routed.attach_host("h".into()).await.unwrap();
    SshForwardService::open(&routed, open("n-daemon"))
        .await
        .unwrap();
    detached.store(false, Ordering::SeqCst); // detach must not follow the policy
    routed.detach_host("h".into()).await.unwrap();
    routed.detach_host("h".into()).await.unwrap();
    let local_calls = local.fwd_calls.lock().unwrap().clone();
    let remote_calls = remote.fwd_calls.lock().unwrap().clone();
    assert!(local_calls.contains(&"open:n-local".to_string()));
    assert!(remote_calls.contains(&"open:n-daemon".to_string()));
    assert_eq!(
        local_calls
            .iter()
            .filter(|c| c.starts_with("attach:") || c.starts_with("detach:"))
            .count(),
        2
    );
    assert_eq!(
        remote_calls
            .iter()
            .filter(|c| c.starts_with("attach:") || c.starts_with("detach:"))
            .count(),
        2
    );
    assert_eq!(remote_calls.last().map(String::as_str), Some("detach:h"));
    host.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn forward_calls_against_a_v2_daemon_ask_for_a_restart() {
    // A minimal daemon that only answers the handshake, as a v2 host would.
    let dir = PathBuf::from(format!(
        "/tmp/lhs-v2-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _cleanup = DirGuard(dir.clone());
    let paths = HostPaths::new(&dir);
    std::fs::create_dir_all(&paths.dir).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(&paths.socket).unwrap();
    std::thread::spawn(move || {
        use std::io::Write;
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = read_frame_blocking(&mut stream);
        let ack = HelloAck {
            proto_version: 2,
            min_compat: 1,
            build_id: "old".into(),
            pid: 1,
            compatible: true,
            session_count: 0,
        };
        let _ = stream.write_all(&control_frame(&Message::HelloAck(ack)).unwrap());
        while matches!(read_frame_blocking(&mut stream), Ok(Some(_))) {}
    });

    let client = HostClient::new(dir.clone(), Hello::current("proxy-test", None));
    let c = client.clone();
    tokio::task::spawn_blocking(move || c.reconnect())
        .await
        .unwrap()
        .expect("client connects");
    let ctx = ProxyContext {
        bus: EventBus::new(),
        hosts: Arc::new(|id, _route| Ok(snapshot_for(id))),
        transfer_config: Arc::new(|| unreachable!()),
        connected: Arc::new(|_| {}),
    };
    let daemon = DaemonServices::new(client, ctx);
    let message = "Restart the background host to use port forwards";
    let request = labonair_ssh::ForwardOpenRequest {
        spec: forward("f", "h", ForwardState::Active).spec,
        lifetime: labonair_ssh::ForwardLifetime::Pinned,
    };
    assert_eq!(
        SshForwardService::open(&daemon, request).await.unwrap_err(),
        message
    );
    assert_eq!(
        SshForwardService::close(&daemon, ForwardId("f".into()))
            .await
            .unwrap_err(),
        message
    );
    assert_eq!(daemon.attach_host("h".into()).await.unwrap_err(), message);
    assert!(daemon.list().is_empty());
    // The policy push is skipped for a pre-v3 daemon, without error.
    SshForwardService::set_reconnect_policy(&daemon, false);
}

#[tokio::test(flavor = "multi_thread")]
async fn reconnect_policy_reaches_local_and_daemon_and_is_resent_on_link() {
    let remote = Fake::new();
    let host = Harness::start(Some(remote.services())).await;
    let (daemon, _bus) = proxy(&host).await;
    let local = Fake::new();

    // No daemon known yet: only local gets it, and nothing is started.
    let known = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (d1, d2, k) = (daemon.clone(), daemon.clone(), known.clone());
    let routed = RoutedServices::new(
        LocalServices {
            connection: local.clone(),
            pty: local.clone(),
            remote_command: local.clone(),
            remote_file: local.clone(),
            forwards: local.clone(),
            sftp_session: local.clone(),
            sftp_browser: local.clone(),
            transfers: local.clone(),
        },
        DaemonAccess {
            peek: Arc::new(move || k.load(Ordering::SeqCst).then(|| d1.clone())),
            connect: Arc::new(move |_| Ok(d2.clone())),
        },
        Arc::new(|| false),
        Arc::new(|_| {}),
    );
    SshForwardService::set_reconnect_policy(&routed, false);
    assert_eq!(local.policies.lock().unwrap().as_slice(), [false]);
    sleep(Duration::from_millis(200)).await;
    assert!(remote.policies.lock().unwrap().is_empty());

    // The daemon is joined later (app restart): it receives the stored value.
    known.store(true, Ordering::SeqCst);
    let r = routed.clone();
    tokio::task::spawn_blocking(move || r.attach_running_daemon())
        .await
        .unwrap();
    eventually("policy on link", || {
        remote.policies.lock().unwrap().as_slice() == [false]
    })
    .await;

    // A later change reaches both.
    SshForwardService::set_reconnect_policy(&routed, true);
    assert_eq!(local.policies.lock().unwrap().as_slice(), [false, true]);
    eventually("policy on change", || {
        remote.policies.lock().unwrap().as_slice() == [false, true]
    })
    .await;
    host.shutdown().await;
}
