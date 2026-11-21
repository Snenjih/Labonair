# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.
`AGENTS.md` is a symlink to this file — edit only `CLAUDE.md`.

## Priority

**The user's direct instruction in the current session always wins.** The task queue (`tasks/`) and the docs are a default backlog and contract reference, not a gate: do not substitute, reorder, or "first finish" queue tasks, and do not refuse a request because it is not the earliest open task. Scale process to change size — a small fix needs the focused gates, not a full contract write-up.

## What this is

Labonair is a standalone native Rust/GPUI Dev-Op workspace (terminal, editor, SSH, SFTP, Git, transfers, themes, keymap, notifications, AI). macOS first, Linux later. No WebView, JS frontend, or IPC layer. It is a hard fork of a Tauri predecessor: `reference/` (gitignored) holds that frozen copy as a behavioral/visual reference — read-only, never link to an external Labonair repo, never reintroduce web tech.

## Commands

Toolchain is pinned in `rust-toolchain.toml` (1.95.0). On macOS the Metal Toolchain is required (`xcodebuild -downloadComponent MetalToolchain`).

```text
cargo run -p labonair                      # run the native app
cargo check --workspace --all-targets
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p <crate> <test_name>          # single test / focused crate
cargo check -p labonair --features gallery # debug Component Gallery (ui-kit)
scripts/package-macos.sh [--dmg]           # release bundle (see docs/RELEASE.md)
scripts/check-crate-deps.sh                # dependency-graph verifier (run when deps change)
python3 scripts/check_documentation.py     # run when docs change
python3 scripts/verify.py --scope <scope>  # orchestrator; `all` runs everything
```

**Never use `open -a Labonair`** — the legacy Tauri app is installed under the same name at `/Applications/Labonair.app`. Run the Rust build via `cargo run -p labonair` or an explicit `target/**/labonair` path, and never start the Tauri app when testing the Rust one.

Before calling Rust work done run fmt/check/clippy/test. UI changes also need a visual check of the running native window; if capture is unavailable, say so rather than claiming it was verified.

## Architecture

Workspace of ~65 crates under `crates/`. The invariant: **one product capability → one owning module → one canonical capability crate.** The owner holds the capability's state, behavior, UI, persistence, commands, notifications and tests. A capability may split into sibling crates (`-core`, `-ui`, `-host`, `-storage`, `-ssh`/`-transport`) only for a real dependency or lifecycle boundary; siblings never create a second owner.

- `crates/app` (`labonair`) and `crates/shell` (`labonair-shell`) are **composition roots only**: they wire registered capabilities and own the permanent chrome. No feature logic, no static shell-wide command/panel/theme/host/notification tables.
- Feature crates must not depend on `labonair-shell` or reach into another feature's private state. Cross-module communication uses the smallest typed contract, typed event, or registry (registries only when there are multiple providers/consumers). New contributions are registered by the owner and wired by the composition root.
- Foundation crates (notably `labonair-ui-kit`, `theme`, `theme-tokens`, `errors`, `events`) must not depend on product modules. New dependencies need a documented reason and must keep the graph acyclic (`check-crate-deps.sh`).
- **Settings hold values only.** Hosts, themes, icon themes, keymap editing, transfers, notifications, and command registration belong to their own modules. Settings live in `settings`/`settings-content`/`settings-json`/`settings-macros`/`settings-ui`.
- **Detached session host:** `labonair-host` daemon hosts terminals/SSH/SFTP/transfers (the `*-host` crates; ADR 0009).
- **Shell zones:** titlebar (tabs + one global menu), workspace (active tab, split panes), docks (registered panels), statusbar (panel controls left, info items right), overlay layer (command palette, dialogs). User-facing messages go to the statusbar notification dropdown — there is no toast system. Don't add parallel settings/menu/panel/status/error surfaces; new permanent chrome needs an ADR.
- **UI:** every reusable control comes from `labonair-ui-kit` (design system with shared `Palette` tokens). Feature crates compose it; no local button/list/menu/input/badge/dropdown styles. Themes are JSON families resolved by `ThemeRegistry`.
- The AI frontend is intentionally parked (git tag `ai-ui-v1`); the AI backend crates are kept.

Prefer removal/deferral over a new framework when a feature has no current user workflow. Compatibility code needs a named removal condition. Move the contract first, then adapters/consumers, then delete the old path.

## Code rules

- Code, comments, commits, and normative docs are in English.
- Never block the GPUI foreground thread: use async tasks, Tokio, or `spawn_blocking` for I/O.
- No `unwrap()` for predictable runtime errors — return descriptive results.
- Consult GPUI docs and the vendored GPUI source (`vendor/`, `reference/zed`) before relying on undocumented APIs. Non-obvious GPUI / alacritty_terminal / gpui-component discoveries go in `docs/gotchas.md`.
- Never expose secrets in source, logs, SQLite, tests, or commits.

## Docs and tasks

`docs/README.md` indexes the normative contracts (product, architecture, modules, registries, settings, design system, release). Before changing a feature, identify its owning crate, public contract, settings, persistence, notifications and UI-kit components using the ownership matrix in `docs/modules.md`. Update the relevant doc or ADR when a contract changes. Open work is tracked in `tasks/`; `tasks/archive/` is historical and not instructions.
