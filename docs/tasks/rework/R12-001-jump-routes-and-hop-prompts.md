# R12-001: Multi-hop jump routes and hop prompts

## Status

`🟨 Implemented — visual verification pending` — governed by [ADR 0016](../../adr/0016-multi-hop-jump-routes-and-port-forwards.md) (read it first; it is the contract).

## Common rules for every R12 sub-task

- Do exactly the sub-task you are given. Change nothing outside its scope; do not "improve" neighbouring code; do not commit.
- Follow `CLAUDE.md` (English code/comments, no `unwrap()` on runtime errors, never block the GPUI thread, no secrets in logs/tests, UI only from `labonair-ui-kit`).
- Reuse existing helpers before writing new ones. If a sub-task needs a name or type not in ADR 0016, pick the most obvious one and report it.
- Write focused unit tests for pure logic. Verify with `cargo check -p <each touched crate> --all-targets`, `cargo test -p <touched crate>` and `cargo clippy -p <touched crate> --all-targets -- -D warnings` and `cargo fmt`. Fix everything you caused. If a failure is pre-existing, say so.
- Final report (max 15 lines): files changed, new public names, tests added, commands run with results, anything not done.

## J1 — Chain resolution and multi-hop dialing

Owner: `crates/ssh-transport/src/client.rs`, `crates/ssh/src/lib.rs`.

1. In `labonair-ssh` add `hop_session_id(base: &str, n: usize) -> String` (`"{base}_jump{n}"`) and `split_hop_session_id(id: &str) -> (&str, Option<usize>)` with unit tests (also `"a_jump1_jump2"` must not be produced/handled specially: split on the **last** `_jump<digits>` suffix only).
2. In `client.rs` add `pub const MAX_JUMP_HOPS: usize = 8`, `pub enum JumpRouteError { Cycle, TooDeep, Missing(String), SelfJump }` (implements `Display`, `std::error::Error`) and the pure `pub fn route_ids(start: &str, parent_of: impl Fn(&str) -> Option<String>) -> Result<Vec<String>, JumpRouteError>` returning ancestor ids **outermost first** excluding `start`. Unit tests: no jump, one hop, three hops order, cycle (`a→b→a`), self (`a→a`), depth > 8, missing is reported by the caller's lookup (parent_of returns an id that resolves to nothing → handled in `resolve_jump_chain`).
3. Add `pub fn resolve_jump_chain(hosts_db, secrets, host_id: &str) -> Result<Vec<JumpHostParams>, LabonairError>`: reads `jump_host_id` links from the `hosts` table (one query per hop), uses `route_ids`, then reuses the existing per-host resolution code of `resolve_jump_host` (credential + keyring) for each hop. Remove `resolve_jump_host` once all callers use the chain (grep callers: `client.rs`, `tunnels.rs`, `sftp-ssh/src/connection.rs`).
4. `JumpHostParams` gains `host_id: String` and `label: String` (host `name`, falling back to `host_address`).
5. Change every `jump: Option<JumpHostParams>` parameter to `jump: Vec<JumpHostParams>` (empty = direct) across `client.rs`, `tunnels.rs`, `sftp-ssh/src/connection.rs` and any other caller the compiler finds. In `connect_transport_maybe_via_jump` (and `connect_via_jump`) dial hop 1 over TCP, authenticate, open `direct-tcpip` to hop 2 on that session, authenticate over the channel stream (`establish_authenticated_session_from_stream` is stream-generic), … and finally open `direct-tcpip` to the target; return the final stream. Keep every intermediate `Handle` alive for the lifetime of the final stream (move them into a wrapper stream struct that implements `AsyncRead + AsyncWrite` by delegating to the inner stream, or whatever makes dropping order correct; add a comment). Keep hops fail-fast on unknown host keys for now (J5 changes that). Hop session ids use `hop_session_id`.
6. Errors: `format!("Hop {i}/{n} ({label}): {cause}")`. Log steps per hop (`log_step!`) in the form `Hop {i}/{n} ({label}): connecting…`, `…authenticated ✓`. Reject `target == any hop` (`SelfJump`).
7. Do not touch the daemon snapshot (J2) or UI.

Accept: chain of 3 resolves in order in a unit test; `cargo test -p labonair-ssh -p labonair-ssh-transport -p labonair-sftp-ssh` green.

## J2 — Daemon snapshot carries the whole chain

Owner: `crates/terminal-host/src/proxy/snapshot.rs`, `crates/terminal-host/src/stack.rs`, `crates/terminal-host/src/protocol.rs` (comments/types only).

1. `snapshot.rs` follows `jump_host_id` for the whole chain (use `labonair_ssh_transport::client::route_ids`), lists target + every hop host row, their credentials and exactly the secrets a connect reads (same rule as today, per hop). Max depth/cycle errors surface as `Err(String)` with the `JumpRouteError` text.
2. `stack.rs::install_host` installs hosts so a host referenced as a jump is present before use regardless of list order (the existing regression test covers one hop; extend it for a 3-hop chain listed in arbitrary order).
3. Tests: snapshot of a 3-hop chain contains 4 host rows in `ids` order target, hop-inner…outer (document the order you choose) and only the needed secrets; no secret appears in `Debug` output.

Accept: `cargo test -p labonair-terminal-host` green.

## J3 — Route labels and connection display

Owner: `crates/hosts-ui`, `crates/hosts-host`, `crates/workspace`, `crates/shell/src/bootstrap.rs` (wiring only).

1. `HostManagerView::route_labels(&self, host_id) -> Vec<String>` (outermost first, names, excluding the host itself; stops at a cycle) replacing `jump_host_label`; update the `HostView` bundle in `hosts-host` (`jump_host_label` → `route_labels`) and the bootstrap closure.
2. Pure `ssh_route_title(host_label, route: &[String]) -> String` in `workspace.rs` replacing `ssh_tab_title`: none → `host`; 1 → `host  ⇢ a`; 2 → `host  ⇢ a › b`; ≥3 → `host  ⇢ N hops`. Unit tests. Both call sites (`connect_host`, retry path) use it.
3. Loading screen (`workspace.rs` `jump_badge`, `ssh_connection.rs`): show the whole route (`via a › b`) and the per-hop stage while connecting (parse `Hop i/n (label)` log lines in `ssh_connection.rs` into a `current_hop: Option<(usize, usize, String)>` field and render `Hop 2/3: label` in the stage area). `classify_error`: lines starting `Hop i/n (label):` produce title `Jump host failed` and a hint that names the hop label. Tests for parsing and classification.
4. Tab hover card (`tab_hover.rs`): for SSH tabs add a `Route` line `You → a → b → host`. Read how hover data is built; add the field with the minimum plumbing.
5. Hosts list row: a muted `via a › b` chip for hosts with a route (reuse an existing ui-kit badge/chip component).

Accept: unit tests for title/parse/classify; `cargo check` of touched crates; note in report that visual check is pending.

## J4 — Hosts form: route preview and delete warning

Owner: `crates/hosts-ui/src/hosts/page.rs` (+ `hosts.rs` helpers).

1. In the Jump host group of the host form show the resolved route below the dropdown as `You → a → b → this host` (read-only chips, each hop name clickable to open that host for editing if an existing "open host" action exists; otherwise plain). Show a muted warning if the depth would exceed 8. Use `MAX_JUMP_HOPS` from `labonair-ssh-transport` only if `labonair-hosts-ui` already depends on it (do NOT add a new crate edge); otherwise define `const MAX_JUMP_HOPS: usize = 8;` locally with a comment `// keep in sync with labonair-ssh-transport::client::MAX_JUMP_HOPS (ADR 0016)`.
2. Before deleting a host that other hosts use as jump host, the existing delete confirmation lists those hosts (`Used as jump host by: x, y`). Pure helper `dependents_of_jump(hosts, id) -> Vec<String>` with tests.
3. The jump dropdown excludes candidates that would create a cycle **or** make the chain deeper than 8 (extend `jump_creates_cycle` tests).

Accept: tests for the two helpers; check/clippy of `labonair-hosts-ui`.

## J5 — Interactive prompts for hops (broker)

Split into three tasks, in order.

### J5a — Contract and transport

Owner: `crates/ssh`, `crates/ssh-transport`.

1. `labonair-ssh`: add `PromptKind`, `PendingPrompt`, `HopInfo`, `PromptAnswer` exactly as in ADR 0016 §2 (derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`; `Secret` answers must not derive `Debug` output of the secret: implement `Debug` manually for `PromptAnswer` printing `Secret(<redacted>)`), `SshConnectionEvent::HopPrompt { session_id: String, pending: PendingPrompt }`, and `SshConnectionService::answer_prompt(session_id, PromptRequestId, PromptAnswer) -> BoxFuture<Result<(), String>>`. Update all trait impls/fakes the compiler finds with a minimal implementation (daemon proxy gets `Err("not supported")` until J5c).
2. `ssh-transport/src/prompts.rs`: `PromptBroker` (cheap `Clone`, inner `Arc<Mutex<HashMap<PromptRequestId, oneshot::Sender<PromptAnswer>>>>`), `async fn ask(&self, sink: &dyn PromptSink, session_id, kind, hop) -> Option<PromptAnswer>` (generates a unique `PromptRequestId`, registers, calls `sink.publish(PendingPrompt)`, awaits with 300 s timeout, always removes the entry), `fn answer(&self, id, answer) -> bool`. `trait PromptSink: Send + Sync { fn publish(&self, session_id: &str, pending: PendingPrompt); }`. Tests: answer delivers, timeout (use `tokio::time::pause`), cleanup after cancel.
3. Wire the broker into the connect flow: `ClientHandler` gets an optional `interactive_hop: Option<HopPromptCtx { broker, sink, info }>`. For hops: host-key unknown/mismatch → if `interactive_hop` is set, `ask(HostKey…)` instead of failing fast (trust = learn known_hosts exactly like the main hop code path; reject/timeout → error `Hop i/n (label): host key rejected`); missing stored password / encrypted key without passphrase / failed auth on a hop → `ask(Password|Passphrase)` once more with the answer, up to 3 attempts, then error. Stored secrets are used first. The terminal connect passes an event-bus based sink publishing `SshConnectionEvent::HopPrompt`; SFTP/explorer background connects and forward sessions pass `None` for now (stay fail-fast) — F4 supplies a sink for forwards.
4. `SshConnectionService::answer_prompt` implementation in `ssh-transport/src/contract.rs` calls `PromptBroker::answer`. The broker lives next to `TrustState` in the transport state set composed in `crates/shell/src/composition.rs` and `crates/terminal-host/src/stack.rs` (add the field and construct it the same way `TrustState` is).

Accept: broker tests green; `cargo check --workspace --all-targets` compiles.

### J5b — Workspace prompt UI for hops

Owner: `crates/workspace` (`workspace.rs`, `ssh_connection.rs`, `ssh_event_bridge.rs`).

1. Handle `SshConnectionEvent::HopPrompt`: map `session_id` (base) to the ssh tab, store the `PendingPrompt` in the connection entry, and render it on the loading screen with the same widgets as the main-hop trust/password/passphrase prompts, titled with the hop (`Hop 2/3: label`). Reuse the existing `SshPrompt` rendering code paths; extend `SshPrompt` with a `Hop { ssh_id, pending, buffer }` variant (or equivalent) rather than duplicating widgets.
2. Answering calls `answer_prompt(base session id, request_id, answer)` on the SSH service; Esc/Reject answers `Trust(false)` / `Secret(None)`.
3. Events whose `session_id` carries a hop suffix (existing `known_hosts_warning`, `auth_required`, `passphrase_required` for hop ids must not occur any more for interactive hops, but guard anyway): strip with `split_hop_session_id` before looking up the tab.
4. Tests for the pure mapping/state transitions you add.

Accept: `cargo test -p labonair-workspace` green; report that the visual check is pending.

### J5c — Daemon protocol for hop prompts (v3)

Owner: `crates/terminal-host`.

1. Bump `PROTO_VERSION` to 3 (document in the protocol module header what v3 adds: hop prompts now, forwards in R12-002).
2. Add RPC `SshAnswerPrompt { session_id, prompt_request_id, answer }` and a server→client event carrying `SshConnectionEvent::HopPrompt` (follow how `KnownHostsWarning` with `PromptRequestId` already travels); redact secrets in `Debug` of every frame type that carries a `PromptAnswer`. Daemon-side `PromptSink` publishes into the existing SSH event stream so the app sees `HopPrompt`; `DaemonServices::answer_prompt` implements the trait method; `RoutedServices` routes `answer_prompt` by `split_hop_session_id(...).0`.
3. Tests in `tests/ssh.rs` style: the `Fake` service receives `answer_prompt`; frame `Debug` output never contains a secret.

Accept: `cargo test -p labonair-terminal-host` green.

## J6 — ssh-config import/export chains

Owner: `crates/ssh-transport/src/config_parser.rs`.

1. Parse `ProxyJump a,b,c` into an ordered list (each element may be `user@host:port` or an alias; strip `-J`-style schemes). `SshConfigEntry.proxy_jump` stays a single string field in `labonair-ssh`; keep the raw comma list in it and split in the importer.
2. Import (ADR 0016 §1): resolve each alias to a host id; link `target.jump = last`, `prev.jump = previous` along the list; never overwrite a host that already has a different `jump_host_id` — skip and include a warning string in the import result if the contract allows (otherwise log at `warn`).
3. Export writes the whole chain as `ProxyJump a,b` (outermost first) by walking links (cycle-safe, depth ≤ 8).
4. Tests: parse list, import builds a 2-hop chain, no overwrite of an existing link, export chain, round trip.

Accept: `cargo test -p labonair-ssh-transport` green.

## Acceptance for the whole task

- [ ] J1–J6 accepted, `scripts/check.sh` green.
- [ ] A configured chain A→B→C connects in a native run against a real or containerised chain (record how it was verified, or state clearly that it was not).
- [ ] Tab title, hover, loading screen and hosts list show the route (visual check or stated as unverified).
