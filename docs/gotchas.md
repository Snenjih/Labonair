# Gotchas

Non-obvious API behavior, constraints and fixed-bug lessons, grouped by topic. Entries describe the code as of when they were recorded; where a symbol was later renamed, the current code and normative docs win. Add new findings under the matching heading.

Index: [GPUI](#gpui) · [Terminal and alacritty_terminal](#terminal-and-alacritty_terminal) · [Editor and Vim](#editor-and-vim) · [Settings and keymap](#settings-and-keymap) · [Composition and ownership](#composition-and-ownership) · [SSH, SFTP, transfers, MCP](#ssh-sftp-transfers-mcp) · [Build, test, toolchain](#build-test-toolchain) · [Native launch and screenshots](#native-launch-and-screenshots)

## GPUI

- **`.hover(..)` may be applied once per `Stateful` element.** A second call panics at render time with "hover style already set". `ui-kit::ListItem` therefore offers `hover_style(f)` (replaces the default hover fill, applied before `.extra(..)`); call sites such as hosts rows and SCM file rows use it instead of calling `.hover` inside `.extra`.
- **`uniform_list` needs a uniform item height.** It measures item 0 and reuses it. Mixed section-header and command rows (keymap tab) must share one fixed height (`ROW_HEIGHT == SECTION_HEIGHT`, wrapped in `div().h(..).w_full()`), otherwise a shorter header overlaps the rows below.
- **`flex_1()` only works inside a flex container.** The editor text area collapsed to 0 px height (all rows clipped by `overflow_hidden`) because `editor_surface_container` was a default block element and the text area's children are absolutely positioned. Fix: make the container a flex row so the viewport fills the height beside the minimap.
- **`Window::with_element_offset` is prepaint-only.** A custom flyout must apply the offset while prepainting its child and call the child's `paint` directly; applying it again in `paint` trips a debug assertion when the dropdown opens (`ui-kit` `context_menu.rs`, `popover.rs`).
- **Context-menu submenus must place from measured bounds.** `labonair-ui-kit::context_menu` measures the real panel in prepaint and places it against the viewport on both axes: prefers right/below, flips independently to left/above, clamps only when larger than the viewport. The absolute host uses `FlexStart` so cross-axis layout does not inflate measured bounds. Fixed-size assumptions plus `snap_to_window` clamping put flyouts on top of their parent.
- **`MouseDownEvent::position` is window-space.** A global menu anchor must keep the event's x and use the titlebar bottom as y; a local element coordinate reproduces the top-left/opposite-side placement bug.
- **Capture a keystroke without running its action with `App::intercept_keystrokes`, not `on_key_down`.** Interceptors run before action matching and `cx.stop_propagation()` there suppresses dispatch; `on_key_down` runs after global bindings. `gpui-component`'s `Input` also binds `up`/`down`/`enter` as actions in its own `"Input"` context, so an ancestor `on_key_down` never sees them while an `Input` is focused (hand-roll one view-level key router, as `HostManagerView::on_search_key` and the keymap tab do).
- **`gpui::Keystroke`'s `Display` prints platform glyphs (`⌘⇧P`), not a parseable string.** `labonair-keymap-ui::keystroke::to_binding_string` emits `cmd-shift-p` in `normalize_single_keystroke` order (ctrl, alt, shift, cmd) so it round-trips through `Keystroke::parse`. Lone-modifier taps (`key == "cmd"`) are dropped during capture.
- **`App::prompt_for_paths(PathPromptOptions)`** (GPUI 0.2.2) returns a oneshot receiver; call it through `Context`'s `Deref` to `App` (resolving through `BorrowMut<App>` is ambiguous).
- **`bool::then_some` evaluates eagerly.** `(contexts.len() == 1).then_some(contexts[0])` panicked for an empty slice; use `first().copied().filter(..)`.
- **`Option<&Path>::cloned()` does not exist** for `WorkspaceIdentity::project_root()`; use `map(Path::to_path_buf)`.
- Stand-alone element views reaching into another view: a chrome view may call `workspace.update(cx, |w, cx| w.render_*(cx))` inside its own `render` (titlebar does this for `render_tab_bar`).
- Preview layers (app theme, icon theme) are separate transient, non-persisted slots in `ThemeStore`; `icon_theme()` resolves the preview before the active id, so already-rendering consumers (Explorer) pick it up with no second registry. Each palette page emits its own preview events so switching pages cannot leave another preview active.
- Background image layers must be added before their content in the element tree; appended after it they composite over the UI on every redraw.
- `Window` render must not load or sync state (see Settings). Do not write project-settings I/O or notifications from `Workspace::render`.

## Terminal and alacritty_terminal

- **Do not size a new PTY from `window.viewport_size()`.** That includes shell chrome. Keep the initial 80x24 grid until the terminal canvas reports its own bounds, then resize once; otherwise there is a startup redraw burst and flicker.
- **First terminal in an empty Space inherited the previous Space's cwd.** `Workspace::apply_space_switch` now clears the active tab when the destination has none, and cwd/render lookups ignore tabs outside the active Space.
- **Project identity is never derived from a terminal cwd.** `WorkspaceContext` starts standalone and becomes `Project` only via an explicit `WorkspaceTransition::OpenProject { root }`. Explorer and Git roots resolve the explicit project root first; only standalone workspaces fall back to the active terminal cwd (Explorer may then use home; Git stays unscoped).
- **alacritty_terminal 0.24.2 APIs used:** `Term::set_options(Config)` re-tunes scrollback (`grid.update_history`), `default_cursor_style` and `semantic_escape_chars` live; `Term::cursor_style() -> CursorStyle { shape, blinking }` (respects DECSCUSR); `Selection::new(SelectionType::Semantic, point, side)` + `include_all()` for word selection (double-click, `click_count >= 2`).
- **Blinking cursor is renderer-side.** `RenderableCursor` carries `blinking`; `AlacEvent::CursorBlinkingChange` is ignored. `TerminalView` drives a blink task from `terminalCursorBlinkInterval`, holds the cursor solid for one interval after key/output, blinks only while focused.
- **Settings must be pushed into live panes.** `Workspace::reapply_terminal_settings` (from the shell's `SettingsStore` observer) calls `apply_runtime_config` on each pane and `cx.notify()`s each `TerminalView`; without an observer, `terminalOpacity` only changed on the next unrelated repaint, and scrollback/cursor style were spawn-only. Wheel step is multiplied by `terminalScrollSensitivity` (x5 with `terminalFastScrollModifier`); `confirmCloseTerminalTab` gates closing a running local pane; line height/weight flow through `theme_ui::font_overrides_from_settings`.
- **A tab bar in the sidebar** (`tabsLocation == "sidebar"`) is `TabsPanel` (`crates/workspace/src/tabs_panel.rs`) owned by Workspace: `init_docks` auto-places it only if a persisted layout had it; otherwise `Workspace::sync_tabs_in_sidebar` decides, called after `init_docks` and from a `SettingsStore` observer in `shell/src/bootstrap.rs` (no-op when dock membership already matches). Tab rows use `render_tab(sidebar: true)` with click-anchored menus.

## Editor and Vim

- **Editor `Revision` values restart per loaded buffer.** `(path, revision)` is not a global identity; a reload or different buffer can repeat a revision number. Re-apply each typed buffer-change event or add an independent buffer generation to any dedupe key.
- **Normalize the font stack:** pass only the first family to GPUI while keeping the theme fallback chain (a separate rendering issue from the zero-height bug).
- **Moved legacy wire types keep legacy defaults.** The historical `EditorPrefs` (now private to the v1->v2 settings migrator) must keep `hlsearch`/`incsearch`/`smartcase` defaulting to `true`; derived defaults turned them into false overrides.
- **Editor depends on `labonair-interaction-contracts`** for `SearchFocus` identity; the edge is registered in the crate graph (`scripts/crate-graph.toml`).
- **Local Vim gaps (as of 2026-10-11, `crates/editor/src/vim.rs`):** `toggle_case` keeps only the first char of an expanding case mapping (`ß` becomes `S`, not `SS`) and works from cursor/count only, not on a Visual selection; uppercase `C` deletes to line end without applying a pending count and without copying to the register.

## Settings and keymap

- **Keep project-settings loading out of render.** Project settings sync from the typed `WorkspaceTransition` boundary and the explicit refresh command only; rejected-key notifications are deduplicated there. The pure `WorkspaceContext` owns transition mutation and no-op detection.
- **Project settings are keyed by explicit workspace identity**, never by the active terminal cwd.
- **Migrations must not recreate removed areas.** The v1->v2 migrator writes only the seven canonical `SettingsContent` areas, leaves standalone MCP preferences alone, skips host-manager preferences, and runs host-store migration as a separate step. `settings-content/src/{hosts,mcp,personalization}.rs` stay public only as migration wire types; never add them back to `SettingsContent` or the schema. Removed fields (e.g. `notify_on_errors`) stay forward-compatible because unknown fields are ignored on deserialize.
- **Settings navigation:** removing `Themes`/`Hosts`/`Shortcuts` from `AREAS` also required removing their panes, registry rows and deep-link tests; later the Hosts page returned as a full Settings page (see ADR 0007).
- **`OpenShortcuts` compatibility:** the old `settings::OpenShortcuts` action name maps to `OpenKeymapJson`, preserving old user keymap files without a second Shortcuts surface.
- **Command metadata is shared.** `labonair-command-palette-core` holds the UI-free descriptor/provider registry; palette, keymap discovery and dispatch all resolve through it, and `action_name()` is on `CommandId`, not `CommandDescriptor`. Keymap core must not depend on GPUI (`KeybindDisplay` stays in the palette crate). The workspace empty-state hint shows the registered Command Palette binding (`⌘P`).
- **`jsonc-parser` 0.23** exposes byte ranges on AST nodes (`parse_to_ast`, `Ranged`); `keymap::file::remove_user_binding_override` uses them for a surgical text edit (delete property span plus one adjacent comma, drop an emptied block), re-parses, and returns `Err` rather than write a file that would not round-trip. `parse_to_serde_value` has no ranges (validation only).

## Composition and ownership

- **Typed events instead of shell hooks.** `Workspace` emits `WorkspaceEvent::OpenHosts`; the shell subscribes and opens the Hosts palette page. Re-export new event types from `crates/shell/src/shell.rs` (the composition-root boundary) or the shell cannot name them. Titlebar likewise emits `TitlebarEvent`; bootstrap delegates.
- **Host contracts instead of owner entities.** `Workspace`/`TerminalView` hold `labonair_background_host::BackgroundHost` (layer callback plus a `BackgroundPulse` entity the background crate bumps on store notify), `LayerScope` lives in that leaf crate and is re-exported by `labonair-background`. Explorer holds `labonair_explorer_host::ExplorerHost` (four callbacks) and the composition root calls `ExplorerView::notify_active_file_changed` from the workspace observer; `DraggedPaths`, `quote_paths`, `shell_quote`, `is_previewable`, `PREVIEW_EXTENSIONS` live in `labonair-explorer-host`.
- **Persistence moves with UI state.** A capability's store and its persistence module belong to the same crate (`labonair-background` took both); a split owner keeps the old facade alive.
- **Share one `SecretsState`.** `AppInner.secrets` is an `Arc<SecretsState>`; a second instance would split the cache and give inconsistent reads. `labonair-persistence::Database` is `Clone` (`Arc<Mutex<_>>`), so panels receive the handle directly. `AppInner::snippet_run` is an `Arc` so the injected SSH executor and cancellation share one run registry.
- **Hosts UI callback.** `HostEventHandler` preserves MCP grant revocation when a host is blocked (`HostEvent::AgentAccessBlocked`) without giving the UI crate the broad application facade.
- **One error surface.** Operation errors go to the statusbar notification registry (structured, neutral summary, expandable details, source id, dedupe key). Immediate-decision prompts stay modal: transfer conflict/file error, SFTP permission validation, editor external-change. `labonair-notifications-core` is UI-free; `labonair-notifications` adapts it and owns the statusbar item and dropdown (history, read state, unread badge from read state). Status items consumed by `labonair-panel` must not depend back on notifications; constructing entities in that module needs `gpui::AppContext` imported.
- **Registration signatures.** Metadata-only command composition passes `None` for owner entities that only exist at runtime (Hosts, Workspace); name complex callback types (`OpenRawHandler`) to satisfy `clippy::type_complexity`; workspace-owned commands have no shell fallback in the metadata-only registry.
- **Transfers wire format.** `TransferStatus` needs `#[serde(rename_all = "snake_case")]`; without it worker payloads like `"status": "running"` fail to decode even though everything compiles (regression test in `labonair-transfers`).

## SSH, SFTP, transfers, MCP

- **SFTP must not own host authentication.** SSH creates the authenticated session; SFTP receives an opaque `SftpSessionHandle` (`SftpSessionService` / `SftpBrowserService`) and exposes only remote filesystem operations.
- **Close SSH when SFTP subsystem setup fails**, otherwise the authenticated session stays registered; normal tab retirement closes the SFTP handle before disconnecting SSH.
- **Snippet execution:** local runs use `labonair_snippets::exec` (`LocalRunRegistry`, `LocalRunEvent`); SSH runs go through the `SshCommandExecutor` trait with `SnippetRunEvent`s.
- **MCP command capture:** SSH broadcast receivers and the injected local terminal receiver have different error types; convert to `String` at the MCP adapter boundary so both flow as `Result<Vec<u8>, String>`.
- **`labonair-mcp-server` declares `rusqlite` directly** (it uses `rusqlite::params!`).
- **Host and credential secrets** are resolved at the composition boundary; the `Host` read model must never contain secrets (store test enforces it).

## Build, test, toolchain

- **Metal Toolchain is required** to compile GPUI on macOS: `xcodebuild -downloadComponent MetalToolchain`. In a sandbox, GPUI checks also fail when Metal compilation cannot write Clang modules to the user's compiler cache; allow that cache path. Pure SSH/SFTP/logic crates compile without it.
- **Sandboxed `cargo test --workspace` failures that are environmental:** the AI end-to-end SSE test (`client::tests::end_to_end_streams_openai_sse_over_http`) binds a local listener (`Operation not permitted`), and the Settings file-watcher test needs file-event permission. Rerun them with loopback/FS permission.
- **Interrupted parallel rebuild after `cargo clean`** (e.g. during low disk) can leave stale artifact locks and misleading `syn`/`serde` errors. Stop only the stale build PIDs, recreate the regenerable target dir, rerun serially.
- **Clippy runs with `-D warnings`** (`scripts/check.sh`); the crate graph is enforced by `scripts/check-crate-deps.sh` against `scripts/crate-graph.toml` and must stay acyclic.
- **Cross-crate API breaks compile fine but fail at runtime** when wire attributes are lost (see transfers); add decoder regression tests for serialized contracts.

## Native launch and screenshots

- **Never launch through the shared display name.** `open -a Labonair` can resolve the legacy Tauri app. Use `cargo run -p labonair` or the absolute path of the Rust bundle/binary; the native bundle id is `com.labonair.rust` (the legacy app used `com.labonair.app`).
- **Reliable macOS launch form is `open -n -W <absolute .app path>`** (LaunchServices). Launching the executable directly can initialize AppKit without a visible window, and in one runner aborted after about a second with `Abort trap: 6` in `NSApplication`/`_RegisterApplication`. The smoke launch check therefore resolves the exact new Rust PID, checks it with `ps` at both ends of the 5 s interval, and kills that PID. From a restricted shell `open` can fail with LaunchServices error `-10827`; do not accept such runs as GUI evidence.
- **`scripts/screenshot.sh` fails closed:** it resolves the exact Rust process PID, filters CoreGraphics windows by it (layer 0), and refuses when none matches; there is no full-display fallback, and the legacy app is never a valid target. It also validates the command path of the PID against `target/debug/labonair` and the packaged bundle executable, so launch by absolute path or `cargo run -p labonair`; a `./target/debug/labonair` launch produces `$REPO_ROOT/./target/...` and was rejected before normalization was added.
- **`screencapture` needs Screen Recording permission** for the runner; without it the window is found but capture is denied. Keep the visual matrix `Pending` and never substitute a screenshot of another application.
- In some sessions the desktop exposes no Labonair window at all (only another app's); a visual check cannot be marked passed from such an environment.
