# R12-003: Forwards UI, palette, settings and integration

## Status

`🟨 Implemented — visual verification pending` — governed by [ADR 0016](../../adr/0016-multi-hop-jump-routes-and-port-forwards.md). Common rules: see R12-001. Requires R12-002 (F1–F6) done.

Reference patterns to copy (read them first): `crates/transfers-ui/src/status_item.rs` (show-on-demand item + shell registration), `crates/activity-ui/src/status_item.rs` (`popover_with_exit`, `Presence`, rows with `ListItem`), `crates/hosts/src/command_provider.rs` + `crates/hosts-ui/src/command_provider.rs` (palette provider + submenu snapshots), `crates/workspace/src/modal_layer.rs` (dialogs), `crates/notifications-core` (notifications), `docs/registries.md`, `docs/design-system.md`.

## U1 — Palette contributions (`labonair-forwards`)

Owner: `crates/forwards`, `crates/command-palette-core`, `crates/command-palette-runtime` / handler registration site, `crates/shell` (wiring only).

1. Add `CommandId` variants and (where the enum requires it) `CommandSubmenu` variants: `ForwardsNew`, `ForwardsStartSaved`, `ForwardsStop`, `ForwardsStopAll`, `ForwardsManage`, `JumpEditRoute`, `OpenSshVia` (titles per ADR §4: `Port Forwards: New…`, `Port Forwards: Start Saved…`, `Port Forwards: Stop…`, `Port Forwards: Stop All`, `Port Forwards: Manage`, `Jump Hosts: Edit Route…`, `Open SSH via…`; category `Connections`). Stable ids; no keybindings by default.
2. `ForwardsCommandProvider` (`CommandProvider`) in `labonair-forwards` plus pure submenu builders: `saved_forwards_submenu(rows)`, `active_forwards_submenu(rows)` (rows = `ForwardInfo` + host name; item subtitle = `route_label`, `active` = running) and the typed `SubmenuAction` variants needed (`ToggleForward { forward_id }`, `CloseForward { id }`, `OpenForwardEditor { host_id }`, `QuickForward { spec }`). If `SubmenuAction` is closed in `command-palette-core`, add the minimum variants there.
3. Quick syntax: when the query of `ForwardsNew` parses via `labonair_forwards::quick::parse_quick`, show a single result row `Forward -L 8080:db:5432 on <host>` after a host is chosen (host picker first, then the parsed spec; follow the existing two-step submenu mechanism used by hosts if there is one; otherwise the first step is the host picker and the query is carried over).
4. Handlers are contributed by the owner via `PaletteActionHandlerRegistry` (see `docs/registries.md`); the shell only registers. Handlers emit typed `ForwardsUiEvent`s (defined in U2) — no direct view imports from the palette crates.
5. `Open SSH via…`: pick target host, then pick one or more jump hosts in order (multi-select submenu or repeated single picks — choose the simpler existing mechanism), then connect with a one-off route override carried as `HostOpenRequest.route_override: Option<Vec<String>>` (extend `labonair-hosts::HostOpenRequest`; the workspace passes it through to `new_ssh_session`; the transport applies it in `resolve_jump_chain` via an optional parameter that replaces the stored chain; never persisted). Add the transport plumbing in `SshConnectRequest` (`route_override: Option<Vec<String>>`, also in the daemon `SshHostSnapshot` selection) with unit tests.
6. Tests: provider lists the commands; submenu snapshots for sample rows; quick parse integration.

Accept: `cargo test -p labonair-forwards -p labonair-command-palette-core -p labonair-hosts`.

## U2 — `labonair-forwards-ui`: model, statusbar item, popover

Owner: new `crates/forwards-ui`, `scripts/crate-graph.toml`, `crates/shell` (registration), `crates/workspace` (feeding the model).

1. `ForwardsView` (GPUI entity) holds `Vec<ForwardRow>` (`ForwardInfo` + `host_label` + `route: Vec<String>`), open/closed popover state and emits `ForwardsUiEvent { New{host_id: Option<String>}, Close(ForwardId), Restart(ForwardId), Prompt(ForwardId), OpenInBrowser(url), CopyAddress(String), StopAll, Manage{host_id} }` (the composition root/workspace performs the actions through `SshForwardService`; the view never talks to services). `set_rows` replaces the snapshot only when changed.
2. `ForwardsStatusItem` implements `StatusItem` with id `forwards`, `StatusSide::Right`, order 15, group 1, `is_empty` true when there are no rows (exactly like Transfers); icon button (use an existing arrow/link icon from `IconName`; if none fits add one Lucide SVG the way other icons are added) with a count and a status dot (green all active, amber any `Reconnecting|Starting`, blue `AwaitingInput`, red any `Failed`) built from `Palette` tokens; registered through `status_item_registration` in the shell like Transfers.
3. Popover (`popover_with_exit`): header (`Port forwards`, `New…`, `Stop all`), groups per host (host name + route chip `via a › b`), rows: kind glyph, `route_label`, state text (`Active`, `Reconnecting (2)`, `Needs input`, `Failed: reason`), `N connections`, and per-row buttons (copy address, open in browser only for Local with `http`-looking targets `80/443/3000/8000/8080/…` — keep a tiny pure helper `looks_like_http(port)`, restart for Failed/Stopped, close). `AwaitingInput` rows show an `Authenticate` button emitting `Prompt`. Footer: `Manage in Hosts`. Use ui-kit components only; follow `docs/design-system.md` spacing/tokens; keyboard: Esc closes, arrows move, Enter activates the focused row's primary action.
4. Workspace feeds the view from the revision task of F5 (resolve host labels and routes via `HostView`), and handles `ForwardsUiEvent`s: `Close/Restart` → service; `StopAll` → close all; `CopyAddress` → clipboard; `OpenInBrowser` → existing open-url helper; `New`/`Prompt`/`Manage` are consumed in U3/U5/U4.
5. Persisted placement uses the existing status placement blob; add `forwards` to the status item id list if one is enumerated (`status_placements.rs`), keeping the migration map untouched.
6. Tests: row building and dot-colour aggregation as pure functions (`aggregate_health(&[ForwardInfo]) -> Health`), `looks_like_http`, event emission for row actions via the model API.

Accept: `cargo check -p labonair-forwards-ui --all-targets`, tests, `scripts/check-crate-deps.sh`.

## U3 — Forward editor

Owner: `crates/forwards-ui` (`editor.rs`), `crates/workspace` (dialog hosting).

1. `ForwardDraft` (pure, in `labonair-forwards`): fields as strings (`kind`, `name`, `listen_host`, `listen_port`, `target_host`, `target_port`, `auto_start`, `save_to_host`), `validate() -> Vec<FieldError>`, `to_spec(host_id)`, `from_saved(&SavedForward)`, `public_bind_warning() -> Option<&'static str>`, and a `diagram() -> String` preview (`localhost:8080 ──ssh──▶ db:5432`, `server:9000 ◀──ssh── localhost:3000`, `SOCKS5 127.0.0.1:1080 ──ssh──▶ any`). Unit tests for each.
2. `ForwardEditor` GPUI view in `forwards-ui`: segmented control (Local / Remote / Dynamic) with one-line descriptions, host picker (when opened without a host), fields via ui-kit text inputs with inline validation, bind address select (`127.0.0.1`, `localhost`, `0.0.0.0` with the public-bind warning and, when `forwards.confirmPublicBind` is true, an explicit confirm checkbox), live diagram, `Start` / `Save to host and start` / `Save only` actions. It is hosted by the workspace modal layer (Esc cancels). It emits `ForwardEditorEvent::Submit { spec, lifetime, save_to_host }`.
3. Workspace handles the event: `open` the forward (Pinned for ad-hoc, `WithConnection` for saved+auto), and when `save_to_host` writes the host's `tunnels` column through the Hosts owner API (typed request, no direct SQL) — find the existing `update host` path the Hosts form uses (`apply_host_change`) and reuse it.
4. Wire `ForwardsUiEvent::New` and the palette `ForwardsNew` / quick-syntax handler to open the editor (prefilled from the parsed quick spec).
5. Tests: draft validation/diagram; submit mapping.

Accept: tests green; visual check pending note.

## U4 — Hosts Forwards tab and `~/.ssh/config` forwards

Owner: `crates/hosts-ui` (`hosts.rs`, `hosts/page.rs`), `crates/ssh-transport/src/config_parser.rs`, `crates/ssh/src/lib.rs` (only if the entry type needs the new fields).

1. Replace `TunnelDraft`/`parse_tunnels`/`serialize_tunnels` with the shared `labonair_forwards::saved` codec and a per-row UI that shows kind (segmented), listen host/port, target host/port, name, auto-start toggle, and a live state dot + `bound_port` for forwards currently running for this host (data from the workspace snapshot via the `HostView` bundle — replace `ActiveTunnelRow`/`set_active_tunnels` with `set_forward_states(HashMap<ForwardId, ForwardState>)`). Tab title `Port forwards`; empty-state text explains the three kinds. Rows have `Start/Stop` buttons that emit `HostManagerEvent::ForwardAction { host_id, forward_id, action }` handled by the workspace via the service.
2. Remove the `Active tunnels` settings group and `ActiveTunnelRow` from `hosts-host`/`hosts-ui`/workspace/bootstrap (duplicate surface); add a one-line link row `Open port forwards` that emits the `Manage`/`New` request to open the popover.
3. Editing saved forwards of a host with running forwards: after autosave the workspace diffs saved vs running (`open`/`close`/`restart` the changed ones) — implement the pure diff `plan_forward_changes(saved, running) -> Vec<Change>` in `labonair-forwards` with tests.
4. Port-conflict hint: inline validation warns when another host has a saved forward with the same `listen_host:listen_port` (pure helper over all hosts' saved forwards, tested).
5. ssh-config import: parse `LocalForward`, `RemoteForward`, `DynamicForward` lines (`LocalForward [bind:]port host:hostport`) into saved forwards on the imported host (`SshConfigEntry` gets `forwards: Vec<String>` raw lines or typed values — keep serde defaults so existing callers compile); export writes them back. Tests.

Accept: `cargo test -p labonair-hosts-ui -p labonair-forwards -p labonair-ssh-transport`.

## U5 — Prompt dialog, notifications, settings

Owner: `crates/forwards-ui`, `crates/workspace`, `crates/settings-content`, `crates/settings`, `crates/settings-ui`.

1. `ForwardPromptDialog` (modal): renders `PendingPrompt` for a forward — host key (host, fingerprint, mismatch warning, Trust / Reject), password, passphrase (masked input, Enter submits, Esc rejects) and the hop label when present. Answers go through `SshForwardService::answer_prompt`. It opens from `ForwardsUiEvent::Prompt`, and **automatically** when the user started the forward (`open`/Start) and it enters `AwaitingInput`.
2. Notifications (registry only): forward `Failed` (title `Port forward failed`, body = reason, action `Show`), reconnect give-up, `AwaitingInput` for a forward not started by the user in this moment (action `Authenticate`), and the old-daemon version gate message. De-duplicate by forward id + state so a flapping forward does not spam; resolve/clear when it becomes `Active`.
3. Settings values (ADR §4): `forwards.defaultListenHost` (string, default `127.0.0.1`), `forwards.autoReconnect` (bool, default true), `forwards.confirmPublicBind` (bool, default true) in `settings-content` under the hosts area per `docs/settings.md`; a small settings group on the Hosts page or the closest existing SSH settings group (one group `Port forwards` with three rows) via the existing owner-surface mechanism; feed `autoReconnect` into the registry's `ReconnectPolicy` through the service adapter (typed `set_policy`), and `defaultListenHost`/`confirmPublicBind` into the editor.
4. Tests: notification de-dup logic (pure), settings defaults/serialization.

Accept: tests green; `cargo check --workspace --all-targets`.

## U6 — Docs, gates, verification

Owner: docs + whole repo.

1. Update `docs/modules.md` (SSH and Hosts rows; new `Port forwards` row: `labonair-forwards`, `labonair-forwards-ui`, entry statusbar item `forwards` + palette), `docs/registries.md` (status item `forwards`, palette commands), `docs/testing.md` (visual states: forwards popover empty/active/reconnecting/failed/awaiting input, editor three kinds with public-bind warning, route display on tab/hover/loading screen, hop prompt), `docs/security.md` (public bind confirmation, AI/MCP cannot create forwards, secrets never in `ForwardInfo`), `docs/gotchas.md` for any non-obvious russh/GPUI finding made during R12, `docs/tasks/rework/README.md` table rows for R12-001..003 with real state.
2. Run `scripts/check.sh` and fix all issues in R12 code.
3. Build and run `cargo run -p labonair`; with a reachable test SSH server (or state clearly that none is available) verify and record: route display, forward popover appears only with forwards, create/close local/remote/dynamic forwards, prompt dialog. Capture screenshots with `scripts/screenshot.sh` if available. Do not claim UI verification that did not happen.

Accept: gates green; doc rows present; verification record in the task file's Notes.

## Acceptance for the whole task

- [ ] U1–U6 accepted.
- [ ] No feature-local error surface; all messages via Notifications.
- [ ] No duplicate "active tunnels" surface remains.
