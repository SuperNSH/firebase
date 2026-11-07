#!/usr/bin/env bash
# Exit 0 only if the auth (19099), database (19000), storage (19199) and
# hub (14400) emulators all answer. The storage emulator answers plain
# requests with 501, so the check is "some HTTP answer", not 2xx.
set -euo pipefail
answers() {
  local url="$1"
  [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 2 "$url")" != "000" ]
}
curl -fsS -o /dev/null --max-time 2 http://127.0.0.1:19099/ || exit 1
curl -fsS -o /dev/null --max-time 2 'http://127.0.0.1:19000/.json?shallow=true' || exit 1
answers http://127.0.0.1:19199/ || exit 1
answers http://127.0.0.1:14400/ || exit 1
echo "emulator suite is up (auth 19099, database 19000, storage 19199, hub 14400)"
