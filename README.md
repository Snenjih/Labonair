# Labonair-rust

Labonair is a standalone native Rust application built with GPUI. It is a fast, keyboard-first Dev-Op workspace for local and remote work, combining terminal, editor, SSH, SFTP, Git, transfers, themes, keymap, notifications, and AI as independently owned capabilities.

The product supports both project workspaces and standalone use. It is an independently implemented native Rust/GPUI Dev-Op workspace with one coherent interaction model across its capabilities.

## Documentation

Start at [`docs/README.md`](docs/README.md). Agent/contributor rules are in [`CLAUDE.md`](CLAUDE.md) (`AGENTS.md` is a symlink to it); open work is in [`docs/tasks/rework/`](docs/tasks/rework/README.md) and [`TODO.md`](TODO.md).

The architectural invariant: one product capability, one owning module, one canonical capability crate. The shell composes registered capabilities; it does not implement them. User-visible messages go to the notification dropdown, and settings contain values only.

## Build commands

```text
cargo run -p labonair
cargo build
scripts/check.sh   # fmt, check, clippy, test, crate-dependency rules
```

Platform priority is macOS first, with Linux later. The application contains no WebView, JavaScript frontend, or IPC layer.

## License

Copyright (C) 2026 Snenjih. Licensed under **GPL-3.0-or-later** ([`LICENSE`](LICENSE)) with additional terms under GPL §7 ([`ADDITIONAL-TERMS.md`](ADDITIONAL-TERMS.md)): forks must be renamed, marked as modified and credit the origin. The name, logo and icon are not licensed ([`TRADEMARK.md`](TRADEMARK.md)).
