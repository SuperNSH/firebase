#!/usr/bin/env bash
# The Rust gate for one crate: fmt, clippy -D warnings, test.
# usage: gate-crate.sh <crate-name>
set -euo pipefail
if [ $# -ne 1 ]; then
  echo "usage: gate-crate.sh <crate-name>" >&2
  exit 2
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
cargo fmt -p "$1" -- --check
cargo clippy -p "$1" --all-targets -- -D warnings
cargo test -p "$1"
