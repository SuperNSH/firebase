# remote-config evidence

This area was built under the battery constraint, so no gate, test, or
capture was run while writing it — see `docs/status/remote-config.md`
("Verified how" / "Not verified"). There is no screenshot to look at:
the area has no visual surface of its own, and the emulator suite has
no Remote Config service, so there is no sandbox run to capture either.

What CI should drop in this directory as it runs:

- `gate-crate.log` — output of
  `bash scripts/gate-crate.sh nsh-firebase-remote-config`
  (fmt, clippy `-D warnings`, 32 tests).
- `gate-tsx.log` — output of
  `bash scripts/gate-tsx.sh tests/remote-config-app.tsx
  tests/remote-config.test.tsx` (check, warning-free build, 11 tests).

Reading sources used instead of runtime probes (already quoted in the
status doc): the Remote Config REST reference and automate-rc page, the
v1 discovery document of firebaseremoteconfig.googleapis.com, and
RNFB/FlutterFire remote-config sources under `~/fo/.refs`.
