#!/usr/bin/env bash
# The evidence manifest the final smoke checks: every listed file must
# exist and be non-empty, and the PNG minimums must hold. Fails with a
# MISSING list.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

missing=()

check_file() {
  if [ ! -s "$1" ]; then
    missing+=("$1")
  fi
}

check_pngs() {
  local dir="$1" min="$2" n=0
  for f in "$dir"/*.png; do
    [ -f "$f" ] && n=$((n + 1))
  done
  if [ "$n" -lt "$min" ]; then
    missing+=("$dir/*.png (found $n, need at least $min)")
  fi
}

for f in \
  docs/evidence/native-ios/*.log \
  docs/evidence/native-android/*.log \
  docs/evidence/demo/vision.md \
  docs/evidence/perfmon/vision.md \
  docs/evidence/perfmon/collector.jsonl \
  docs/evidence/native-ios/simctl-push.log \
  docs/evidence/native-android/logcat.txt \
  docs/evidence/analytics/collector-assert.log \
  docs/evidence/core/access-denied.log
do
  check_file "$f"
done

check_pngs docs/evidence/native-ios 2
check_pngs docs/evidence/native-android 2
check_pngs docs/evidence/demo 2
check_pngs docs/evidence/perfmon 2

if [ ${#missing[@]} -gt 0 ]; then
  echo "MISSING evidence:"
  printf '  %s\n' "${missing[@]}"
  exit 1
fi
echo "evidence manifest complete"
