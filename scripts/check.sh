#!/usr/bin/env bash
# Canonical local/CI gate: fmt, check, clippy, test, crate-graph rules.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/check-crate-deps.sh
