# Setting Firebase up on Android, by hand

What React Native Firebase's Expo config plugin automates, and what this
repository does instead. Applies to `native/android` (the library module) and
`examples/host-android` (the embedding host).

## What RNFB's Expo plugin automates

RNFB's plugin (`~/fo/.refs/react-native-firebase/packages/app/plugin/src/android`):

- `copyGoogleServices.ts`: copies the app's `google-services.json` to
  `app/google-services.json` (`expo.android.googleServicesFile` is the source,
  `DEFAULT_TARGET_PATH = 'app/google-services.json'`).
- `buildscriptDependency.ts`: appends `classpath 'com.google.gms:google-services:<version>'`
  to the project-level `build.gradle`'s `dependencies` block.
- `applyPlugin.ts`: appends `apply plugin: 'com.google.gms.google-services'` to
  `app/build.gradle`.
- `constants.ts` takes the plugin version from `sdkVersions.android.gmsGoogleServicesGradle`.

RNFB's `packages/app/android/build.gradle` separately reads
`sdkVersions.android.firebase` and declares
`implementation platform("com.google.firebase:firebase-bom:$bom")`; every
product module adds its artifacts without versions so the BoM decides.

## What this repository does instead

No Expo, no google-services Gradle plugin. The plugin's build-time step
(parsing `google-services.json` into resources) is replaced by a runtime step:
`NshFirebase.init(Context)` builds `FirebaseOptions` from the host's
`assets/google-services.json` (parsed with `org.json`), falling back to the
fake fixture values baked into `NshFirebase.defaultOptions()`. This needs no
real credentials at any point — the google-services plugin would happily
accept the fake fixture too, but the runtime options approach keeps the whole
setup visible in one place and works without touching the host's build.

### The steps, one by one

1. **Config file.** `fixtures/google-services.json` is the fixture: project id
   `demo-nsh`, project number `123456789012`, api key `fake-android-key-abc123`,
   mobilesdk app id `1:123456789012:android:abc123def456`, package name
   `dev.nsh.firebase.host`. **These values are fake.** They initialize the SDK
   locally; anything that calls Google's network fails, and that failure is the
   point (see below). The host copies it into its assets, which is exactly what
   `copyGoogleServices.ts` does, minus Expo:
   `cp fixtures/google-services.json examples/host-android/app/src/main/assets/google-services.json`
2. **BoM and artifacts.** `native/android/build.gradle` pins
   `com.google.firebase:firebase-bom:35.1.0` (checked on Google Maven,
   2026-11) and adds the artifacts with no versions: `firebase-messaging`,
   `firebase-analytics`, `firebase-perf`, plus the six stretch products
   `firebase-firestore`, `firebase-config`, `firebase-crashlytics`,
   `firebase-installations`, `firebase-appcheck`,
   `firebase-appcheck-debug` (the debug provider factory lives in its own
   artifact) and `firebase-functions`. Under this BoM the host resolves
   firebase-messaging 26.0.1, firebase-analytics 24.0.0, firebase-perf
   23.0.1, firestore 27.1.0, config 24.0.1, crashlytics 21.0.1,
   installations 20.0.0, appcheck and appcheck-debug 20.0.0 and functions
   23.0.0 (read from the BoM pom in the Gradle cache). The module compiles
   against `libs/nsh.aar` with `compileOnly` — the host already packages the
   AAR, and two copies would clash at dex time. The host includes the
   module: `implementation project(':nshfirebase')`, with
   `project(':nshfirebase').projectDir = file('../../native/android')` in
   `settings.gradle` so the module exists in one place.
3. **Service and manifest entries.** `native/android/src/main/AndroidManifest.xml`
   declares `NshFirebaseMessagingService` with the
   `com.google.firebase.MESSAGING_EVENT` intent filter (`exported=false`),
   `INTERNET`/`ACCESS_NETWORK_STATE`/`WAKE_LOCK`, and the
   `com.google.firebase.messaging.default_notification_channel_id` meta-data —
   modelled on RNFB's `packages/messaging/android/src/main/AndroidManifest.xml`.
   The manifest merger applies all of it to the host. RNFB's extra c2dm
   receiver + `HeadlessJsTaskService` pair (see below) is replaced by a queue.
   **The stretch products add nothing to any manifest.** The SDK AARs
   self-register through `ComponentDiscoveryService` meta-data (read out of
   the cached firebase-crashlytics 21.0.1 AAR's own manifest) and the
   product modules RNFB ships for firestore, remote-config and
   crashlytics declare empty or permission-only manifests
   (`packages/{firestore,remote-config}/android/src/main/AndroidManifest.xml`).
   Crashlytics on a modern BoM needs no manifest entry and no init provider
   (RNFB's `ReactNativeFirebaseCrashlyticsInitProvider` exists only to
   toggle collection off before RNFB's own init — we do not turn collection
   off). App Check's debug provider needs nothing at build time either: the
   factory is swapped at runtime from `firebase.appCheck.getToken({debug:true})`
   (`DebugAppCheckProviderFactory.getInstance()`, the same call FlutterFire's
   `FirebaseAppCheckPlugin.kt` makes), before the first token fetch.
4. **Notification channel.** Created at runtime by `NshFirebase.init` (and
   idempotently before every `show`) as `dev.nsh.firebase.notifications`,
   `IMPORTANCE_HIGH`, badge on — following nsh's unreleased `Notifications`
   module. The manifest meta-data points FCM's own auto-posts (background
   notification messages, which never reach the service) at the same channel
   id, so creating it in `init` keeps those posts out of FCM's fallback
   channel.
5. **POST_NOTIFICATIONS.** Declared in the host's manifest
   (`examples/host-android/app/src/main/AndroidManifest.xml`). Without it, an
   Android 13+ device reads permission `denied` and `requestPermission` never
   prompts — the rule nsh's `Notifications.java` documents. RNFB gets this
   from the app's own manifest too; the plugin does not add it.
6. **Embedding.** `app/libs/nsh.aar` (from
   `$(brew --prefix nsh)/libexec/embed/android/nsh.aar`) and
   `implementation files('libs/nsh.aar')`. `native/android/libs/nsh.aar` is
   the same file — the module compiles against it `compileOnly` — and the two
   copies must be replaced together on an nsh upgrade, then in `MainActivity`:
   `NshFirebase.init(this)`, build the `NshView`,
   `setGrantedCapabilities`, `NshFirebase.register(nsh)`, and only then
   `nsh.loadAssetBundle("nsh")` followed by `NshFirebase.appLoaded()`.
   Presses that start the app are reported by
   `NshFirebase.handleNotificationIntent(intent)` from `onCreate` and
   `onNewIntent`.
7. **Bundle.** The demo bundle goes into `app/src/main/assets/nsh/` via
   `nsh build --release --out app/src/main/assets/nsh` when
   `examples/demo/dist/nsh` exists. Until then the checked-in mini bundle
   (`examples/host-android/mini-app`, built with `nsh check` + `nsh build`)
   occupies the same path.

## Messaging without RNFB's headless machinery

RNFB delivers background messages with three pieces, all under
`~/fo/.refs/react-native-firebase/packages/messaging/android/src/main/java/io/invertase/firebase/messaging/`:
`ReactNativeFirebaseMessagingReceiver.java` (c2dm `RECEIVE`; stores the
message and starts the headless service),
`ReactNativeFirebaseMessagingHeadlessService.java` (a `HeadlessJsTaskService`
that runs a JS task for 60s), and `ReactNativeFirebaseMessagingStoreImpl.java`
(SharedPreferences, capped at 100 messages). That machinery exists because React Native needs a running JS
runtime without an activity. An NSH host does not: `nsh.send` from the host
reaches the loaded app, and events the app has no listener for yet are kept by
nsh itself (256 events in all, oldest dropped first — one shared VecDeque for
the whole channel, nsh-core 0.4.0's `HostInbox` in `host_channel.rs`). So
this module keeps the RNFB parts that still
apply — the service subclass and the capped SharedPreferences queue, here in
`NshFirebase` — and drops the receiver and the headless task. Messages that
arrive while no app is loaded wait in `pending` and replay on
`appLoaded` (only after a successful load); token refreshes keep only the
newest.

## The fake project's behavior, on purpose

`FirebaseApp.initializeApp` succeeds offline with the fake options. What
fails is anything that talks to Google:

- `firebase.messaging.token` → FCM registration (Firebase Installations)
  rejects; `NshFirebase` surfaces it as a rejection of the host call
  (`firebase/token: FCM registration failed: …`). That is the sandbox's
  proof that the request was made and its failure is visible.
- The same is the point of the stretch handlers that call Google's network:
  `firebase.installations.getId`/`getToken`, `firebase.remote-config.fetchAndActivate`,
  `firebase.appCheck.getToken`, `firebase.functions.call` and the Firestore
  handlers (no host wires `useEmulator`, so they target the real backend)
  all reject with a `firebase/<product>: …` reason under the fake project.
  The handlers that never touch the network — Crashlytics logs, custom keys
  and recorded errors (queued locally, shipped when a real project exists),
  Remote Config defaults, `get` and `getDefaults` — resolve without it.
- Analytics and Performance ship over their own pipelines; use
  `adb shell setprop debug.firebase.analytics.app dev.nsh.firebase.host`
  and logcat to watch events leave.

## Building

```sh
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME="$HOME/Library/Android/sdk"   # else "SDK location not found"
cd examples/host-android
~/.gradle/wrapper/dists/gradle-9.3.1-bin/23ovyewtku6u96viwx3xl3oks/gradle-9.3.1/bin/gradle :app:assembleDebug
```

The Gradle binary is the wrapper distribution already on this Mac (9.3.1);
these projects carry no wrapper. AGP 9.1.0 — 9.3.0 rejects Gradle 9.3.1 with
"Minimum supported Gradle version is 9.5.0" — compileSdk 36, minSdk 28 (the
AAR needs 28+), targetSdk 36. Output:
`examples/host-android/app/build/outputs/apk/debug/app-debug.apk`.

One API note for anyone touching the handlers, since it bit during the build:
in the resolved BoM, `setConsent` takes
`Map<FirebaseAnalytics.ConsentType, FirebaseAnalytics.ConsentStatus>` (enums,
not the old `Consent` string constants), `FirebasePerformance.startTrace` is
static, HTTP metrics are created with `getInstance().newHttpMetric(url,
method)` (the two-argument `HttpMetric` constructor is package-private), and
topic subscribe/unsubscribe return `Task<Void>`.
