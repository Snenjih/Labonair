// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! In-process tests of the detached session host: the real server on a temp
//! socket, driven by a tiny client that keeps its own emulator (the way the
//! app will).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use labonair_terminal_core::{
    scrollback, TermDimensions, TerminalColors, TerminalEmulator, TerminalEvent,
};
use labonair_terminal_host::protocol::*;
use labonair_terminal_host::server::*;
use tokio::io::AsyncWriteExt;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

static COUNTER: AtomicUsize = AtomicUsize::new(0);
const WAIT: Duration = Duration::from_secs(20);

// ── Harness ──────────────────────────────────────────────────────────────────

struct Harness {
    dir: PathBuf,
    socket: PathBuf,
    handle: ServerHandle,
    task: JoinHandle<ExitReason>,
    _cleanup: DirGuard,
}

struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Harness {
    async fn start() -> Self {
        Self::start_with(|_| {}).await
    }

    async fn start_with(tweak: impl FnOnce(&mut ServerConfig)) -> Self {
        let dir = PathBuf::from(format!(
            "/tmp/lh-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let mut cfg = ServerConfig::new(&dir);
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
        Client::connect(&self.socket).await
    }

    async fn shutdown(self) -> ExitReason {
        self.handle.stop(ExitReason::Shutdown);
        timeout(WAIT, self.task)
            .await
            .expect("run finishes")
            .unwrap()
    }
}

/// The client's local emulator, as the app keeps one.
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
    resyncs: usize,
    attachment_ids: HashMap<SessionId, TerminalAttachmentId>,
}

impl Client {
    async fn connect(socket: &PathBuf) -> Self {
        let mut client = Self::connect_raw(socket).await;
        client
            .send(Message::Hello(Hello::current("test", None)))
            .await;
        let ack = client.wait_msg(|m| matches!(m, Message::HelloAck(_))).await;
        assert!(matches!(ack, Message::HelloAck(a) if a.compatible));
        client
    }

    async fn connect_raw(socket: &PathBuf) -> Self {
        let stream = UnixStream::connect(socket).await.expect("connect");
        let (read, write) = stream.into_split();
        Self {
            read,
            write,
            screen: Screen::new(80, 24),
            msgs: Vec::new(),
            resyncs: 0,
            attachment_ids: HashMap::new(),
        }
    }

    async fn send(&mut self, message: Message) {
        let bytes = control_frame(&message).unwrap();
        self.write.write_all(&bytes).await.unwrap();
    }

    async fn input(&mut self, session: SessionId, text: &str) {
        let bytes = data_frame(DataKind::Input, session, text.as_bytes());
        self.write.write_all(&bytes).await.unwrap();
    }

    /// Read and apply one frame.
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
            Frame::Data {
                kind: DataKind::Resync,
                data,
                ..
            } => {
                let (cols, rows) = self.screen_size();
                self.screen = Screen::new(cols, rows);
                self.screen.feed(&data);
                self.resyncs += 1;
            }
            Frame::Data { .. } => {}
            Frame::Control(message) => {
                if let Message::Attached { cols, rows, .. } = message {
                    self.screen = Screen::new(cols, rows);
                }
                self.msgs.push(message);
            }
        }
    }

    fn screen_size(&self) -> (u16, u16) {
        let dims = self.screen.emulator.dimensions();
        (dims.columns as u16, dims.screen_lines as u16)
    }

    async fn wait_msg(&mut self, pred: impl Fn(&Message) -> bool) -> Message {
        loop {
            if let Some(i) = self.msgs.iter().position(&pred) {
                return self.msgs.remove(i);
            }
            self.step().await;
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
            "never saw {needle:?}; screen:\n{}",
            self.screen.text()
        );
    }

    async fn create(&mut self, options: CreateOptions) -> SessionId {
        self.send(Message::CreateSession {
            req_id: 1,
            id: None,
            options,
            colors: None,
        })
        .await;
        match self
            .wait_msg(|m| matches!(m, Message::Created { .. } | Message::Error { .. }))
            .await
        {
            Message::Created { session, .. } => session,
            other => panic!("create failed: {other:?}"),
        }
    }

    async fn attach(&mut self, session: SessionId, cols: u16, rows: u16) -> Message {
        let attachment_id = *self.attachment_ids.entry(session).or_default();
        self.send(Message::Attach {
            session,
            cols,
            rows,
            attachment_id: Some(attachment_id),
        })
        .await;
        self.wait_msg(|m| matches!(m, Message::Attached { .. }))
            .await
    }

    async fn sequenced_input(
        &mut self,
        session: SessionId,
        attachment_id: TerminalAttachmentId,
        sequence: u64,
        text: &str,
    ) {
        let bytes = sequenced_input_frame(session, attachment_id, sequence, text.as_bytes());
        self.write.write_all(&bytes).await.unwrap();
        let ack = self
            .wait_msg(|message| {
                matches!(
                    message,
                    Message::InputAck {
                        session: got_session,
                        attachment_id: got_attachment,
                        sequence: got_sequence,
                    } if *got_session == session
                        && *got_attachment == attachment_id
                        && *got_sequence >= sequence
                )
            })
            .await;
        assert!(matches!(ack, Message::InputAck { .. }));
    }

    async fn list(&mut self) -> Vec<SessionInfo> {
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

fn sh() -> CreateOptions {
    CreateOptions {
        shell: Some("/bin/sh".into()),
        env: vec![
            ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
            ("PS1".into(), "$ ".into()),
            ("FOO".into(), "bar".into()),
        ],
        cols: 80,
        rows: 24,
        ..Default::default()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_attach_echo_and_environment_is_exact() {
    // Mark the daemon's own environment; it must not reach the shell.
    std::env::set_var("LABONAIR_HOST_TEST_LEAK", "leaked");
    let host = Harness::start().await;
    let mut client = host.client().await;
    let id = client.create(sh()).await;
    client.attach(id, 80, 24).await;
    client
        .input(id, "echo hello-$((1+2)) [$LABONAIR_HOST_TEST_LEAK][$FOO]\n")
        .await;
    client.wait_text("hello-3 [][bar]").await;

    let sessions = client.list().await;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, SessionStatus::Running);
    assert!(sessions[0].attached);
    host.shutdown().await;
}

/// A pane restored after its session died: the persisted history reaches the
/// daemon in `CreateOptions.replay_scrollback` and shows above the new prompt.
#[tokio::test]
async fn replayed_scrollback_of_a_restored_pane_is_visible_after_attach() {
    let host = Harness::start().await;
    let mut client = host.client().await;
    let id = client
        .create(CreateOptions {
            replay_scrollback: Some("OLD_HISTORY_LINE\r\n".into()),
            ..sh()
        })
        .await;
    client.attach(id, 80, 24).await;
    client.wait_text("OLD_HISTORY_LINE").await;
    host.shutdown().await;
}

#[tokio::test]
async fn output_while_detached_shows_up_in_the_next_snapshot() {
    let host = Harness::start().await;
    let mut client = host.client().await;
    let id = client.create(sh()).await;
    client.attach(id, 80, 24).await;
    client
        .input(
            id,
            "echo BACKGROUND-STARTED; (sleep 1; echo LATE-OUTPUT) &\n",
        )
        .await;
    client.wait_text("BACKGROUND-STARTED").await;
    client.send(Message::Detach { session: id }).await;
    sleep(Duration::from_millis(1800)).await;
    assert!(!client.list().await[0].attached);

    client.attach(id, 80, 24).await;
    // The snapshot frame follows `Attached`; pump until the screen has it.
    client.wait_text("LATE-OUTPUT").await;
    host.shutdown().await;
}

#[tokio::test]
async fn alt_screen_program_survives_reattach() {
    let host = Harness::start().await;
    let mut client = host.client().await;
    let id = client.create(sh()).await;
    client.attach(id, 80, 24).await;
    client
        .input(
            id,
            "printf '\\033[?1049h\\033[2J\\033[HALT-SCREEN-%s' TEXT; sleep 30\n",
        )
        .await;
    client.wait_text("ALT-SCREEN-TEXT").await;
    assert!(client.screen.emulator.is_alt_screen());

    client.send(Message::Detach { session: id }).await;
    client.attach(id, 80, 24).await;
    client.wait_text("ALT-SCREEN-TEXT").await;
    assert!(client.screen.emulator.is_alt_screen());

    // A different size on reattach is applied before the snapshot.
    client.attach(id, 100, 30).await;
    client.wait_text("ALT-SCREEN-TEXT").await;
    assert_eq!(client.screen_size(), (100, 30));
    host.shutdown().await;
}

#[tokio::test]
async fn multiple_clients_share_output_and_resize_has_one_controller() {
    let host = Harness::start().await;
    let mut a = host.client().await;
    let id = a.create(sh()).await;
    let Message::Attached {
        attachment_id: Some(a_attachment),
        resize_controller: Some(a_controller),
        ..
    } = a.attach(id, 80, 24).await
    else {
        panic!("first attachment response");
    };
    assert_eq!(a_attachment, a_controller);
    let mut b = host.client().await;
    let Message::Attached {
        attachment_id: Some(b_attachment),
        attached_count: 2,
        resize_controller: Some(controller),
        ..
    } = b.attach(id, 80, 24).await
    else {
        panic!("second attachment response");
    };
    assert_eq!(
        controller, a_attachment,
        "first attachment keeps resize control"
    );

    a.input(id, "echo from-a\n").await;
    b.input(id, "echo from-b\n").await;
    a.wait_text("from-a").await;
    b.wait_text("from-a").await;
    a.wait_text("from-b").await;
    b.wait_text("from-b").await;

    b.send(Message::Resize {
        session: id,
        cols: 100,
        rows: 30,
    })
    .await;
    assert!(matches!(
        b.wait_msg(
            |message| matches!(message, Message::Error { session: Some(got), .. } if *got == id)
        )
        .await,
        Message::Error { .. }
    ));
    b.send(Message::RequestResizeControl { session: id }).await;
    assert!(matches!(
        b.wait_msg(|message| matches!(message, Message::AttachmentState { session, resize_controller: Some(controller), .. } if *session == id && *controller == b_attachment)).await,
        Message::AttachmentState { .. }
    ));
    b.send(Message::Resize {
        session: id,
        cols: 100,
        rows: 30,
    })
    .await;
    let sessions = b.list().await;
    assert_eq!(sessions[0].attached_count, 2);
    assert_eq!(sessions[0].resize_controller, Some(b_attachment));
    assert_eq!((sessions[0].cols, sessions[0].rows), (100, 30));

    a.send(Message::Detach { session: id }).await;
    b.input(id, "echo after-a-left\n").await;
    b.wait_text("after-a-left").await;
    assert_eq!(b.list().await[0].attached_count, 1);
    host.shutdown().await;
}

/// Child-process target for `separate_process_clients_share_one_host_session`.
/// Running the integration binary normally has no `LH_PROCESS_*` variables,
/// so this helper exits immediately as an ordinary test.
#[tokio::test]
async fn subprocess_attachment_client_helper() {
    let (Some(socket), Some(session), Some(ready), Some(observed)) = (
        std::env::var_os("LH_PROCESS_SOCKET"),
        std::env::var("LH_PROCESS_SESSION")
            .ok()
            .and_then(|value| value.parse::<SessionId>().ok()),
        std::env::var_os("LH_PROCESS_READY"),
        std::env::var_os("LH_PROCESS_OBSERVED"),
    ) else {
        return;
    };
    let mut client = Client::connect(&PathBuf::from(socket)).await;
    client.attach(session, 80, 24).await;
    std::fs::write(ready, b"attached").expect("write ready marker");
    client.wait_text("PROCESS-BOUNDARY-OUTPUT").await;
    std::fs::write(observed, b"received").expect("write output marker");
}

struct ChildProcesses(Vec<Child>);

impl Drop for ChildProcesses {
    fn drop(&mut self) {
        for child in &mut self.0 {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[tokio::test]
async fn separate_process_clients_share_one_host_session() {
    let host = Harness::start().await;
    let mut owner = host.client().await;
    let session = owner.create(sh()).await;
    owner.attach(session, 80, 24).await;

    let helper = std::env::current_exe().expect("integration test executable");
    let mut children = ChildProcesses(Vec::new());
    let mut ready_paths = Vec::new();
    let mut observed_paths = Vec::new();
    for index in 0..2 {
        let ready = host.dir.join(format!("client-{index}.ready"));
        let observed = host.dir.join(format!("client-{index}.observed"));
        let child = Command::new(&helper)
            .arg("--exact")
            .arg("subprocess_attachment_client_helper")
            .arg("--nocapture")
            .env("LH_PROCESS_SOCKET", &host.socket)
            .env("LH_PROCESS_SESSION", session.to_string())
            .env("LH_PROCESS_READY", &ready)
            .env("LH_PROCESS_OBSERVED", &observed)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn attachment process");
        children.0.push(child);
        ready_paths.push(ready);
        observed_paths.push(observed);
    }

    let deadline = Instant::now() + WAIT;
    while ready_paths.iter().any(|path| !path.exists()) {
        assert!(Instant::now() < deadline, "child clients did not attach");
        assert!(
            children
                .0
                .iter_mut()
                .all(|child| child.try_wait().expect("check child status").is_none()),
            "an attachment child exited before signaling readiness"
        );
        sleep(Duration::from_millis(20)).await;
    }
    let sessions = owner.list().await;
    assert_eq!(sessions[0].attached_count, 3);

    owner.input(session, "echo PROCESS-BOUNDARY-OUTPUT\n").await;
    owner.wait_text("PROCESS-BOUNDARY-OUTPUT").await;
    while observed_paths.iter().any(|path| !path.exists()) {
        assert!(
            Instant::now() < deadline,
            "child clients did not receive output"
        );
        sleep(Duration::from_millis(20)).await;
    }
    for child in &mut children.0 {
        let status = timeout(WAIT, async {
            loop {
                if let Some(status) = child.try_wait().expect("check child status") {
                    break status;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("child client exits");
        assert!(
            status.success(),
            "child attachment process failed: {status}"
        );
    }
    host.shutdown().await;
}

#[tokio::test]
async fn sequenced_input_retries_are_acknowledged_without_duplicate_writes() {
    let host = Harness::start().await;
    let mut client = host.client().await;
    let id = client.create(sh()).await;
    let Message::Attached {
        attachment_id: Some(attachment_id),
        ..
    } = client.attach(id, 80, 24).await
    else {
        panic!("attachment response");
    };
    client.input(id, "stty -echo\n").await;
    sleep(Duration::from_millis(50)).await;
    client
        .sequenced_input(id, attachment_id, 1, "printf 'ONCE-MARKER\\n'\n")
        .await;
    client.wait_text("ONCE-MARKER").await;
    client
        .sequenced_input(id, attachment_id, 1, "printf 'ONCE-MARKER\\n'\n")
        .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(client.screen.text().matches("ONCE-MARKER").count(), 1);
    host.shutdown().await;
}

#[tokio::test]
async fn slow_client_overflows_into_a_resync_that_matches_the_truth() {
    let host = Harness::start_with(|cfg| cfg.queue_limit = 1).await;
    let mut client = host.client().await;
    let id = client.create(sh()).await;
    client.attach(id, 80, 24).await;
    client.input(id, "stty -echo\n").await;
    sleep(Duration::from_millis(50)).await;
    client.input(id, "seq 1 400000; echo DONE-MARKER\n").await;
    // Do not read: output cannot accumulate in this deliberately tiny queue,
    // while the PTY reader continues independently.
    sleep(Duration::from_millis(2500)).await;

    let result = timeout(WAIT, async {
        loop {
            let text = client.screen.text();
            if text.contains("DONE-MARKER") && text.trim_end().ends_with('$') {
                break;
            }
            client.step().await;
        }
    })
    .await;
    assert!(result.is_ok(), "screen:\n{}", client.screen.text());
    assert!(client.resyncs >= 1, "expected at least one resync");
    let after_resync = client.screen.text();

    // The resynced client state equals a fresh attach.
    client.send(Message::Detach { session: id }).await;
    client.attach(id, 80, 24).await;
    sleep(Duration::from_millis(300)).await;
    while let Ok(()) = timeout(Duration::from_millis(200), client.step()).await {}
    let trim =
        |text: String| -> Vec<String> { text.lines().map(|l| l.trim_end().to_string()).collect() };
    assert_eq!(trim(after_resync), trim(client.screen.text()));
    host.shutdown().await;
}

#[tokio::test]
async fn exited_session_stays_until_acked() {
    let host = Harness::start().await;
    let mut client = host.client().await;
    let id = client
        .create(CreateOptions {
            startup_command: Some("echo bye; exit 3".into()),
            ..sh()
        })
        .await;
    // Nobody is attached, so there is no push: poll the list.
    let sessions = timeout(WAIT, async {
        loop {
            let sessions = client.list().await;
            if sessions[0].status == SessionStatus::Exited {
                return sessions;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("session exits");
    assert_eq!(sessions[0].status, SessionStatus::Exited);
    assert_eq!(sessions[0].exit_code, Some(3));

    // Attaching still shows the final screen and the exit.
    client.attach(id, 80, 24).await;
    client.wait_text("bye").await;
    client
        .wait_msg(|m| matches!(m, Message::SessionExited { .. }))
        .await;

    client.send(Message::Ack { session: id }).await;
    assert!(client.list().await.is_empty());
    host.shutdown().await;
}

#[tokio::test]
async fn unacked_exited_session_expires() {
    let host = Harness::start_with(|cfg| cfg.exited_ttl = Duration::from_millis(300)).await;
    let mut client = host.client().await;
    client
        .create(CreateOptions {
            startup_command: Some("exit 0".into()),
            ..sh()
        })
        .await;
    sleep(Duration::from_millis(1500)).await;
    assert!(client.list().await.is_empty());
    host.shutdown().await;
}

#[tokio::test]
async fn kill_removes_the_session_and_its_process() {
    let host = Harness::start().await;
    let mut client = host.client().await;
    let id = client.create(sh()).await;
    client.send(Message::Kill { session: id }).await;
    assert!(client.list().await.is_empty());
    client.send(Message::Kill { session: id }).await;
    client
        .wait_msg(|m| matches!(m, Message::Error { .. }))
        .await;
    host.shutdown().await;
}

#[tokio::test]
async fn idle_daemon_exits_but_not_while_busy() {
    let jobs = Arc::new(AtomicBool::new(true));
    let hook = Arc::clone(&jobs);
    let mut host = Harness::start_with(|cfg| {
        cfg.idle_timeout = Duration::from_millis(400);
        cfg.jobs_active = Arc::new(move || hook.load(Ordering::SeqCst));
    })
    .await;

    // An active job keeps it alive.
    sleep(Duration::from_millis(900)).await;
    assert!(!host.task.is_finished());

    // So does a connected client, even with the job gone.
    let client = host.client().await;
    jobs.store(false, Ordering::SeqCst);
    sleep(Duration::from_millis(900)).await;
    assert!(!host.task.is_finished());

    // And a session.
    drop(client);
    let mut client = host.client().await;
    let id = client.create(sh()).await;
    drop(client);
    sleep(Duration::from_millis(900)).await;
    assert!(!host.task.is_finished());

    // Once the last session is gone it leaves after the timeout.
    let mut client = host.client().await;
    client.send(Message::Kill { session: id }).await;
    client.list().await;
    drop(client);
    let reason = timeout(Duration::from_secs(5), &mut host.task)
        .await
        .expect("daemon should exit by itself")
        .unwrap();
    assert_eq!(reason, ExitReason::Idle);
    assert!(!host.socket.exists(), "socket removed on exit");
}

#[tokio::test]
async fn single_instance_lock_and_stale_socket_cleanup() {
    let host = Harness::start().await;
    assert!(matches!(
        Server::bind(ServerConfig::new(&host.dir)),
        Err(ServerError::AlreadyRunning)
    ));

    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&host.socket), 0o600);
    assert_eq!(mode(host.socket.parent().unwrap()), 0o700);

    let dir = host.dir.clone();
    let socket = host.socket.clone();
    host.shutdown().await;
    // A crashed daemon leaves its socket file behind; a new one cleans it up.
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    std::fs::write(&socket, b"stale").unwrap();
    let again = Server::bind(ServerConfig::new(&dir)).expect("stale socket is replaced");
    let handle = again.handle();
    let task = tokio::spawn(again.run());
    handle.stop(ExitReason::Shutdown);
    task.await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn long_data_dirs_fall_back_to_a_short_socket_path() {
    let long = PathBuf::from(format!("/tmp/{}", "x".repeat(120)));
    let paths = HostPaths::new(&long);
    let uid = unsafe { libc_getuid() };
    assert!(paths.socket.starts_with(format!("/tmp/labonair-{uid}")));
    assert!(paths.socket.as_os_str().len() < 100);
    assert!(paths.lock.starts_with(&long), "lock stays in the data dir");
    let short = HostPaths::new(std::path::Path::new("/tmp/lh"));
    assert_eq!(short.socket, PathBuf::from("/tmp/lh/host/host.sock"));
}

extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

#[tokio::test]
async fn sigterm_path_persists_scrollback_for_replay() {
    let host = Harness::start().await;
    let mut client = host.client().await;
    let key = "00000000-0000-0000-0000-0000000000aa";
    let id = client
        .create(CreateOptions {
            persist_key: Some(key.into()),
            ..sh()
        })
        .await;
    client.attach(id, 80, 24).await;
    client.input(id, "echo persist-me\n").await;
    client.wait_text("persist-me").await;

    let dir = host.dir.clone();
    host.handle.stop(ExitReason::Signal);
    let reason = timeout(WAIT, async { host.task.await.unwrap() })
        .await
        .unwrap();
    assert_eq!(reason, ExitReason::Signal);
    let saved = scrollback::load(&dir, key, None).expect("scrollback persisted");
    assert!(saved.contains("persist-me"), "{saved}");
    // The client was told why it lost the session.
    let detached = client
        .wait_msg(|m| matches!(m, Message::Detached { .. }))
        .await;
    assert!(matches!(
        detached,
        Message::Detached {
            reason: DetachReason::Shutdown,
            ..
        }
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn shutdown_message_kills_everything() {
    let host = Harness::start().await;
    let mut client = host.client().await;
    client.create(sh()).await;
    client.send(Message::Shutdown).await;
    let reason = timeout(WAIT, async { host.task.await.unwrap() })
        .await
        .unwrap();
    assert_eq!(reason, ExitReason::Shutdown);
}

#[tokio::test]
async fn incompatible_peer_only_gets_the_recovery_requests() {
    let host = Harness::start().await;
    let mut good = host.client().await;
    good.create(sh()).await;

    let mut old = Client::connect_raw(&host.socket).await;
    old.send(Message::Hello(Hello {
        proto_version: 1,
        min_compat: PROTO_VERSION + 1,
        build_id: "future".into(),
        scrollback_lines: None,
    }))
    .await;
    let ack = old.wait_msg(|m| matches!(m, Message::HelloAck(_))).await;
    assert!(matches!(ack, Message::HelloAck(a) if !a.compatible && a.session_count == 1));
    old.send(Message::CreateSession {
        req_id: 9,
        id: None,
        options: sh(),
        colors: None,
    })
    .await;
    old.wait_msg(|m| matches!(m, Message::Error { .. })).await;
    assert_eq!(old.list().await.len(), 1);
    host.shutdown().await;
}

#[tokio::test]
async fn garbage_and_missing_hello_do_not_take_the_host_down() {
    let host = Harness::start().await;
    let mut garbage = UnixStream::connect(&host.socket).await.unwrap();
    garbage
        .write_all(&[0x7f, 1, 2, 3, 4, 5, 6, 7, 8])
        .await
        .unwrap();
    let mut no_hello = Client::connect_raw(&host.socket).await;
    no_hello.send(Message::ListSessions).await;
    // Both connections are dropped; a proper client still works.
    let mut client = host.client().await;
    assert!(client.list().await.is_empty());
    host.shutdown().await;
}

/// Not part of the normal run: raw PTY -> client throughput through the host.
/// `cargo test -p labonair-terminal-host --release -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn throughput_sanity() {
    let host = Harness::start().await;
    let mut client = host.client().await;
    let id = client.create(sh()).await;
    client.attach(id, 120, 40).await;
    let started = std::time::Instant::now();
    client
        .input(id, "seq 1 3000000; echo THROUGHPUT-DONE\n")
        .await;
    client.wait_text("THROUGHPUT-DONE").await;
    let elapsed = started.elapsed();
    eprintln!("3M lines (~22 MB) via host in {elapsed:?}");
    host.shutdown().await;
}
