# ADR 0010 — Shared Workspaces and multi-client Detached sessions

* **Status:** Accepted; implementation in progress
* **Date:** 2026-11-19
* **Deciders:** project owner (requested product behavior) and implementation team
* **Related:** [`0009-detached-session-host.md`](0009-detached-session-host.md), [`../workspace-model.md`](../workspace-model.md), [`../capabilities.md`](../capabilities.md), [`../architecture.md`](../architecture.md), [`../security/trust-boundaries.md`](../security/trust-boundaries.md)

## Context

ADR 0009 established a per-user Detached Host and explicitly deferred
multi-client session mirroring. The current daemon already accepts multiple
same-UID Unix socket connections, but each terminal session has one replaceable
output sink. Workspace Spaces are also local to one app process: numeric IDs
are allocated from process-local counters, and the v3 session snapshot stores
all spaces in one JSON file with the active space represented by an index.
Concurrent app processes therefore cannot safely share tabs, panes, sessions,
or persisted workspace changes.

The product now requires multiple native app processes to use the same local
Detached Host. Each process normally opens a different Space, while an explicit
user action may join an occupied Space. Shared Space structure and terminal
output must update across processes without turning Workspace into a Terminal
registry or making the daemon depend on GPUI.

## Decision

1. **One local host, multiple clients.** Keep one `labonair-host` daemon per
   data directory and keep its `flock`, private Unix socket, `0700` host
   directory, `0600` socket, and peer-UID check. Multiple app processes of
   that OS user may connect at once. This decision does not add TCP, remote
   pairing, or another authentication layer.
2. **Ownership remains split.** Workspace owns Space, tab, pane, layout,
   revision, lease, and workspace persistence state. Terminal owns PTY/SSH-PTY
   sessions, host emulators, client attachments, input ordering, resize
   control, and output delivery. `labonair-shell` only composes the typed
   contracts. The Detached Host does not depend on Workspace UI, GPUI, or
   Workspace private state.
3. **Workspace persistence is transactional and Workspace-owned.** Replace
   concurrent full-snapshot writes as the authority for live shared state with
   a Workspace-owned SQLite store. Add `labonair-workspace-storage` as a
   GPUI-free storage sibling under the Workspace owner; it owns schema
   migrations, lease transactions, revision checks, and the committed change
   log, while Workspace owns the domain IDs and typed mutation contract. Store
   these tables in a dedicated `workspace.db` beside the legacy Workspace
   snapshot, rather than adding Workspace schema to the app-wide
   `labonair.db` managed by `labonair-persistence`. The storage sibling owns
   its connection and migrations and uses the existing workspace dependency
   on `rusqlite` 0.40. Use SQLite write transactions and per-Space monotonic
   revisions. Keep a workspace-wide monotonic `space_usage_sequence` and a
   `last_used_sequence` per Space. Advance and write the sequence in the same
   transaction as a successful startup claim, explicit join, or active-Space
   selection. This transaction order defines recency without relying on
   process clocks. Automatic startup orders unclaimed Spaces by descending
   `last_used_sequence`, with stable `SpaceId` order as a deterministic tie
   breaker. If no Space is free, copy the project identity from the highest
   `last_used_sequence` Space, create an empty Space, and claim it in that
   transaction. The v3 snapshot has no per-Space history: migration ranks
   non-active entries in their stored order and the saved active entry last as
   a deterministic bootstrap; subsequent claims and selections record actual
   use order. Keep the database under the private per-user app data directory
   with restrictive file permissions; do not persist credentials or secret
   material. A mutation
   must compare its expected revision and return a typed stale-revision result
   when it loses a race; it must never report a stale full-snapshot replacement
   as success. The current `session.json` v3 and older supported formats are
   migration inputs, not a parallel write path. Keep migration idempotent and
   preserve the source snapshot until the imported state is verified.
4. **Stable domain IDs, local UI entities.** Use durable UUID-backed
   `SpaceId`, `TabId`, and `PaneId` values in shared Workspace state and
   serialized references. Keep GPUI entities and view state local to each app
   process. Give each Workspace window/client a separate random instance ID;
   do not use process-local counters as shared IDs. During the one-time legacy
   import, assign UUIDs to every Space, tab, and serialized pane-tree node,
   including split nodes. Translate the saved active-Space index, active-tab
   index, and active-pane node reference through those mappings while retaining
   order and tree shape. Commit these IDs with the imported records so every
   process and later restart observes the same identities.
5. **Space allocation uses expiring, fenced leases.** Store one current lease
   row per Workspace instance, referencing its active Space; multiple
   instances may hold rows for the same Space after explicit joins. Each
   successful claim or join receives a
   database-generated, globally increasing lease generation. Every mutation,
   renewal, and release must present its exact Space, Workspace instance, and
   generation; a stale generation returns a typed `LeaseLost` result and cannot
   mutate the Space, renew an expired lease, or release a later claim. Startup
   atomically claims the most recently used unclaimed Space, based on the
   persisted usage sequence. If that Space is already leased, claim the next
   free Space by the same ordering. If the store has no Space rows, seed and
   claim one empty Standalone default Space. If Space rows exist but all are
   leased, create one empty Space with the most recently used Space's project
   identity and no copied tabs or sessions. Successful startup claims,
   explicit joins, and active-Space
   selections advance the usage sequence atomically with their
   lease/selection transaction. The existing Spaces picker is the explicit
   join path for an occupied Space. A lease is renewed while its Workspace
   instance is alive and released on clean close; expiry makes a crashed
   instance's Space available again. Switching Spaces changes that instance's
   lease only after the target claim succeeds, in the same transaction. Other
   participants' rows remain intact. Simultaneous starts and joins serialize
   through the Workspace store.
6. **Shared structure, local presentation.** Space identity, ordered tabs,
   pane trees, and session references are shared. Active tab, focus, window
   geometry, terminal size, selection, search, and scroll position remain
   local to each app window. Workspace mutations are typed and revisioned.
   Other clients consume committed changes through an off-thread revision
   poller with a target update latency of 250 ms; on reconnect, a revision gap,
   or an unknown change, they load a complete current Space snapshot. No
   SQLite or IPC wait runs on the GPUI foreground thread.
7. **One hosted session, many attached terminal clients.** A session keeps one
   PTY or SSH PTY and one host-side emulator. Every attached client receives a
   snapshot followed by live output. Output uses a separate bounded queue per
   client; overflow and resync affect only that client and never block the PTY
   reader or another client. V2 session status reports the attached-view count
   and the opaque ID of the current resize controller; attach/detach and
   controller changes update that status for every participant. The existing
   Terminal status surface shows the count and whether the local view owns
   resize control. Participant summaries do not add PIDs, usernames, client
   hostnames, or secret material. V1 clients retain the legacy attached
   boolean. A disconnected client loses only its attachment.
8. **Input and resize are explicit.** Each v2 terminal attachment has a stable
   random `TerminalAttachmentId` across socket reconnects and numbers its input
   frames monotonically. The host accepts only the next sequence for that
   attachment, assigns a per-session admission sequence, and acknowledges
   accepted frames. Retrying an already accepted sequence returns a cumulative
   acknowledgement without enqueuing the bytes again; a sequence gap, detached
   sender, or full/closed writer queue receives a typed rejection. The client
   retains a bounded in-memory suffix of unacknowledged frames and retries it in
   order after reconnect. The host orders complete frames by admission, so
   concurrent clients have a deterministic total order and each accepted frame
   remains contiguous. An acknowledgement means the frame entered the ordered
   PTY-writer queue, not that the child application interpreted it. A later
   writer failure is reported to every participant as a session error. Queue
   exhaustion and connection loss are visible; neither side silently drops an
   unacknowledged frame. V1 input has no sequence or acknowledgement; the v2
   host feeds it through the same ordered writer and reports writer errors via
   the v1 `Message::Error` contract, without promising replay. Each session has
   one resize-control lease. A focused view requests it through a typed
   operation; if no controller exists, the first attached view may claim it.
   Only the current holder can resize the shared PTY. The lease is released
   when that client detaches or disconnects. All attached clients may still
   send terminal input.
9. **Close and quit retain their distinct meanings.** Closing a shared tab
   removes its Workspace reference for every participant. Detaching one app
   process or closing its window never kills a session another Space or
   participant still references. A globally requested session kill is sent
   to every participant. App quit detaches only that app's client; the existing
   “Quit and stop all sessions” action remains the explicit global shutdown.
10. **Live state does not depend on session restore.** `general.sessionRestore`
    continues to control restoration after all Workspace clients have gone;
    it does not turn off sharing while clients are alive. Leases and active
    revisions remain available during the process lifetime. When restore is
    disabled, releasing the final lease or reaping the final expired lease
    atomically clears shared Spaces, tabs, panes, session references, and the
    Workspace change log. Startup reaps leases and applies this policy before
    allocating a Space. The SQLite transaction orders a racing startup against
    final release: if startup claims first, live state remains; if final release
    or expiry is processed first, the next start creates a fresh empty
    Standalone Space. Detached terminal sessions are not killed by this purge;
    they keep their existing host lifecycle and recovery surface.
11. **Cleanup follows shared references.** The Detached Host is the canonical
    writer for an active shared session's scrollback; attached apps do not
    persist competing copies from their client emulators. App-mode terminals
    keep their existing Workspace-owned persistence. Workspace cleanup reads
    every persisted/live Space reference before pruning files, and retains
    data for live host sessions. Before deleting a key, Workspace storage
    issues a per-key GC permit only when the reference count is zero; the
    permit tombstones that key so a stale mutation cannot reintroduce it.
    The permit is generation-fenced and expires, so another cleanup can retry
    after a process crash; failed deletion releases it for retry and a missing
    file counts as completed. Terminal consumes the permit to delete the file
    off-thread, then confirms completion using the same generation. Shell
    supplies the current live-host key set; if host status is unavailable,
    cleanup retains the files. Shell stores no duplicate reference state. A
    final-lease Workspace purge does not remove scrollback
    still owned by a running host session. A process may not delete another
    active Space's scrollback.
12. **Protocol compatibility fails safely.** Bump the current protocol to v2;
    a v2 host advertises `min_compat = 1`, while a v2 app requires
    `min_compat = 2`. A v2 host accepts v1 clients using only v1 frames and
    does not send them v2-only acknowledgements or control events; the host
    still isolates output queues. V1 clients may attach to sessions but do not
    participate in shared Workspace mutations and retain their legacy
    unsequenced input behavior. A v1 client's resize is accepted only while it
    holds the resize lease. If a v1 client requests `Kill` while another client
    is attached, treat it as a detach and report that the session remains live.
    A v2 app requires a v2 host for shared
    Workspace/session behavior. Against a v1 host, it reports incompatibility
    and does not silently create a local replacement. Never terminate or
    replace an incompatible host that reports active sessions or jobs. An
    idle incompatible host may be restarted only through an explicit
    upgrade/recovery path. Test the v1-app/v2-host and v2-app/v1-host matrix.
13. **Prompts have one authority.** Host-key and transfer-conflict decisions
    receive unique typed request IDs and are bound to the socket connection
    that initiated their SSH-connect or transfer-enqueue operation.
    Authentication and passphrase dialogs are routed only to the connection
    that initiated the failed connect; retry is a new connect request. Only
    the owning connection receives an actionable prompt; v2 peers receive
    read-only transfer state, while v1 peers do not receive another client's
    conflict event. V2 resolution carries the request ID, and the host checks
    the current connection owner and claims the token in-flight before
    applying the response. It consumes the token after success and rejects
    duplicate, stale, and wrong-owner responses. A v1 transfer owner may omit
    the new request ID; connection ownership and one-use consumption still
    apply. Event replay to another subscriber never replays an actionable
    token. On owner disconnect, revoke the token and fail closed: reject an
    unanswered host-key decision and leave a transfer conflict parked without
    a modal. Another v2 client may take ownership only through an explicit
    `TransferRecoverConflict { job_id }` operation that issues a fresh request
    ID; do not open duplicate dialogs in every app.
14. **No editor buffer collaboration.** Shared Editor tabs and file
    references do not synchronize unsaved buffer contents or editing cursors.
    Collaborative editing, mobile access, TCP transport, and remote pairing
    require separate product and security decisions.

## Consequences

* The Workspace owner needs a process-safe persistence API for typed mutations,
  revisions, leases, migration, and resynchronization. Its database work must
  run off the GPUI foreground thread.
* The Terminal host protocol and per-session state must change from one sink to
  per-client attachments. Independent slow-client recovery and a unique resize
  controller become testable host contracts.
* The current persisted snapshot can be migrated without losing tabs or
  recoverable terminal references, but its index-based IDs cannot remain the
  shared identity model.
* The existing Spaces picker is extended to distinguish explicit join from
  default startup allocation. No permanent shell chrome or duplicate status
  surface is added.
* Local Unix-socket security and the existing notification center remain the
  trust and user-message boundaries.
* These accepted rules define the current implementation contract. The source
  now includes the Workspace-owned shared store, multi-client session host,
  owner-fenced prompts, and recoverable transfer decisions. Native visual
  evidence and the full repository verification run remain completion gates.
