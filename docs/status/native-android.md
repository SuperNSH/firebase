# native-android status

Area: native/android (library module) + examples/host-android (embedding host)
+ fixtures/google-services.json + docs/setup-android.md.

## Done

- `fixtures/google-services.json`: fake fixture (project `demo-nsh`, number
  `123456789012`, key `fake-android-key-abc123`, app id
  `1:123456789012:android:abc123def456`, package `dev.nsh.firebase.host`).
  Marked fake here and in docs/setup-android.md. The host app packs a copy in
  `app/src/main/assets/google-services.json` (verified byte-identical with
  diff).
- `native/android`: Gradle library module (`com.android.library`, AGP 9.1.0),
  Firebase BoM **35.1.0** pinned, with version-less `firebase-messaging` (resolves
  26.0.1), `firebase-analytics` (24.0.0), `firebase-perf` (23.0.1), and the
  six stretch artifacts `firebase-firestore` (27.1.0), `firebase-config`
  (24.0.1), `firebase-crashlytics` (21.0.1), `firebase-installations`
  (20.0.0), `firebase-appcheck` and `firebase-appcheck-debug` (20.0.0),
  `firebase-functions` (23.0.0) — all names and versions read from the BoM
  pom in the Gradle cache. Compiles
  against `libs/nsh.aar` with `compileOnly` (the host packages the AAR; two
  copies clash at dex time).
- `NshFirebase.java` installs exactly the contract handlers:
  `firebase.messaging.{token,permission,requestPermission,show,close,setBadge,subscribe,unsubscribe}`,
  `firebase.analytics.{logEvent,setUserId,setUserProperty,logScreenView,setConsent}`,
  `firebase.perf.{startTrace,stopTrace,recordHttpMetric}`, and sends
  `firebase.messaging.{message,tokenRefreshed,press}` (queued in a capped
  SharedPreferences store when the app is not loaded; token refreshes keep
  only the newest; a loaded app without listeners is covered by nsh's own
  event buffer (one shared queue per channel, 256 events in all)).
- **Stretch handlers** (19 new names, same reply/reject shape as the
  existing 16 — rejections use a `firebase/<product>: <reason>` code):
  firestore getDoc/setDoc/addDoc/deleteDoc/runQuery, remote-config
  setDefaults/getDefaults/fetchAndActivate/get, crashlytics
  log/setUserId/setCustomKey/recordError/sendUnsentReports/
  didCrashDuringPreviousExecution, installations getId/getToken,
  appCheck.getToken (debug provider swapped in on the first `{debug:true}`
  call, before the first fetch), functions.call. The runQuery wire encoding
  matches iOS exactly (`where` = `{field, op, value}`, ten ops; `orderBy` =
  `{field, direction?}`; `limit` number) — table in docs/status/native-ios.md.
  `NshFirebaseMessagingService` handles
  `MESSAGING_EVENT`. show/close/setBadge/permission mirror nsh-main's
  unreleased `Notifications.java` semantics (one channel, same id replaces,
  badge via `setNumber`, presses open the launcher with the id in an extra,
  Android 13 asked/denied/default logic).
- init approach: **no google-services Gradle plugin, no credentials**.
  `NshFirebase.init(Context)` parses `assets/google-services.json` into
  `FirebaseOptions`, else uses the baked-in fake fixture values.
  Documented in docs/setup-android.md, including what RNFB's Expo plugin
  (`packages/app/plugin/src/android/{applyPlugin,buildscriptDependency,copyGoogleServices,constants}.ts`)
  automates and the manual equivalent.
- Background delivery: RNFB's receiver + `HeadlessJsTaskService` pair
  (`ReactNativeFirebaseMessagingReceiver`, `ReactNativeFirebaseMessagingHeadlessService`,
  `ReactNativeFirebaseMessagingStoreImpl`) is replaced by the queue above;
  citations in docs/setup-android.md.
- `examples/host-android`: Gradle app (applicationId `dev.nsh.firebase.host`,
  minSdk 28, targetSdk 36) modelled on nsh 0.4.0's `examples/embed-android`;
  `NshView`, grant `{ "host": ["firebase.*"], "network": {"hosts": ["127.0.0.1"]} }`
  (trailing-`*` prefix match verified in nsh-core 0.4.0 `access.rs`
  `allows_host`), `NshFirebase.init` + `register` before `loadAssetBundle`,
  `appLoaded()` after it, status TextView fed by a host-side listener,
  `handleNotificationIntent` in `onCreate` / `onNewIntent`.
  `app/libs/nsh.aar` copied from
  `$(brew --prefix nsh)/libexec/embed/android/nsh.aar`.
- `examples/host-android/mini-app`: fallback TSX bundle calling every handler
  class and listening for the three events; its sources stay for rebuilds.
  The packaged `app/src/main/assets/nsh` is now the real demo bundle copied
  from `examples/demo/dist/nsh` (app id `dev.nsh.demo`, bundle.js 63,151 B /
  bundle.nshc 76,131 B — diff -r against dist clean, APK uncompressed sizes
  match; docs/evidence/native-android/demo-bundle-check.txt).

## Verified how

- Gate: `JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"`
  and Gradle
  `~/.gradle/wrapper/dists/gradle-9.3.1-bin/23ovyewtku6u96viwx3xl3oks/gradle-9.3.1/bin/gradle`
  (no wrapper in these projects): `:app:assembleDebug` BUILD SUCCESSFUL
  (docs/evidence/native-android/gradle.log), APK at
  `examples/host-android/app/build/outputs/apk/debug/app-debug.apk`;
  `gradle -p native/android assembleDebug` also BUILD SUCCESSFUL (module
  standalone).
- Merged manifest checked in the APK with aapt2
  (docs/evidence/native-android/manifest-check.txt): `POST_NOTIFICATIONS`,
  `INTERNET`, `ACCESS_NETWORK_STATE`, `WAKE_LOCK` present;
  `dev.nsh.firebase.NshFirebaseMessagingService` with the
  `com.google.firebase.MESSAGING_EVENT` filter, `exported=false`.
- `nsh check App.tsx` and `nsh build App.tsx --release --out
  ../app/src/main/assets/nsh` pass on mini-app (bundle.js + bundle.nshc +
  app.json + capabilities.json in assets).
- 0.4.0 API checks: `NshView` methods read from the AAR's classes with javap
  (`handle`, `send`, `setGrantedCapabilities`, `HostHandler.handle(Object,
  NshHostReply)`, `NshHostReply.resolve(Object)/reject(String)`); host
  wildcard grant from `nsh-core-0.4.0/src/access.rs`; `host.on` event
  buffering from the generated `.nsh/types/nsh/index.d.ts`. Firebase API
  shapes (consent enums, static `startTrace`, `newHttpMetric`, `Task<Void>`
  topics, `getToken()` returning `Task<String>`) verified with javap against
  the BoM-resolved artifacts and confirmed by the successful compile — three
  of these were wrong on first draft and the compiler caught each.
- Stretch-handler API checks, split by evidence source:
  - **javap against the cached BoM artifacts**: firebase-config 24.0.1
    (`getInstance`, `fetchAndActivate` → `Task<Boolean>`,
    `setDefaultsAsync(Map<String,Object>)` → `Task<Void>`, `getValue` →
    `FirebaseRemoteConfigValue` with `asString`/`getSource`,
    `getInfo().getLastFetchStatus()`, `getAll()`, the
    `LAST_FETCH_STATUS_*`/`VALUE_SOURCE_*` int constants); firebase-crashlytics
    21.0.1 (`getInstance`, `log`, `setUserId`, the six `setCustomKey`
    overloads, `recordException(Throwable)`, `sendUnsentReports`,
    `didCrashOnPreviousExecution`); firebase-installations 20.0.0
    (`getId` → `Task<String>`, `getToken(boolean)` →
    `Task<InstallationTokenResult>`) with `InstallationTokenResult.getToken()`
    javap'd from the transitive firebase-installations-interop 18.0.0.
  - **Reference sources by path** (the firestore, app-check and functions
    artifacts are not in the local Gradle cache, so javap was impossible):
    firestore — RNFB
    `packages/firestore/android/src/reactnative/java/io/invertase/firebase/firestore/`
    `ReactNativeFirebaseFirestoreQuery.java` (`whereEqualTo`,
    `whereNotEqualTo`, `whereGreaterThan`, `whereGreaterThanOrEqualTo`,
    `whereLessThan`, `whereLessThanOrEqualTo`, `whereArrayContains`,
    `whereArrayContainsAny`, `whereIn`, `whereNotIn`,
    `orderBy(field, Query.Direction)`, `limit`) and
    `NativeRNFBTurboFirestoreDocument.java` (`set(data, SetOptions.merge())`,
    plain `set(data)`), `ReactNativeFirebaseFirestoreSerialize.java`
    (`querySnapshot.getDocuments()`); app-check — FlutterFire
    `packages/firebase_app_check/firebase_app_check/android/src/main/kotlin/io/flutter/plugins/firebase/appcheck/FirebaseAppCheckPlugin.kt`
    (`com.google.firebase.appcheck.debug.DebugAppCheckProviderFactory`,
    `FirebaseAppCheck.getInstance(app).setAppCheckProviderFactory(…)`) and
    RNFB `packages/app-check/android/.../NativeRNFBTurboAppCheck.java`
    (`getAppCheckToken(forceRefresh)`, `token.getToken()`); functions — RNFB
    `packages/functions/android/.../NativeRNFBTurboFunctions.java`
    (`FirebaseFunctions.getInstance()`, `getHttpsCallable(name)`,
    `call(data)`, `result.getData()`); crashlytics JS-error recording — RNFB
    `packages/crashlytics/android/.../NativeRNFBTurboCrashlytics.java`
    (`recordException` on a named synthetic exception with
    `setStackTrace(StackTraceElement[])`).
  - The compile check for the uncached modules (firestore 27.1.0,
    appcheck 20.0.0, functions 23.0.0) and the new handler code lands in
    CI's Android host build job (ubuntu).
- **No manifest additions for the stretch products** (verified, stated in
  docs/setup-android.md step 3): the cached firebase-crashlytics 21.0.1 AAR's
  own AndroidManifest self-registers via `ComponentDiscoveryService`
  meta-data and carries `INTERNET`; RNFB's firestore and remote-config
  product manifests are empty
  (`packages/{firestore,remote-config}/android/src/main/AndroidManifest.xml`);
  App Check's debug provider needs nothing at build time.

## Not verified

- **The stretch handler code has no local compile behind it** (the build
  machine is on battery; no build commands allowed): the javap and
  reference-source evidence above is read-only. The Android host build job
  on ubuntu is the compiler for this — the seven new BoM artifacts and the
  19 new handlers compile there first. `native/android` standalone
  `assembleDebug` passed before the stretch additions
  (docs/evidence/native-android/gradle.log).
- Nothing on an emulator (device verifier's job): app launch, FCM token
  request failure surfacing in the app, notification show/press flows, and
  press delivery — `adb shell am start -n dev.nsh.firebase.host/.MainActivity
  --es dev.nsh.firebase.NOTIFICATION <id>` for this module's notifications,
  `--es google.message_id <id>` for the FCM auto-post path. The build proves
  compilation and manifest merging, not pixels.
- The stretch handlers' rejections under the fake project
  (installations, remote-config fetchAndActivate, app-check token,
  functions call) are expected to surface like the FCM token's does, but
  only an emulator run can show it. Firestore handlers have no wired
  emulator route (`useEmulator` is not called), so under the fake project
  they reject against the real backend — the honest sandbox behavior.
- `NshView.handleNotificationIntent` does not exist on 0.4.0 (verified by
  javap; it is in nsh-main only). `NshFirebase.handleNotificationIntent`
  reflectively calls it when present and otherwise delivers the press itself;
  the reflection path is untested until a newer AAR exists.
- FCM behavior against a real project (token success, actual delivery) — out
  of sandbox scope on purpose; the fake project must surface token failure
  as a rejection, which only an emulator run can show.

## Audit fixes (round 1)

- **Taps on FCM auto-posted background notifications now press.** A
  background notification message never reaches `onMessageReceived`, and FCM's
  launch intent carries `google.message_id` instead of this module's extra.
  `NshFirebase.handleNotificationIntent` falls back to `google.message_id`
  (removing it after reporting, so an activity recreate cannot double-report)
  — the same extra RNFB's `ReactNativeFirebaseMessagingReceiver` and
  `getInitialNotification` key on. Press id for those taps is the FCM message
  id.
- **The notification channel is created in `init`**, not lazily in `show`:
  the manifest meta-data points FCM's auto-posts at
  `dev.nsh.firebase.notifications`, and firebase-messaging 26.0.1 falls back
  to its own channel when the app has not created it yet. Creation is now an
  idempotent `ensureChannel` called from `init` and before every `show`;
  docs/setup-android.md step 4 corrected.
- **The pending queue only drains after a successful load.** The eager drain
  in `register()` is gone; `MainActivity` calls `NshFirebase.appLoaded()` only
  when `loadAssetBundle` returns true and logs through logcat otherwise, so a
  failed load no longer flushes durable prefs into a view that cannot
  deliver.
- **Perf handler shapes per the coordinator contract decision:**
  `firebase.perf.startTrace({name})` → `{id}` (an opaque UUID, not the name,
  so same-name traces cannot collide), `firebase.perf.stopTrace({id})`
  (rejects when the id is not open), `firebase.perf.recordHttpMetric(<NetworkEvent>)`
  — mapping `url`, `method`, `status` → response code, `requestBytes`,
  `responseBytes`; `HttpMetric`'s public API cannot take an explicit
  `durationMs`, so real call timing comes from the SDK's automatic capture.
  The iOS host answers the same handler names with `{id}`; both hosts match.
  mini-app calls `stopTrace` with the `id` startTrace returned and posts a
  full NetworkEvent-shaped object.
- **Doc corrections in docs/setup-android.md:** nsh's event buffer is 256
  events in all per channel (one VecDeque, oldest evicted —
  host_channel.rs `MAX_QUEUED_EVENTS`), not per name; the RNFB
  background-delivery classes now carry their source paths; the Building
  block exports `ANDROID_HOME` (a fresh shell without it fails with
  "SDK location not found").

## Audit fixes (round 2)

- The nsh event-buffer description is corrected everywhere (docs, status,
  mini-app comment, appLoaded javadoc): one shared VecDeque per channel,
  256 events in all, oldest evicted — nsh-core 0.4.0's `HostInbox` — not per
  name.
- `MainActivity.onDestroy` clears `NshFirebase.setListener(null)` before
  `nsh.destroy()`: the static listener held a lambda capturing the activity.
- `results(Activity)` commits the retained permission fragment with
  `commitNowAllowingStateLoss()`: a `firebase.messaging.requestPermission`
  host call can arrive while the activity is stopped, where `commitNow`
  throws and would turn a legitimate call into a confusing rejection.
- docs/setup-android.md step 6 now states `native/android/libs/nsh.aar` and
  `app/libs/nsh.aar` are the same `$(brew --prefix nsh)` file and must be
  replaced together.
- Mini-app assets rebuilt after the comment change (`nsh check` +
  `nsh build --release`, pass). Standalone module build and
  `:app:assembleDebug` both BUILD SUCCESSFUL
  (docs/evidence/native-android/gradle.log refreshed).

## Versions

- Gradle 9.3.1 (dist path above), AGP 9.1.0 (9.3.0 demands Gradle 9.5.0 —
  hit and recorded), Firebase BoM 35.1.0, compileSdk 36, minSdk 28,
  targetSdk 36, Java 17 toolchain (Android Studio JBR).
