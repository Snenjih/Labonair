# Performance

Related: [`architecture.md`](architecture.md), [`testing.md`](testing.md), [`design-system.md`](design-system.md) (Motion).

Performance is the motive of the port. The GPUI-native architecture (no WebView, no IPC, no JSON round-trips) removes the structural overhead of the old Tauri/React build; this document holds the target budgets, the measurement method, the regression checklist, and the hot-path guards already in the code. It is measure-first and adds no speculative micro-optimization. There are no benchmark targets in the workspace; every budget below is a **target, not a measured claim**. Nothing is `Verified` until a reproducible release-build measurement is recorded on a macOS host with a graphical session (CI/container runners have no display server and cannot produce credible GPU, frame-time, or RSS numbers).

## 1. Budgets (macOS Apple Silicon, release build)

| Area | Target | State |
|---|---|---|
| Cold start to visible window | < 400 ms | Pending native measurement. |
| Start to interactive (first terminal keypress echoes) | < 700 ms | Pending native measurement. |
| Idle RSS, one tab | < 150 MB | Pending native measurement. |
| Terminal output | No visible hitch at 200k lines; bounded scrollback (`terminal.terminalScrollback`, default 5000) | Source guards exist; native workload pending. |
| Large lists | 5k-row Explorer/SFTP/Git-Graph stay virtualized and scroll at display refresh rate; directory enumeration is measured separately | Source guards exist; native workload pending. |
| Git status | One poll per interval (2 s local, scaled for remote), no overlap, skipped without a repo root | Source guard and tests; runtime measurement pending. |
| UI foreground | No filesystem, process, network, or database blocking on the GPUI thread | Code review and async boundaries; profiler evidence pending. |

For comparison, the old Tauri/React build had a multi-hundred-ms cold start and roughly 250-400 MB idle with one webview; any Rust number materially worse is a regression to investigate.

## 2. Measurement method

Use `cargo run --release -p labonair` (or the packaged bundle, see [`testing.md`](testing.md)); record results with the machine and date in the task handoff.

| Metric | How to measure |
|---|---|
| Cold start to window visible | Wall-clock from process spawn to first paint (`tracing` `info!("Labonair-rust starting")` to first `render`). |
| Start to interactive | First keypress in a terminal echoes. |
| Idle RSS | `footprint` / Activity Monitor, one Home tab, 30 s after launch. |
| RSS, 6 terminals + 3 editors | Same, after opening them and running `yes \| head -c 5M` in one. |
| Terminal throughput | `time seq 1 200000` in a pane; watch for frame hitching. |
| Large-list scroll | Explorer on a 5k-entry dir, SFTP on a large remote dir, Git-Graph on a 5k-commit repo; flick-scroll and look for dropped frames. |
| Git status cadence | `RUST_LOG=labonair=debug`; one `git_get_workspace_state` per interval, no pile-up. |

### Render-path tracing

The render path is instrumented through the opt-in `labonair::perf` tracing target (normal runs do not enable it):

```sh
RUST_LOG=labonair::perf=trace cargo run -p labonair
```

Every core render emits a `render` span with a `view` field (`shell`, `titlebar`, `workspace`, `status_bar`, and the Explorer, SCM, Git Graph, Snippets panels); the target also covers `settings_recompute`, `active_theme_recompute`, and `live_snapshot_recompute`. Duplicate live snapshots are explicit no-ops (`identical_snapshot_is_an_idle_noop` in `crates/workspace/src/live_bridge.rs`), and empty deferred Workspace queues are skipped without `mem::take`.

| Scenario | Expected renders | Capture |
|---|---|---|
| Idle, unfocused terminal, 10 s (cursor blink and PTY output off) | 0 spans after initial paint | trace on a graphical macOS host |
| Tab switch | Workspace and the changed tab child; shell at most two paints | trace spans + `heaptrack`/`dhat` |
| Terminal typing | Terminal child only; no shell render | trace spans + allocation sample |
| Panel toggle / split resize | Workspace and the affected dock/panel only | trace spans |
| Settings field change | Settings recompute, active theme only if relevant, affected view | trace spans |

The graphical sign-off is still open: run the above on macOS, attach traces plus `dhat`/`heaptrack` samples for tab switch and terminal typing, and replace the pending states with measured milliseconds and peak bytes.

## 3. Manual regression checklist (before a release)

- [ ] Cold start feels instant (side by side with the reference if possible); typing into a fresh local terminal right after launch has no lag.
- [ ] `seq 1 200000` in a pane: scrollback stays smooth and memory settles afterwards (buffer bounded).
- [ ] Open/close about 20 terminal and editor tabs in a loop: RSS returns close to baseline (sessions and pane maps dropped on close).
- [ ] Explorer and SFTP on a large directory: smooth scroll, bounded virtualized rows; directory-load latency recorded separately from first paint.
- [ ] Git-Graph on a large repo: only visible rows render. Git panel: with `RUST_LOG=labonair=debug` the poll cadence holds and leaving a repo stops useful work.
- [ ] Retina: 1x and 2x displays render crisp text; drag a window between displays with different scale factors.
- [ ] `appearance.animations = reduced` / `off` collapse animations per [`design-system.md`](design-system.md) Motion.

## 4. Hot-path guards (in the code)

- **Terminal PTY.** A bounded queue with coalesced wakeups; views wait for session notifications instead of a 16 ms poll (`crates/terminal-core/src/session.rs`, `crates/workspace/src/views/terminal.rs`). PTY bytes are copied into the agent broadcast tap only while an agent subscriber exists.
- **Terminal search.** Marks its scrollback index dirty while output arrives; navigation refreshes at once, render-time highlight refresh is rate limited (`crates/terminal-core/src/engine.rs`).
- **Settings.** UI writes commit in memory and are serialized on one FIFO writer thread, flushed at shutdown; watcher reads, JSONC parsing, schema validation, and project filtering run on the background executor, and only applying the parsed layer reaches the GPUI thread (`crates/settings/src/store.rs`, `watch.rs`; `crates/settings-ui/src/view.rs`).
- **Editor.** Display layouts and document symbols are cached by buffer revision and relevant configuration; repaints copy only visible rows (`crates/editor/src/display.rs`, `crates/workspace/src/views/editor.rs`).
- **Session.** Periodic session snapshot JSON, scrollback compression/file I/O, and window-geometry writes run off the GPUI thread; the explicit quit path stays synchronous so final state is not lost. Workspace metadata is batched and shell metadata read under one session access (`crates/workspace/src/workspace.rs`).
- **Lists.** Explorer: a `generation` counter discards stale async directory reads, with lazy expansion (`crates/panel-explorer/src/panel_explorer.rs`). SFTP and Git-Graph rows use `uniform_list` so only visible rows build elements; remote listing is async (`crates/workspace/src/views/sftp.rs`, `crates/panel-git-graph/src/panel_git_graph.rs`).
- **Git status poll.** Refresh guards prevent overlap and stale results (`crates/panel-scm/src/panel_scm.rs`, 2 s local interval).
- **Startup.** Backend workers start through `Composition::spawn_workers` and the window opens without waiting on them; fonts are registered once (`crates/theme/src/fonts.rs`).
- **AI.** The frontend is parked; backend streaming remains for the rebuild.

Deferred (need a profiler and a real workload): streaming/paging directory enumeration for Explorer/SFTP (loaders still collect and sort a directory before the first list shows); pausing the Git poll while the panel is off-screen or the window unfocused (needs a visibility signal from `AppShell`; low payoff at 2 s); glyph-run caching in the terminal renderer beyond GPUI's own.

## 5. Platform notes

- **macOS (primary).** The main window uses a transparent native titlebar (`appears_transparent: true`) with the shell's own titlebar; menus are native (`cx.set_menus` via `labonair_shell::init_menus`, `crates/shell/src/menu.rs`); GPUI's Metal renderer is scale-factor aware and all sizes are logical `px(..)`, so Retina is automatic (terminal cell metrics derive from `text_system().ch_advance`); window bounds persist (`crates/shell/src/window_state.rs`).
- **Linux (later).** Keep it buildable (CI has an informational Linux check job): no macOS-only APIs outside window setup and `menu.rs`; the view layer is renderer-agnostic (logical `px`, theme tokens, no platform branches in feature views). Open items: file-dialog/open-in-browser shims, keychain backend (`keyring` already abstracts it), font fallback list.

## 6. Build-time reference (historical)

Captured 2026-09-03 right after the `ui` monolith was dissolved, when the workspace had 20 crates (it now has 64, so absolute numbers no longer apply; use the scenarios as a method): Intel Xeon 4 vCPU Linux VPS, `CARGO_INCREMENTAL=0`, `-j4`. Cold `cargo check --workspace --all-targets` 327 s; warm (no edits) 2 s; clippy after the cold check 23 s; a one-line edit in `crates/shell/src/app_shell.rs` re-checks only `labonair-shell` in about 3 s (the first `-p` run after an `--all-targets` plus clippy pass was a 158 s fingerprint-rebuild outlier). Cold/warm `cargo build --release` and `cargo test --workspace` were never recorded. Open follow-up: `labonair-terminal` still depends on `gpui` and `labonair-theme`; an engine/renderer split would let a pure-engine check skip GPUI (the GPUI-free engine now lives in `labonair-terminal-core`).
