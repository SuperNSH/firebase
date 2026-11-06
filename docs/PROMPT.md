# Build nsh-firebase

## Goal

Make `nsh-firebase`: the Firebase libraries for Native Shell apps, in Rust and TypeScript, with the per-platform native integration that React Native Firebase and FlutterFire provide. Cover at least Core, Authentication, Realtime Database, Cloud Storage, Cloud Messaging with notifications, Analytics, Performance Monitoring and network monitoring. Then add Firestore, Remote Config, Crashlytics, Installations, App Check and Cloud Functions, in that order, once those work. All of it is proven in a sandbox, with no Firebase account.

The most important results are Performance Monitoring and Analytics, network monitoring that shows which calls take what time, Authentication, and Messaging.

This repository holds only directories, `.agents/skills`, `.zcode` and this prompt. You build everything else.

## Study first

Read how the references do it, per platform, and write `docs/research.md` with file paths and what you take from each. Clones are at `~/fo/.refs/react-native-firebase` and `~/fo/.refs/flutterfire`. Starting points, already checked to exist:

- React Native Firebase: `packages/{app,auth,database,storage,messaging,analytics,perf,firestore,crashlytics,remote-config,installations,app-check,functions}`. Each has `ios/RNFB*/`, `android/src/main/java/io/invertase/firebase/*`, and its JS in `lib/`. `packages/app/RNFBApp.podspec`, `firebase_json.rb` and `firebase_spm.rb` show how SDK versions are pinned on iOS, and `packages/app/android/build.gradle` with `firebase-json.gradle` does so on Android, with a BoM. `packages/app/plugin/src/{android,ios}` copy `google-services.json` and `GoogleService-Info.plist` and wire the Gradle plugin. `packages/messaging/ios/RNFBMessaging/RNFBMessaging+{AppDelegate,FIRMessagingDelegate,UNUserNotificationCenter}.*` show how delegates are taken over and forwarded. `packages/messaging/android/.../ReactNativeFirebaseMessagingService.java`, `HeadlessService`, `Receiver` and `Store` show background delivery. `packages/perf/lib/HttpMetric.ts` and `UniversalFirebasePerfModule.java` show HTTP metrics.
- FlutterFire: `packages/firebase_*/` is federated, with `firebase_x`, `firebase_x_platform_interface` and `firebase_x_web`. `firebase_core/firebase_core/windows/` uses the C++ SDK. `ios/firebase_*/Package.swift` with a podspec shows both Apple package managers. Pigeon generates its messages. `firebase_messaging` shows background isolates and token refresh.

Also check the Firebase REST surfaces: Identity Toolkit and Secure Token for Auth, the Realtime Database REST API with Server-Sent Events, the Cloud Storage for Firebase REST paths the SDKs use, and the GA4 Measurement Protocol. Search for current official docs and cite them. Answer in `docs/research.md`: for each product on each target, what is the honest path, and what cannot be done.

## The shape it will likely take

Test this against what you find, and change it if the evidence says so. Write down why.

NSH has two facts that decide the design. `nsh package` has no setting for extra native SDKs, so the Firebase Apple and Android SDKs only link through an embedding host. And a TSX app runs in QuickJS with no crate of its own. So there are two tiers:

1. **Pure tier, every target, including `nsh package` apps, desktop and web.** Rust crates (`crates/nsh-firebase-core`, `-auth`, `-database`, `-storage`, `-analytics`, `-perf`) and a TypeScript package (`packages/nsh-firebase`, with subpaths) over NSH's `fetch` and WebSocket. Auth over Identity Toolkit, with token refresh. Realtime Database with REST and live listeners over SSE. Storage with resumable upload and progress. Analytics by a batching queue with a pluggable transport, including GA4 Measurement Protocol, with the limits of that documented. Performance traces and HTTP metrics, in a local aggregate with a pluggable transport. These results are signals, and they integrate with `nsh-query`. Same behavior in Rust and TypeScript, as `~/fo/nsh-query` does.
2. **Native tier, iOS and Android hosts.** `native/ios/NshFirebase` (a Swift package over the Firebase Apple SDK) and `native/android` (a Gradle module with the Firebase BoM), which register `firebase.*` handlers on `NshAppView` and `NshView`. These give what only the SDKs give: FCM and APNs tokens, background and foreground messages, notification taps, the native Analytics pipeline, automatic Performance traces and network capture. The app calls them through one facade whatever the tier, using `host.call`, `host.post`, `host.on`, and `capabilities.host`. Rust apps reach them through a small trait with an implementation per host. `examples/host-ios` and `examples/host-android` are real hosts, modelled on `~/fo/.refs/nsh-v0.4.0/examples/embed-ios` and `embed-android`.

Include the plugin-like setup step: `tools/` or `docs/setup.md` covering what the Expo plugin automates for RN Firebase, meaning config files, the Gradle plugin, delegates and entitlements.

## Notifications

0.4.0 has no notifications API. `~/fo/.refs/nsh-main` has a `Notifications` module in `nsh` (`permission`, `requestPermission`, `show` by id, `close`, `setBadge`, `on("press")`), unreleased, with native parts in `crates/nsh-platform-apple/src/notifications.rs` and `crates/nsh-platform-android/java/com/longbridge/nsh/Notifications.java`. Read them. Do not build a general local-notifications library. Match those names in the messaging facade, so moving to the released module later is one small change. Show a received FCM message through the host's `UNUserNotificationCenter` and `NotificationManager` in the native tier.

## Network monitoring

This is a first-class deliverable, and a separate project, `nsh-http`, is building the app's HTTP client at the same time. Do not depend on it. Both projects use this exact event shape, which your perf package consumes:

```ts
interface NetworkEvent {
  id: string;
  method: string;
  url: string;
  host: string;
  path: string;
  status: number | null;
  startedAtMs: number;
  durationMs: number;
  ttfbMs: number | null;
  requestBytes: number | null;
  responseBytes: number | null;
  error: string | null;
  initiator: string | null;
}
```

Provide a monitor with a ring buffer, per-host and per-path aggregates (count, error rate, p50, p95), an instrumented `fetch` wrapper in TypeScript and Rust, HTTP metrics from these events into Performance Monitoring, and a screen in the demo that shows which calls took how long, sorted and filterable. The native tier adds the SDK's automatic capture where it exists, and the docs say honestly what each path can and cannot see.

## Areas

Disjoint files:

1. **core**: app and options, config loading, emulator switches, the shared error type, the event bus, the tier facade.
2. **auth**: email and password, anonymous, custom token, `signInWithCredential` taking an ID token from another library, refresh, `currentUser` as a signal, persistence through a storage trait.
3. **database**: refs, get, set, update, remove, push, transactions, queries, and live listeners, with reconnect and offline behavior documented.
4. **storage**: upload with progress and resume, download URL, metadata, list, delete.
5. **messaging**: native tier on iOS and Android, token, refresh, permission, foreground and background and opened-from-notification handlers, topics, and a pure-tier stub that fails clearly.
6. **analytics**: events, user properties, screen views, consent, batching, debug view, native and pure.
7. **performance and network monitoring**: traces, HTTP metrics, the monitor, the demo screen, native automatic capture.
8. **native-ios** and 9. **native-android**: the packages and hosts, and the setup docs.
10. **demo**: `examples/demo`, a TSX app and a Rust app showing each product. Use the `nsh-component-library` rules for its look, and `nsh-query` and `nsh-form` for its state and forms.
11. **stretch**, only once criteria 1-8 hold: Firestore, Remote Config, Crashlytics, Installations, App Check and Functions, each with an honest tier table.

## Sandbox: no account needed

- Use the Firebase Local Emulator Suite through `firebase-tools` as a dev dependency, in `emulator/firebase.json`, with a project id starting `demo-`, such as `demo-nsh`. A `demo-` project needs no login and no credentials. Set `JAVA_HOME` as above. Auth, Realtime Database, Firestore, Storage and Functions all run this way, and tests run against them with `emulators:exec`.
- Use fake `GoogleService-Info.plist` and `google-services.json` fixtures, with obviously fake values. The native SDKs initialize with them.
- Messaging has no emulator. Prove what can be proved: use `xcrun simctl push` with an APNs payload on the simulator, and `adb shell` broadcasts or direct service calls on the emulator, to prove foreground, background and tap handling all reach the app. Prove the token request is made, and that its failure with a fake project is surfaced as a clear error. A real token is not required.
- Analytics and Performance have no emulator. `tools/collector` is a small local HTTP sink for the pure tier's transports, which asserts what was sent. For the native tier, use the SDK's debug logging, `-FIRDebugEnabled`, `-FIRAnalyticsDebugEnabled`, `adb shell setprop debug.firebase.analytics.app`, and assert on the logged events. A real dashboard is not required.

## Satisfaction criteria

All of these hold, each with evidence in `docs/STATUS.md`:

1. `docs/research.md` exists, cites both references by file path and has the tier table, for every product and target.
2. Gates pass for all crates and packages.
3. With the emulator suite running under `demo-nsh`, tests in Rust and TypeScript sign up, sign in, refresh, read and write the database, get a live update through a listener, upload a file with progress, download it, and delete it. These pass from a clean checkout with one documented command.
4. `examples/host-ios` builds with `xcodebuild`, runs on a simulator and shows the demo. The Firebase SDK initializes against the fake plist. `xcrun simctl push` shows a notification, and tapping it reaches the app's handler. Screenshots saved and looked at.
5. `examples/host-android` builds with Gradle, runs on the emulator, and does the same with its service, including from the background. Screenshots saved and looked at.
6. Analytics events and Performance traces from the demo reach `tools/collector`, and on each native host appear in debug logs. Saved logs.
7. The monitoring screen, in the demo on macOS and a phone target, shows real per-call timings against a local server with a deliberately slow route, and the slow route is visibly on top. Screenshot saved and looked at.
8. Revoking network capabilities gives a clear `ERR_ACCESS_DENIED` through the facade, with a test.
9. A migration note compares the API with React Native Firebase, so someone who knows it can find each call.
10. `README.md` gets a person from a clean clone to the emulator tests and a running demo.

## Environment, as measured on this Mac

- `nsh` 0.4.0 (e626a8d), from Homebrew. Do not upgrade it. Embedding frameworks: `$(brew --prefix nsh)/libexec/embed/ios/{NativeShell,NativeShellDev}.xcframework` and `$(brew --prefix nsh)/libexec/embed/android/{nsh,nsh-dev}.aar`.
- Rust stable with `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android` and `wasm32-unknown-unknown` installed.
- Xcode with an iOS 27 simulator runtime. Android SDK at `$ANDROID_HOME`, NDK 29, one AVD named `indopak-audit`.
- `/usr/bin/java` is a stub with no runtime behind it. Use Android Studio's JDK: `export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"`.
- `bun` and `node` 24 exist. Installing dev tools with them inside this repository is fine.
- Reference source, read-only: `~/fo/.refs/nsh-v0.4.0` is exactly what is installed, including its `examples/` (`gallery`, `gallery-rs`, `embed-ios`, `embed-android`, `embed-gpui`, `rust-exports`, `host-echo`). `~/fo/.refs/nsh-main` is unreleased. `~/fo/.refs/react-native-firebase` and `~/fo/.refs/flutterfire` are the Firebase references.
- Sibling libraries, already built and tested by someone else: `~/fo/nsh-query` and `~/fo/nsh-form`. Each has a Rust crate and a TypeScript package with the same behavior. Read their READMEs. Do not edit them. If one has a bug, write it up in `docs/upstream-issues.md`, and work around it.
- Installed for this repository: the `z-nsh-engineer` agent and the skills under `.agents/skills`, by `z-project-init`. Start with `nsh`, `nsh-testing` and `nsh-design-guides`.

## Tools you may use to look at the real thing

You have full access to this machine. Use it. A headless pass alone never finishes visible or native work.

- `nsh test` headless, and `nsh test --target macos|ios|android|web`, which use real input, and `app.screenshot(path)`.
- `nsh run --target ios|android|macos|web`, `nsh package --target ...`, `xcodebuild`, `xcrun simctl`, Gradle, `adb`, and the Android emulator.
- The computer-use, ios-simulator and android-emulator plugins, the `mobile-mcp` server, `mobilecli`, the `mobile-automation` and `open-computer-use` skills for live capture, and `agent-browser` for the web.
- Save evidence as PNGs and logs under `docs/evidence/<area>/`, and look at every PNG you save. A screenshot nobody looked at proves nothing.

## This machine is shared right now

Other agents are building `nsh-starter`, `nsh-firebase`, `nsh-http` and `nsh-auth` on this Mac at the same time, one per repository. Do not interfere with them.

- Stay inside this repository. Reading `~/fo/.refs`, `~/fo/nsh-query` and `~/fo/nsh-form` is fine. Never write to another repository.
- Use your own devices, named after this repository: `xcrun simctl create <repo> "<a current iPhone>"` and refer to it by UDID, and `avdmanager create avd -n <repo> ...` for your own AVD, with your own `-port`, and `adb -s <serial>` on every call. Never run `xcrun simctl shutdown all`, `xcrun simctl erase all`, `adb kill-server`, `killall`, or anything that stops a process you did not start.
- Pick your own ports for dev servers and local servers with `--port 0` or a free port. Never kill whatever holds a port.
- Prefer capture that does not take over the screen: `xcrun simctl io <udid> screenshot`, `adb -s <serial> exec-out screencap -p`, `nsh test --target ... ` with `app.screenshot(path)`. Use whole-screen computer-use only when nothing else can do the step, and keep it short.
- If a build is slow because the machine is busy, wait. Do not lower a check because of it.

## Facts that bite on 0.4.0

Each of these was hit by a real run. Trust them over the docs.

- The upstream `nsh` skill follows NSH's `main` and is ahead of 0.4.0. Not in 0.4.0: `nsh update-manifest`, `nsh/updater`, the `Notifications` module, and the `notifications` capability. Check `.nsh/types` for TSX and the crate source in `~/.cargo/registry/src/*/nsh-core-0.4.0` for Rust.
- Real capability keys, from the build's own error: `network`, `webview`, `statusBar`, `clipboard`, `picker`, `speech`, `openUrl`, `process`, `fs`, `host`. A typo fails `nsh build` and passes `nsh check`. Run both.
- `If` and `Each` are globals in TSX. Importing them from `nsh` is a type error.
- `nsh check` needs an `App.tsx` at the package root, or an entry passed to it. Generated files in `.nsh/` are git-ignored, and `nsh` writes them.
- The docs' `nsh-engine = "0.1"` samples are stale. Dev-dependencies must be `0.4.0`, as all NSH crates must come from one place. The generated `Cargo.toml` has no `log` crate.
- `Headless::start` can return before the first frame, so `wait_for` something first or a step sees `on screen: []`.
- `Headless::shows` takes plain text. `wait_for`, `press` and TypeScript's `app.shows` take selectors. `wait_for_gone` also takes a timeout. `screen().count` returns a `Result`.
- `Signal::update` needs `T: Clone`. For state machines holding user types, pair `stored(..)` with a version `signal`. See `~/fo/nsh-query/crates/nsh-query/src/tracked.rs`.
- A component's signals are already disposed when its `cleanup` runs, so reading one there panics. Capture what cleanup needs in an `Rc<Cell<..>>`. `create_root` gives an owner that outlives the component, which suits caches and in-flight writes.
- `open`'s host handlers need annotated parameters: `async ({ id }: { id: number }) => ...`. Selector regexes use Rust's `regex` syntax, so no look-ahead. On a manual clock, `open(entry, { clock: {} })`, a test that took 13.7 s took 18 ms.
- A TSX app consumes a sibling package through `node_modules`, with `mkdir -p node_modules && ln -s <path> node_modules/<name>`. The bundler and `nsh check` both read it, and `tsconfig.json` needs no change. Overriding `compilerOptions.paths` there breaks the `nsh` types.
- `nsh package` needs no Xcode project and no Gradle, and has no setting for extra native SDKs, URL schemes or entitlements beyond the provisioning profile. Anything needing those goes through an embedding host: `NshAppView` with `app.handle(..)` on iOS, and `NshView` with `nsh.handle(..)` on Android, as `examples/embed-ios` and `examples/embed-android` do. The app talks to it with `host.call`, `host.post` and `host.on`, and must list the names in `capabilities.host`.
- Failed host calls reject with `ERR_ACCESS_DENIED` when undeclared, `ERR_NSH_HOST_ERROR` when unhandled or thrown, and `ERR_FEATURE_UNAVAILABLE_ON_PLATFORM` with no host.
- Bytecode loads only from the app bundle, the APK or a verified package.

## How to run this

Use the `z-workflow` skill. All subagents run on GLM-5.3-Flash with high thinking. Split the work into the areas named below, with disjoint files, each advancing alone through its own chain. Name these phases for every area: implement, audit, unit tests. Add native device testing to any area touching iOS, Android or desktop, and a vision phase to any area with a screen. End with a whole-tree smoke gate. Run audit and smoke until zero issues, capped at three rounds, and report honestly if a cap is reached rather than weakening a check.

Set this session's goal to the goal above, and keep working until the satisfaction criteria hold or you are truly blocked. When you are blocked on one area, finish every other area first. Do not stop to ask whether to continue.

## Hard rules

- No real accounts, no real credentials, no billing, and no signing in as anyone. Never type a password or create an account on any provider's site. Everything here must work in a sandbox, as described below.
- Never commit a secret, a real `google-services.json` or `GoogleService-Info.plist`, or a real client secret. Fixtures with obviously fake values go under `fixtures/`.
- Do not publish to crates.io or npm, do not change repository visibility, and do not push. Commit on `main` as each milestone passes, using Conventional Commits with the reason in the body.
- Comments are 1-2 lines and state a non-obvious constraint. Never narrate the next line. Dead code is deleted. No speculative abstractions.
- Docs read like a competent human wrote them: direct, concrete, no marketing filler. The `antislop-copy` and `code-comments` skills apply.
- Never invent an API. Look the name up, in the source for 0.4.0.
- A Rust gate is `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`. A TSX gate is `nsh check`, `nsh build` with every warning fixed, and `nsh test`. No check is weakened, skipped or deleted to get a pass.
- Tests go through the UI on a manual clock, with no real network and no real time, except the labelled sandbox runs below. Every new test is seen to fail once, by breaking the code on purpose. Use `test-quality-guardrails`.
- `docs/STATUS.md` stays current with what is done, what was verified how, and what was not verified. A claim with no evidence is reported as unverified.
