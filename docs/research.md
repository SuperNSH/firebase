# Research: how nsh-firebase gets built

Studied the two references by file path, the Firebase REST surfaces from the
official docs (cited), and the unreleased NSH `Notifications` module. Read-only
references: `~/fo/.refs/react-native-firebase`, `~/fo/.refs/flutterfire`,
`~/fo/.refs/nsh-v0.4.0`, `~/fo/.refs/nsh-main`. Everything below names the file
it came from. Companion file: `docs/upstream-issues.md` (nsh-query / nsh-form).

## 1. React Native Firebase — what each file teaches

### 1.1 SDK pinning, iOS (`packages/app`)

- `packages/app/package.json` holds the pins in one place:
  `sdkVersions.ios.firebase = "13.1.1"`, `sdkVersions.ios.firebaseSpmUrl`
  (the firebase-ios-sdk git URL), plus `iosTarget`/`macosTarget`/`tvosTarget`,
  and `sdkVersions.android.firebase = "35.0.2"` (the BoM version),
  `playServicesAuth`, min/target/compileSdk. One JSON file decides both
  platforms' native SDK versions.
- `packages/app/RNFBApp.podspec` reads `sdkVersions.ios.firebase` out of that
  package.json (with a `$FirebaseSDKVersion` / `FIREBASE_SDK_VERSION` env
  override) and declares only `Firebase/CoreOnly` + `FirebaseInstallations`
  through the shared `firebase_dependency` helper. Each product podspec does
  the same with its own products. Take: our `native/ios/Package.swift` pins one
  exact firebase-ios-sdk version in one file, and each product asks only for
  the products it needs.
- `packages/app/firebase_spm.rb`: the Firebase Apple SDK 13+ ships only via
  SPM ("CocoaPods trunk stopped at 12.19.3", their words); SPM products are
  automatic libraries, so RNFB *requires* `use_frameworks! :linkage =>
  :dynamic` and fails `pod install` loudly on static linkage. It also
  (`rnfirebase_add_spm_core_to_app_target`) links `FirebaseCore` directly into
  the *app* target, because Xcode SPM product deps do not propagate from pods
  to the app, and the app must call `FirebaseApp.configure()` itself. Take:
  in our hosts the embedding app configures Firebase in its own
  AppDelegate/Application class and links the package there directly — the
  same requirement, without any CocoaPods to fight.
- `packages/app/firebase_json.rb`: walks up from the podspec to find the app's
  `firebase.json` and reads the `react-native` key for per-app flags
  (data-collection defaults, `messaging_android_headless_task_timeout`,
  `messaging_android_notification_channel_id` — see
  `packages/app/firebase-schema.json` for the full flag list). Take: our
  `nsh.json`-side config or a small `firebase.json` of our own carries the
  same flags; keep the flag names close to RNFB's so the migration note maps
  1:1.

### 1.2 SDK pinning and config wiring, Android

- `packages/app/android/build.gradle` reads `sdkVersions.android.firebase`
  and declares `implementation platform("com.google.firebase:firebase-bom:$bom")`
  plus `firebase-common`; product modules add their artifacts *without*
  versions so the BoM decides. Take: `native/android` is a Gradle module with
  `firebase-bom` and version-less artifacts.
- `packages/app/plugin/src/android/{applyPlugin,buildscriptDependency,copyGoogleServices}.ts`
  (Expo config plugin): adds the `com.google.gms.google-services` Gradle plugin
  to the buildscript and modules and copies `google-services.json` into the
  module. `plugin/src/ios/{googleServicesPlist,appDelegate,podfile}.ts`:
  copies `GoogleService-Info.plist` into the source root, registers it as a
  resource in the Xcode project, and edits the AppDelegate. Take: our
  `tools/` + `docs/setup.md` step does exactly these things for the two
  example hosts, by hand and scripted — config file into the target, plugin
  applied, delegates wired, entitlements (APNs, background modes,
  POST_NOTIFICATIONS) listed. There is nothing else to automate because our
  hosts are ours.

### 1.3 Messaging delegate takeover, iOS (`packages/messaging`)

- `RNFBMessaging+AppDelegate.m`: registers a `GULAppDelegateSwizzler`
  interceptor and proxies `application:didReceiveRemoteNotification:
  fetchCompletionHandler:` — adding a donor implementation on the real
  delegate's class only if the user has none. It also owns the
  register-for-remote-notifications promise machinery, with a generation
  counter so a superseded registration rejects with `registration-superseded`
  instead of hanging.
- `RNFBMessaging+FIRMessagingDelegate.m`: swaps `FIRMessaging.delegate` for
  itself, keeps the previous delegate, and forwards
  `messaging:didReceiveRegistrationToken:` to it after emitting the
  `messaging_token_refresh` event. Token refresh is a *pushed* event, not
  something the app polls.
- `RNFBMessaging+UNUserNotificationCenter.m`: swaps
  `UNUserNotificationCenter.delegate` the same way, records whether the
  original delegate responded to `willPresentNotification`,
  `didReceiveNotificationResponse`, `openSettingsForNotification`, and holds
  `getInitialNotification` (the tap that started the app, consumed once).
- `RNFBMessaging+NSNotificationCenter.m` re-posts notification events as
  `NSNotification`s for SwiftUI-less code paths.
- Take: a Swift host can do this natively and *without* swizzling — we own
  the AppDelegate, so we set the delegates in it, keep whatever the host set
  before us, and forward. The RNFB lesson is the three delegates that must
  all be owned (app lifecycle, FCM, UNUserNotificationCenter) and that the
  initial notification is a one-shot read.

### 1.4 Background delivery, Android

- `ReactNativeFirebaseMessagingService.java`: extends
  `FirebaseMessagingService`; `onNewToken`, `onSendError`,
  `onDeletedMessages`, `onMessageSent` each emit an event through the shared
  emitter. `onMessageReceived` is a no-op — delivery is handled one layer
  out.
- `ReactNativeFirebaseMessagingReceiver.java`: a `BroadcastReceiver` that
  FCM's SDK delivers to. Guards first: ignore intents without
  `google.message_id` and non-`gcm` message types (control broadcasts,
  delete-intents) — otherwise empty messages surface to the app. Foreground:
  emit an event. Background/quit: start
  `ReactNativeFirebaseMessagingHeadlessService` with the parcelled message
  and `HeadlessJsTaskService.acquireWakeLockNow`; data-only messages need
  `high` priority to start a headless task at all (their catch block says
  so).
- `ReactNativeFirebaseMessagingStoreImpl.java`: persists the last N
  (default 100) notification messages in SharedPreferences so a notification
  received while no JS runtime existed can be delivered when it starts.
- `RNFBMessagingNotificationRegistry.kt`: last-wins upsert of in-flight
  notifications.
- Take: our Android host registers its own `FirebaseMessagingService` and
  receiver, routes foreground to the bridge and background to a stored
  pending message the app reads on start; persist small state in
  SharedPreferences like theirs.

### 1.5 Performance / HTTP metrics

- `packages/perf/lib/HttpMetric.ts`: one `HttpMetric` object per request with
  `setHttpResponseCode`, `setRequestPayloadSize`, `setResponsePayloadSize`,
  `setResponseContentType`, then `start()` / `stop()`; guards against
  start-start and stop-stop. `packages/perf/lib/Trace.ts` (same pattern) has
  `putAttribute`/`putMetric`. The Android side,
  `android/.../UniversalFirebasePerfModule.java`, keeps handle registries of
  `Trace`/`ScreenTrace`/`HttpMetric` and calls `FirebasePerformance
  .newTrace(...).start()` / `newHttpMetric(url, method)` on the native side.
- Take: the `NetworkEvent` shape this repo consumes (see PROMPT.md) maps
  directly onto an `HttpMetric`: `startedAtMs`/`durationMs`/`ttfbMs` ->
  start/stop, `requestBytes`/`responseBytes` -> payload sizes, `status` ->
  `setHttpResponseCode`, `initiator`/`path` -> the metric's URL attributes.
  The monitor wrapper creates one metric per instrumented fetch and stops it
  in a `finally`.

### 1.6 JS API shapes to mirror (`packages/*/lib`)

- Auth (`packages/auth/lib/index.ts`, `User.ts`): `createUserWithEmailAndPassword`,
  `signInWithEmailAndPassword`, `signInAnonymously`, `signInWithCustomToken`,
  `signInWithCredential(credential)`, `sendPasswordResetEmail`,
  `onAuthStateChanged(listener) -> unsubscribe`; `User.getIdToken(forceRefresh)`
  and `getIdTokenResult`; notably `User.refreshToken` *throws* — RNFB refuses
  to expose the refresh token to JS. Our Rust/TSX facade can expose the
  refresh token only through the storage trait (it must persist it), but not
  advertise it as a casual accessor.
- Database (`packages/database/lib/DatabaseReference.ts`,
  `DatabaseQuery.ts`, `DatabaseSyncTree.ts`): `ref(path)`, `set`, `update`
  (multi-path, keys validated against `.`, `#`, `$`, `[`, `]`), `remove`,
  `push`, `transaction(fn, { applyLocally })`, query modifiers
  (`orderByChild/Key/Value`, `startAt/endAt/equalTo`,
  `limitToFirst/limitToLast`), and `on(event, cb)`-style listeners driven by
  `DatabaseSyncTree`.
- Storage (`packages/storage/lib/StorageReference.ts`, `StorageUploadTask.ts`,
  `StorageDownloadTask.ts`): `put`, `putString`, `writeToFile`, `putFile`,
  `getDownloadURL`, `listAll`, `delete`; upload/download tasks emit
  `state_changed` with progress snapshots and support `pause`/`resume`/
  `cancel`.
- Analytics (`packages/analytics/lib/index.ts`): `logEvent(name, params)`
  with client-side validation (reserved names; reserved prefixes
  `firebase_`, `google_`, `ga_`; names must start with an alphabetic
  character, then 1–40 alphanumeric-or-underscore — the `ValidEventName`
  regex and `ReservedEventNamePrefixes` in that file), plus
  `setUserId`, `setUserProperty`, `setAnalyticsCollectionEnabled`,
  `logScreenView`, `setConsent` (in `structs.ts` the same event-name constants
  the web SDK uses).

## 2. FlutterFire — what each file teaches

- Federated layout: every product is `packages/firebase_x` (the app-facing
  Dart), `firebase_x_platform_interface` (the platform contract +
  `pigeons/messages.dart`), and `firebase_x_web` (the web build over the JS
  SDK), e.g. `firebase_database/firebase_database_platform_interface/lib/src/`
  holds `method_channel/`, `pigeon/`, `query_modifiers.dart`,
  `server_value.dart`, `transaction.dart`. Take: our `packages/nsh-firebase`
  with subpath exports is the same idea at smaller scale: one shared core per
  product, per-target adapters behind it (pure REST transport vs native host
  facade), web included in the pure tier.
- Windows/desktop uses the C++ SDK:
  `firebase_core/firebase_core/windows/CMakeLists.txt` pins
  `FIREBASE_SDK_VERSION "14.1.0"` and reads `$ENV{FIREBASE_CPP_SDK_DIR}` with
  a version check against `include/firebase/version.h`;
  `firebase_core_plugin.cpp` registers `FirebaseCoreHostApi`/`FirebaseAppHostApi`
  (Pigeon-generated `messages.g.h/.cpp` sit beside it). There is no Firebase
  Rust SDK; if a Rust desktop host ever wants the *native* pipeline, C++ is
  the only supported desktop path — our milestone does not take it (see the
  tier table).
- Both Apple package managers side by side:
  `firebase_core/firebase_core/ios/firebase_core/Package.swift` declares
  `let firebaseSdkVersion: Version = "13.1.1"` and a `.package(url: ...,
  exact: firebaseSdkVersion)` dependency with per-product `.product(...)`
  deps, and an ObjC target + Swift target split because SPM does not allow
  mixing Swift and ObjC in one target. Its sibling podspec
  (`packages/firebase_core/firebase_core/ios/firebase_core.podspec`) resolves
  the same version through
  `packages/firebase_core/firebase_core/ios/firebase_sdk_version.rb` and pins
  `s.dependency 'Firebase/CoreOnly', firebase_sdk_version`. Take: our
  `native/ios` is SPM-only (our hosts build with Xcode and can consume
  packages directly), but the lesson stands: one version constant, product
  deps per capability, exact pin.
- Pigeon generates the channel code: `pigeons/messages.dart` ->
  `lib/src/pigeon/messages.pigeon.dart` in e.g. `firebase_auth` and
  `firebase_crashlytics` platform interfaces (and `messages.g.cpp` on
  Windows). Take: our Rust/TSX facade is typed by hand but follows the same
  rule — one message catalogue per product, named like the API, with the
  native host implementing exactly that catalogue.
- Messaging background isolates: Dart registers a background
  `BackgroundMessageHandler` through
  `packages/firebase_messaging/firebase_messaging/lib/src/messaging.dart`
  (`FirebaseMessaging.onBackgroundMessage`); the Android side
  (`FlutterFirebaseMessagingBackgroundExecutor.java`) stores the Dart
  `callbackHandle` in prefs and, when a message arrives with the app dead,
  spawns a headless isolate running
  `FlutterCallbackInformation.lookupCallbackInformation(callbackHandle)`.
  Token APIs: `getToken()`, `deleteToken()`, `onTokenRefresh` (a Stream),
  `setAutoInitEnabled`. Take: NSH TSX has no isolate API, so the NSH
  analogue is "store the message natively; the facade replays it on next
  start" — same as RNFB's `ReactNativeFirebaseMessagingStore`, which is the
  pattern we follow.

## 3. The REST surfaces (with sources)

All four surfaces the pure tier uses are public, documented HTTP. The
emulator suite implements the Auth and RTDB REST at `localhost` ports
(Auth: `http://localhost:9099/identitytoolkit.googleapis.com/v1/...`, RTDB
emulator answers `.json` at its port; documented in the pages below and the
emulator's own banner).

- Auth = Identity Toolkit. Docs: <https://firebase.google.com/docs/reference/rest/auth>
  (and the Identity Platform superset: <https://docs.cloud.google.com/identity-platform/docs/reference/rest>).
  All endpoints take the Web API key as `?key=[API_KEY]`, HTTPS only.
  - `POST https://identitytoolkit.googleapis.com/v1/accounts:signUp?key=…`
    with `{email, password, returnSecureToken: true}` -> `{idToken, email,
    refreshToken, expiresIn, localId}`; anonymous sign-up is the same endpoint
    with no email/password. Errors: `EMAIL_EXISTS`,
    `OPERATION_NOT_ALLOWED`, `TOO_MANY_ATTEMPTS_TRY_LATER`.
  - `POST …/accounts:signInWithPassword?key=…` with `{email, password,
    returnSecureToken}` -> `{idToken, email, refreshToken, expiresIn,
    localId, registered}`. Errors: `EMAIL_NOT_FOUND`, `INVALID_PASSWORD`,
    `USER_DISABLED`.
  - `POST …/accounts:signInWithCustomToken?key=…` with `{token,
    returnSecureToken}` -> `{idToken, refreshToken, expiresIn}`. Errors:
    `INVALID_CUSTOM_TOKEN`, `CREDENTIAL_MISMATCH`.
  - `POST …/accounts:verifyCustomToken?key=…` with `{token,
    returnSecureToken: true}` -> `{idToken, refreshToken, expiresIn}`
    ("Verify custom token", same reference page): what
    `signInWithCredential` posts to exchange a custom token or an ID
    token another library minted. The emulator suite does not serve it
    (404; its `signInWithCustomToken` is separate).
  - Errors come back as `{error: {code: 400, message: "EMAIL_NOT_FOUND"}}`;
    the `message` field is the machine-readable code.
- Token refresh = Secure Token. Same docs page, "Sign in / token refresh":
  `POST https://securetoken.googleapis.com/v1/token?key=…`,
  `Content-Type: application/x-www-form-urlencoded`, body
  `grant_type=refresh_token&refresh_token=…` -> snake_case
  `{expires_in, token_type: "Bearer", refresh_token, id_token, user_id,
  project_id}`. Note the case flip: `refreshToken` in, `refresh_token` out.
  Errors: `TOKEN_EXPIRED`, `USER_DISABLED`, `USER_NOT_FOUND`,
  `INVALID_REFRESH_TOKEN`, `INVALID_GRANT_TYPE`, `MISSING_REFRESH_TOKEN`,
  `PROJECT_NUMBER_MISMATCH`.
- Realtime Database REST. Docs:
  <https://firebase.google.com/docs/reference/rest/database> and
  <https://firebase.google.com/docs/database/rest/retrieve-data> /
  <https://firebase.google.com/docs/database/rest/save-data>.
  - Verbs on `https://<db>.firebaseio.com/<path>.json` (emulator: the
    emulator host): GET (read), PUT (set), PATCH (multi-path update), POST
    (push; returns `{"name": "<push-key>"}`), DELETE (remove; returns
    `null`).
  - Writes conditional (transactions): GET with `X-Firebase-ETag: true`
    returns an `ETag`; PUT/DELETE with `if-match: <etag>` writes only if
    unchanged; on mismatch the server answers `412 Precondition Failed` with
    the current value and a fresh ETag, and *does not retry* — the client
    rebuilds. Docs also note only one ETag per `if-match` and that ETags are
    returned only when asked for.
  - Queries: `orderBy` (child key, `$key`, `$value`, `$priority`) with
    `startAt`/`endAt`/`equalTo`/`limitToFirst`/`limitToLast`, one `orderBy`
    per request; results come back unsorted, sort client-side; indexes need
    `.indexOn` rules; `shallow=true` returns keys-only but cannot combine
    with query params; optional `timeout` bounds a server-side read.
  - Auth: `?auth=<idToken>` on any request (the docs' REST pages describe
    the query parameter; the SDKs use `Authorization: Bearer` — both are
    accepted in practice, we use `?auth=` to stay exactly on the documented
    path).
  - Live listeners = SSE: a GET with `Accept: text/event-stream` streams
    `event:` / `data:` lines. Events: `put` (`{path, data}` — replace the
    cache at `path`), `patch` (merge keys at `path`), `keep-alive` (data is
    `null`), `cancel` (rules revoked read; data `null`), `auth_revoked`
    (data is a string; re-authenticate). Honor redirects (307). One
    connection per listen; writes go over separate requests.
- Cloud Storage REST. Neither reference repo contains the raw wire format;
  the source of truth is the firebase-js-sdk
  `packages/storage/src/implementation/requests.ts`, verified here through
  its compiled mirror in this machine's bun cache
  (`~/.bun/install/cache/@firebase/storage@0.14.6@@@1/dist/index.esm.js`,
  functions `createResumableUpload` / `getResumableUploadStatus` /
  `continueResumableUpload`, constant `RESUMABLE_UPLOAD_CHUNK_SIZE = 256 *
  1024`) — the same layer RNFB's `packages/storage/lib/web` re-exports via
  `packages/app/lib/internal/web/firebaseStorage.ts` (`export * from
  'firebase/storage'`). Firebase Storage does *not* speak the GCS
  `uploadType=resumable` dialect; it has its own `X-Goog-Upload-*` protocol
  on the Firebase endpoints (<https://firebase.google.com/docs/storage/web/upload-files>
  for SDK-level behavior; <https://docs.cloud.google.com/storage/docs/performing-resumable-uploads>
  is general resumable-upload background only — its session URI comes back
  in a `location` header, which Firebase's endpoints do not do).
  - Start: `POST https://firebasestorage.googleapis.com/v0/b/<bucket>/o
    ?name=<objectPath>` with `Content-Type: application/json; charset=utf-8`
    (body: the metadata JSON) and headers `X-Goog-Upload-Protocol:
    resumable`, `X-Goog-Upload-Command: start`,
    `X-Goog-Upload-Header-Content-Length: <byteSize>`,
    `X-Goog-Upload-Header-Content-Type: <contentType>`. The response must
    carry `X-Goog-Upload-Status: active`; its `X-Goog-Upload-URL` header is
    the session URI to keep.
  - Resume/status query: `POST` to the session URI with only
    `X-Goog-Upload-Command: query`; read `X-Goog-Upload-Size-Received` (bytes
    already stored) and `X-Goog-Upload-Status` (`active` or `final`). That
    is what makes progress and resume work with only NSH `fetch`.
  - Chunks: `POST` to the session URI with `X-Goog-Upload-Command: upload`
    (or `upload, finalize` on the last slice, or `finalize` when the file is
    already complete) and `X-Goog-Upload-Offset: <bytesSentSoFar>`, body =
    the next slice. Slices must be multiples of 256 KiB (except the final
    one). A `final` status response carries the object metadata. No
    `Content-Range` anywhere.
  - Download: `GET .../o/<path>?alt=media`, with an `Authorization: Firebase
    <idToken>` header for private objects (`addAuthHeader_` in the compiled
    mirror, index.esm.js line 816 — `?auth=` is the Realtime Database
    parameter, not Storage's), or `&token=<downloadToken>` for a public
    download URL. The server issues a download token at upload and keeps the
    comma-separated `downloadTokens` list in the object's metadata; the JS
    SDK only *reads* that list and builds the `alt=media&token=` URL
    (`downloadUrlFromResourceString`, index.esm.js lines 1476–1509). A
    client can add its own stable token through a metadata PATCH of the GCS
    metadata key `firebaseStorageDownloadTokens` — verified against this
    repo's running Storage emulator (project `demo-nsh`, port 19199):
    PATCHing `{"metadata":{"firebaseStorageDownloadTokens":"auditr3token456"}}`
    appended the token to `downloadTokens`, and
    `GET .../o/<obj>?alt=media&token=auditr3token456` then returned `200`
    with the object body; PATCHing the top-level `downloadTokens` field
    itself was ignored (server-managed). Test objects were deleted after the
    check.
- GA4 Measurement Protocol. Docs:
  <https://developers.google.com/analytics/devguides/collection/protocol/ga4>,
  <https://developers.google.com/analytics/devguides/collection/protocol/ga4/reference>,
  <https://developers.google.com/analytics/devguides/collection/protocol/ga4/sending-events>.
  - `POST https://www.google-analytics.com/mp/collect?api_secret=…&
    firebase_app_id=…` (debug: `/debug/mp/collect`), JSON body with
    `app_instance_id` (the Firebase installation ID for app streams;
    `client_id` for web streams), optional `user_id`, `timestamp_micros`,
    `non_personalized_ads`, and `events: [{name, params}]`. The `api_secret`
    is generated in the GA4 admin for the stream; it is not a Firebase key.
  - Limits (reference page): max 25 events per request, ~40 KB web / 130 KB
    app payload, `timestamp_micros` no more than 72 hours in the past.
  - What MP cannot do vs the native pipeline (the protocol page's own
    words: it is meant "to augment automatic collection through gtag, Tag
    Manager, and Google Analytics for Firebase, not to replace it"):
    tagging (the Firebase SDK) is a prerequisite; reserved
    automatic events/params cannot be sent through MP; using it alone gives
    only partial reporting; no rule-based event creation or renaming fires
    from MP events; no automatic IDFA/GAID or privacy/geo/device metadata —
    that is joined only via the `app_instance_id` the native SDK generated;
    session stitching needs `session_id` sent within 24h of session start;
    remarketing is same-device unless a User ID is set. Honest conclusion
    for the pure tier: it can deliver *events*, never sessions, ad-ID
    attribution, or audiences that rely on automatic collection.
  - In this repo's sandbox nothing reaches Google at all: the batching
    queue's transport points at `tools/collector`, which asserts payloads.

## 4. Two-tier shape: verdict and the per-product table

PROMPT.md's two-tier shape is right, and the evidence above does not change
it. The reasons, in order of weight:

1. `nsh package` cannot carry extra native SDKs (a 0.4.0 fact, not a
   choice), and a TSX app has no Rust crate, so *everything* that must work
   everywhere has to exist over NSH `fetch`/WebSocket — exactly tier 1. NSH
   0.4.0's fetch streams response bodies incrementally
   (`crates/nsh-core/src/platform.rs`: `FetchEvent::{BodyWanted, Head, Data,
   End, Failed}`, with `Pause` and `Cancel` ops), which is what SSE listeners
   and upload progress need; verified in source, this is not an assumption.
2. FCM, APNs, the native Analytics pipeline and automatic Performance
   capture exist only inside the Apple/Android SDKs, and those SDKs only
   link through an embedding host — exactly tier 2. RNFB/FlutterFire confirm
   there is no way around the delegates/service (they exist precisely to
   smuggle those callbacks into a JS/Dart runtime).
3. The one-facade-over-two-tiers pattern is what both references converge on
   themselves (FlutterFire's platform_interface, RNFB's modular API on top of
   native modules), so the shape is proven at the architecture level.

Adjustments the evidence did require, none structural:

- **macOS**: PROMPT.md scopes the native tier to iOS and Android hosts. The
  Firebase Apple SDK does support macOS, so a future macOS embedding host
  could adopt `native/ios/NshFirebase` nearly as-is (SwiftPM platforms
  declaration is the only meaningful delta). Out of scope for this
  milestone; recorded here so nobody "discovers" it later as a surprise.
- **Auth credentials across tiers**: the native tier's SDK cannot hand its
  ID token to the pure tier's RTDB client for free — but `getIdToken` exists
  on both sides, and tier 1's `signInWithCredential` accepts a foreign ID
  token (the PROMPT.md area list). That is the crossing point; it must be a
  documented, explicit hand-off.
- **`User.refreshToken`**: RNFB deliberately throws on it. We expose the
  refresh token only to the persistence layer, not as a public accessor.

### Tier table — every product, every target

| Product | Pure tier (every target: `nsh package`, desktop, web, and also inside both hosts) | Native iOS host | Native Android host |
|---|---|---|---|
| **Core** | App options (`apiKey`, `projectId`, `appId`, `databaseURL`, `storageBucket`, `messagingSenderId` — the `FirebaseAppOptions` shape from `packages/app/lib/types/app.ts`), fake fixtures, emulator switches, shared error type, event bus, one app-instance id generated locally and persisted | `FirebaseApp.configure()` with the fake plist; the SDK's real installation/FIS ids | `FirebaseApp.initializeApp` with the fake `google-services.json`; real FIS ids |
| **Auth** | Full email/password, anonymous, custom token, `signInWithCredential`, refresh over Secure Token; emulator-supported (Auth emulator serves the same REST at `localhost:9099`) | SDK adds OAuth providers, phone, MFA, email-link | same as iOS |
| **Database** | Full REST: get/set/update/remove/push, queries, conditional-write transactions, SSE live listeners with reconnect + backoff; emulator-supported (`<emulator-host>/?... .json` incl. streaming) | SDK adds offline persistence, presence, onDisconnect | same |
| **Storage** | Resumable upload with progress + resume via session URI, download, download URL/token, metadata, list, delete; emulator-supported | SDK adds automatic retry tuning, `gs://` URL handling | same |
| **Messaging / Notifications** | **Stub that fails clearly**: no push without a native transport. The facade's `permission()/requestPermission()/show({id,title,body,silent})/close(id)/setBadge(n)/on("press")` names match the unreleased NSH `Notifications` module (section 5) so the released module later drops in | APNs + FCM token, refresh event, foreground/background/tap through the three delegates, topics, notification shown via `UNUserNotificationCenter` | FCM token + `onNewToken`, service + receiver routing (foreground event, background stored then replayed, tap), notification via `NotificationManager`, topics |
| **Analytics** | Events, user properties, screen views, consent, batching queue, pluggable transport; GA4 MP transport with documented limits (above); verified against `tools/collector` | Native pipeline; verified via `-FIRAnalyticsDebugEnabled` logs | Native pipeline; verified via `adb shell setprop debug.firebase.analytics.app` logs |
| **Performance** | Traces + HTTP metrics built from `NetworkEvent`s into a local aggregate with a pluggable transport; cannot reach the Firebase console — Performance has no public ingestion REST, and this repo says so | SDK's real traces + automatic HTTP capture (`NSURLSession` instrumentation) | SDK's real traces + automatic capture (OkHttp/URLConnection instrumentation) |
| **Network monitoring** | The whole deliverable: ring buffer, per-host/per-path p50/p95 aggregates, instrumented `fetch` wrapper (Rust and TSX), the demo screen | Pure-tier monitor *plus* SDK automatic capture for native-made calls | same |
| **Firestore** (stretch) | **Built honestly** over the v1 REST surface (`firestore.googleapis.com/v1/projects/{project}/databases/(default)/documents`; emulator-supported on the suite's 18080): `getDoc` (a 404 reads as `exists: false`), `setDoc` (patch; `merge` sends leaf field paths as `updateMask.fieldPaths`), `addDoc` (createDocument POST, id from the resource name), `deleteDoc` (idempotent), `runQuery` (`structuredQuery`: one where filter, sorts, a `limit`), values in the REST typed-value encoding — the encoding the js-SDK's JsonProtoSerializer writes, read from its compiled source. **No realtime listeners: the REST surface has none to speak of, and none is faked.** Composite filters, cursors, server timestamps and transactions are outside the facade; value types the JSON contract cannot carry fail as `firestore/invalid-data` | The five calls route through `firebase.firestore.*` per the coordinator-fixed contract; **real-time snapshots: native tier could add it later; not in this contract** — no watch handler exists in the contract, so none is registered or invented | same |
| **Remote Config** (stretch) | REST exists (`firebaseremoteconfig.googleapis.com`: `getRemoteConfig`, `updateRemoteConfig`, `listVersions`, `rollback`, `downloadDefaults` — per <https://firebase.google.com/docs/reference/remote-config/rest>) but is authorized with an OAuth 2.0 access token from a service account (scope `https://www.googleapis.com/auth/firebase.remoteconfig`, `Authorization: Bearer`; per <https://firebase.google.com/docs/remote-config/automate-rc>), not the web API key — so a pure-tier NSH app cannot fetch a real template at all, and the emulator suite has no Remote Config service (this repo's suite runs Auth, Realtime Database, Storage). **Built honestly**: `setDefaults`/`getDefaults`, typed gets with the SDKs' coercions, template parse/validation of the REST `RemoteConfig` shape (parameters, conditionalValues, parameterGroups, conditions; `useInAppDefault` and the dynamic personalization/experiment/rollout values stay unset — per the v1 discovery document), and a `fetchAndActivate` that rejects with `remote-config/unavailable` naming the OAuth requirement instead of faking a fetch, unless a pluggable `Transport { fetchTemplate() }` supplies a template from wherever it *can* authorize | Native tier routes the same four calls through `firebase.remote-config.*` (`setDefaults {map}`, `getDefaults()`, `fetchAndActivate()`, `get {key}`) when a host answers; real fetch + activation still only against a real project | same |
| **Crashlytics** (stretch) | Nothing honest: no REST — the pure tier is a clearly-failing facade, every call rejects with `crashlytics/unavailable` naming what is missing; no local log-only recorder pretends to buffer crashes, because the product's meaning is reports reaching Firebase | SDK captures crashes automatically; the facade adds the explicit calls (`log`, `setUserId`, `setCustomKey`, `recordError`, `sendUnsentReports`, `didCrashDuringPreviousExecution`); dSYM upload out of sandbox scope; the native proof is the SDK's debug logging on a device | same |
| **Installations** (stretch) | REST implemented (`firebaseinstallations.googleapis.com/v1`): locally generated FID (the js-SDK's 22-char base64url shape), `POST .../projects/{project}/installations` + `.../{fid}/authTokens:generate` (`x-goog-api-key`, `FIS_v2` auth), `{fid, refreshToken}` persisted + restored, auth token cached with the one-week/one-hour margin; no emulator service and a `demo-` project is refused server-side, so tests stub the network and pin the request/response shapes plus the clear `installations/request-failed` mapping | the facade's contract handlers `firebase.installations.getId` → `{id}` / `.getToken` → `{token}` (reject clearly under the fake project); the SDK's real FIS id is what MP's `app_instance_id` needs | same |
| **App Check** (stretch) | Debug-token exchange only, implemented faithfully: `POST https://firebaseappcheck.googleapis.com/v1/projects/{project}/apps/{appId}:exchangeDebugToken?key={apiKey}` with `{"debug_token": ...}` (the one provider with no device attestor, per the REST reference and the client SDKs' exchange), reply `{token, ttl}` cached for its ttl behind a storage trait. It needs a debug token registered in a real console, so the sandbox proves the request shape and the clear failure only — the emulator suite has no App Check service and `demo-` projects refuse; `getToken` without `debug:true` rejects (`app-check/needs-attestation`), and no token is ever faked | SDK (DeviceCheck/App Attest) through `firebase.appCheck.getToken`; the honest native proof is the SDK's debug logging on a device | SDK (Play Integrity), same routing |
| **Functions** (stretch) | Shipped: the callable wire protocol verbatim — POST `{functionsEmulator:127.0.0.1:15001}/{projectId}/{region}/{name}` (`Content-Type: application/json`, body `{data}`, null when absent; production `https://{region}-{projectId}.cloudfunctions.net/{name}`), 2xx reads `data` then legacy `result`, errors map the HTTP status and `{error:{status,message,details}}` to `functions/*` codes, missing function -> `functions/not-found`; verified against the emulator's v1 `ping`/`add` callables (sources quoted in `docs/evidence/functions/wire-protocol.md`). Not implemented, said honestly: App Check and instance-id context headers, streaming (SSE) callables, the 70 s client deadline, `@type` Int64Value encode/decode | Host routing only (`firebase.functions.call`); the SDKs add the context headers, streaming and region config | same |

### What cannot be done, per target, stated once

- Anywhere: no Firebase account in this repo, so nothing touches production
  Google endpoints in tests; the GA4 MP and Installations transports exist
  but the sandbox asserts them against `tools/collector` only.
- `nsh package` apps: no messaging push, no native analytics/perf — a facade
  call that needs a host rejects with `ERR_FEATURE_UNAVAILABLE_ON_PLATFORM`
  (or the messaging stub's typed error), never silently no-ops.
- Desktop/web: no NSH web/desktop target exposes service-worker
  registration, and no desktop host embeds the Apple/Android SDKs, so no
  FCM transport is reachable — the messaging facade fails clearly there even
  inside a host. FCM itself does exist on web (the firebase JS SDK's
  `packages/messaging` does web push over the Push API + a service worker,
  and FlutterFire's `firebase_messaging_web` wraps exactly it); the
  constraint is NSH's targets, not FCM's.
- Messaging with a fake project: token *requests* are provable (logged),
  token *success* is not — the fake `google-services.json` must surface
  registration failure as a clear error (PROMPT.md sandbox section; both
  references show the same failure mode in their CI setups).
- iOS simulator: real APNs delivery is not possible; `xcrun simctl push`
  injects payloads, which proves the delegate plumbing, not the network
  path. Android emulator: `adb shell` broadcast / direct service starts play
  the FCM role the same way.

## 5. The unreleased `Notifications` module the facade must match

NSH 0.4.0 has no notifications API (checked against
`~/fo/.refs/nsh-v0.4.0`; the nsh skill and nsh.dev docs follow main and are
ahead). `~/fo/.refs/nsh-main` carries it unreleased:

- `crates/nsh-core/src/notifications.rs`: `NotificationPermission` =
  `default | granted | denied | unavailable` (`as_str`/`from_name`);
  `Notification { id, title, body, silent }` with `Notification::new(id,
  title, body)` and `.silent(bool)`; commands `Permission { ask }`, `Show`,
  `Close(String)`, `Listen`, `SetBadge(u32)`; event fields `id`,
  `permission`, `shown`, `pressed`. Two constraints worth copying: showing
  again with the same `id` *replaces*; presses that started the app are
  buffered until the first listener (`Listen`).
- `website/modules/nsh.md` § "Notifications" (lines ~1857–1913): TSX surface
  `Notifications.permission()`, `Notifications.requestPermission()` (both
  resolve `"default" | "granted" | "denied" | "unavailable"`),
  `Notifications.show({ id, title, body, silent })` (resolves whether the
  system took it), `Notifications.close(id)`, `Notifications.setBadge(n)`
  (0 clears), `Notifications.on("press", ({ id }) => …)` (a press can start
  the app; listen at startup). Native parts:
  `crates/nsh-platform-apple/src/notifications.rs` and
  `crates/nsh-platform-android/java/com/longbridge/nsh/Notifications.java`;
  on Android an embedding activity forwards intents via
  `NshView.handleNotificationIntent` in `onCreate`/`onNewIntent`; on Apple an
  embedding app that keeps its own notification-center delegate receives
  presses there.

The messaging facade uses exactly these names and shapes for the
notification-facing half of its API, so the 0.4.0-native implementation and
the later released-module implementation differ by one adapter. Do not
build a general local-notifications library, and do not extend the shape
(`title`/`body`/`id`/`silent` only; FCM data payloads ride the message
handler, not the notification API).

## 6. Integration with nsh-query and nsh-form

Both sibling libraries hold results and form state in signals, which is what
tier 1's products emit; the READMEs document the mapping points
(`createQuery`/`use_query_with` for a Firebase-backed read, `invalidates`
after a mutation, `form.submit` for sign-in screens). Bugs and gaps found
are recorded in `docs/upstream-issues.md`; none block this repo.
