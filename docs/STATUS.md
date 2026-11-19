# STATUS

The honest ledger [PROMPT.md](PROMPT.md) asks for: what holds, what does
not, and the evidence behind each claim. A claim with no evidence path is
reported as unverified. Sections written 2026-11-07 are marked by their own text; the
"Plan audit 2026-11-19" section below supersedes any older line it
contradicts (NSH is 0.6.0 now; the device proofs ran).

## How this was built

The build ran under the `z-workflow` skill: eleven disjoint areas (core,
auth, database, storage, messaging, analytics, perfmon, native-ios,
native-android, demo, stretch), each advanced by subagents on
GLM-5.3-Flash through implement -> audit -> unit-test phases, plus native
device testing and a vision phase where an area owns a screen. Audit loops
ran per area until zero issues or the three-round cap; where the cap was
reached, the final round's findings were fixed and engine-verified — that
is recorded per area in `docs/status/<area>.md`, and summarized below.

Until the battery pause (2026-11-07 afternoon), every compile and test
gate ran locally and its result was recorded. From the pause on, the build
machine stays off compilers: every compile/test gate runs in GitHub CI
instead — [.github/workflows/ci.yml](.github/workflows/ci.yml) on push and
PRs (jobs: `rust-gate`, `packages`, `emulator`, `demo`, `clean-proof`,
`host-ios`, `host-android`), and
[.github/workflows/device-proofs.yml](.github/workflows/device-proofs.yml)
for the native device proofs, `workflow_dispatch` only.

Commit state: the whole tree is committed on `main` and pushed to the
public repository https://github.com/SuperNSH/firebase (the user
directed compile testing there after the battery pause; the brief's
no-push rule is superseded by that instruction for this repository).

**CI runs and is green** for the push-triggered matrix: run
[42364995246](https://github.com/SuperNSH/firebase/actions/runs/42364995246)
passes `rust-gate` (fmt + clippy `-D warnings` + tests, linux),
`packages` (every area's TSX gate), `emulator` (the full demo-nsh
sandbox suite against the Firebase emulator suite plus the collector),
`demo` (demo TSX gates + the demo-rs crate gate), `clean-proof` (fresh
clone, `bun install`, one command) and `host-android` (the BoM module
and host app assemble). The macOS `host-ios` build gate runs on macOS
hosted runners now and is green on every push (the earlier org-level
block — runs cancelled while queued with zero steps, observed on run
42361739726 — was lifted). The iOS device proof stays dispatch-only in
[ci-macos.yml](.github/workflows/ci-macos.yml) purely because it sleeps
for real push/tap timing, not because of any runner block.

## Plan audit 2026-11-19 (NSH 0.6.0)

Done on this Mac against nsh 0.6.0, by reading the code and running it.
Nothing here is carried from the older sections.

### Gates (all exit 0, run in this session)

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --all-targets -- -D warnings` | exit 0 |
| `cargo test` | exit 0: 395 passed, 0 failed, 5 ignored (the 5 are the emulator-only tests, run below) |
| `bash scripts/test-packages.sh` (nsh check, warning-gated nsh build, nsh test per area) | exit 0: 144 tests passed, 0 failed; log `docs/evidence/core/packages-gate-0.6.0.log`. The `monitor` pair prints SKIPPED: its tests live in `perf.test.tsx` |
| `bash scripts/test-demo.sh` (demo nsh check/build/test + `cargo test -p demo-rs`) | exit 0: 11 TSX tests, 8 Rust tests; log `docs/evidence/demo/demo-gate-0.6.0.log` |
| `cargo check -p nsh-firebase-perf --target aarch64-apple-ios / aarch64-linux-android` | exit 0 both; wasm32-unknown-unknown also exit 0 |
| `bash scripts/test-emulator.sh` against the demo-nsh suite + collector | exit 0: 5 Rust sandbox tests (auth, database, storage, firestore, functions), 7 TSX sandbox files; log `docs/evidence/core/test-emulator-0.6.0.log` |
| `bash scripts/check-evidence.sh` | "evidence manifest complete" |

`nsh test` still prints `warn the status bar is not granted` once per
test on 0.6.0; `nsh build` has no warnings, which is what the gate checks.

### Defects found by running the hosts, and fixed

- `fixtures/GoogleService-Info.plist` and `google-services.json`: the fake
  API key was not 39 characters starting with `A`, so the Apple SDK threw
  `I-FIS008000` at `FirebaseApp.configure` and the host crashed at launch.
  The key is now `AIzaSyFAKE-nsh-firebase-demo-0000000000` (39 chars) in
  both fixtures, the Android defaults and the docs.
- `PROBE_TRACE_NAME` was `__nsh_probe__`. A leading underscore is invalid
  for Firebase Performance: the Apple SDK asserts (`NSException`, host
  abort in `FIRTrace initTraceWithName:`), and Android rejects it, which
  made the native perf path look unavailable. It is `nsh_probe` in Rust and
  TS; tests assert it is SDK-valid; the Swift handler also validates trace
  names before the SDK call.
- Android host crashed at launch: "The Crashlytics build ID is missing".
  The host now carries `res/values/crashlytics.xml` (fixed fake id) and
  `res/values/google_services.xml` (what the google-services plugin would
  generate; without `google_app_id` Firebase Analytics reports "Missing
  google_app_id. Firebase Analytics disabled").
- `scripts/test-emulator.sh` installed `emulator/functions/node_modules`
  after the suite started, so the Functions emulator never loaded the
  functions on a clean checkout, and it tested the Rust functions file with
  a wrong relative path (so it printed SKIPPED). Both fixed; the TS
  functions sandbox regex now matches what the screen prints
  (`missing(err=functions/not-found)`); it failed before the fix.
- Demo: Cloud Messaging screen gained Request permission and a Last event
  line (message / tap); the Analytics screen a Log SDK event button
  (native handler, plain error without a host); the Performance screen
  runs a native trace around its calls. Three new demo tests, each seen to
  fail once.

### Criterion 4: host-ios device proof — PASS (iPhone simulator `nsh-firebase-ios`, iOS 27)

- `xcodebuild` build: `docs/evidence/native-ios/xcodebuild-0.6.0.log`
  (`** BUILD SUCCEEDED **`), demo bundle built with `nsh build --release`.
- Launch with `-FIRDebugEnabled -FIRAnalyticsDebugEnabled`: Firebase
  initializes against the fake plist; screen `native-ios/host-launch.png`.
- Token request fails clearly with a fake project
  (`core/host-error ... Too many server requests`): `native-ios/token-failure.png`.
- Notification permission granted through the system prompt (driven by the
  demo's Request permission button), then
  `xcrun simctl push ... push.apns` (`native-ios/simctl-push.log`):
  foreground banner plus `Message: FCM proof` in the app
  (`push-foreground.png`); backgrounded push (`push-background.png`);
  tapping the banner brought the app forward with `Tapped: <id>`
  (`notification-tap.png`). Every PNG looked at.

### Criterion 5: host-android device proof — PASS (AVD `nsh-firebase`, API 37, port 5586)

- `gradle :app:assembleDebug` BUILD SUCCESSFUL (`native-android/gradle-0.6.0.log`).
- App launches and shows the demo (`host-launch.png`). Token request fails
  with the SDK's own message surfaced as `core/host-error`
  (`token-failure.png`).
- Push: the sandbox cannot deliver through Play services (a shell cannot
  hold the c2dm SEND permission, and `FirebaseInstanceIdReceiver` drops the
  broadcast), so `examples/host-android` has a debug receiver
  (`DebugPushReceiver`, action `dev.nsh.firebase.host.DEBUG_PUSH`) that
  builds the `RemoteMessage` from the broadcast extras and calls the
  module's public `NshFirebase.onMessageReceived`. Foreground:
  `Message: nsh-proof-5` in the app (`push-foreground.png`). Backgrounded
  (HOME): the module posts the notification (`push-background.png`);
  tapping it relaunches the host and the press reaches the app's handler
  once the screen listens (`Tapped: nsh-proof-8`, `notification-tap-page.png`).
  The press path is this module's own notification, not Google's.
- Not shown: a message delivered by the real FCM service while the process
  is dead (needs Play services and a real project).

### Criterion 6: native debug logs

- Android: with `log.tag.FA VERBOSE`, the app's Log SDK event press
  produced `Logging event: origin=app,name=demo_sdk_tap` in
  `native-android/analytics-debug.log`; the host log also shows
  `handle firebase.perf.startTrace` / `stopTrace` pairs
  (`monitor-phone.png`).
- iOS: `native-ios/sdk-debug-0.6.0.log` (fresh simulator `nsh-firebase-0.6`,
  launched with `-FIRDebugEnabled -FIRAnalyticsDebugEnabled`, UI driven with
  `idb ui tap`) shows the SDK's per-event lines for `demo_sdk_tap`:
  `I-ACS023051 Logging event: origin, name, params: app, demo_sdk_tap`,
  `I-ACS023073`, `I-ACS023072 Event logged` and `I-ACS023105`
  (screen `native-ios/analytics-sdk-event-0.6.0.png`). **Cause of the
  earlier "missing" line: not this repo.** Those lines are os_log
  `Debug`-level messages; `log show`/`log stream` and `simctl launch
  --console-pty` omit them unless the stream runs with `--level debug`
  (`xcrun simctl spawn <udid> log stream --level debug --predicate
  'process == "NshFirebaseHost"'`). The earlier captures were default
  level. No code change.
- Android: Analytics event lines carry the tag `FA-SVC`, not `FA`: set
  `adb shell setprop log.tag.FA-SVC VERBOSE` too
  (`native-android/analytics-sdk-event-0.6.0.log`).
- Performance Monitoring native SDK logs, both platforms, fake project:
  - iOS: `native-ios/sdk-debug-0.6.0.log` has `I-PRF100009 Logging trace
    metric - nsh_probe 8.5630ms` and `monitor_load 1200.3810ms`
    (`native-ios/perf-trace-0.6.0.png`).
  - Android: the SDK logs nothing to logcat by default and
    `setprop log.tag.FirebasePerformance` no longer enables it in
    firebase-perf 23.0.1. The host manifest now carries
    `firebase_performance_logcat_enabled=true` (the real cause; a repo
    change in `examples/host-android`, debug host only). Then
    `native-android/perf-sdk-0.6.0.log` shows the SDK initialised and
    `trace metric: nsh_probe (duration: 29.342ms)` and `monitor_load
    (duration: 1164.024ms)`, both "Event dropped due to device sampling"
    (the SDK's own sampling on a project with no remote config, so no
    delivery is expected). Build log `native-android/gradle-perflog.log`;
    screen `native-android/perf-trace-0.6.0.png`. No unit test applies: a
    manifest flag in a host app.
- The pure tier reaches `tools/collector` (the analytics sandbox test in
  `core/test-emulator-0.6.0.log`, copied to `analytics/collector-assert.log`).
- Not shown: a trace reaching the Performance dashboard (needs a real project).

### Criterion 7: phone targets — PASS

`native-ios/monitor-phone.png` (1209 ms `/slow` on top, other rows 6-7 ms)
and `native-android/monitor-phone.png` (1207 ms `/slow` on top; the
failed rows are from the first load before `adb reverse tcp:18742`).
Record: `docs/evidence/demo/vision.md`.

### NSH 0.6.0 finish pass (embedding SDKs)

- The hosts were still linking NSH **0.4.0** embedding SDKs (the Homebrew
  `nsh.aar` and `NativeShell.xcframework`) under a 0.6.0 CLI and crates.
  Both now use the 0.6.0 SDKs: `tools/fetch-nsh-embed.sh` (sha256-pinned,
  unpacks into git-ignored `.nsh-embed/0.6.0/`; this Mac's copy came from
  a sibling checkout that fetched it with the same script), the Android
  AARs under `app/libs` and `native/android/libs` replaced, and the iOS
  project's `NSH_EMBED_IOS` default pointed at `.nsh-embed/0.6.0/ios`.
- `NshFirebase.handleNotificationIntent` now calls
  `NshView.handleNotificationIntent` directly (it exists in the 0.6.0 AAR;
  the reflection for 0.4.0 is gone).
- Evidence: `gradle :app:assembleDebug` BUILD SUCCESSFUL on the 0.6.0 AAR;
  the host on AVD `nsh-firebase` shows the demo without a crash:
  `native-android/host-launch-0.6.0-aar.png` (looked at).
  `native-ios/xcodebuild-0.6.0-sdk.log`: BUILD SUCCEEDED linking the 0.6.0
  xcframework. Not re-run on the 0.6.0 SDKs: push, tap and perf proofs on
  both platforms (the earlier proofs may have run on the 0.4.0 SDKs). An
  attempted Android background push this session showed no notification and
  was discarded as inconclusive.
- Notifications: NSH 0.6.0 ships a `Notifications` module. The facade keeps
  its host path on purpose (FCM messages arrive in the SDK's own delegate
  and service, which own the notification center and press intent); see
  `docs/research.md` section 5.
- Still true on 0.6.0, rechecked in source: Rust `fetch` returns the whole
  body (`element/services.rs`), `host_call` is `pub(crate)`, `HostInbox`
  keeps 256 events, `nsh check` rejects a nested entry path, `nsh package`
  has no flag for extra native SDKs.

### Still blocked or unverified

- A real FCM token and a real delivery: no Firebase account (by design).
- Native Performance Monitoring dashboards: no emulator for them (the SDKs'
  own logs are captured, above).
- iOS/Android host runs are local-only; CI cannot run them (see criteria
  4 and 5 history). The device-proof workflows remain dispatch-only.
- Criteria 1, 2, 3, 8, 9, 10 were re-checked by the gates above; the
  README's prerequisite text already names nsh 0.6.0.

## Satisfaction criteria

| # | Criterion | Status |
|---|---|---|
| 1 | research.md with tier table | PASS |
| 2 | Gates pass for all crates and packages | PASS — run [42403998509](https://github.com/SuperNSH/firebase/actions/runs/42403998509) green across every job including host-ios on macOS |
| 3 | Emulator tests, one documented command | PASS — `clean-proof` job green (fresh clone, `bun install`, `bun run test:emulator`); suite green locally pre-pause too |
| 4 | host-ios device proof | PASS on 2026-11-19 — see Plan audit; the older UNVERIFIED text below is history. Was: UNVERIFIED — the launch crash is fixed (UIScene lifecycle) and the macOS build gate is green; dispatch [ci-macos.yml](https://github.com/SuperNSH/firebase/blob/main/.github/workflows/ci-macos.yml) for the proof |
| 5 | host-android device proof | PASS on 2026-11-19 on this Mac's emulator — see Plan audit. Was: UNVERIFIED on CI — three attempts (42366965860, 42368309047, 42383580047) all end with CI's software-mode emulator ANRing the System UI: the captures show dialogs or a black screen, never the app, so their green conclusions are **not** criterion-5 evidence. The proof runs on this Mac's hardware emulator when it is on power (drive commands in `docs/status/native-android.md`); [device-proofs.yml](.github/workflows/device-proofs.yml) remains for a faster runner |
| 6 | Analytics/perf reach collector; native debug logs | pure tier PASS; Android native debug log PASS (`demo_sdk_tap` in `native-android/analytics-debug.log`); iOS per-event line PASS (`native-ios/sdk-debug-0.6.0.log`, needs `log stream --level debug`); native perf SDK logs captured on both |
| 7 | Monitoring screen with slow route on top | PASS on macOS, iOS simulator and Android emulator (see Plan audit) |
| 8 | ERR_ACCESS_DENIED with a test | PASS (locally pre-pause; re-run in CI `packages` job, green) |
| 9 | Migration note vs React Native Firebase | PASS |
| 10 | README from clean clone to tests and demo | PASS |

### 1. Research — PASS

`docs/research.md` cites both references by file path (RNFB
`packages/{app,auth,database,storage,messaging,analytics,perf}/...`,
FlutterFire `packages/firebase_*/...`), covers the Firebase REST surfaces
(Identity Toolkit, Secure Token, RTDB + SSE, Storage `X-Goog-Upload-*`,
GA4 Measurement Protocol) with sources, and carries the per-product,
per-target tier table (§4) plus the honest can't-do list. `docs/upstream-issues.md`
records the nsh-query/nsh-form bugs found while integrating with them.
Three audit rounds; citations spot-checked by auditors. Committed as
`08c5aef`.

### 2. Gates — pass locally pre-pause; CI confirmation expected on push

Every area's scoped gate was run and passed before the pause: Rust gates
(`scripts/gate-crate.sh <crate>`: fmt, clippy `-D warnings`, test) and
TSX gates (`scripts/gate-tsx.sh`: `nsh check`, warning-gated `nsh build`,
headless `nsh test`). Saved logs:

- `docs/evidence/auth/gate-crate.log`, `docs/evidence/auth/gate-tsx.log`
- `docs/evidence/database/rust-gate.log`, `docs/evidence/database/tsx-gate.log`
- `docs/evidence/storage/gate-crate.log`, `docs/evidence/storage/gate-tsx.log`
- `docs/evidence/perfmon/gate-crate.log`, `docs/evidence/perfmon/gate-tsx.log`

The core, messaging and analytics areas record their gate passes in
`docs/status/core.md`, `docs/status/messaging.md` and
`docs/status/analytics.md` without saved logs (their runs predate the
log-saving habit); `scripts/gate-workspace.sh` passed for all 14 crates
per `docs/status/core.md`. The native compile gates: `swift build` of
`native/ios/NshFirebase` plus the full `xcodebuild` host build
(`docs/evidence/native-ios/xcodebuild.log`, `** BUILD SUCCEEDED **` with
the demo bundle embedded), and Gradle `:app:assembleDebug` for the Android
host (`docs/evidence/native-android/gradle.log`, BUILD SUCCESSFUL, with
`JAVA_HOME` at Android Studio's JBR and `ANDROID_HOME=$HOME/Library/Android/sdk`).

**CI re-runs all of it and is green** — latest full run
[42403998509](https://github.com/SuperNSH/firebase/actions/runs/42403998509):
`rust-gate` on three OSes (fmt + clippy `-D warnings` + cargo test),
every cross-target check, `packages` (every area's TSX gate), `demo`,
`clean-proof`, `emulator` (now including the Firestore and Functions
sandbox tests) and both host builds on their native toolchains —
`host-ios` compiles the Swift package and the host app on macOS.

### 3. Emulator tests — PASS (local suite + CI clean-clone proof)

The one command is `bun run test:emulator` (scripts/test-emulator.sh). It
starts the suite itself if it is not up (`scripts/probe-emulator.sh` ||
`scripts/start-emulator.sh`, project `demo-nsh`: auth 19099, database
19000, storage 19199). The full green run is saved in
`docs/evidence/auth/test-emulator.log`: all three Rust sandbox tests
(`emulator_auth.rs`, `emulator_database.rs`, `emulator_storage.rs`,
run with `--ignored`) and all five TSX sandbox files (`auth`,
`database`, `storage`, plus `analytics` and `monitor` against
`tools/collector` on 127.0.0.1:18742) pass — sign up, sign in, refresh,
restore, database write/read/live-update-over-SSE, upload with progress,
download, delete. Individual logs: `docs/evidence/database/rust-sandbox.log`,
`docs/evidence/database/tsx-sandbox.log`,
`docs/evidence/storage/rust-sandbox-green.log`,
`docs/evidence/storage/ts-sandbox-green.log`,
`docs/evidence/perfmon/sandbox-monitor.log`.

The clean-checkout proof (fresh clone, `bun install`, one command) was
never run locally under the battery constraint — it ran as CI job
`clean-proof` in run 42364995246 and **passed**: a fresh `git clone`,
`bun install --frozen-lockfile`, then `bun run test:emulator` against
the emulator suite and collector, with the sibling libraries cloned at
their pinned commits (see `.github/actions/setup-siblings`).

### 4. host-ios device — history (2026-11-07 text; superseded, PASS in the Plan audit above)

What is verified: the host compiles (`docs/evidence/native-ios/xcodebuild.log`,
BUILD SUCCEEDED, the real demo bundle embedded in `App/nsh`, fake
`GoogleService-Info.plist` as a resource) and the Swift package compiles
(`swift build`), with the 16 contract handlers, delegate takeover and
event forwarding wired.

What is not: the device phase was started once and **failed at launch**,
then was cancelled under the battery constraint and not resumed.
`docs/evidence/native-ios/simctl-push.log` records it: the app installed
and launched but crashed immediately — `UIScene life cycle is required for
apps built with this SDK` (Info.plist has no `UIApplicationSceneManifest`;
`Sources/main.swift` uses legacy `UIApplicationMain`; two `.ips` crash
reports on the simulator). `xcrun simctl push` consequently failed
(`UNErrorDomain 2003`, exit 211). `host-sim-1.png`/`host-sim-2.png` show
the simulator home screen with the app installed — not a running app.
**The defect is now fixed in source** (a `SceneDelegate` builds the
window, `configurationForConnecting` wires it, and the
`UIApplicationSceneManifest` in project.yml names it), but the fix has
not run on a device yet — the proof runs as `device-proofs.yml`'s
`ios-proofs` job (manual dispatch) or
a plugged-in local run: launch, screenshot, `simctl push` of
`docs/evidence/native-ios/push.apns`, tap reaching the app's handler,
screenshots saved and looked at. None of that has happened.

### 5. host-android device — history (2026-11-07 text; superseded, PASS in the Plan audit above)

What is verified: the module and host assemble
(`docs/evidence/native-android/gradle.log`, `:app:assembleDebug` BUILD
SUCCESSFUL; standalone `native/android` module too), the merged manifest
carries `POST_NOTIFICATIONS`/`INTERNET`/`WAKE_LOCK` and the
`MESSAGING_EVENT` service (`docs/evidence/native-android/manifest-check.txt`),
and the packaged assets are the real demo bundle
(`docs/evidence/native-android/demo-bundle-check.txt`).

What is not: no emulator device proof. An emulator was booted locally
(`docs/evidence/native-android/emulator.log`, pid in
`docs/evidence/native-android/emulator.pid`) but the proof steps did not
complete before the battery pause — there is no logcat capture, no
screenshot of the running app, no token-failure observation, no
notification show/press flow. The proof moves to `device-proofs.yml`'s
`android-proofs` job or a plugged-in local run.

### 6. Analytics and Performance reach the collector — PASS (native logs captured in the Plan audit above)

Pure tier (local, before the pause):

- The collector sink ran (`bun tools/collector/server.ts`) and its `/collect`,
  `/assert` and `/slow` contract is proven: sink lines in
  `docs/evidence/perfmon/collector.jsonl`, the sink's unit tests in
  `docs/evidence/perfmon/collector-bun-test.log`, and the live sandbox pair in
  `docs/evidence/perfmon/sandbox-monitor.log` (a queued HTTP metric posted,
  `/assert` answered `ok:true`, and the `/slow?ms=900` route recorded a real
  duration).
- Analytics events reached the collector with its assertion green: the
  analytics sandbox test ("three events reach the collector and its
  assertion says ok") passes in the saved full run
  `docs/evidence/auth/test-emulator.log`. The earlier honest skip —
  collector not yet running, the test stops with "collector: unreachable" —
  is `docs/evidence/analytics/sandbox-skip.log`; later green runs supersede
  it. There is no separate `docs/evidence/analytics/collector-assert.log`
  file; `scripts/check-evidence.sh` still expects one (see the manifest
  note below).
- The demo's monitor measured the real live collector: see criterion 7.

Native debug-log assertion (criterion 6's second half — `-FIRDebugEnabled`
/ `-FIRAnalyticsDebugEnabled` on iOS, `adb shell setprop
debug.firebase.analytics.app` on Android): was unverified on 2026-11-07,
when the one iOS attempt crashed at launch. The 2026-11-19 runs captured
both logs (criterion 6 above).

### 7. Monitoring screen — PASS on macOS and both phone targets

`nsh test examples/demo/tests/perfmon-shots.test.tsx --target macos`
passed (`docs/evidence/perfmon/demo-shots.log`) and saved
`docs/evidence/perfmon/monitor-light.png` and `monitor-dark.png`; the
vision record `docs/evidence/perfmon/vision.md` describes both after a
person looked at them: the collector's slow route (`GET /slow`, ~1200 ms)
is visibly the first row, above `/collect`, `/assert`, `/health` at
single-digit milliseconds, with the All / >= 1 s / Errors filters shown.
Corroborated from the demo's own walk
(`docs/evidence/demo/macos-monitor.png`, runner output
`docs/evidence/demo/macos.log`, 2/2 passed): `/slow` at ~1230 ms on top of
`/collect` 31 ms, `/assert` 29 ms, `/health` 29 ms, all 200 — real calls
against the real collector on 127.0.0.1:18742.

The criterion also names a phone target. That shot (`--target ios` /
`--target android` run of the same test) was cancelled under the battery
constraint. It was run on 2026-11-19: `native-ios/monitor-phone.png` and
`native-android/monitor-phone.png` (criterion 7 above).

### 8. ERR_ACCESS_DENIED — PASS

Revoking network capabilities yields the clear, mapped error through the
facade: `packages/nsh-firebase/tests/core.test.tsx` (headless, the stub
route for the denied host never answers, error shown as
`core/access-denied (ERR_ACCESS_DENIED)`) and the
`crates/nsh-firebase-core` tests, plus a run on the real macOS target
where the platform's own capability check refuses the same request.
Evidence: `docs/evidence/core/access-denied.log`. The host-call refusal
(`ERR_ACCESS_DENIED` for undeclared `capabilities.host` names) is covered
by the same suite.

### 9. Migration note — PASS

`docs/migration-rnfirebase.md` maps each React Native Firebase call
(import names taken from the RNFB `lib/` sources) to its nsh-firebase
equivalent in TypeScript (`nsh-firebase/<area>` subpath) and Rust
(crate), with the tier differences stated per product: messaging is
native-tier-only and the pure tier rejects with `core/unavailable`;
analytics is a batched Measurement Protocol/collector queue, not the
native pipeline; perf traces are local aggregates with a pluggable
transport (no console); RTDB listeners are SSE, not the SDK socket;
Firestore and the other former stretch products are implemented (below)
with their remaining tier limits stated honestly.

## The whole stack (user direction, 2026-11-08) — IMPLEMENTED, CI-verified at unit level

After the initial build the user directed the whole Firebase stack: the
six former stretch products are no longer stubs. Each ships a pure tier
(Rust crate + TSX subpath, wire shapes verified by reading the
firebase-js-sdk compiled layers and the official REST references) and —
where the SDKs provide the product — native wrappers over the real
libraries: iOS Package.swift adopts the SDK's Firestore trait and links
all six 13.1.1 products; Android adds the six BoM artifacts
(firestore 27.1.0, config 24.0.1, crashlytics 21.0.1, installations
20.0.0, appcheck(+debug) 20.0.0, functions 23.0.0). Both hosts register
the 19 stretch handlers with identical wire shapes. Honest tier rows:
Firestore pure tier has no realtime over REST; Remote Config's fetch is
OAuth-gated (defaults + template parse ship; fetchAndActivate refuses
without a transport); Crashlytics' pure tier is a clearly-failing
facade (no honest pure path); Installations speaks the FIS REST (the
real endpoint refuses demo- projects; the refusal is the proof);
App Check ships the debug-token exchange with expiry caching; Functions
speaks the callable wire against the suite's Functions emulator
(ping/add served from emulator/functions). CI executes all of it:
run [42403998509](https://github.com/SuperNSH/firebase/actions/runs/42403998509)
is green across rust-gate (three OSes), every cross-target check,
packages, the emulator suite (firestore 18080 + functions 15001 sandbox
tests included), demo, clean-proof, host-android and host-ios (the
first macOS compile of all 35 handlers). What CI cannot see: the
stretch handlers' fake-project rejections on a device (needs a run),
Realtime-style listeners for Firestore on the pure tier (none exist to
honestly ship), and Crashlytics/App Check device attestor paths
(unverified-honest in their status docs).

### 10. README — PASS

`README.md` gets a person from a clean clone through prerequisites (bun
1.4+, Rust stable, nsh 0.6.0 from the upstream installer, JDK 17 for the
emulator suite), `bun install`, the one emulator-test command
`bun run test:emulator`, the demo (including the `node_modules` symlinks,
which are **not** committed — `.gitignore` excludes `node_modules/`),
`nsh run`, and both native hosts, with CI named as where the gates run.

## Audit rounds by area

Per the records in `docs/status/<area>.md` (the z-workflow cap is three
rounds; "cap reached" means the third round's findings were fixed and the
gates re-run green, not that findings were dropped):

| Area | Recorded audit-fix rounds | Outcome |
|---|---|---|
| research | 3 | converged; citations spot-checked |
| core | none recorded | converged early (committed `89c2304`); the storage-rules runtime bug was found cross-area and fixed (`8318a75`, `docs/evidence/core/storage-rules-fix.log`) |
| auth | 3 (second pass, round 2, round 3) | **cap-3-reached**; final-round findings fixed, gates green |
| database | none recorded | converged with fail-first verification only |
| storage | 1 auditor round | converged (auditor fixes verified fail-first) |
| messaging | auditor fixes + round 3 | **cap-3-reached**; final-round findings fixed, both gates re-run green |
| analytics | round 1 + round 2 | converged |
| perfmon | audit fix round + round 2 | converged |
| native-ios | none recorded | converged; the launch defect is fixed and the device run passed (criterion 4) |
| native-android | round 1 + round 2 | converged |
| demo | none recorded | converged; gates green (`docs/status/demo.md`) |

## Evidence manifest state

`bash scripts/check-evidence.sh` (the smoke gate's manifest) currently
fails with the expected MISSING list, honestly:

- `docs/evidence/demo/vision.md` — the demo's vision record lives in
  `docs/status/demo.md` prose instead of a file in the evidence dir.
- `docs/evidence/native-android/logcat.txt` and the device PNGs — belong
  to the cancelled Android device proof (criterion 5).
- `docs/evidence/analytics/collector-assert.log` — the analytics collector
  assertion's green run is inside `docs/evidence/auth/test-emulator.log`;
  no separate file was saved.
- `docs/evidence/native-ios/simctl-push.log` exists but records the
  failed attempt of criterion 4, not a pass.

## What was never run

- Anything on a booted iOS simulator or Android emulator past install:
  the device phases (criteria 4, 5) and the phone-target monitoring shot
  (part of criterion 7) were cancelled under the battery constraint.
- CI itself: no push has happened, so no workflow has run.
- The clean-checkout proof of criterion 3 (CI job `clean-proof`).
- Production Google endpoints: by design, never (no account, no
  credentials; the sandbox is the emulator suite plus `tools/collector`).
