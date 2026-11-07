#!/usr/bin/env bash
# The demo's gate: TSX check + warning-gated build + its test file when it
# exists, and the Rust demo crate's tests. Parts that do not exist yet
# print SKIPPED.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/examples/demo"

if [ -f App.tsx ]; then
  nsh check App.tsx
  out="$(nsh build App.tsx 2>&1)"
  printf '%s\n' "$out"
  if printf '%s' "$out" | grep -qi 'warn'; then
    echo "test-demo: nsh build warned; fix the warning" >&2
    exit 1
  fi
  if [ -f tests/demo.test.tsx ]; then
    nsh test tests/demo.test.tsx
  else
    echo "SKIPPED tests/demo.test.tsx (not written yet)"
  fi
else
  echo "SKIPPED examples/demo App.tsx (stub until the demo area lands)"
fi

cargo test -p demo-rs
