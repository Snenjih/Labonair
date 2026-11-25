# R12-002: Forward engine, contract and detached host

## Status

`🟨 Implemented — visual verification pending` — governed by [ADR 0016](../../adr/0016-multi-hop-jump-routes-and-port-forwards.md). Common rules: see R12-001 ("Common rules for every R12 sub-task"). Requires R12-001 J1 (chain dialing) and J5a (prompt broker) to be done.

## F1 — Contract types and the `labonair-forwards` crate (pure)

Owner: `crates/ssh/src/lib.rs`, new `crates/forwards`, `scripts/crate-graph.toml`, workspace `Cargo.toml`.

1. In `labonair-ssh` add the forward contract exactly as in ADR 0016 §3 (`ForwardId`, `ForwardKind`, `ForwardSpec`, `ForwardLifetime`, `ForwardOrigin`, `ForwardState`, `ForwardInfo`, `ForwardOpenRequest`, trait `SshForwardService`; all value types `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`; `ForwardInfo` helpers `is_running()`, `address_label()`). **Keep `SshTunnelService`/`TunnelConfig`/`ActiveTunnel` for now** (F5 removes them). `revision()` returns `tokio::sync::watch::Receiver<u64>` — add `tokio` (sync feature only) to `labonair-ssh` if absent and justify it in a comment.
2. New crate `crates/forwards` (`labonair-forwards`, GPUI-free; deps: `labonair-ssh`, `serde`, `serde_json`, `labonair-command-palette-core` only if needed in F-U1 — not now). Modules:
   - `saved`: `SavedForward` serde model with aliases (`local_port`→`listen_port`, `remote_host`→`target_host`, `remote_port`→`target_port`), defaults (`type=local`, `listen_host="127.0.0.1"`, `auto_start=true`), `parse_saved(raw: &Option<String>) -> Vec<SavedForward>` (tolerates empty/garbage → empty) and `serialize_saved(&[SavedForward]) -> String`; `SavedForward::into_spec(host_id) -> ForwardSpec`.
   - `validate`: `validate_spec(&ForwardSpec) -> Result<(), ValidationError>` (ports; host non-empty for Local/Remote; Dynamic needs no target; `listen_port 0` allowed; `is_public_bind(listen_host)`).
   - `quick`: `parse_quick("-L 8080:db:5432" | "-L 127.0.0.1:8080:db:5432" | "-R 9000:localhost:3000" | "-D 1080" | "-D 0.0.0.0:1080") -> Result<QuickSpec, String>` (ssh syntax: `-L [bind:]port:host:hostport`, `-R [bind:]port:host:hostport`, `-D [bind:]port`).
   - `label`: `listen_label(&ForwardSpec)`, `route_label(&ForwardSpec)` (`127.0.0.1:8080 → db:5432`, `server:9000 ← localhost:3000`, `socks5://127.0.0.1:1080`), `kind_glyph`/`kind_name`.
3. Add the crate to the workspace members and to `scripts/crate-graph.toml` following the surrounding entries (look at how `labonair-activity` is declared); run `scripts/check-crate-deps.sh`.
4. Tests: legacy JSON round trip (`[{"id":"a","type":"local","local_port":8080,"remote_host":"db","remote_port":5432}]` parses and re-serializes with new keys), quick parsing for every form + errors, validation, labels.

Accept: `cargo test -p labonair-forwards -p labonair-ssh`, `scripts/check-crate-deps.sh`.

## F2 — Engine: registry, Local and Dynamic forwards, state machine

Owner: `crates/ssh-transport/src/forwards.rs` (new; `tunnels.rs` stays until F5), `lib.rs` module line.

1. `ForwardRegistry` (cheap `Clone`, `Arc<Inner>`): `forwards: Mutex<HashMap<ForwardId, ForwardEntry>>`, `hosts: Mutex<HashMap<host_id, HostSession>>` (ref count + the shared SSH `Handle` + a `watch` for the session state), `revision: watch::Sender<u64>`. Public methods mirror `SshForwardService`; `list()` is a cheap sorted snapshot (host id, then listen port, then id). Every state/counter-relevant change bumps `revision` — **coalesce counter updates** (bump at most every 250 ms for connection/byte counters; state changes bump immediately).
2. One SSH session per host through the host's full jump route (use `resolve_jump_chain` and `establish_authenticated_session`, session id `forward_{host_id}`), created on first forward start and shared; closed when its last forward stops.
3. Local forward: bind `listen_host:listen_port` (support `0` → ephemeral, report `bound_port`; IPv6 literal hosts allowed; when `listen_host` is `localhost` bind both `127.0.0.1` and `::1` if available), accept loop with `CancellationToken`, per connection `channel_open_direct_tcpip` + `copy_bidirectional`, atomics for `connections` (inc/dec) and bytes (use a counting wrapper or the byte totals `copy_bidirectional` returns). Per-connection errors only log at `debug` and never change the forward state.
4. Dynamic forward: local SOCKS5 server (no auth; CONNECT only; ATYP IPv4/IPv6/domain; reply codes for failure; reject BIND/UDP with 0x07) bridging to `direct-tcpip` like Local. Parser is a pure function over bytes with unit tests (including truncated and oversized domain input).
5. Preflight bind errors → `Failed{reason}` with plain-language reasons: address in use (`Port 8080 is already in use`), permission denied (`Ports below 1024 need elevated rights`), other (`io::Error` text). Connect/auth errors of the host session → `Failed{reason}` for all of its forwards.
6. Health & reconnect: observe the host session end (the `ClientHandler::disconnected` hook already stores a reason; add an `Arc<Notify>`/channel so the registry is told when the transport goes down, plus a periodic `handle.is_closed()` check is NOT needed if the hook fires). On loss: all forwards of the host → `Reconnecting{attempt}`, listeners stay bound, new connections wait up to 10 s for the new session or are closed; reconnect with exponential backoff 1 s → 30 s (cap), max 10 attempts → `Failed`. The `autoReconnect` setting arrives in U5; for now read it from a `ReconnectPolicy { enabled: bool, max_attempts: u32 }` field with default `enabled=true`.
7. `attach_host`/`detach_host` per ADR (ref count; saved forwards read from `hosts.tunnels` via `labonair_forwards::saved`; add `labonair-forwards` as a dependency of `labonair-ssh-transport` — allowed: pure crate; update `crate-graph.toml`); `open`/`close`/`restart`; closing the last forward of a host closes the host session; `Pinned` forwards do not count as connections.
8. Tests (no network to the outside; use loopback only): SOCKS5 parser, state transitions via a mock "session factory" trait so the registry logic (ref counts, lifetimes, coalesced revisions, backoff schedule as a pure function `backoff(attempt)`) is testable without SSH; port-in-use preflight against a real `TcpListener`.

Accept: `cargo test -p labonair-ssh-transport` green; no clippy warnings.

## F3 — Remote forwards

Owner: `crates/ssh-transport/src/client.rs`, `forwards.rs`.

1. `ClientHandler` implements `server_channel_open_forwarded_tcpip` (russh 0.62 — read the vendored/dep source for the exact signature) and dispatches the new channel to a `RemoteForwardDispatch` (`Arc<Mutex<HashMap<(String, u32), RemoteTarget>>>` keyed by the connected address/port the server reports) shared with the registry through the handler's constructor (add an optional field; hops/other connects pass `None`).
2. In the engine: `Handle::tcpip_forward(listen_host, listen_port)`; if `listen_port == 0` use the port the server returns as `bound_port`; register the dispatch entry; on incoming channel connect to `target_host:target_port` locally (TCP, 5 s timeout) and `copy_bidirectional` with the channel stream; on failure close the channel. `close` calls `cancel_tcpip_forward`. After a reconnect (F2 step 6) re-request every remote forward.
3. Server denial (`tcpip-forward` returns failure) → `Failed{reason: "The server refused the remote forward (AllowTcpForwarding / GatewayPorts?)"}`.
4. Tests: dispatch table logic (pure), reconnect re-request ordering with the mock session factory.

Accept: `cargo test -p labonair-ssh-transport`.

## F4 — Prompts for forward sessions

Owner: `crates/ssh-transport/src/forwards.rs`, `prompts.rs`.

1. The host session of the registry uses a `PromptSink` that records the `PendingPrompt` on every forward of that host (`ForwardState::AwaitingInput(pending)`) and bumps `revision`; the broker `ask` blocks the connect (300 s timeout → `Failed{reason: "No answer"}`); the main-hop host key and credentials use the same interactive path as hops (generalise the `interactive_hop` ctx from J5a to `interactive: Option<PromptCtx>` where `hop` is optional) — main host: stored secret first, then ask Password/Passphrase/HostKey.
2. `answer_prompt(id, request_id, answer)` forwards to the broker; state returns to `Starting` after an answer.
3. A prompt timeout or rejection → `Failed` with a reason; `restart(id)` retries.
4. Tests with the mock factory: prompt published → state `AwaitingInput`, answer → `Starting`; timeout with `tokio::time::pause`.

Accept: `cargo test -p labonair-ssh-transport`.

## F5 — Service adapter and caller migration (remove `tunnels.rs`)

Owner: `crates/ssh-transport/src/contract.rs`, `crates/shell`, `crates/workspace`, `crates/terminal-host` (compile fixes only), `crates/hosts-host`, `crates/hosts-ui` (Active-tunnels plumbing only), `crates/ssh`.

1. `SshForwardServiceAdapter` over `ForwardRegistry` in `contract.rs`; compose it where `SshTunnelServiceAdapter` is composed (`shell/composition.rs`, `shell/session_host.rs`, `terminal-host/stack.rs`); `LocalServices` field `tunnels` → `forwards`.
2. Workspace: replace `ssh_tunnels: Arc<dyn SshTunnelService>` by `ssh_forwards: Arc<dyn SshForwardService>`. `start_tunnels` → `attach_host` in the `SessionEstablished` branch **and** for SFTP tab connections (find where an SFTP connection becomes established); every connection path must `detach_host` exactly once when its tab/session ends. **Fix the refcount leak**: on reconnect/retry of an SSH tab (`workspace.rs` retry path that removes `old_session`) the old session's attach must be balanced (detach before the new attach, or do not re-attach for the same tab). Keep a `HashSet<(SessionId or tab id)>` of attached connections so double attach/detach is impossible; unit-test that helper.
3. Replace the 40 ms `refresh_active_tunnels` poll with a task awaiting `forwards.revision().changed()` that updates a workspace-owned `Vec<ForwardInfo>` snapshot and emits a `WorkspaceEvent`/notifies observers (the U2 view will consume it; for now keep `HostView::set_active_tunnels` fed from the snapshot so the existing "Active tunnels" group still works until U4).
4. Delete `tunnels.rs`, `SshTunnelService`, `SshTunnelServiceAdapter`, `TunnelConfig`, `ActiveTunnel`, `RpcCall::SshTunnel*` usages that become dead — **but** the daemon RPC side is migrated in F6; until then keep the daemon proxy compiling by implementing `SshForwardService` for `DaemonServices` with `Err("forwards are not available on this background host yet")` / empty `list()` (F6 replaces it) and make `RoutedServices` route `Route::Local` only.

Accept: `cargo check --workspace --all-targets`, tests of touched crates.

## F6 — Detached host: RPCs, events, replay, idle, routing

Owner: `crates/terminal-host`.

1. Protocol (still `PROTO_VERSION` 3): RPCs `SshForwardOpen { request, host: SshHostSnapshot }` (host snapshot as `SshTunnelStart.host` carried — chain + secrets), `SshForwardClose`, `SshForwardRestart`, `SshForwardAnswerPrompt`, `SshForwardAttachHost { host_id, host }`, `SshForwardDetachHost`, `SshForwardList`; pushed events `ForwardChanged(ForwardInfo)` / `ForwardRemoved(ForwardId)`; the daemon replays its forwards to every newly attached client. Debug output never prints secrets. Remove the `SshTunnel*` RPCs.
2. Server: dispatch RPCs to `services.forwards`, subscribe to `revision()` and push diffs (compare with the last sent snapshot per connection so only changed forwards are sent).
3. Idle rule (`idle_loop`, `jobs_active`-style): a daemon with at least one forward in `Starting|Active|Reconnecting|AwaitingInput` is busy (not idle).
4. Client (`DaemonServices`): `list()` from a cache updated by events (origin forced to `Daemon`), own `revision` `watch` bumped on events; implement `SshForwardService` fully; version gate: if the daemon's handshake `proto_version < 3` return `Err("Restart the background host to use port forwards")` from mutating calls.
5. `RoutedServices`: `open` and `attach_host` choose Local vs Daemon by the same `choose`/`HostPolicy` logic used for sessions (`route_host`), `close/restart/answer_prompt` route by the id's `origin` from the merged `list()`, `detach_host` routes by where `attach_host` went (store in `Routes`; fall back to origin lookup); `list()` merges local + daemon; `revision()` merges both watches into one `watch` (spawn a small forwarder task). Remove `Routes.hosts` if now unused.
6. Tests in `tests/ssh.rs` style: Fake service for forwards; list replay on attach; idle rule; route-by-origin after a simulated app restart (new `RoutedServices` instance that has never seen the id can still close a daemon forward).

Accept: `cargo test -p labonair-terminal-host`, `cargo check --workspace --all-targets`.

## Acceptance for the whole task

- [ ] F1–F6 accepted, `scripts/check.sh` green.
- [ ] Manual or containerised check (state how, or state not verified): local, remote and SOCKS forward through a 2-hop route; kill the SSH session → `Reconnecting` → `Active`; port-in-use shows a reason; daemon mode survives an app restart and can be closed afterwards.
