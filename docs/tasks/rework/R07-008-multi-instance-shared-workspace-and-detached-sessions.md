# R07-008 — Multi-instance shared Workspaces and Detached sessions

## Status

`⏳ Planned`

## Owner

- Modules: Workspace owns shared Spaces, tab/pane structure, identity,
  revisions, leases, persistence, migration, and Workspace UI. Terminal owns
  host-side session attachments, PTYs, input sequencing, resize control,
  output fanout, and Terminal status. Shell only composes both contracts.
- Capability-matrix rows: [`../../docs/modules.md`](../../modules.md)
- Composition entry point: existing Workspace construction and Terminal
  `SessionHost` registration in `labonair-shell`
- Architecture decision: [`../../docs/adr/0010-shared-workspaces-and-multi-client-detached-sessions.md`](../../adr/0010-shared-workspaces-and-multi-client-detached-sessions.md)

## Goal

When Detached mode is enabled, two or more app processes connect to one local
Detached Host. Each process normally opens a different Space; a user may
explicitly join an occupied Space. Joined processes see the same Space, tab,
pane, and terminal-session structure and can attach to the same live terminal
or SSH terminal without stealing one another's view. Workspace changes,
leases, host inputs, resizing, disconnects, and recovery have explicit
cross-process semantics.

This task is downstream of the active R07 parity work. It is not permission to
start before the task becomes the earliest incomplete item in
`tasks/rework/README.md`.

## Dependencies

- `R07-000-zed-parity-baseline-and-inventory.md` — reconcile Workspace,
  Terminal, collaboration, and existing Spaces entry points in the crosswalk.
- `R07-001-product-surface-acceptance.md` — accept the existing Space-picker,
  shared-tab, Terminal, notification, and failure surfaces.
- `R07-006-documentation-knowledge-system.md` and
  `R07-007-transitional-edge-review.md` — complete the earlier ordered R07
  documentation and dependency-policy work.
- The queue order is authoritative; all tasks before R07-008 must be complete
  before implementation begins.

## Scope

- In scope: `crates/workspace/`, a justified GPUI-free
  `labonair-workspace-storage` sibling, `crates/terminal/`,
  `crates/terminal-core/`, `crates/terminal-host/`, shell composition, tests,
  Workspace/Terminal capability and workflow records, ADR 0009/0010, and the
  architecture dependency manifest/inventory.
- In scope: the existing Spaces popover and its explicit occupied-Space join
  path; existing Terminal pane state and notification-center/status surfaces.
- In scope: protocol versioning, cross-build behavior, session prompts,
  migrations, scrollback retention, and two-process integration evidence.
- Out of scope: mobile clients, TCP, remote pairing/authentication, shared
  Editor buffers or collaborative text editing, a new global Workspace
  registry, a new permanent shell surface, and feature state in `labonair-shell`.
- `reference-src/` and `zed-refrence/` remain read-only.

## Verified current-state baseline (2026-11-19)

This is a source baseline for the later implementation, not a claim that any
target behavior below is already implemented.

- The Detached Host accepts independent socket connections, and
  `HostClient` clones share one socket within a process. `spawn::shared_client`
  caches that client process-wide, while `ensure_daemon` reuses a reachable
  host. Each session nevertheless has one `SinkTarget`: attaching a second
  connection sends `Detached(Stolen)` to the first, and input/resize are
  accepted only from the connection currently in that sink. The regression
  contract is captured by
  [`second_attach_steals_the_session`](../../../crates/terminal-host/tests/host.rs).
  The server already has a bounded Outbox per connection with per-session
  overflow/resync, so multi-client output can build on that isolation; the
  current gap is that a session routes output to only one connection. See
  [`server.rs`](../../../crates/terminal-host/src/server.rs) and
  [`spawn.rs`](../../../crates/terminal-host/src/spawn.rs).
- The wire protocol currently declares `PROTO_VERSION = 1` and
  `MIN_COMPAT = 1` in [`protocol.rs`](../../../crates/terminal-host/src/protocol.rs).
  Thus multi-process host reuse exists, but independent attachments and the
  v2 compatibility contract in this task do not.
- V1 input uses an unacknowledged `DataKind::Input` byte frame. `HostClient::input`
  discards its local send result, the server silently ignores frames from a
  non-attached connection, and `Session::send_input` discards local-PTY/SSH
  writer-channel failures. The current PTY writer also coalesces queued byte
  vectors without frame acknowledgements. V2 therefore needs sequenced input,
  explicit admission/rejection, reconnect deduplication, and visible writer
  failure. The existing `Message::Error` is handled by the v1 client and can
  report legacy writer errors without adding a v1-unreadable event. See
  [`client.rs`](../../../crates/terminal-host/src/client.rs),
  [`server.rs`](../../../crates/terminal-host/src/server.rs), and
  [`protocol.rs`](../../../crates/terminal-host/src/protocol.rs).
- Host startup already avoids replacing any listening daemon: `ensure_daemon`
  returns when the socket is reachable, then the client handshake exposes an
  incompatible status and session count. The existing `restart` action sends
  `Shutdown` and starts a replacement; host shutdown kills every session.
  Compatibility recovery must therefore be a separate idle-only path and
  must not call that restart flow while sessions or jobs remain. See
  [`spawn.rs`](../../../crates/terminal-host/src/spawn.rs),
  [`client.rs`](../../../crates/terminal-host/src/client.rs),
  [`background_host_service.rs`](../../../crates/shell/src/background_host_service.rs),
  and [`server.rs`](../../../crates/terminal-host/src/server.rs).
- Terminal lifecycle currently distinguishes local pane close from releasing
  a process's hosted sessions. `TerminalRegistry::close` terminates a hosted
  session through `Control::kill` (host `Kill` removes it globally), while
  `close_all` releases hosted sessions through `Control::detach`; dropping a
  hosted control also detaches. Workspace pane retirement calls `registry.close`
  and deletes that pane's scrollback. Host `Shutdown` stops the server and
  kills all sessions. Consequently, a shared tab close must remove Workspace
  references first and only issue global Kill when no retained reference
  remains; ordinary process teardown must preserve detach semantics. See
  [`registry.rs`](../../../crates/terminal/src/registry.rs),
  [`daemon_host.rs`](../../../crates/terminal/src/daemon_host.rs),
  [`workspace.rs`](../../../crates/workspace/src/workspace.rs), and
  [`server.rs`](../../../crates/terminal-host/src/server.rs).
- Workspace terminal handles also have two identity layers. Each process has
  a local `TerminalRegistry` session ID; the snapshot stores the Detached
  Host's `host_session_id` plus the pane's `scrollback_id`. Host restore says
  daemon IDs restart with the daemon, so it matches the stable pane tag
  (`scrollback_id`) and uses the numeric ID only as a tie-breaker. New hosted
  sessions pass that tag as both `pane_tag` and `persist_key`. A shared
  Workspace record must preserve the durable terminal/pane key and resolve a
  process-local handle per attachment; it cannot treat either process-local
  registry IDs or the daemon's numeric ID alone as a durable reference. See
  [`session_io.rs`](../../../crates/workspace/src/session_io.rs),
  [`host_restore.rs`](../../../crates/workspace/src/host_restore.rs),
  [`daemon_host.rs`](../../../crates/terminal/src/daemon_host.rs), and
  [`registry.rs`](../../../crates/terminal/src/registry.rs).
- Runtime `SpaceId` is `u64`, seeded at `1`, and allocated by each in-memory
  [`SpaceStore`](../../../crates/workspace/src/spaces.rs). The v3 persisted
  [`SpaceEntryV3`](../../../crates/workspace/src/session_v3.rs) stores a name and
  session only; the active Space is stored as an array index. The single
  `session.json` path still accepts v3, v2, and the pre-Spaces flat format, and
  writes snapshots directly with best-effort `std::fs::write` in
  [`session.rs`](../../../crates/workspace/src/session.rs). The migration target
  must preserve this compatibility while introducing stable shared IDs and
  transactional storage.
- The existing explicit Space entry point is the titlebar Spaces popover,
  also mirrored in the Tabs sidebar. It renders the current process's local
  `SpaceStore` and `TabStore`; selecting a row performs a local switch and the
  plus button creates a local Standalone Space. There is no occupied-Space
  state or join action today. Add explicit join and occupancy to this existing
  picker, and claim the target before changing the local active Space. See
  [`spaces_ui.rs`](../../../crates/workspace/src/spaces_ui.rs),
  [`tab_strip.rs`](../../../crates/workspace/src/tab_strip.rs), and
  [`workspace.rs`](../../../crates/workspace/src/workspace.rs).
- Runtime `Tab.id` and `PaneId` values come from the `pane::next_id` process-
  local counter. A v3 session records tab order by list position and pane-tree
  IDs only to restore the active leaf; restore deliberately mints new pane IDs.
  Thus the current snapshot preserves structure and pane state, not stable Tab
  or Pane identity across processes or restarts. See
  [`tabs.rs`](../../../crates/workspace/src/tabs.rs),
  [`pane.rs`](../../../crates/workspace/src/pane.rs),
  [`session_v3.rs`](../../../crates/workspace/src/session_v3.rs), and
  [`session_io.rs`](../../../crates/workspace/src/session_io.rs).
- The app-wide `labonair.db` is opened by `AppComposition` through
  `labonair-persistence::initialize_database`; `Database` wraps a process-local
  `Arc<Mutex<Connection>>`, and the foundation crate initializes the shared
  Hosts, Credentials, and Snippets schema. Workspace's legacy snapshot instead
  lives at `data_dir/labonair/session.json`. The target therefore uses a
  separate Workspace-owned `workspace.db` alongside that snapshot, opened and
  migrated by `labonair-workspace-storage`; this avoids adding Workspace
  schema lifecycle to the shared foundation crate. See
  [`composition.rs`](../../../crates/shell/src/composition.rs),
  [`persistence/src/lib.rs`](../../../crates/persistence/src/lib.rs),
  [`main.rs`](../../../crates/app/src/main.rs), and
  [`session.rs`](../../../crates/workspace/src/session.rs).
- Workspace cleanup passes only the current instance's pane scrollback IDs to
  `scrollback::cleanup`, which deletes every `.ansi.gz` file absent from that
  list. Pane retirement immediately deletes its file. The Detached Host uses
  the same `<data_dir>/scrollback` directory and persists its emulator by
  `persist_key` on signal; when session restoration is enabled, Workspace
  session capture also writes its client emulator under that same key. Since
  new Detached clients mirror the same terminal, those two writers can race on
  one file. See
  [`scrollback.rs`](../../../crates/terminal-core/src/scrollback.rs),
  [`workspace.rs`](../../../crates/workspace/src/workspace.rs),
  [`session_io.rs`](../../../crates/workspace/src/session_io.rs), and
  [`server.rs`](../../../crates/terminal-host/src/server.rs).
- SSH and transfer events currently have no responsible-client ownership.
  `SubscribeEvents` replays host-key warnings and transfer state to each
  subscriber, and live bus events are broadcast to all subscribers. A
  host-key warning is tracked only by SSH session ID; `SshTrustHost` removes
  that marker but does not verify which client answered. `TransferResolve`
  carries a job ID and resolution without a client-owned prompt token. Under
  multiple app clients this can show duplicate prompts and permits competing
  answers. Add host-issued request identity, single-client ownership, and
  at-most-once resolution before broadcasting any actionable prompt. See
  [`ssh_host.rs`](../../../crates/terminal-host/src/server/ssh_host.rs),
  [`server.rs`](../../../crates/terminal-host/src/server.rs),
  [`protocol.rs`](../../../crates/terminal-host/src/protocol.rs), and
  [`transfers/src/lib.rs`](../../../crates/transfers/src/lib.rs).
- `general.sessionRestore` defaults to `false`; when enabled, the current
  Workspace captures all its Spaces to that one snapshot. Current scrollback
  cleanup derives its retained IDs from that Workspace's local panes in
  [`workspace.rs`](../../../crates/workspace/src/workspace.rs). These lifecycle
  paths are local to one process and need a cross-process last-lease rule and
  shared reference accounting before they can safely own shared state. On
  window close, `bootstrap.rs` clears the snapshot when the preference is off
  and calls scrollback cleanup with an empty retained-ID list; that cleanup
  deletes every non-temporary scrollback file. This must not be carried into a
  multi-process path because another live Workspace lease or Detached Host
  session may still own the data.

## Contracts and ownership

- **Workspace domain contract:** Workspace owns UUID-backed `SpaceId`,
  `TabId`, `PaneId`, and per-window `WorkspaceInstanceId`; typed Space
  snapshots, mutations, revisions, leases, and change records. GPUI `Entity`
  values remain local to each process and never cross persistence or IPC.
- **Workspace persistence:** add `labonair-workspace-storage` under the
  Workspace owner. It remains GPUI-free and owns the SQLite schema, atomic
  lease claims/renewals/releases, per-Space revision checks, ordered changes,
  shared scrollback-reference counts and per-key GC permits/tombstones,
  and the one-time migration from `session.json` v3 and older supported
  snapshots. Use the existing `rusqlite` 0.40 workspace dependency; justify
  the direct dependency in the architecture graph. Do not add a Terminal Host
  dependency to Workspace or make the host own Workspace data. Persist a
  workspace-wide monotonic usage sequence and each Space's last-used sequence;
  update both transactionally on startup claim, explicit join, and active-Space
  selection. Define this sequence as the allocator's recency source so startup
  decisions do not depend on wall clocks from separate processes. Store one
  current lease row per Workspace instance, referencing its active Space;
  multiple instances may hold leases on one Space. The lease table is unique by
  WorkspaceInstanceId, not SpaceId; automatic allocation treats any unexpired
  row for a Space as occupied. Assign every successful claim or join a globally
  increasing lease generation. Mutations, renewals, and releases must validate
  the current Space, instance, and generation together; an expired owner's
  stale generation cannot mutate, renew, or release a later claim.
- **Workspace writes:** every structural action commits a typed mutation
  against an expected Space revision. The storage transaction serializes
  writes and returns a typed stale-revision result; the client reloads and
  reapplies only when the operation is safe. An old snapshot cannot overwrite
  newer Space state. Changes are consumed off-thread with a target update
  latency of 250 ms; reconnect, revision gaps, and unknown events trigger a
  full Space snapshot.
- **Space leases:** one lease per Workspace window/client, with a random
  instance ID and heartbeat expiry. Each SpaceLease also carries a
  database-generated lease generation that changes on every successful claim
  or explicit join. Shared mutations, renewals, and releases validate the
  exact Space, instance, and generation in the same SQLite transaction. If a
  heartbeat expires and another instance reclaims the Space, the old process
  receives typed `LeaseLost`; its delayed writes, heartbeats, or close handler
  cannot affect the new lease. An explicit join by another instance adds its
  distinct lease row and does not replace existing participants. A switch
  updates that instance's current row only after the target claim succeeds, in
  the same transaction. Startup atomically claims the free Space
  with the greatest persisted last-used sequence, breaking any ties by stable
  Space ID; if that Space is leased, it claims the next free Space in that
  order. The successful claim advances the usage sequence in the same
  transaction, so simultaneous starts select distinct Spaces. An explicit
  join or active-Space selection advances the same sequence. If the store has
  no Space rows, seed and claim one empty Standalone default Space. If Space
  rows exist but all are leased, create and claim one empty Space inheriting
  only the project identity of the globally most recently used Space. The existing Spaces popover is
  the explicit join path for an occupied Space. Clean close releases the
  lease; expiry recovers from a crashed client. A window switch releases the
  old lease only after the target claim succeeds.
- **Legacy recency migration:** v3 stores only `active_space_index`, not Space
  usage history. Give non-active entries a deterministic initial order from
  their stored array order and rank the saved active entry as most recent. This
  is a bootstrap fallback, not recovered historical usage; later claims and
  selections establish actual recency.
- **Legacy identity migration:** v3 Space entries and tabs have no durable
  domain IDs, and serialized pane-tree node IDs are file-local. Assign UUIDs
  to each imported Space, tab, leaf, and split node in the same SQLite
  transaction as the import. Translate active Space/tab/pane references
  through the generated ID map and preserve tab order and pane-tree shape.
  Commit the import marker with those rows so concurrent startup or a retry
  cannot create a second set of IDs; after commit, every process reads the
  same identifiers.
- **Local state:** active tab, active pane, focus, window geometry, terminal
  size, selection, search, and scroll position remain local to a process/window.
  Shared state includes Space identity, ordered tabs, pane trees, and session
  references.
- **Terminal sessions:** one host PTY/SSH-PTY and one host-side emulator per
  session. `Attach` creates an independent client attachment and sends a
  consistent snapshot before that client's live stream. Per-client bounded
  queues and resync prevent slow clients from blocking the PTY reader or other
  clients. Detaching one socket removes only that attachment. V2 `SessionInfo`
  reports `attachment_count` and an opaque `resize_controller_id`; typed
  attach/detach/controller events update participants, and reconnect obtains
  the authoritative current status. The existing Terminal status surface
  shows the attached-view count and whether the local view owns resize control.
  Participant summaries add no process IDs, usernames, client hostnames, or
  secret material; existing session metadata keeps its current contract. V1
  status retains its legacy attached boolean.
- **Input and resize:** each v2 attachment has a stable random
  `TerminalAttachmentId` across socket reconnects and a monotonically
  increasing client input sequence. The host accepts only the next frame,
  atomically assigns a per-session admission sequence, and returns a typed
  acknowledgement. A retry at or below the accepted high-water mark is
  acknowledged cumulatively without writing the bytes twice; a sequence gap,
  non-attached sender, or full/closed writer queue is explicitly rejected and
  does not advance that mark. The client holds a bounded in-memory suffix of
  unacknowledged frames and replays it in order after reconnect. Acknowledgement
  means admission to the ordered PTY-writer queue, not execution by the shell;
  writer failure after admission is reported to every participant. Both client
  and host queue exhaustion are visible and never silent. A v1 input frame has
  no retry or acknowledgement metadata but enters the same writer order, and
  writer failure uses the existing `Message::Error` understood by v1 clients.
  Concurrent clients are ordered by host admission; each complete frame stays
  contiguous. A single resize-control lease is granted to the focused view;
  only its holder may resize the PTY. The lease is released on detach/disconnect
  and may be claimed by another focused view. Every attached client may send
  terminal input.
- **Close and quit:** closing a shared tab is a shared Workspace mutation.
  Remove the terminal session only if no Space/session reference remains and
  the Detached lifecycle permits it. App/window quit detaches only that
  participant. Global “Quit and stop all sessions” and explicit session Kill
  remain global and notify every participant.
- **Prompt authority:** every host-key, authentication, and transfer-conflict
  decision gets a fresh `PromptRequestId` bound by the host to the socket that
  initiated `SshConnect` or `TransferEnqueue`. Only that owner receives the
  actionable prompt; other subscribers receive read-only status. Resolution
  includes the request ID, and the host validates both the current connection
  owner and pending token, then consumes the token before calling the service.
  Duplicate, stale, or wrong-owner answers return a typed rejection. Subscribe
  replay never replays actionable prompt tokens. On owner disconnect, revoke
  the token, reject unanswered host-key decisions, cancel or park SSH auth, and
  leave transfer conflicts parked without a modal. Another client must call
  `TransferRecoverConflict { job_id }` to receive a fresh token; it cannot
  answer using the old one.
- **Settings:** `general.sessionRestore = false` disables restoration after
  the last live Workspace lease ends; it must not disable live sharing between
  running processes. Final clean release and startup lease reaping apply this
  policy in the same transaction that determines whether another live lease
  exists: if no lease remains, purge shared Workspace structure, session
  references, and the Workspace change log. A startup that wins the transaction
  first preserves live state; a release/reaper that wins first leaves the next
  startup a fresh empty Standalone Space. Never kill Detached Host sessions as
  part of Workspace purge. Existing `terminal.sessionHost` remains the opt-in
  for Detached mode.
- **Notifications:** use `labonair-notifications-core` and the statusbar
  notification dropdown for passive host/store failures. Use the existing
  Terminal state surface for attachment, controller, and recovery state. Add
  no toast or duplicate shell status surface.
- **UI kit:** reuse the current Workspace Spaces popover, shared list/menu/
  button components, Terminal pane, and existing status/notification
  components. No local copies of controls.
- **Dependencies:** no Workspace-to-`labonair-terminal-host` edge. Add the
  `labonair-workspace-storage` sibling and its `workspace → workspace-storage`
  edge only for the process-safe Workspace persistence boundary. Add no new
  external crate unless a measured gap requires a separately documented
  decision.
- **Compatibility:** set the new protocol to v2, the v2 host minimum to v1,
  and the v2 app minimum to v2. A v2 host accepts v1 frames but sends no v2-only
  acknowledgements/events to a v1 client; v1 input keeps its legacy
  unsequenced/no-replay semantics and is written in host admission order. V1
  clients do not sync Workspace structure, can resize only while holding the
  resize lease, and a v1 `Kill` with another attachment becomes a detach with
  an explicit response. A v2 app
  reports a v1 host as incompatible and does not silently fall back to a local
  session. Never terminate or replace an incompatible host with active
  sessions/jobs; an idle host upgrade uses an explicit recovery path.
- **Scrollback:** the Detached Host is the canonical writer for active shared
  session history; attached app processes must not overwrite the same key from
  their local emulator snapshots. Preserve the existing app-owned path for
  App-mode terminals. Cleanup must consult Workspace references across all
  Spaces and processes, preserve data for active host sessions, and delete a
  key only after its last Workspace reference and host session are gone. A
  `sessionRestore=false` final-lease purge removes Workspace references but
  keeps scrollback for any still-live Detached Host session. Workspace storage
  issues a per-key GC permit only when no persisted Space references the key;
  claiming it tombstones the key so stale writes cannot reintroduce it.
  The permit has a generation and expiry so a different cleanup can retry
  after a crash; a failed deletion releases it for retry, and an already-
  missing file counts as complete. Terminal consumes the permit to delete the
  file off-thread and confirms completion using the same generation. Shell
  supplies the current live-host key set; if host status is unavailable,
  cleanup retains files. Shell stores no duplicate reference state. A local
  process may not prune a different process's scrollback.

## User-visible behavior

1. With Detached mode enabled, process A opens the last-used free Space.
2. Process B starts concurrently and atomically receives another free Space,
   or one new empty Space with the prior project identity if all are claimed.
3. The Spaces popover marks occupied Spaces and offers an explicit action to
   join one. Choosing it shares structural tabs and panes without sharing
   focus or local Terminal view state.
4. Opening or changing tabs, splits, pane order, or Space identity in either
   process commits one revision and appears in the other process. A stale
   write is reloaded/retried or reported as a typed conflict; it never silently
   wins.
5. Both processes attach to one existing terminal/SSH session, receive the
   same ordered PTY output, and may send input. A slow client resynchronizes
   alone. Only the resize-controller lease holder changes the PTY size.
6. A participant disconnect or ordinary quit leaves other views and sessions
   running. Closing a shared tab removes its shared references. Explicit global
   kill/shutdown reaches all clients.
7. Reconnect reloads the latest Space revision and host snapshot before live
   updates resume. With `sessionRestore=false`, closing one participant keeps
   state for other live leases; final clean release or crash expiry atomically
   purges Workspace references, preserves live Detached sessions, and makes a
   concurrent next startup either join the surviving state or start fresh,
   according to which transaction wins.
8. Incompatible host/build and prompt ownership states are visible through the
   existing notification and owner surfaces. No detached failure is
   represented as a local session that appears shared.

## Implementation plan

1. Update ADR 0009/0010, capability/workflow records, and dependency inventory
   to the final contract; keep this task in `In Progress` only when it reaches
   the queue head.
2. Add the Workspace storage sibling, UUID domain IDs, transactional schema,
   v3/legacy snapshot migration, atomic leases, expected-revision mutations,
   change log, and cross-process reload path. Keep all DB operations off the
   GPUI foreground thread.
3. Replace the single Terminal sink with per-client attachments and isolated
   output queues; implement per-client snapshot/resync, input sequence/ack,
   resize-control leases, disconnect cleanup, and protocol compatibility.
4. Connect Workspace and Terminal through their existing owner APIs and shell
   composition. Route every shared structural action through Workspace
   revisions; preserve local focus/view state and current Spaces entry points.
5. Audit tab close, session Kill, app quit, global shutdown, SSH/host-key and
   transfer prompts, reconnect, session restore, and scrollback cleanup. Remove
   obsolete `Stolen` UI/state once the compatibility removal condition is met.
6. Add two-process integration evidence, update command/surface/workflow,
   security, architecture, and capability documents, then run all required
   verification scopes and native visual checks.

## Acceptance criteria

- [ ] Two native app processes use one Detached Host at the same time.
- [ ] Simultaneous startup claims distinct Spaces in persisted last-used order;
      each claim advances recency atomically. When none is free, exactly one
      new empty Space is created with the most recently used project's identity.
- [ ] A fresh or purged store seeds exactly one empty Standalone default Space.
- [ ] Explicit join of a leased Space works and all participants see the same
      ordered Space/tab/pane structure and session references.
- [ ] Shared structural mutations are revisioned, conflict-safe, and delivered
      live; reconnect and revision gaps load a complete current snapshot.
- [ ] Space, tab, and pane IDs remain stable across processes and restarts;
      GPUI entities and per-view selection/focus remain local.
- [ ] A v3 and pre-v3 supported snapshot migrate without losing restorable
      tabs, panes, session refs, active selection, or project identity.
- [ ] Migrated Space/tab/pane UUIDs remain identical across a second process
      and a later restart; migration retries do not duplicate imported rows.
- [ ] `sessionRestore=false` leaves live sharing enabled while clients are
      attached; one participant's exit preserves state, while final clean
      release or expiry purges Workspace state atomically against a racing
      startup and does not kill live Detached Host sessions or delete their
      scrollbacks.
- [ ] Two or more clients attach one local and one SSH terminal session without
      stealing one another; each receives a consistent snapshot and live output.
- [ ] Session status and attach/detach/controller events report participant
      count and current resize owner; reconnect restores authoritative status,
      and participant summaries expose no process or user identity details.
- [ ] Input from multiple v2 clients is admitted in a documented per-session
      sequence; every accepted frame is acknowledged and no rejected frame is
      reported as successful. Concurrent frames remain contiguous.
- [ ] A lost acknowledgement followed by reconnect/retry admits bytes exactly
      once; sequence gaps and queue exhaustion are visible and do not advance
      the accepted sequence. Writer failure reaches every participant.
- [ ] Resize-control handoff is deterministic; non-controller resize is
      rejected; disconnect releases the lease.
- [ ] Slow-client overflow/resync affects only that client. Session exit,
      kill, acknowledgement, and host shutdown reach all attached clients.
- [ ] Closing one participant or its app does not kill a session still
      referenced elsewhere; explicit global Kill and “Quit and stop all
      sessions” still terminate as specified.
- [ ] Startup, clean release, heartbeat expiry, crash/reconnect, simultaneous
      claims, lease-generation fencing (including stale mutation, heartbeat,
      and release), stale revisions, the no-free-Space path, and the
      `sessionRestore=false` final-release/startup race are covered.
- [ ] Scrollback cleanup preserves references from other Spaces, app
      processes, and host sessions; stale cleanup snapshots cannot delete a
      concurrently referenced key, unavailable host state fails closed, and
      abandoned GC permits are safely reclaimable after a cleaner crash.
- [ ] Host/app compatibility is documented and tested; an incompatible host
      with live sessions is not stopped or replaced automatically.
- [ ] Host-key, authentication, and transfer-conflict prompts have one
      responsible client and at-most-once resolution.
- [ ] Unix socket permissions, peer UID checks, secret redaction, and fail-
      closed recovery remain intact; no TCP listener is added.
- [ ] Existing app-mode terminal behavior remains unchanged.
- [ ] Editor buffers remain independent; no collaborative-editing claim is
      made.
- [ ] Required package tests, process-level tests, workspace gates, document
      and queue checks, dependency graph checks, and native visual evidence
      pass or have a precise environment blocker recorded.

## Verification and evidence

Focused tests must cover:

- **Terminal host:** two and multiple clients; independent snapshots and output;
  participant status count and controller transitions; one-client detach;
  detached input/resize denial; ordered concurrent input,
  cumulative acknowledgement, retry deduplication, sequence-gap rejection,
  explicit backpressure and writer failure; resize-controller transfer; independent slow-client
  overflow/resync; exit/Kill/Ack/shutdown fanout; legacy protocol behavior;
  two subscribers receive one actionable SSH/transfer prompt only at its owner
  while others see read-only status; competing, stale, and wrong-owner answers
  are rejected; owner disconnect revokes the token, host-key fails closed,
  transfer conflict stays parked, and explicit recovery issues one fresh token.
- **Workspace store:** simultaneous claims; explicit occupied-Space join;
  exactly-one new Space; heartbeat/release/expiry; stale lease-generation
  mutation/renewal/release rejection; revision conflict; event replay and
  snapshot resync; deterministic last-used ordering and atomic
  recency advancement under concurrent claims; v3 migration recency bootstrap;
  empty-store default seeding; restore preference off with concurrent
  final-release/startup and host-session survival; scrollback reference
  retention, generation-fenced/reclaimable per-key GC permits/tombstones and
  no-delete when host state is unavailable; one canonical
  scrollback writer for a multi-attached Detached session, with no racing
  app-side writes to the same key.
- **Process boundary:** launch two OS client processes against one host/store
  in an integration harness. Two `SessionSink`s in one process alone are not
  sufficient evidence.
- **User flow:** launch two native `labonair` processes, confirm default
  allocation, join a Space, mutate tabs/panes in each process, share local and
  SSH terminal input/output, close one window, reconnect, and confirm the
  surviving process and host remain correct.

Run:

```text
cargo test -p labonair-workspace-storage
cargo test -p labonair-terminal-host
cargo test -p labonair-terminal
cargo test -p labonair-workspace
cargo fmt --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/check-crate-deps.sh
python3 scripts/check_documentation.py
python3 scripts/check_rework_queue.py
python3 scripts/verify.py --scope all
git diff --check
```

Native visual evidence is required for the Space popover's free/occupied/join
states, shared structural updates, Terminal multi-attach/controller state,
host-incompatible and reconnect errors, and normal/narrow/focused layouts.
Record the exact binary, two process IDs, host PID, viewport, and capture paths.

## Change contract

```text
Request type: Product workflow and architecture change; multi-process Workspace and Detached Terminal coordination.
Owner module: Workspace for shared Space/tab/pane structure, identifiers, leases, persistence, migrations, and Workspace UI; Terminal for host sessions, per-client attachments, PTY I/O, output resync, resize ownership, and session lifecycle. Shell composes typed owner APIs only.
Canonical capability crates: labonair-workspace and labonair-terminal; new GPUI-free labonair-workspace-storage sibling for transactional multi-process persistence.
Canonical user entry point: Existing Workspace Spaces popover; explicit join of an occupied Space. Existing Terminal tabs/panes display attached shared sessions.
Public typed contract: UUID-backed SpaceId, TabId, PaneId, WorkspaceInstanceId; revisioned SpaceMutation/WorkspaceChange/SpaceLease with a database-generated lease generation; typed Workspace scrollback-reference snapshot and per-key ScrollbackGcPermit/ScrollbackRetired result passed through shell composition to Terminal cleanup; per-view TerminalAttachmentId, SessionInfo attachment count/controller ID, sequenced InputFrame/InputAck/InputRejected, ResizeLease, and typed attach/detach/controller SessionEvents; socket-owned PromptRequestId, single-use PromptResolve, and TransferRecoverConflict. No workspace state in terminal-host.
State owner: Workspace owns shared structure and lease metadata; Terminal host owns PTY/SSH-PTY, shadow emulator, attachment state, input order, and resize lease.
Persistence owner: Workspace owns the SQLite store and one-time import from session.json v3/v2/flat. Terminal host retains its independent session lifecycle and scrollback store.
Commands and keymap entries: Extend the Workspace-owned Spaces picker; add a stable command only if the existing picker cannot expose explicit join. Expose `TransferRecoverConflict` in the existing Transfer surface. Do not add shell-wide command tables or permanent chrome.
Events and registry contributions: Workspace revision/change records; Terminal per-client attach/input/resize/session events and owner-only actionable prompt events; transfer progress/status events remain read-only broadcasts. Use existing owner command/surface/notification registries only.
Settings: Existing terminal.sessionHost gates Detached mode. general.sessionRestore controls restoration after all leases end, not live sharing.
Notifications: Existing notification center/statusbar for passive host/store failures; existing Terminal pane/owner surfaces for actionable reconnect, resize, host-key, auth, and transfer decisions.
UI-kit components: Existing shared popover, menu, list, button, status, and notification components.
Allowed dependency changes: workspace -> new workspace-storage sibling; workspace-storage -> rusqlite at the existing workspace version. No terminal-host -> workspace/UI/GPUI edge and no new external crate without a separate reason.
Tests: Focused storage and host package tests, Workspace integration tests, protocol compatibility tests, a two-client-process integration harness, full workspace Rust gates, and native two-app visual workflow.
Visual evidence: Existing Spaces picker free/occupied/join states, cross-process tab/pane update, shared Terminal/controller, reconnect/incompatible host, and two native process IDs; Pending until R07-008 is active and a supported visual host is available.
Security impact: Keep per-user Unix socket, 0700/0600 permissions, peer UID check, in-memory-only secrets, fail-closed prompts; no TCP or remote access.
Performance impact: No foreground I/O. Measure SQLite write/claim latency, 250 ms change-poll overhead, per-client queue memory, and slow-client recovery with at least three clients.
Removal condition for compatibility code: Remove v1 host/client handling only after the release support policy declares v1 unsupported and a separate removal task is complete. Keep the legacy snapshot reader until every supported `session.json` generation has migration fixtures and the documented migration window has elapsed; it exists to protect user data.
```

## Current status and next step

The architecture and regression requirements are recorded. No feature source
code or tests have been changed by this planning task. R07-000 remains the
active queue item; R07-008 must not start until every preceding queue task is
complete.
