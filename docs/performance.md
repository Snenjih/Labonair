# Performance & Cross-Platform Baseline

**Status:** Supporting baseline; historical measurements may reference the pre-reset implementation.

Reference doc for the "performance is the motive of the port" goal. Captures
the measurement method, the target envelope, the manual regression checklist,
and an inventory of the hot-path guards already in the code so a future change
can be judged against them.

> Scope note: per the task warning, this pass is **measure-first**. The
> GPUI-native architecture (no WebView, no IPC, no JSON round-trips) already
> removes the structural overhead that made the Tauri/React build slow. This
> document is the yard-stick for the native hot-path guards and does not add
> speculative micro-optimisation.

## 0. Hot-path changes implemented in the native port

The current performance pass removed the following foreground work without
changing the user-facing contracts:

- Terminal PTY notifications use a bounded queue and coalesced wakeups. Views
  wait for session notifications instead of polling every 16 ms. Agent output
  is copied only while an agent subscriber is present.
- Terminal search marks its scrollback index dirty while output is arriving.
  Navigation refreshes immediately; render-time highlight refresh is rate
  limited to avoid rescanning scrollback for every PTY chunk.
- Settings UI writes are committed to the in-memory store immediately and
  serialized by one FIFO writer thread. User/project watcher parsing also runs
  on the background executor; only applying an already-parsed result reaches
  the GPUI thread.
- Editor display layouts and document symbols are cached by buffer revision and
  relevant configuration. Repaints copy only the visible display rows.
- Periodic session snapshot JSON, scrollback compression, scrollback file I/O,
  and window-geometry writes run off the GPUI thread. The explicit quit path
  remains synchronous so the final session state is not lost during shutdown.

These are source-level guarantees. No release-build frame-time or RSS number
is claimed until the graphical measurement procedure below has been run on a
macOS host.

## 1. Baseline measurement method

All numbers are taken on the primary target (macOS, Apple Silicon, release
build: `cargo run --release -p labonair`). Record them in the table when a machine is
available; the method is what matters for regression comparison.

| Metric | How to measure |
|---|---|
| Cold start → window visible | wall-clock from process spawn to first paint (`tracing` `info!("Labonair-rust starting")` → first `render`). |
| Start → interactive | first keypress in a terminal echoes. |
| Idle RSS | `footprint` / Activity Monitor, 1 Home tab, 30 s after launch. |
| RSS, 6 terminals + 3 editor tabs | same, after opening that many tabs and running `yes | head -c 5M` in one. |
| Terminal throughput | `time seq 1 200000` inside a pane; watch for frame hitching. |
| Large-list scroll | Explorer on a 5k-entry dir, SFTP on a large remote dir, Git-Graph on a 5k-commit repo — flick-scroll, look for dropped frames. |
| Git status cadence | `tracing` at `debug` — confirm one `git_get_workspace_state` per `POLL_INTERVAL`, not a pile-up. |
| AI streaming | stream a long completion, confirm incremental token append (no full re-layout per chunk). |

### Recorded runs

| Date | Machine | Cold start | Interactive | Idle RSS | Heavy RSS | Notes |
|---|---|---|---|---|---|---|
| _pending_ | | | | | | fill on a release build |

The Tauri/React reference for comparison: WebView process + renderer + Node
sidecar, multi-hundred-ms cold start, ~250–400 MB idle with one webview.
Any Rust number materially worse than that is a regression to investigate.

## 2. Target envelope (macOS, release)

- Cold start to visible: **< 400 ms**, to interactive: **< 700 ms**.
- Idle RSS with one tab: **< 150 MB**.
- Terminal output: no visible hitching at 200k lines; scrollback capped
  (`terminalScrollback` pref, default from `preferences.rs`).
- 5k-row lists scroll at display refresh rate (Git-Graph and SFTP row rendering
  are `uniform_list` virtualised; directory enumeration remains a separate
  measurement).
- Git status: exactly one poll per interval (2 s local, ×N remote), skipped
  when there is no repo root.

## 3. Manual regression checklist (run before a release)

- [ ] Cold start feels instant vs. the reference (side-by-side if possible).
- [ ] Type into a fresh local terminal immediately after launch — no lag.
- [ ] `seq 1 200000` in a pane — scrollback stays smooth, memory settles
      after it finishes (buffer is bounded, not retained unbounded).
- [ ] Open/close ~20 terminal + editor tabs in a loop — RSS returns close to
      baseline (sessions/among `panes` map are dropped on close).
- [ ] Explorer + SFTP on a large directory — scroll is smooth and the
      virtualised row surface stays bounded; record directory-load latency
      separately from first paint.
- [ ] Git-Graph on a large repo — only visible rows render; scroll is smooth.
- [ ] Git panel: with `RUST_LOG=labonair=debug`, confirm the poll cadence and
      that switching away from a repo stops useful work.
- [ ] AI chat: stream a long answer — text appends incrementally, the window
      stays responsive.
- [ ] Retina: 1× and 2× displays both render crisp text; drag the window
      between displays with different scale factors.
- [ ] `prefers-reduced-motion` / the "Reduce motion" setting collapses the
      tab-entrance animation (mirrors the reference `0.01ms` clamp).

## 4. Hot-path guard inventory (already in the code)

| Path | Guard | Location |
|---|---|---|
| Startup | backend workers `spawn_workers()` + event logger are `tokio::spawn`; the window opens without waiting on them. SQLite open is the only sync step and is cheap. | `crates/app/src/main.rs` |
| TreeSitter grammars | behind the `build-grammars` feature / loaded lazily, not at boot. | editor crate |
| Fonts | bundled assets registered once at `init_fonts`. | `crates/theme/src/fonts.rs` |
| Terminal render | The PTY reader feeds the emulator on its own thread; the event-driven view drains bounded/coalesced notifications and repaints on output instead of maintaining a 16 ms timer. | `crates/workspace/src/views/terminal.rs`, `crates/terminal/` |
| Terminal agent tap | PTY bytes are copied into the broadcast tap only when at least one agent subscriber exists. | `crates/terminal/src/session.rs` |
| Terminal search | Full scrollback matching is deferred and render-time refresh is rate limited while output is active. | `crates/terminal/src/engine.rs` |
| Settings persistence | Typed changes update memory immediately; surgical JSONC writes are serialized on one writer thread and flushed at shutdown. | `crates/settings/src/store.rs`, `crates/settings-ui/src/view.rs` |
| Settings watchers | File reads, JSONC parsing, schema validation, and project filtering run in the background; GPUI only applies the loaded layer. | `crates/settings/src/watch.rs`, `crates/settings/src/store.rs` |
| Editor layout/symbols | Display coordinates and document symbols are reused for a buffer revision; paints clone only visible rows. | `crates/editor/src/display.rs`, `crates/workspace/src/views/editor.rs` |
| Explorer | `generation` counter discards stale async dir reads; bounded rendering and lazy expansion. | `crates/panel-explorer/src/panel_explorer.rs` |
| SFTP list | `uniform_list` bounds row-element construction and remote listing is asynchronous; directory enumeration itself is measured separately. | `crates/workspace/src/views/sftp.rs` |
| Git-Graph | row list virtualised with `uniform_list` — only visible commit rows build elements. | `crates/panel-git-graph/src/panel_git_graph.rs` |
| Git status poll | refresh guards prevent overlap and stale results. | `crates/panel-scm/src/panel_scm.rs` |
| Session sync | Workspace metadata is batched and shell metadata is read under one session access; periodic session serialization and scrollback persistence are background work. | `crates/workspace/src/workspace.rs` |
| AI streaming | frontend AI is currently paused; backend streaming remains available for the future rebuild. | `crates/ai/` |

### Follow-ups deliberately deferred (need a profiler + a real workload)

- Streaming/paging directory enumeration for Explorer/SFTP. The SFTP row
  surface is virtualized, but the current local and remote loaders still
  collect and sort a directory before the first list is shown.
- Pausing the Git status poll while the panel is off-screen or the window is
  unfocused — would need a visibility signal from `AppShell`. Low payoff at a
  2 s interval with the existing guards; revisit if profiling shows it.
- Glyph-run caching in the terminal renderer beyond what GPUI's text system
  already caches.

## 5. Cross-platform notes

**macOS (primary)**

- Native title bar: `TitlebarOptions { appears_transparent: false }` in
  `main.rs` — standard traffic-light chrome, no custom drag region.
- Menus: native `cx.set_menus` (App menu bar + Dock menu) from
  `crates/shell/src/menu.rs` — no in-window menu rendering to pay for.
- DPI: GPUI's Metal renderer is scale-factor aware; all sizes are logical
  `px(..)` so Retina is automatic. Terminal cell metrics are derived from
  `text_system().ch_advance` at the current scale.
- Window bounds are persisted/restored (`window_state`).

**Linux (later)**

- Keep it buildable: no macOS-only APIs leak outside `main.rs`'s window
  setup and `menu.rs`. GPUI selects the Vulkan/Blade renderer on Linux; the
  view layer is renderer-agnostic (logical `px`, theme tokens, no platform
  branches in feature view code).
- Open items for the Linux pass: file-dialog / open-in-browser shims,
  keychain backend (`keyring` already abstracts this), font fallback list.

## 6. Visual-parity items closed here (D1–D6 from T15-001)

- **D1** — canonical interaction fills: `ThemeStore::hover_fill()` = `accent`
  (neutral). Selection is now brand-tinted: `selected_fill()` = `primary` at
  0.16 alpha and `selected_accent()` = solid `primary` for the 2px active
  bar/border (`Palette::selected_fill` / `selected_accent` mirror these).
  Hover deliberately stays neutral. Applied to the active editor tab, Explorer
  / tree rows, command-palette selection, the Settings sidebar's active
  category (+ left accent bar), and `icon_button_builder`'s selected state
  (dock rail, notification bell, snippet toggles).
- **D2** — `ThemeStore::scrollbar_thumb()` / `scrollbar_thumb_hover()` =
  foreground at 22% → 34% alpha, `SCROLLBAR_SIZE = 10.0` — matches
  `.themed-scrollbar`. (Wire into a scrollbar widget when one is adopted.)
- **D3** — terminal cursor proportions: beam 1 px, underline 1 px,
  `HollowBlock` renders as a 1 px outline (xterm `cursorInactiveStyle:
  "outline"`), focused block keeps the translucent fill.
- **D4** — tab-entrance animation: `CubicBezier::eval` drives a GPUI
  `with_animation` opacity fade over `--dur-base` with `--ease-premium`;
  `TAB_IN_FROM_SCALE` constant records the reference `scale(0.86)` (GPUI 0.2.2
  `Div` has no scale transform, so opacity-only). Reduce-motion clamps the
  duration to ~0 like the reference `0.01ms` rule.
- **D5** — `theme::menu_metrics` constants (container `p-1.5`, item `px-3
  py-2`, `gap-2.5`, popover `p-4`) from the reference `dropdown-menu` /
  `command` / `popover` components; command-palette row padding aligned to
  `ITEM_PAD_X`.
- **D6** — `community_theme_partial_import_round_trips_visually` test: a
  partial community theme applies only its tokens, leaves the rest on the
  default, and survives export → re-import with no channel drift.

## Build performance baseline — end of Phase 15 (T16-010)

Reference point for the Phase 20 perf sign-off (T21-002 "build budget"). Captured
right after the `crates/ui` monolith was fully dissolved into ~20 crates
(T16-001 … T16-009).

### Environment

| | |
|---|---|
| Date | 2026-09-03 |
| Machine | Intel Xeon E5-2695 v4 @ 2.10 GHz, 4 vCPU (KVM/Proxmox guest), 503 GiB RAM |
| OS | Linux 7.0.14-11-pve |
| Toolchain | `rustc 1.98.0 (88d9e12ae 2026-08-18)`, `cargo 1.98.0` |
| Disk | single ext4 loop device, ~24 GiB free during the run |
| Cargo env | `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` (session-local, disk-constrained VPS) |
| Workspace | 20 crates, 81 internal edges, 8 dependency tiers (see `docs/architecture.md` §9) |

`CARGO_INCREMENTAL=0` means the "incremental" figures below are cargo's
*dependency-tracking* rebuilds (recompile only the crates whose inputs changed),
not LLVM incremental codegen. This matches the VPS dev setup and CI.

### Measurements

Wall-clock, single sample each (`date +%s` deltas), `-j4`.

| Scenario | Command | Time |
|---|---|---|
| **Cold** check | `cargo clean` → `cargo check --workspace --all-targets` | **327 s** (5 m 27 s) |
| **Warm** check (no edits) | `cargo check --workspace --all-targets` | **2 s** |
| Clippy (after cold check) | `cargo clippy --workspace --all-targets -- -D warnings` | **23 s** |
| **Incremental** — 1-line comment added to `crates/shell/src/app_shell.rs`, caches warm | `cargo check -p labonair-shell` | **~3 s** |
| Incremental — same edit, fingerprints cold (first `-p` run after an `--all-targets` + clippy pass) | `cargo check -p labonair-shell` | 158 s (noise — see note) |

`target/` after the cold check: 1.3 GiB.

#### Note on the incremental figure

The **~3 s** stabilised value is the number the crate split was meant to move:
a one-line change in `app_shell.rs` now recompiles **only `labonair-shell`**, not
"half the `ui` monolith". The 158 s outlier was the very first `cargo check -p`
after a `--workspace --all-targets` build followed by a clippy pass — cargo had
to redo the non-test-cfg fingerprints for `labonair-shell` and its reverse-dep
closure once. Every subsequent 1-line edit re-checks in ~3 s.

T21-002 should compare against **~3 s** for the incremental budget and **327 s /
2 s** for the cold / warm `check --workspace --all-targets` budget.

### Coordinator-forced deviations (disk budget)

The task also asks for cold/warm `cargo build --release` and
`cargo test --workspace` timings. Both are **disk-banned** on this VPS (a full
debug/test build is ~20–28 GiB against ~25 GiB headroom; `cargo test --workspace`
was explicitly forbidden for this task). They are deferred to T21-002, which runs
on CI hardware with a real disk budget. The gate actually run here was:
`cargo fmt --check`, `cargo check --workspace --all-targets`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`scripts/check-crate-deps.sh` — all green.

### Open follow-up

`labonair-terminal` still pulls `gpui` + `labonair-theme` (it renders the ANSI
cell grid). An engine/renderer split would let `cargo check` on the pure engine
skip GPUI entirely — candidate for a later phase, not scoped to Phase 15.

### Render-path baseline — T21-001 (2026-09-05)

The render path is instrumented through the opt-in `labonair::perf` tracing
target. Enable it with:

```sh
RUST_LOG=labonair::perf=trace cargo run -p labonair
```

Every core render emits a `render` span with its `view` field (`shell`,
`titlebar`, `workspace`, `status_bar`, and the Explorer, SCM, Git Graph,
Snippets, and AI panels). The same target also covers `settings_recompute`,
`active_theme_recompute`, and `live_snapshot_recompute`. This is deliberately
opt-in: normal runs do not enable the tracing subscriber for this target.

| Scenario | Core render expectation | Timing / allocation capture |
|---|---|---|
| Idle, unfocused terminal, 10 s | 0 spans after initial paint | Trace capture required on a graphical macOS host |
| Tab switch | Workspace and its changed tab child; shell at most two paints | Capture trace span durations + `heaptrack`/`dhat` on the host |
| Terminal typing | Terminal child only; no shell render | Capture trace span durations + allocation sample |
| Panel toggle / split resize | Workspace and affected dock/panel only | Capture trace span durations |
| Settings field change | Settings recompute, active theme only if relevant, affected view | Capture trace span durations |

This CI/container runner has no display server, so it cannot launch GPUI or
produce credible GPU frame-time / heap-profiler measurements. The source-level
audit confirms that `build_palette_data` and frame-side live-bridge refreshes
remain absent; live snapshots are observer-driven and duplicate snapshots are
now explicit no-ops (`identical_snapshot_is_an_idle_noop`). The remaining
deferred Workspace queues are skipped without `mem::take` when empty.

The required graphical sign-off is therefore still open: run the commands
above on macOS, attach the trace plus `dhat` or `heaptrack` samples for tab
switch and terminal typing, then replace the capture placeholders with measured
milliseconds and peak bytes. Terminal cursor blink / active PTY output must be
disabled for the idle run.

## Performance Budgets

**Related:** [`../performance.md`](performance.md), [`../perf-baseline.md`](performance.md), [`../testing/evidence-model.md`](testing.md)

The machine-readable source is `budgets.toml`, and the
generated index is [`../generated/performance.md`](performance.md).
Run `python3 scripts/verify.py --scope performance` before changing a budget
or claiming a measurement.

These are target budgets, not measured claims. A budget becomes `Verified` only
after a reproducible release-build measurement is recorded on the supported
native host.

| Area | Target | Current evidence |
|---|---|---|
| Cold start to visible window | `< 400 ms` on macOS Apple Silicon release build | Pending native measurement. |
| Start to interactive terminal | `< 700 ms` | Pending native measurement. |
| Idle RSS | `< 150 MB` with one tab | Pending native measurement. |
| Terminal output | No visible hitch at 200k lines; bounded scrollback | Source guards exist; native workload pending. |
| Large lists | 5k-row Explorer/SFTP/Git views remain virtualized and responsive | Source guards exist; native workload pending. |
| Git status | One poll per configured interval, no overlap | Source guard and tests; runtime measurement pending. |
| UI foreground | No filesystem, process, network, or database blocking | Code review and async boundaries; profiler evidence pending. |

The container has no display server and cannot provide credible GPU/frame/RSS
evidence. Keep these items pending until the documented macOS measurement
procedure runs.
