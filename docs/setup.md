# Native setup: iOS and Android

What React Native Firebase's Expo config plugin automates, done by hand
for this repository's embedding hosts. This document merges and
supersedes `docs/setup-ios.md` and `docs/setup-android.md` (both kept for
reference). Applies to `native/ios/NshFirebase` + `examples/host-ios` and
`native/android` + `examples/host-android`.

This repo owns its hosts, so nothing needs to be rewritten behind our
backs — the plugin's steps are done once, by hand, in the host projects.
The plugin exists because a React Native app's build must be amended from
the outside: config files copied, Gradle wiring injected, delegates and
entitlements added. An embedding host we compile ourselves can do all of
that directly, visibly, in its own sources.

**The fixtures are fake.** `fixtures/GoogleService-Info.plist` (project
`demo-nsh`, API key `fake-ios-key-abc123`, app id
`1:123456789012:ios:abc123def456`) and `fixtures/google-services.json`
(project `demo-nsh`, project number `123456789012`, API key
`fake-android-key-abc123`, app id `1:123456789012:android:abc123def456`,
package name `dev.nsh.firebase.host`) initialize the SDKs locally.
Anything that calls Google's network fails, and that failure is the
point: a token request that fails loudly is the sandbox's proof that the
request was made. Never replace them with real files, and never commit a
real plist or `google-services.json`.

## iOS

### What the Expo plugin automates, and the manual equivalent

The reference automation is
`~/fo/.refs/react-native-firebase/packages/app/plugin/src/ios/`.

1. **Copy the plist into the source root and register it as a resource**
   (`plugin/src/ios/googleServicesPlist.ts` copies the file and calls
   `addResourceFileToGroup`). Here: `project.yml` lists
   `../../fixtures/GoogleService-Info.plist` with `buildPhase: resources`,
   so the fixture is the bundle resource — one copy, no drift. Runtime:
   `Bundle.main.url(forResource: "GoogleService-Info", withExtension: "plist")`
   -> `NshFirebase.configure(from:)`, which builds
   `FirebaseOptions(contentsOfFile:)` and calls
   `FirebaseApp.configure(options:)` (RNFB's AppDelegate step).
2. **AppDelegate wiring** (`plugin/src/ios/appDelegate.ts` injects
   `import FirebaseCore` and `FirebaseApp.configure()`). Here the
   AppDelegate is ours (`examples/host-ios/Sources/main.swift`), so there
   is no injection: it calls `registerForRemoteNotifications()`, hands the
   APNs token to `NshFirebase.shared.setAPNSToken`, and forwards
   `didReceiveRemoteNotification` to `NshFirebase.shared.onRemoteMessage`.
   With `FirebaseAppDelegateProxyEnabled = false` in Info.plist nothing
   swizzles in between — the explicit counterpart of RNFB's
   `GULAppDelegateSwizzler` interceptors.
3. **Podfile / dependency management** (`plugin/src/ios/podfile.ts`;
   Firebase 13+ is SPM-only, RNFB now rejects `disableSPM`). Here: no
   CocoaPods at all. `native/ios/NshFirebase/Package.swift` pins
   `firebase-ios-sdk` **13.1.1**, exact (the SPM pin RNFB ships in
   `packages/app/ios/RNFBFirebase/Package.swift`) with empty traits (no
   Firestore, so no gRPC/abseil), and `project.yml` declares it as a local
   package dependency next to `NativeShell.xcframework` from the `nsh`
   Homebrew install.
4. **Delegates** — RNFB's
   `packages/messaging/ios/RNFBMessaging/RNFBMessaging+{AppDelegate,FIRMessagingDelegate,UNUserNotificationCenter}.m`
   swap `FIRMessaging.delegate` and `UNUserNotificationCenter.delegate`
   for themselves, keep the previous delegates, and forward.
   `NshFirebase.configure(from:)` does the same swap natively: it stores
   whatever delegate was set before it and forwards
   `willPresentNotification` and `didReceiveNotificationResponse` to it,
   emitting `firebase.messaging.*` events on the way.
5. **Entitlements (push)**: the plugin's RNFB docs ask for the Push
   Notifications capability and background modes. Here `project.yml`
   generates `aps-environment: development` and
   `UIBackgroundModes: [remote-notification]`. Limits: a real APNs
   transport needs a real Apple developer account and Firebase project,
   which this sandbox never has. On the simulator the plumbing is proven
   with `xcrun simctl push <udid> payload.apns`; a fake project makes the
   token request fail loudly instead of producing a token.

### Build and run

```bash
# 1. the bundle the host embeds (App/nsh; mini-app is the fallback probe app)
cd examples/demo && nsh build App.tsx --release --out ../host-ios/App/nsh

# 2. regenerate the Xcode project (needed after project.yml or sources change)
cd ../host-ios && xcodegen generate

# 3. build
xcodebuild -project NshFirebaseHost.xcodeproj \
  -scheme NshFirebaseHost -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath build build
```

`NSH_EMBED_IOS` in `project.yml` points at the Homebrew nsh embedding
framework (`/opt/homebrew/opt/nsh/libexec/embed/ios`); override it there
if your prefix differs. SwiftPM fetches the Firebase SDK on first resolve
— several hundred MB of binary frameworks, be patient. `ARCHS=arm64`
exists because `NativeShell.xcframework` ships an arm64 simulator slice
only.

Known issue, recorded in [STATUS.md](STATUS.md) criterion 4: this host
crashed at launch on the iOS 27 simulator — `UIScene life cycle is
required for apps built with this SDK` (`Info.plist` has no
`UIApplicationSceneManifest`; `Sources/main.swift` uses legacy
`UIApplicationMain`). Evidence: `docs/evidence/native-ios/simctl-push.log`.
Fix that before expecting a proof run to pass.

## Android

### What RNFB's Expo plugin automates

RNFB's plugin
(`~/fo/.refs/react-native-firebase/packages/app/plugin/src/android`):

- `copyGoogleServices.ts`: copies the app's `google-services.json` to
  `app/google-services.json` (`expo.android.googleServicesFile` is the
  source, `DEFAULT_TARGET_PATH = 'app/google-services.json'`).
- `buildscriptDependency.ts`: appends
  `classpath 'com.google.gms:google-services:<version>'` to the
  project-level `build.gradle`'s `dependencies` block.
- `applyPlugin.ts`: appends `apply plugin: 'com.google.gms.google-services'`
  to `app/build.gradle`.
- `constants.ts` takes the plugin version from
  `sdkVersions.android.gmsGoogleServicesGradle`.

RNFB's `packages/app/android/build.gradle` separately reads
`sdkVersions.android.firebase` and declares
`implementation platform("com.google.firebase:firebase-bom:$bom")`; every
product module adds its artifacts without versions so the BoM decides.

### What this repository does instead

No Expo, no google-services Gradle plugin. The plugin's build-time step
(parsing `google-services.json` into resources) is replaced by a runtime
step: `NshFirebase.init(Context)` builds `FirebaseOptions` from the
host's `assets/google-services.json` (parsed with `org.json`), falling
back to the fake fixture values baked into `NshFirebase.defaultOptions()`.
This needs no real credentials at any point — the google-services plugin
would happily accept the fake fixture too, but the runtime options
approach keeps the whole setup visible in one place and works without
touching the host's build.

The steps, one by one:

1. **Config file.** Copy the fixture into the host's assets — exactly
   what `copyGoogleServices.ts` does, minus Expo:
   `cp fixtures/google-services.json examples/host-android/app/src/main/assets/google-services.json`.
   The values are fake (see above); they initialize the SDK locally and
   everything that talks to Google fails loudly.
2. **BoM and artifacts.** `native/android/build.gradle` pins
   `com.google.firebase:firebase-bom:35.1.0` and adds `firebase-messaging`,
   `firebase-analytics`, `firebase-perf` with no versions. Under this BoM
   the host resolves firebase-messaging 26.0.1, firebase-analytics 24.0.0
   and firebase-perf 23.0.1. The module compiles against `libs/nsh.aar`
   with `compileOnly` — the host already packages the AAR, and two copies
   would clash at dex time. The host includes the module:
   `implementation project(':nshfirebase')`, with
   `project(':nshfirebase').projectDir = file('../../native/android')` in
   `settings.gradle` so the module exists in one place.
3. **Service and manifest entries.** `native/android/src/main/AndroidManifest.xml`
   declares `NshFirebaseMessagingService` with the
   `com.google.firebase.MESSAGING_EVENT` intent filter (`exported=false`),
   `INTERNET`/`ACCESS_NETWORK_STATE`/`WAKE_LOCK`, and the
   `com.google.firebase.messaging.default_notification_channel_id`
   meta-data — modelled on RNFB's
   `packages/messaging/android/src/main/AndroidManifest.xml`. The manifest
   merger applies all of it to the host. RNFB's extra c2dm receiver +
   `HeadlessJsTaskService` pair (see below) is replaced by a queue.
4. **Notification channel.** Created at runtime by `NshFirebase.init`
   (and idempotently before every `show`) as
   `dev.nsh.firebase.notifications`, `IMPORTANCE_HIGH`, badge on —
   following nsh's unreleased `Notifications` module. The manifest
   meta-data points FCM's own auto-posts (background notification
   messages, which never reach the service) at the same channel id, so
   creating it in `init` keeps those posts out of FCM's fallback channel.
5. **POST_NOTIFICATIONS.** Declared in the host's manifest
   (`examples/host-android/app/src/main/AndroidManifest.xml`). Without it,
   an Android 13+ device reads permission `denied` and
   `requestPermission` never prompts — the rule nsh's `Notifications.java`
   documents. RNFB gets this from the app's own manifest too; the plugin
   does not add it.
6. **Embedding.** `app/libs/nsh.aar` (from
   `$(brew --prefix nsh)/libexec/embed/android/nsh.aar`) and
   `implementation files('libs/nsh.aar')`. `native/android/libs/nsh.aar`
   is the same file — the module compiles against it `compileOnly` — and
   the **two copies must be replaced together** on an nsh upgrade. Then in
   `MainActivity`: `NshFirebase.init(this)`, build the `NshView`,
   `setGrantedCapabilities`, `NshFirebase.register(nsh)`, and only then
   `nsh.loadAssetBundle("nsh")` followed by `NshFirebase.appLoaded()`.
   Presses that start the app are reported by
   `NshFirebase.handleNotificationIntent(intent)` from `onCreate` and
   `onNewIntent`.
7. **Bundle.** The demo bundle goes into `app/src/main/assets/nsh/`:
   `cd examples/demo && nsh build App.tsx --release --out
   ../host-android/app/src/main/assets/nsh`. Until that exists the
   checked-in mini bundle (`examples/host-android/mini-app`) occupies the
   same path.

### Messaging without RNFB's headless machinery

RNFB delivers background messages with three pieces, all under
`~/fo/.refs/react-native-firebase/packages/messaging/android/src/main/java/io/invertase/firebase/messaging/`:
`ReactNativeFirebaseMessagingReceiver.java` (c2dm `RECEIVE`; stores the
message and starts the headless service),
`ReactNativeFirebaseMessagingHeadlessService.java` (a
`HeadlessJsTaskService` that runs a JS task for 60s), and
`ReactNativeFirebaseMessagingStoreImpl.java` (SharedPreferences, capped
at 100 messages). That machinery exists because React Native needs a
running JS runtime without an activity. An NSH host does not: `nsh.send`
from the host reaches the loaded app, and events the app has no listener
for yet are kept by nsh itself (256 events in all, oldest dropped first —
one shared VecDeque for the whole channel, nsh-core 0.4.0's `HostInbox`
in `host_channel.rs`). So this module keeps the RNFB parts that still
apply — the service subclass and the capped SharedPreferences queue, here
in `NshFirebase` — and drops the receiver and the headless task. Messages
that arrive while no app is loaded wait in `pending` and replay on
`appLoaded` (only after a successful load); token refreshes keep only the
newest.

### Building

```sh
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME="$HOME/Library/Android/sdk"   # else "SDK location not found"
cd examples/host-android
gradle :app:assembleDebug
```

These projects carry no Gradle wrapper; this Mac's 9.3.1 distribution
lives under `~/.gradle/wrapper/dists/gradle-9.3.1-bin/` and CI installs
9.3.1 via `gradle/actions/setup-gradle`. AGP 9.1.0 — 9.3.0 rejects Gradle
9.3.1 with "Minimum supported Gradle version is 9.5.0" — compileSdk 36,
minSdk 28 (the AAR needs 28+), targetSdk 36. Output:
`examples/host-android/app/build/outputs/apk/debug/app-debug.apk`; install
and launch with `adb install <apk>` then
`adb shell am start -n dev.nsh.firebase.host/.MainActivity`.

One API note for anyone touching the handlers, since it bit during the
build: in the resolved BoM, `setConsent` takes
`Map<FirebaseAnalytics.ConsentType, FirebaseAnalytics.ConsentStatus>`
(enums, not the old `Consent` string constants),
`FirebasePerformance.startTrace` is static, HTTP metrics are created with
`getInstance().newHttpMetric(url, method)` (the two-argument `HttpMetric`
constructor is package-private), and topic subscribe/unsubscribe return
`Task<Void>`.

### The fake project's behavior, on purpose

`FirebaseApp.initializeApp` succeeds offline with the fake options. What
fails is anything that talks to Google:

- `firebase.messaging.token` -> FCM registration (Firebase Installations)
  rejects; `NshFirebase` surfaces it as a rejection of the host call
  (`firebase/token: FCM registration failed: ...`). That is the sandbox's
  proof that the request was made and its failure is visible.
- Analytics and Performance ship over their own pipelines; use
  `adb shell setprop debug.firebase.analytics.app dev.nsh.firebase.host`
  and logcat to watch events leave.
