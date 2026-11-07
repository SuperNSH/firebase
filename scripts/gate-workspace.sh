#!/usr/bin/env bash
# The full Rust gate for every crate in the workspace; the milestone's
# whole-tree smoke runs this.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
