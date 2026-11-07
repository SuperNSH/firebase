# firestore area evidence

Empty for now, on purpose: the area was written under battery mode (no
compile, build, test, or gate command was run locally), so there are no
gate or sandbox logs yet.

What lands here from the first CI run, per docs/status/firestore.md:

- `gate-crate.log` — `bash scripts/gate-crate.sh nsh-firebase-firestore`
  (fmt, clippy -D warnings, in-crate unit tests + tests/firestore_ui.rs).
- `gate-tsx.log` — `bash scripts/gate-tsx.sh tests/firestore-app.tsx
  tests/firestore.test.tsx` (nsh check, warning-gated nsh build, the
  headless tests).
- `rust-sandbox.log` — `cargo test -p nsh-firebase-firestore --test
  emulator_firestore -- --ignored` against the emulator suite.
- `tsx-sandbox.log` — `nsh test
  tests/emulator/firestore.sandbox.test.tsx` against the emulator suite.
