# STATUS

The honest ledger [PROMPT.md](PROMPT.md) asks for: what holds, what does
not, and the evidence behind each claim. A claim with no evidence path is
reported as unverified. Everything here was true on 2026-11-07.

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

Commit state: three milestones are committed — research `08c5aef`, core
`89c2304` and the storage-rules fix `8318a75`. The other areas' files
(crates, package, hosts, demo, evidence, status docs, workflows) sit in
the working tree, verified per area before the pause, pending commit.
**CI has not run yet** — nothing has been pushed (the hard rules forbid
pushing), so every "CI confirmation expected" below is an expectation,
not a result.

## Satisfaction criteria

| # | Criterion | Status |
|---|---|---|
| 1 | research.md with tier table | PASS |
| 2 | Gates pass for all crates and packages | pass locally pre-pause; CI confirmation expected on push — CI has not run |
| 3 | Emulator tests, one documented command | PASS locally; clean-clone proof unverified until CI |
| 4 | host-ios device proof | UNVERIFIED — device phase cancelled; a first attempt hit a launch crash (recorded) |
| 5 | host-android device proof | UNVERIFIED — device phase cancelled |
| 6 | Analytics/perf reach collector; native debug logs | pure tier PASS locally; native debug-log assertion UNVERIFIED |
| 7 | Monitoring screen with slow route on top | PASS on macOS; phone target UNVERIFIED |
| 8 | ERR_ACCESS_DENIED with a test | PASS |
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

**CI has not run.** Jobs `rust-gate`, `packages`, `emulator`, `demo`,
`clean-proof`, `host-ios`, `host-android` re-run the whole tree on push;
that confirmation is still outstanding.

### 3. Emulator tests — PASS locally; clean-clone proof unverified until CI

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

The clean-checkout proof (fresh clone, `bun install`, one command) needs
a clone-plus-compile and was never run locally under the battery
constraint — it is CI job `clean-proof`. **Unverified until CI.**

### 4. host-ios device — UNVERIFIED (device phase cancelled; launch crash recorded)

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
**This is an open defect to fix before the proof can pass**, then the
proof runs as `device-proofs.yml`'s `ios-proofs` job (manual dispatch) or
a plugged-in local run: launch, screenshot, `simctl push` of
`docs/evidence/native-ios/push.apns`, tap reaching the app's handler,
screenshots saved and looked at. None of that has happened.

### 5. host-android device — UNVERIFIED (device phase cancelled)

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

### 6. Analytics and Performance reach the collector — pure tier PASS; native UNVERIFIED

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
debug.firebase.analytics.app` on Android): **UNVERIFIED**. It belongs to
the cancelled device phases, and the one iOS attempt crashed at launch
before Firebase could log anything.

### 7. Monitoring screen — PASS on macOS; phone target UNVERIFIED

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
constraint — **UNVERIFIED**, pending CI or a plugged-in run.

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
transport (no console); RTDB listeners are SSE, not the SDK socket; no
Firestore in the core scope — the stretch products are stubs that say so.

### 10. README — PASS

`README.md` gets a person from a clean clone through prerequisites (bun
1.4+, Rust stable, nsh 0.4.0 from `longbridge/tap`, JDK 17 for the
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
| native-ios | none recorded | converged; open device-phase defect (criterion 4) is not an audit finding |
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
