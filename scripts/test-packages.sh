#!/usr/bin/env bash
# The package unit gates, one explicit entry/test pair per area. A pair
# runs only once its test file exists; until then the area is a stub and
# the pair prints SKIPPED.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG="$ROOT/packages/nsh-firebase"

run_pair() {
  local entry="$1" testfile="$2"
  if [ ! -f "$PKG/$testfile" ]; then
    echo "SKIPPED $entry $testfile (stub until its area lands)"
    return 0
  fi
  echo "== gate: $entry $testfile"
  bash "$ROOT/scripts/gate-tsx.sh" "$entry" "$testfile"
}

run_pair tests/core-app.tsx tests/core.test.tsx
run_pair tests/auth-app.tsx tests/auth.test.tsx
run_pair tests/database-app.tsx tests/database.test.tsx
run_pair tests/storage-app.tsx tests/storage.test.tsx
run_pair tests/messaging-app.tsx tests/messaging.test.tsx
run_pair tests/analytics-app.tsx tests/analytics.test.tsx
run_pair tests/perf-app.tsx tests/perf.test.tsx
run_pair tests/monitor-app.tsx tests/monitor.test.tsx
run_pair tests/firestore-app.tsx tests/firestore.test.tsx
run_pair tests/remote-config-app.tsx tests/remote-config.test.tsx
run_pair tests/crashlytics-app.tsx tests/crashlytics.test.tsx
run_pair tests/installations-app.tsx tests/installations.test.tsx
run_pair tests/app-check-app.tsx tests/app-check.test.tsx
run_pair tests/functions-app.tsx tests/functions.test.tsx
