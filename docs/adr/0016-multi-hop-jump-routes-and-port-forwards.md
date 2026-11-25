# 0016: Multi-hop jump routes and first-class port forwards

## Status

Accepted

## Context

- **Jump hosts** are single-hop: `resolve_jump_host` never follows a jump host's own `jump_host_id`, although the Hosts form already allows (and cycle-checks) chains, so a configured chain is silently truncated. Hops always fail fast on an unknown host key, hop failures are not attributed to a hop, and the route is shown only as a baked-in tab title `host → jump`.
- **Port forwards** are `local` only, bound to `127.0.0.1`, stored in `hosts.tunnels`, started only when an SSH terminal tab connects and ref-counted by tabs. Start errors and "port in use" go to the log only; a dead SSH session leaves listeners bound and the forward listed as active; the workspace polls the list every 40 ms. In detached mode `RoutedServices` routes start/stop by the host id's last connection (in memory, default `Local`), so after an app restart a daemon forward can be listed but not stopped, and the daemon's idle rule (no sessions, jobs or clients) ignores forwards.
- `docs/modules.md` ("Jump-host primary menu/badge: no statusbar item") and `docs/product.md` ("not a standalone menu") predate the need to show routes and forwards; a forwards statusbar item is permanent chrome and needs this ADR.

## Decision

### 1. Jump routes are resolved chains

- `hosts.jump_host_id` stays the only persisted link (ssh `-J` semantics, no migration). A host's route is the chain obtained by following `jump_host_id` recursively.
- `labonair-ssh-transport::client::resolve_jump_chain(host_id) -> Result<Vec<JumpHostParams>, JumpRouteError>` returns hops **outermost first** (first one dialed). `JumpRouteError`: `Cycle`, `TooDeep` (`MAX_JUMP_HOPS = 8`), `Missing(id)`, `SelfJump`. The pure core `route_ids(start, parent_of)` is unit-tested; the UI cycle check and the transport use the same rule.
- The transport dials hop 1 over TCP, authenticates, opens `direct-tcpip` to hop 2 over that session, authenticates over the channel stream, and so on, then opens the final `direct-tcpip` to the target. `establish_authenticated_session_from_stream` is already stream-generic.
- Hop sessions use ids `{session_id}_jump{n}` (`n` 1-based, outermost = 1). `labonair-ssh` provides `hop_session_id(base, n)` and `split_hop_session_id(id) -> (base, Option<n>)`; everything that maps events to tabs strips the hop suffix first.
- `JumpHostParams` gains `host_id` and `label` (host display name) so errors and prompts name the hop. Errors read `Hop 2/3 (bastion-b): <cause>`; `classify_error` keeps "Jump host failed" and adds the hop label.
- The daemon snapshot (`SshHostSnapshot`) lists the target plus the **whole chain** and the secrets each hop needs; the daemon installs hosts in dependency order.
- ssh-config import maps `ProxyJump a,b` (ssh dials `a` first, then `b`, then the target) to the links `target.jump = b`, `b.jump = a`. An existing host's own `jump_host_id` is never overwritten; a conflicting link is skipped and reported. Export writes the whole chain as a comma list, outermost first.

### 2. Interactive prompts for hops and background sessions

Hop sessions and forward sessions have no tab-owned prompt UI, so today they fail fast. They now use a **prompt broker** instead of a retry loop:

- `labonair-ssh`: `PromptKind { HostKey { host, fingerprint, is_mismatch }, Password, Passphrase }`, `PendingPrompt { request_id: PromptRequestId, kind, hop: Option<HopInfo> }`, `HopInfo { index, total, label }`, `PromptAnswer { Trust(bool), Secret(Option<String>) }`.
- `labonair-ssh-transport::prompts::PromptBroker`: `ask(session_id, kind, hop) -> Option<PromptAnswer>` registers a oneshot keyed by `PromptRequestId`, publishes the `PendingPrompt` through a `PromptSink`, and times out after 300 s (`None`). Cleanup always runs. The main-hop retry flow (`TrustState`, `retry_ssh`) is unchanged.
- Hop stored secrets are used first; only missing or wrong ones prompt. Host keys on hops use the same known-hosts check as the main hop.
- Terminal connects publish a hop prompt as `SshConnectionEvent::HopPrompt { session_id (base), pending }`; the workspace shows it in the tab's loading screen like the main-hop prompt, labelled with the hop. `SshConnectionService` gains `answer_prompt(session_id, PromptRequestId, PromptAnswer)`.
- Forward sessions publish the same `PendingPrompt` as `ForwardState::AwaitingInput`; the answer goes through `SshForwardService::answer_prompt`.

### 3. Forwards are a first-class capability

- **Contract** (in `labonair-ssh`, replacing `SshTunnelService`, `TunnelConfig`, `ActiveTunnel`):

  ```text
  ForwardId(String)
  ForwardKind     = Local | Remote | Dynamic            (serde lowercase)
  ForwardSpec     { id, host_id, kind, name: Option<String>,
                    listen_host, listen_port,           // 0 = pick a free port
                    target_host, target_port }          // Dynamic: empty / 0
  ForwardLifetime = WithConnection | Pinned
  ForwardOrigin   = App | Daemon
  ForwardState    = Starting | Active | Reconnecting{attempt}
                  | AwaitingInput(PendingPrompt) | Failed{reason} | Stopped
  ForwardInfo     { spec, state, lifetime, origin, bound_port: Option<u16>,
                    connections: u32, bytes_in: u64, bytes_out: u64, saved: bool }
  SshForwardService:
    open(ForwardOpenRequest{spec, lifetime}) -> ForwardId
    close(id)  restart(id)  answer_prompt(id, request_id, PromptAnswer)
    attach_host(host_id)   // a connection for the host started: refcount++, start saved auto-start forwards
    detach_host(host_id)   // refcount--; at 0 close WithConnection forwards
    list() -> Vec<ForwardInfo>          // sync, cheap, cached for the daemon proxy
    revision() -> tokio::sync::watch::Receiver<u64>   // bumps on every change
  ```

  Semantics: `Local` binds `listen_host:listen_port` in the app/daemon and connects `target_host:target_port` through the server (`direct-tcpip`); `Remote` asks the server to listen on `listen_host:listen_port` (`tcpip-forward`) and connects each incoming channel to `target_host:target_port` on the client side; `Dynamic` is a local SOCKS5 proxy (CONNECT only, no auth) that opens `direct-tcpip` per request. `listen_port = 0` picks a free port (Remote: the server-assigned one); the result is `bound_port`.
- **Persistence.** Saved forwards stay in the `hosts.tunnels` JSON column as an array of `SavedForward { id, type: local|remote|dynamic (default local), name?, listen_host (default "127.0.0.1"), listen_port, target_host, target_port, auto_start (default true) }`. Legacy keys `local_port`, `remote_host`, `remote_port` are accepted via serde aliases; writes use the new names. Ad-hoc forwards (`saved = false`) are runtime-only and can be saved to a host.
- **Engine** (`labonair-ssh-transport::forwards`, replaces `tunnels.rs`): one SSH session per host (through the host's full jump route) shared by that host's forwards; per-forward cancellation; preflight bind with a descriptive `Failed{reason}` ("port in use", "permission denied", server-denied); keep-alive loss or `disconnected` moves every forward of the host to `Reconnecting` with exponential backoff (1 s → 30 s, 10 attempts, then `Failed`; remote forwards are re-requested after reconnect); per-connection errors only bump counters. Connection and byte counters use atomics.
- **Lifetime.** `WithConnection` forwards start on `attach_host` (any SSH or SFTP connection, not only terminal tabs) and stop when the last connection `detach_host`s; `Pinned` forwards run until closed. Every `attach_host` must be balanced by exactly one `detach_host` per connection, including reconnects.
- **Placement.** A forward lives where `terminal.sessionHost` placed it when it was opened and never moves (same rule as sessions). `ForwardInfo.origin` records it; `RoutedServices` routes `close/restart/answer_prompt` by looking the id up in `list()`, never by the host's last connection. `attach_host`/`detach_host` and `open` follow the session-host policy.
- **Detached host.** Protocol `PROTO_VERSION` 2 → 3 with RPCs `SshForwardOpen/Close/Restart/AnswerPrompt/AttachHost/DetachHost/List` and a pushed `ForwardChanged(ForwardInfo)` / `ForwardRemoved(id)` event; on attach the daemon replays its forwards. `DaemonServices` keeps a cache (`list()`) updated from events and bumps its `revision()`. A daemon with at least one forward is **not idle** (counts like a job). The new RPCs are version-gated: against a v2 daemon the client reports "restart the background host to use port forwards" through the Notifications registry. Multi-instance (ADR 0010): all attached clients see all forwards and may close them.
- **Event delivery.** Consumers `await revision().changed()` and re-read `list()`; the 40 ms polling of tunnels is removed.

### 4. Ownership and surfaces

- New crates: `labonair-forwards` (GPUI-free: quick-spec parser `-L 8080:db:5432` / `-R` / `-D`, validation, display labels, saved-forward JSON codec, palette command provider and submenu snapshots) and `labonair-forwards-ui` (GPUI: `ForwardsView` model, statusbar item and popover, editor, prompt dialog). Transport stays in `labonair-ssh-transport`; `labonair-hosts` only stores the column; `labonair-hosts-ui` embeds the shared editor draft model for its Forwards tab and loses its "Active tunnels" group (duplicate surface).
- **Statusbar item** `forwards`: shown only while any forward exists (like Transfers), right side, order 15, popover (`popover_with_exit`) grouped by host with state dot, route chip, address, connection count, and per-row copy/open/restart/close; header "New forward…" and "Stop all"; footer link to the host's Forwards tab. Placement is persisted by the existing status placement blob.
- **Palette** (owner-contributed): `Port Forwards: New…`, `Port Forwards: Start Saved…`, `Port Forwards: Stop…`, `Port Forwards: Stop All`, `Port Forwards: Manage`, `Jump Hosts: Edit Route…`, `Open SSH via…` (one-off route override, not persisted). The palette query also accepts the ssh quick syntax.
- **Route display.** `HostView::route_labels(host_id) -> Vec<String>` (outermost first). Tab title `host ⇢ a`, `host ⇢ a › b`, `host ⇢ 3 hops` (from 3 hops); the tab hover and the loading screen show the full route (`You → a → b → host`) and per-hop stages `Hop 2/3: b`. The hosts list shows a `via …` badge; deleting a host that is a jump for others warns and lists them.
- **Notifications.** Forward failures, reconnect give-up and prompt-needed use the Notifications registry; no inline error surfaces.
- **Settings** (values only, under the existing hosts area): `forwards.defaultListenHost` (`127.0.0.1`), `forwards.autoReconnect` (`true`), `forwards.confirmPublicBind` (`true`). Binding a non-loopback address asks for confirmation.
- **Security.** Forward creation is not exposed to AI/MCP tools; `block_agent_access` hosts are unaffected. Secrets never enter `ForwardInfo`, logs or persisted forward JSON.

### 5. Documentation changes

`docs/modules.md` (SSH, Hosts, new Forwards row, disposition "Jump-host primary menu/badge"), `docs/product.md`, `docs/registries.md` (status item, palette), `docs/README.md` ADR index, `docs/testing.md` (visual states) are updated in the same change. The disposition row is superseded: jump hosts still have **no statusbar item or menu of their own**; routes surface in tab title, hover, loading screen, hosts list and palette entries.

## Consequences

- Configured chains work end to end; hop failures and prompts name the hop; first connect through an untrusted bastion no longer fails.
- Forwards survive tab closes when pinned, show real state, reconnect, and (detached) survive app restarts and keep the daemon alive.
- `PROTO_VERSION` 3 requires matching app and daemon; an old daemon keeps working for sessions but not for forwards until restarted.
- `SshTunnelService` and `tunnels.rs` are removed in the same change (no compatibility layer); the `hosts.tunnels` column format is read compatibly forever (aliases), written in the new shape.
- New crates `labonair-forwards` and `labonair-forwards-ui` are added to `scripts/crate-graph.toml`; the feature crates depend only on `labonair-ssh`, `labonair-ui-kit`, `labonair-panel`, `labonair-command-palette-core`, `labonair-notifications-core`.
- Not verified in a running window or against a real multi-hop network at the time of writing; see the acceptance lists in the R12 task files.

## Implementation notes

Deviations from the Decision text, as implemented:

- Forward change events travel as daemon `BusEvent`s (`forward_reset`, `forward_changed`, `forward_removed`) instead of new `Message` variants.
- Settings keys are flat camelCase `forwardsDefaultListenHost`, `forwardsAutoReconnect`, `forwardsConfirmPublicBind` in the `connections` group, shown in a `Port forwards` group on the Terminal settings page (not `forwards.*`).
- `ForwardEditorEvent`, `EditorDefaults` and `ForwardDraft` live in `labonair-forwards`.
- The hover route line appears only on tabs with at least two panes.
- The hosts-list chip is a styled div because ui-kit has no text badge.
- A daemon older than protocol 3 needs a restart before forwards work.
- Verification: production paths are covered by the in-process russh e2e test `forwards_e2e`; nothing is visually verified in a running window yet.
