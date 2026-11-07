# nsh-firebase

Firebase libraries for Native Shell apps, in Rust and TypeScript. It has
two tiers that share one API shape:

- **Pure tier** — crates (`crates/nsh-firebase-*`) and a TypeScript package
  (`packages/nsh-firebase`, subpath imports) over NSH's `fetch` and
  WebSocket: Auth over Identity Toolkit, Realtime Database over REST with
  SSE listeners, Storage with resumable uploads, Analytics as a batching
  queue, Performance traces and a network monitor. Works on every target,
  including `nsh package` apps.
- **Native tier** — `native/ios/NshFirebase` (Swift package over the
  Firebase Apple SDK) and `native/android` (Gradle module over the Firebase
  BoM), registered as `firebase.*` handlers by the embedding hosts in
  `examples/host-ios` and `examples/host-android`. This is what gives FCM
  tokens and messages, the native Analytics pipeline and automatic
  Performance capture.

Everything runs in a sandbox: the Firebase Local Emulator Suite with a
`demo-` project and fake config fixtures. No Firebase account, no real
credentials, no billing.

Status and the per-criterion evidence live in
[docs/STATUS.md](docs/STATUS.md).

## Prerequisites

- [bun](https://bun.sh) 1.4 or newer (Node is not needed; bun runs
  everything, including the collector).
- Rust stable (`rustup`) — the crates and the Rust half of the emulator
  tests are cargo builds. iOS/Android cross-compilation targets are only
  needed for the native hosts.
- `nsh` 0.4.0, exactly — do not upgrade:
  ```sh
  brew tap longbridge/tap && brew install longbridge/tap/nsh
  ```
- A JDK 17 for the emulator suite (the Storage emulator's rules runtime
  needs it). `/usr/bin/java` is a stub on macOS; if you have Android
  Studio, the start script falls back to its bundled JBR when `JAVA_HOME`
  is unset.
- Only for the native hosts: Xcode + `xcodegen` (iOS), the Android SDK
  (`ANDROID_HOME`) and Gradle 9.3.1 (Android).

## Emulator tests

```sh
bun install
bun run test:emulator
```

One command. It starts the emulator suite itself if it is not running
(project `demo-nsh`: auth on 19099, database on 19000, storage on 19199),
then runs the labelled sandbox tests in both languages: sign up, sign in,
refresh, restore a persisted session, database write/read and a live
update arriving over SSE, an upload with progress, download, delete, and
analytics/monitor sends asserted at the local collector. The collector
(`tools/collector`, port 18742) must be running for the last pair:

```sh
bun tools/collector/server.ts
```

Other scripts: `bun run gate:rust` (fmt + clippy + test for all crates),
`bun run gate:ts` / `bun run test:packages` (the package TSX gates),
`bun run test:demo` (demo TSX gate plus the Rust demo crate's tests),
`bun run emulator:start` (suite only), `bun run check:evidence`
(evidence manifest). CI runs all of them — see
[.github/workflows/ci.yml](.github/workflows/ci.yml).

## Demo

`examples/demo` is a TSX app (auth, database, storage, messaging,
analytics and the monitoring screen); `examples/demo-rs` is the same in
Rust. The apps consume the libraries through `node_modules` symlinks,
which are **not** committed (`.gitignore` excludes `node_modules/`):

```sh
cd examples/demo
mkdir -p node_modules
ln -sfn ../../packages/nsh-firebase node_modules/nsh-firebase   # required
ln -sfn ~/fo/nsh-form/packages/nsh-form node_modules/nsh-form   # required (auth screen)
ln -sfn ~/fo/nsh-query/packages/nsh-query node_modules/nsh-query # optional — linked, not imported today
```

`nsh-form`/`nsh-query` are sibling libraries expected at `~/fo/`; adjust
the paths if yours live elsewhere. Then:

```sh
bun tools/collector/server.ts &   # the monitor and Rust analytics screens measure it
nsh run --target macos
```

The Rust demo (`examples/demo-rs`) builds its clients from the crates and
path-depends on the same siblings (`../../../nsh-form/crates/nsh-form`,
`../../../nsh-query/crates/nsh-query` relative to the repo layout at
`~/fo`); `cargo test -p demo-rs` runs its tests, `nsh run` in that
directory starts it.

## Native hosts

Both hosts embed the demo bundle and register the same `firebase.*`
handlers. Config comes from the fake fixtures in `fixtures/`
(`GoogleService-Info.plist`, `google-services.json`) — token requests are
meant to fail loudly against them; that failure is the sandbox's proof.

### iOS (examples/host-ios)

```sh
# 1. bundle the demo into the host (App/nsh is not committed; step 1 is required)
cd examples/demo && nsh build App.tsx --release --out ../host-ios/App/nsh

# 2. regenerate the project after changing project.yml or sources
cd ../host-ios && xcodegen generate

# 3. build
xcodebuild -project NshFirebaseHost.xcodeproj -scheme NshFirebaseHost \
  -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath build build
```

`project.yml` points `NSH_EMBED_IOS` at the Homebrew nsh embedding
framework (`/opt/homebrew/opt/nsh/libexec/embed/ios`); override it if your
prefix differs. SwiftPM fetches the pinned firebase-ios-sdk 13.1.1 on
first resolve — several hundred MB. Known issue: the host currently
crashes at launch on the iOS 27 simulator (scene-lifecycle requirement);
see criterion 4 in [docs/STATUS.md](docs/STATUS.md) before investing time
in a run.

### Android (examples/host-android)

```sh
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME="$HOME/Library/Android/sdk"   # else "SDK location not found"
cd examples/demo && nsh build App.tsx --release --out ../host-android/app/src/main/assets/nsh
cd ../host-android
gradle :app:assembleDebug        # Gradle 9.3.1; these projects carry no wrapper
adb install app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.nsh.firebase.host/.MainActivity
```

The module (`native/android`) compiles against `libs/nsh.aar` copied from
`$(brew --prefix nsh)/libexec/embed/android/nsh.aar`; `native/android/libs/nsh.aar`
and `app/libs/nsh.aar` are the same file and must be replaced together on
an nsh upgrade.

Full platform setup — fixture contents, delegate wiring, the
google-services/BoM/plugin equivalents, notification channel,
`POST_NOTIFICATIONS`, `nsh.aar` provenance — is in
[docs/setup.md](docs/setup.md).

## Documentation

- [docs/STATUS.md](docs/STATUS.md) — what is verified, how, and what is not.
- [docs/setup.md](docs/setup.md) — per-platform native setup.
- [docs/migration-rnfirebase.md](docs/migration-rnfirebase.md) — coming
  from React Native Firebase: every call mapped.
- [docs/research.md](docs/research.md) — the upstream study and the
  per-product tier table.
- [docs/status/](docs/status) — per-area records; [docs/evidence/](docs/evidence)
  — logs and screenshots.
