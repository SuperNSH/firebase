# crashlytics evidence — capture protocol (nothing captured yet)

Battery constraint on this build session: no compile, build, test or
gate command was run. CI runs these first; this file is the protocol it
and the native verifiers execute, and the place their outputs go.

## CI gates (run these; save output here as gate-crate.log / gate-tsx.log)

- `bash scripts/gate-crate.sh nsh-firebase-crashlytics`
  (fmt check, `clippy -p nsh-firebase-crashlytics --all-targets -D
  warnings`, `cargo test -p nsh-firebase-crashlytics` — 12 tests)
- `bash scripts/gate-tsx.sh src/crashlytics/app.tsx tests/crashlytics.test.tsx`
  (from packages/nsh-firebase: `nsh check`, warning-gated `nsh build`,
  `nsh test` — 6 tests)

## Native proof (per the tier table: no emulator service exists)

Needs a device or simulator, so it is unverified here on purpose. The
facade's proof is routing; the SDK's debug logging is the proof the
report reaches Firebase:

- iOS: run `examples/host-ios` with the fake plist, Crashlytics debug
  mode on (`-FIRDebugEnabled` / `FIRCrashlytics` logging), call the
  facade's `recordError`/`log` from the demo, and capture the
  `CRASHLYTICS` log lines.
- Android: run `examples/host-android`, capture
  `adb logcat -s FirebaseCrashlytics` while the demo calls
  `recordError`/`log`/`setCustomKey`.
- `didCrashDuringPreviousExecution` returning `true` needs a real
  previous crash: force one with the native SDK's own debug crash in a
  throwaway native test harness (the facade deliberately has no
  `crash()`), then relaunch and capture the flag.
- dSYM upload is out of sandbox scope (tier table).

Save captured logs here with the date and host in the filename.
