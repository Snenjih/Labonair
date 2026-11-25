// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! End-to-end test of the production forward stack (`SshSessionFactory` +
//! `ForwardRegistry`) against in-process russh servers: a bastion and a target
//! reached through it (`direct-tcpip`). No mocks. HOME is redirected to a temp
//! dir so the real `~/.ssh/known_hosts` is never read or written.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use labonair_persistence::Database;
use labonair_secrets::SecretsState;
use labonair_ssh::{
    ForwardId, ForwardKind, ForwardLifetime, ForwardOpenRequest, ForwardSpec, ForwardState,
    PendingPrompt, PromptAnswer, PromptKind,
};
use labonair_ssh_transport::forwards::{ForwardRegistry, ReconnectPolicy, SshSessionFactory};
use labonair_ssh_transport::TrustState;
use russh::server::{Auth, Config, Handler, Msg, Session};
use russh::{Channel, ChannelId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

const T: Duration = Duration::from_secs(15);
const KEY_A: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACDbhcyamaOujP62H7gMPhlYvLGCfCK1XBik1q6gE2GniwAAAIiQPpnakD6Z
2gAAAAtzc2gtZWQyNTUxOQAAACDbhcyamaOujP62H7gMPhlYvLGCfCK1XBik1q6gE2Gniw
AAAED7po+PGsCIyGPQWCHROWB9g1kCU2QuapYNlZnUyvOqMduFzJqZo66M/rYfuAw+GVi8
sYJ8IrVcGKTWrqATYaeLAAAAAAECAwQF
-----END OPENSSH PRIVATE KEY-----
";
const KEY_B: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACD3CfDi7QIz2R54f+dclGfoS8vdvLGsWtKEdrZd1jcLagAAAIh2e4RhdnuE
YQAAAAtzc2gtZWQyNTUxOQAAACD3CfDi7QIz2R54f+dclGfoS8vdvLGsWtKEdrZd1jcLag
AAAEAueF6043/QA0sJu8gMSAmdfYoIq4C3W0dkFHkmmglf5fcJ8OLtAjPZHnh/51yUZ+hL
y928saxa0oR2tl3WNwtqAAAAAAECAwQF
-----END OPENSSH PRIVATE KEY-----
";

/// Must run before anything touches the home dir (the client handler reads
/// `dirs::home_dir()`); never touches the real one.
fn isolate_home() {
    static HOME: OnceLock<std::path::PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("labonair-fwd-e2e-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".ssh")).unwrap();
        std::env::set_var("HOME", &dir);
        dir
    });
}

// ---- in-process SSH server ----------------------------------------------

type Handles = Arc<Mutex<Vec<russh::server::Handle>>>;

struct Srv {
    port: u16,
    handles: Handles,
    task: JoinHandle<()>,
}

impl Srv {
    async fn start(key: &str, password: &'static str) -> Srv {
        let config = Arc::new(Config {
            keys: vec![russh::keys::PrivateKey::from_openssh(key).unwrap()],
            auth_rejection_time: Duration::from_millis(10),
            auth_rejection_time_initial: Some(Duration::from_millis(0)),
            ..Default::default()
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handles: Handles = Arc::default();
        let h2 = handles.clone();
        let task = tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let handler = SrvHandler {
                    password,
                    tasks: Vec::new(),
                };
                if let Ok(running) = russh::server::run_stream(config.clone(), sock, handler).await
                {
                    h2.lock().unwrap().push(running.handle());
                    tokio::spawn(running);
                }
            }
        });
        Srv {
            port,
            handles,
            task,
        }
    }

    /// Drops every established client connection (the listener keeps accepting).
    async fn kill_sessions(&self) {
        let hs: Vec<_> = self.handles.lock().unwrap().drain(..).collect();
        for h in hs {
            let _ = h
                .disconnect(russh::Disconnect::ByApplication, "kill".into(), "".into())
                .await;
        }
    }
}

impl Drop for Srv {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct SrvHandler {
    password: &'static str,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for SrvHandler {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

impl Handler for SrvHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == "test" && password == self.password {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _oa: &str,
        _op: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let Ok(mut tcp) = TcpStream::connect((host, port as u16)).await else {
            return Ok(()); // dropping `reply` rejects the channel
        };
        reply.accept().await;
        self.tasks.push(tokio::spawn(async move {
            let mut ch = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut ch, &mut tcp).await;
        }));
        Ok(())
    }

    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let Ok(listener) = TcpListener::bind(("127.0.0.1", *port as u16)).await else {
            return Ok(false);
        };
        let Ok(local) = listener.local_addr() else {
            return Ok(false);
        };
        *port = local.port() as u32;
        let (address, bound, handle) = (address.to_string(), *port, session.handle());
        self.tasks.push(tokio::spawn(async move {
            while let Ok((mut tcp, peer)) = listener.accept().await {
                let Ok(ch) = handle
                    .channel_open_forwarded_tcpip(
                        address.clone(),
                        bound,
                        peer.ip().to_string(),
                        peer.port() as u32,
                    )
                    .await
                else {
                    continue;
                };
                tokio::spawn(async move {
                    let mut ch = ch.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut ch, &mut tcp).await;
                });
            }
        }));
        Ok(true)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        _address: &str,
        _port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn data(
        &mut self,
        _c: ChannelId,
        _d: &[u8],
        _s: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

// ---- fixture ---------------------------------------------------------------

struct Fixture {
    reg: ForwardRegistry,
    secrets: Arc<SecretsState>,
    bastion: Srv,
    target: Srv,
    echo_port: u16,
    _echo: JoinHandle<()>,
}

async fn echo_server() -> (u16, JoinHandle<()>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let t = tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    (port, t)
}

impl Fixture {
    /// `target_pw` is the password stored for the target host.
    async fn new(target_pw: &str) -> Fixture {
        isolate_home();
        let bastion = Srv::start(KEY_A, "pw").await;
        let target = Srv::start(KEY_B, "pw").await;
        let (echo_port, _echo) = echo_server().await;
        let db = Database(Arc::new(Mutex::new(
            labonair_persistence::initialize_in_memory_database().unwrap(),
        )));
        {
            let conn = db.0.lock().unwrap();
            for (id, port, jump) in [
                ("bastion", bastion.port, None),
                ("target", target.port, Some("bastion")),
            ] {
                conn.execute(
                    "INSERT INTO hosts (id, name, host_address, port, username, auth_method, \
                     jump_host_id, created_at) VALUES (?1, ?1, '127.0.0.1', ?2, 'test', 'password', ?3, 0)",
                    rusqlite::params![id, port, jump],
                )
                .unwrap();
            }
        }
        let secrets = Arc::new(SecretsState::in_memory());
        labonair_secrets::store_password(&secrets, "labonair-app", "bastion", "pw").unwrap();
        labonair_secrets::store_password(&secrets, "labonair-app", "target", target_pw).unwrap();
        let factory = SshSessionFactory {
            hosts_db: db,
            secrets: secrets.clone(),
            trust_state: TrustState::default(),
            events: labonair_events::EventBus::new(),
            connect_timeout_secs: Some(10),
        };
        Fixture {
            reg: ForwardRegistry::new(Arc::new(factory)),
            secrets,
            bastion,
            target,
            echo_port,
            _echo,
        }
    }

    async fn open(&self, id: &str, kind: ForwardKind, listen_port: u16, target: bool) -> ForwardId {
        let spec = ForwardSpec {
            id: ForwardId(id.into()),
            host_id: "target".into(),
            kind,
            name: None,
            listen_host: "127.0.0.1".into(),
            listen_port,
            target_host: if target {
                "127.0.0.1".into()
            } else {
                String::new()
            },
            target_port: if target { self.echo_port } else { 0 },
        };
        self.reg
            .open(ForwardOpenRequest {
                spec,
                lifetime: ForwardLifetime::Pinned,
            })
            .await
            .unwrap()
    }

    fn info(&self, id: &ForwardId) -> labonair_ssh::ForwardInfo {
        self.reg
            .list()
            .into_iter()
            .find(|i| &i.spec.id == id)
            .unwrap()
    }

    /// Polls until `until(state)` or the forward fails; answers every prompt
    /// with `answer`. Returns the prompts seen (in order) and the final state.
    async fn drive(
        &self,
        id: &ForwardId,
        mut answer: impl FnMut(&PendingPrompt) -> PromptAnswer,
        until: impl Fn(&ForwardState) -> bool,
    ) -> (Vec<PendingPrompt>, ForwardState) {
        let mut seen: Vec<PendingPrompt> = Vec::new();
        let fut = async {
            loop {
                let st = self.info(id).state;
                if until(&st) || matches!(st, ForwardState::Failed { .. }) {
                    return st;
                }
                if let ForwardState::AwaitingInput(p) = &st {
                    if !seen.iter().any(|s| s.request_id == p.request_id) {
                        seen.push(p.clone());
                        let a = answer(p);
                        self.reg
                            .answer_prompt(id.clone(), p.request_id.clone(), a)
                            .unwrap();
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        let st = tokio::time::timeout(T, fut)
            .await
            .expect("timed out waiting for forward state");
        (seen, st)
    }

    async fn activate(&self, id: &ForwardId) -> Vec<PendingPrompt> {
        let (prompts, st) = self
            .drive(
                id,
                |_| PromptAnswer::Trust(true),
                |s| *s == ForwardState::Active,
            )
            .await;
        assert_eq!(st, ForwardState::Active, "prompts: {prompts:?}");
        prompts
    }
}

async fn echo_roundtrip(port: u16, msg: &[u8]) {
    let fut = async {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(msg).await.unwrap();
        let mut buf = vec![0u8; msg.len()];
        s.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, msg);
    };
    tokio::time::timeout(T, fut).await.expect("echo timed out");
}

fn hostkey_hops(prompts: &[PendingPrompt]) -> Vec<Option<String>> {
    prompts
        .iter()
        .filter(|p| matches!(p.kind, PromptKind::HostKey { .. }))
        .map(|p| p.hop.as_ref().map(|h| h.label.clone()))
        .collect()
}

// ---- scenarios --------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn local_forward_through_two_hops_with_host_key_prompts() {
    let f = Fixture::new("pw").await;
    let id = f.open("l1", ForwardKind::Local, 0, true).await;
    let prompts = f.activate(&id).await;
    // One host-key prompt per hop: the bastion (labelled) and then the target.
    let hops = hostkey_hops(&prompts);
    assert_eq!(hops.len(), 2, "{prompts:?}");
    assert_eq!(hops[0].as_deref(), Some("bastion"));
    let port = f.info(&id).bound_port.expect("bound port");
    echo_roundtrip(port, b"hello through two hops").await;
    // Counters are added when the connection ends (the client dropped it).
    tokio::time::timeout(T, async {
        while f.info(&id).bytes_out < 22 || f.info(&id).bytes_in < 22 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("byte counters never updated");
    f.reg.close(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn dynamic_socks5_forward() {
    let f = Fixture::new("pw").await;
    let id = f.open("d1", ForwardKind::Dynamic, 0, false).await;
    f.activate(&id).await;
    let port = f.info(&id).bound_port.unwrap();
    let fut = async {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(&[5, 1, 0]).await.unwrap();
        let mut m = [0u8; 2];
        s.read_exact(&mut m).await.unwrap();
        assert_eq!(m, [5, 0]);
        let mut req = vec![5, 1, 0, 1, 127, 0, 0, 1];
        req.extend_from_slice(&f.echo_port.to_be_bytes());
        s.write_all(&req).await.unwrap();
        let mut reply = [0u8; 10];
        s.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[..2], [5, 0], "CONNECT refused: {reply:?}");
        s.write_all(b"socks hello").await.unwrap();
        let mut buf = [0u8; 11];
        s.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"socks hello");
    };
    tokio::time::timeout(T, fut).await.expect("socks timed out");
    f.reg.close(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_forward_reaches_local_echo() {
    let f = Fixture::new("pw").await;
    let id = f.open("r1", ForwardKind::Remote, 0, true).await;
    f.activate(&id).await;
    let port = f.info(&id).bound_port.expect("server-assigned port");
    assert_ne!(port, 0);
    echo_roundtrip(port, b"remote hello").await;
    f.reg.close(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn killed_target_session_reconnects_and_resumes() {
    let f = Fixture::new("pw").await;
    let id = f.open("k1", ForwardKind::Local, 0, true).await;
    f.activate(&id).await;
    let port = f.info(&id).bound_port.unwrap();
    echo_roundtrip(port, b"before").await;

    f.target.kill_sessions().await;
    // Backoff starts at 1 s, so Reconnecting is observable; the target server
    // keeps accepting, so the forward must come back Active (host keys are
    // already trusted, no prompt expected).
    let (_, st) = f
        .drive(
            &id,
            |_| panic!("no prompt expected on reconnect"),
            |s| matches!(s, ForwardState::Reconnecting { .. }),
        )
        .await;
    assert!(matches!(st, ForwardState::Reconnecting { .. }), "{st:?}");
    let (_, st) = f
        .drive(
            &id,
            |_| PromptAnswer::Trust(true),
            |s| *s == ForwardState::Active,
        )
        .await;
    assert_eq!(st, ForwardState::Active);
    echo_roundtrip(f.info(&id).bound_port.unwrap(), b"after").await;
    f.reg.close(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn reconnect_gives_up_with_failed_when_server_stays_down() {
    let f = Fixture::new("pw").await;
    f.reg.set_reconnect_policy(ReconnectPolicy {
        enabled: true,
        max_attempts: 1,
    });
    let id = f.open("k2", ForwardKind::Local, 0, true).await;
    f.activate(&id).await;
    // Stop accepting, then drop the live session: the only attempt fails.
    f.target.task.abort();
    f.target.kill_sessions().await;
    let (_, st) = f.drive(&id, |_| PromptAnswer::Trust(true), |_| false).await;
    assert!(matches!(st, ForwardState::Failed { .. }), "{st:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn rejecting_a_hop_host_key_fails_with_reason() {
    for reject_at in [0usize, 1] {
        let f = Fixture::new("pw").await;
        let id = f.open("x1", ForwardKind::Local, 0, true).await;
        let mut n = 0;
        let (prompts, st) = f
            .drive(
                &id,
                |_| {
                    n += 1;
                    PromptAnswer::Trust(n - 1 != reject_at)
                },
                |s| *s == ForwardState::Active,
            )
            .await;
        let ForwardState::Failed { reason } = st else {
            panic!("expected Failed, got {st:?}");
        };
        assert!(!reason.is_empty());
        assert_eq!(prompts.len(), reject_at + 1, "{prompts:?}");
        if reject_at == 0 {
            assert_eq!(hostkey_hops(&prompts)[0].as_deref(), Some("bastion"));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_password_fails_or_prompts_instead_of_hanging() {
    let f = Fixture::new("wrong").await;
    let id = f.open("w1", ForwardKind::Local, 0, true).await;
    // Trust host keys; decline any password prompt.
    let (prompts, st) = f
        .drive(
            &id,
            |p| match p.kind {
                PromptKind::HostKey { .. } => PromptAnswer::Trust(true),
                _ => PromptAnswer::Secret(None),
            },
            |s| *s == ForwardState::Active,
        )
        .await;
    assert!(
        matches!(st, ForwardState::Failed { .. }),
        "{st:?} {prompts:?}"
    );
    let _ = &f.secrets;
    let _ = &f.bastion;
}
