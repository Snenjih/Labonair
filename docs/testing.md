# Testing and Visual Verification

Related: [`architecture.md`](architecture.md) §11 (verification gates), [`design-system.md`](design-system.md) (review rule), [`performance.md`](performance.md).

## 1. Automated gates

`scripts/check.sh` is the canonical local and CI gate (`.github/workflows/ci.yml`, macOS job; a Linux check-only job is informational): `cargo fmt --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `scripts/check-crate-deps.sh` (crate-graph rules). Test the public contract and the user flow of what you change. A green build is evidence for the checks it ran only; it does not close visual, security, remote, or workflow verification, and it never proves a surface renders.

`scripts/smoke-test.sh` builds the macOS bundle (`scripts/package-macos.sh`), verifies its structure (executable, `Info.plist` lint, icon, `PkgInfo`, version substituted, identifier `com.labonair.rust`, signature when `LABONAIR_SIGN_IDENTITY` is set), runs `cargo test -p labonair --test smoke` (native app init, PTY shell round-trip, update-manifest check), and with `LABONAIR_SMOKE_LAUNCH=1` opens the exact bundled Rust app for 5 s. A process-only smoke test proves startup and survival, not rendering.

## 2. Native visual verification

A visual check must prove the **native Rust** app rendered. A successful launch, or a screenshot of a window named `Labonair`, is not enough: the legacy Tauri app (`/Applications/Labonair.app`) may be installed under the same display name.

**Launch targets (explicit only):**

```text
cargo run -p labonair
target/release/bundle/macos/Labonair.app        (built by scripts/package-macos.sh)
```

Never use `open -a Labonair`. For a packaged check, open the absolute bundle path with `open -n -W`, or run the executable directly, and identify the process by its exact path `target/release/bundle/macos/Labonair.app/Contents/MacOS/labonair` (the debug build is `target/debug/labonair`). The native bundle ID is `com.labonair.rust`; the legacy app uses a different one.

**Fail closed.** A capture helper must find a layer-0 window owned by the exact Rust PID and never fall back to the application name or the frontmost unrelated window. Even an explicit PID must resolve to the exact native executable before capture.

**Helpers:**

- `scripts/package-macos.sh` builds the canonical bundle.
- `scripts/screenshot.sh [out.png] [rust_pid]` (default `shots/labonair.png`; PID also via `LABONAIR_RUST_PID`, else it looks for the debug/bundle executable) validates the PID against the native executable paths, resolves the layer-0 window with CoreGraphics, and captures it with `screencapture`. It distinguishes an invalid PID from a macOS Screen Recording permission failure.
- `scripts/capture-macos-native.sh <out.png>` launches the packaged bundle with `open -n`, resolves its new PID, waits for the window, delegates to `screenshot.sh`, prints binary/PID/artifact, and kills the process. It infers no surface or state.

**Permissions.** Grant Screen Recording (System Settings > Privacy & Security > Screen Recording) to the terminal or runner executing the scripts; Accessibility is separately required for keyboard/pointer interaction. If permission blocks capture or interaction, record the limitation and leave visual acceptance open; never accept a capture without exact process ownership or infer success from a build.

**Linux.** An isolated Xvfb launch may supplement layout diagnosis when no macOS window server is available. Record the exact PID and label it non-release evidence: Linux is build-prepared, not a packaged target, and an Xvfb capture never closes macOS visual acceptance.

## 3. Capture procedure

Preconditions: a macOS host with a graphical session; a disposable application profile or fixture data (never real passwords, private keys, bearer tokens, host addresses, or other secrets); the bundle built from the commit under review; no legacy app, reference renderer, or generic frontmost-window screenshot.

1. Build: `scripts/package-macos.sh`.
2. Launch the exact executable and keep its PID; the process must stay alive while you prepare the state:

   ```text
   BUNDLE_BINARY="$PWD/target/release/bundle/macos/Labonair.app/Contents/MacOS/labonair"
   "$BUNDLE_BINARY" &
   RUST_PID=$!
   ```

3. Prepare one state using section 4, and inspect the whole surface: focus, clipping, anchoring, loading/error copy, adjacent shell zones. Preparation is not evidence.
4. Capture: `scripts/screenshot.sh <path>.png "$RUST_PID"`.
5. Inspect the PNG. Reject it if the state is not visible, another app appears, a secret is visible, or the layout is clipped or mis-anchored.
6. Record the absolute screenshot path, Rust PID, host, and commit in the task handoff. Capture one state per image. If a state cannot be produced or inspected truthfully, leave it pending with the blocker; do not force it with a mock overlay, and create a bounded task for the owner instead. A temporary `/tmp` image or terminal log is not a durable capture.

## 4. State coverage and preparation

For a shell or layout change verify the applicable states: normal/ready, focused (keyboard), empty, narrow, loading, error, and expanded/secondary interaction. Use the canonical command or surface owner and a disposable fixture. The tables below are preparation guidance, not alternate state definitions.

### Titlebar (`crates/shell/src/titlebar.rs`)

| State | Preparation | Check |
|---|---|---|
| normal | One active workspace tab. | Titlebar, tab strip, global-menu trigger, window controls without overlap. |
| focused tab / multiple tabs | Open several tabs via registered commands; focus one. | Selection treatment, ordering, labels, close affordances, overflow coherent. |
| narrow window | Resize to a narrow viewport. | Content usable; no permanent control clipped or duplicated. |
| zen mode | `ToggleZenMode`. | Only the documented chrome changes; content usable. |

### Global menu (`crates/shell/src/menu.rs`)

| State | Preparation | Check |
|---|---|---|
| closed / open | Dismiss / activate the titlebar menu button. | Absent without a reserved overlay region / anchored at the trigger inside the viewport. |
| disabled item | Open without project/editor context. | Row visibly disabled; cannot execute. |
| submenu | Open a branch (View, Connections). | Anchors to parent, flips/clamps, preserves owner labels/actions. |
| keyboard navigation | Navigate with the keymap, no pointer. | Highlight, submenu traversal, activation, Escape deterministic. |

### Workspace (`crates/workspace/src/workspace.rs`)

| State | Preparation | Check |
|---|---|---|
| home | No project, normal home route. | Intentional, unclipped, documented entry actions. |
| project workspace | Open a disposable local repo via `OpenProject`. | Project identity, tab, docks, statusbar agree on the root. |
| standalone | `ReturnToStandalone` or a standalone terminal/tool tab. | No stale project identity or project-only control. |
| empty pane | Close the active leaf/tab, keep the workspace. | Workspace-rendered empty state with only valid recovery actions. |
| loading / error | Real async restore with a local fixture / an invalid or inaccessible fixture. | Truthful progress; actionable error or notification, no raw path or secret. |
| split panes | `SplitRight`, `SplitDown`. | Handles, focus, resize, close preserve the layout tree. |
| shared Space join | Two native instances on one disposable profile; in the second open the Spaces picker and join the first's Space. | Participant count and explicit Join action; both windows identify the same Space. |

### Docks and statusbar (`crates/workspace/src/dock.rs`, `status_bar.rs`, `status_items.rs`)

| State | Preparation | Check |
|---|---|---|
| left / right / bottom dock | Open a registered panel for that edge. | Dock owns placement; kit controls; bounds, focus, close inside workspace; bottom resize never covers the statusbar. |
| empty dock | Close all panels in a dock. | Chrome absent or intentional. |
| focused panel | Focus a panel, then the adjacent pane. | Focus ring and keyboard routing follow the active owner. |
| narrow + dock | Narrow viewport with a dock open. | Deterministic collapse; no duplicated panel. |
| statusbar normal / hidden item | Standard items / hide one via its owner path. | Left controls and right info zones; only that item disappears. |
| item context menu | Right-click a status item. | Anchored; only owner-registered actions. |
| narrow / unread badge | Resize; publish a safe synthetic notification. | Items collapse without clipping; badge count and read state owned by the registry. |

### Command palette (`crates/command-palette/src/palette.rs`) and dialogs (`crates/workspace/src/modal_layer.rs`, `crates/shell/src/modals.rs`)

| State | Preparation | Check |
|---|---|---|
| palette focused / empty / filtered | `OpenCommandPalette`; a non-matching query; a query matching an owner command. | Input focus and modal anchoring; explicit empty state with no invented action; ranking, labels, icons, keybinding hints match the registry. |
| palette submenu / keyboard / dismissed | Open an owner-contributed submenu; navigate with the keymap; Escape. | Typed palette contract, within viewport; no pointer-only behavior; overlay and focus trap removed without stale dimming. |
| dialog open / focused input | Trigger a documented decision dialog; focus its input. | Scrim, bounds, title, actions, visible focus; typing does not leak to the workspace. |
| validation error | Submit an invalid value in a fixture. | Field-level correction path; no duplicate passive error. |
| confirm / cancel | Inspect both without changing data. | Distinct, safe behavior. |
| loading / failure | Real async dialog-owned operation; deterministic failing fixture. | In-flight state blocks duplicate submits; failure redacted, actionable, routed via notification. |

### Hosts, notifications, settings, terminal

Hosts is a full-page Settings surface ([ADR 0007](adr/0007-hosts-settings-page.md)), source `crates/hosts-ui/src/hosts.rs`; notifications `crates/notifications/src/status_item.rs`; settings `crates/settings-ui/src/window.rs`; terminal `crates/workspace/src/views/terminal.rs`.

| Surface | States and preparation | Check |
|---|---|---|
| Hosts (Settings > Hosts) | list (synthetic non-secret fixture), empty (fresh data dir), new-host form (no real credentials), validation error (invalid host/port), Port forwards tab (synthetic loopback fixture), loading (keep pending if no observable phase) | Hosts owner renders list, selection, actions; no other profile's data; validation names the field and writes nothing partial or secret; forward actions expose no credentials. |
| Forwards popover (statusbar `forwards`) | empty (item hidden), active, reconnecting, failed, awaiting input (synthetic loopback or in-process fixture, no real credentials) | Item appears only while forwards exist; rows grouped by host with state dot, address, connection count; copy/open/restart/close act on that row; failure reason readable and redacted. |
| Forward editor | local, remote, dynamic kinds; non-loopback listen address (public-bind warning); duplicate listen port (conflict hint) | Fields follow the kind; warning and confirmation appear only for public binds; conflict hint names the clash; nothing saved on cancel. |
| Hosts > Port forwards tab | saved forwards list, empty state, running forward state dot and bound port | Rows use the shared draft model; start/stop emit the owner action; no duplicate "active tunnels" group. |
| Route display | host with a 2+ hop synthetic chain: tab title, tab hover (tab with at least two panes), loading screen hop stages (`Hop 2/3`), hosts-list `via` chip | Title collapses to a hop count from 3 hops; hover shows the full route; stages name the hop; chip matches the chain. |
| Hop prompt | untrusted host key or password on an intermediate hop (synthetic fixture) | Prompt shows on the tab's loading screen labelled with the hop; answering continues the connection; cancel fails with the hop named. |
| Forward prompt dialog | forward session awaiting input (synthetic fixture) | Dialog names host and hop, focuses its input, answer resumes the forward; no secret echoed or logged. |
| Notification dropdown | closed, unread badge, open list, empty, expanded details, clear all (all with safe fixtures) | Badge only, no toast; shared kit rows, ordering and deduplication; bounded readable details; clear-all updates badge and read state. |
| Settings window | normal, scope navigation (User/Project), search/filter (match and no-match), validation error, save failure (read-only fixture) | No capability-owned categories; effective value source clear; bounded results, no invented field; last valid value kept with a corrective action; write failure visible, redacted, never claims persistence. |
| Terminal | ready, loading (only if startup visibly shows it), empty, process exit (`exit` in a fixture shell), connection failure (invalid synthetic SSH, no credentials), narrow window | Input/output/focus/status ready; exit state, restart/close, retained output truthful; failure redacted; grid, prompt, statusbar, tabs resize without clipping or input loss. |
| Shared terminal (two instances joined to one Space, one Detached terminal tab) | controller view, non-controller view | Both show the same tab and output; the controller shows its resize control; the other identifies the controller and stays usable without resizing the PTY. |
