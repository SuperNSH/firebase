#!/usr/bin/env bash
# The TSX gate for one entry/test pair: check, a warning-gated build, test.
# usage: gate-tsx.sh <entry> <testfile>
#
# On nsh 0.4.0 a nested entry path is treated as an app directory of its
# own, so check and build run at the package root, whose App.tsx mounts the
# same UI the tests drive.
set -euo pipefail
if [ $# -ne 2 ]; then
  echo "usage: gate-tsx.sh <entry> <testfile>" >&2
  exit 2
fi
ENTRY="$1"
TESTFILE="$2"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG="$ROOT/packages/nsh-firebase"
cd "$PKG"
for file in "$ENTRY" "$TESTFILE"; do
  if [ ! -f "$file" ]; then
    echo "gate-tsx: missing $file" >&2
    exit 2
  fi
done
nsh check
out="$(nsh build 2>&1)"
printf '%s\n' "$out"
if printf '%s' "$out" | grep -qi 'warn'; then
  echo "gate-tsx: nsh build warned (entry $ENTRY); fix the warning" >&2
  exit 1
fi
nsh test "$TESTFILE"
