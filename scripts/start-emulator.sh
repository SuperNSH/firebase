#!/usr/bin/env bash
# Starts the Firebase emulator suite detached for demo-nsh, exactly as
# core starts it: JAVA_HOME from Android Studio's JDK, a pid file and a
# log under emulator/, then waits until the auth and database ports
# answer. Idempotent: if the suite is already up, it does nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if bash scripts/probe-emulator.sh >/dev/null 2>&1; then
  echo "emulator suite already up"
  exit 0
fi

if [ ! -x node_modules/.bin/firebase ]; then
  echo "node_modules/.bin/firebase is missing; run bun install first" >&2
  exit 2
fi

mkdir -p emulator
# /usr/bin/java is a stub on this Mac; the suite needs Android Studio's JDK.
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
nohup node_modules/.bin/firebase emulators:start \
  --project demo-nsh \
  --config emulator/firebase.json \
  > emulator/emulator.log 2>&1 &
echo $! > emulator/emulator.pid

for _ in $(seq 1 90); do
  if bash scripts/probe-emulator.sh >/dev/null 2>&1; then
    bash scripts/probe-emulator.sh
    exit 0
  fi
  sleep 1
done

echo "emulator suite did not answer within 90s; see emulator/emulator.log" >&2
exit 1
