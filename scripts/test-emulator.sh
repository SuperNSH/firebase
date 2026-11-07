#!/usr/bin/env bash
# The labelled sandbox tests against the emulator suite on 127.0.0.1. Each
# test runs only if its file exists; missing areas print SKIPPED. No real
# Firebase account is ever touched.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

bash scripts/probe-emulator.sh || bash scripts/start-emulator.sh

for pair in "nsh-firebase-auth emulator_auth" "nsh-firebase-database emulator_database" "nsh-firebase-storage emulator_storage"; do
  crate=${pair%% *}
  test=${pair##* }
  file="crates/$crate/tests/$test.rs"
  if [ -f "$file" ]; then
    cargo test -p "$crate" --test "$test" -- --ignored
  else
    echo "SKIPPED $file (not written yet)"
  fi
done

cd packages/nsh-firebase
for file in tests/emulator/auth.sandbox.test.tsx tests/emulator/database.sandbox.test.tsx tests/emulator/storage.sandbox.test.tsx; do
  if [ -f "$file" ]; then
    nsh test "$file"
  else
    echo "SKIPPED $file (not written yet)"
  fi
done

# The analytics and monitor sandbox tests assert against tools/collector on
# 127.0.0.1:18742; perfmon owns that process and its start instructions.
collector_up() {
  curl -fsS -o /dev/null --max-time 2 http://127.0.0.1:18742/health
}
if collector_up; then
  for file in tests/emulator/analytics.sandbox.test.tsx tests/emulator/monitor.sandbox.test.tsx; do
    if [ -f "$file" ]; then
      nsh test "$file"
    else
      echo "SKIPPED $file (not written yet)"
    fi
  done
else
  echo "SKIPPED tests/emulator/{analytics,monitor}.sandbox.test.tsx (tools/collector not running on 127.0.0.1:18742; start it per docs/status/perfmon.md)"
fi
