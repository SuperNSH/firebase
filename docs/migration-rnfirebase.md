# Migrating from React Native Firebase

For someone who knows `@react-native-firebase/*`: where every call you
know lives in nsh-firebase, in TypeScript (subpath imports from
`packages/nsh-firebase`) and in Rust (the `nsh-firebase-*` crates). RNFB
names below are read from the pinned reference at
`~/fo/.refs/react-native-firebase/packages/<product>/lib/`.

## The shape differences, first

1. **Imports.** RNFB is one npm package per product
   (`@react-native-firebase/auth`, ...) with a modular API
   (`getAuth(app)`). nsh-firebase is one package with subpaths
   (`nsh-firebase/auth`) exporting classes and functions you construct
   once (`new Auth(app)`); the Rust crates mirror them name for name
   (`nsh_firebase_auth::Auth::new(app, store)`).
2. **Two tiers.** The pure tier (Rust/TSX over NSH `fetch`) works on
   every target, including `nsh package` apps. The native tier (iOS/Android
   embedding hosts over the real SDKs) adds what only the SDKs give: FCM/APNs
   tokens and messages, the native Analytics pipeline, automatic
   Performance capture. A call the pure tier cannot serve rejects with
   `core/unavailable` (`ERR_FEATURE_UNAVAILABLE_ON_PLATFORM`) — it never
   silently no-ops. Messaging is native-tier-only by design.
3. **Emulator config, not per-product connect calls.** RNFB has
   `connectAuthEmulator`, `connectDatabaseEmulator`,
   `connectStorageEmulator`. Here the app options carry one `emulator`
   block (or use `demoOptions()` for the `demo-nsh` sandbox on
   127.0.0.1); every product reads it from the app.
4. **Auth tokens are wired explicitly to database/storage.** RNFB wires
   the ID token into every request internally. Here `database.setAuthToken(token)`
   and `storage.withAuth(provider)` take it (an `auth.currentUser`
   subscription at the call site is the usual provider).
5. **State is signals, not just promises.** `auth.currentUser` (TSX) and
   `auth.user()` (Rust) are signals a binding reads; RTDB listeners
   expose values as signals too. Writes stay promises.
6. **No Firestore.** Firestore, Remote Config, Crashlytics, Installations,
   App Check and Functions are outside the core scope; their subpaths and
   crates exist as stubs that export nothing (`export {};`) and fail
   clearly if asked. See [research.md](research.md) §4 for the honest
   per-product tier table.

| Product | RNFB package | nsh-firebase TSX | Rust crate |
|---|---|---|---|
| App | `@react-native-firebase/app` | `nsh-firebase/core` | `nsh-firebase-core` |
| Auth | `@react-native-firebase/auth` | `nsh-firebase/auth` | `nsh-firebase-auth` |
| Realtime Database | `@react-native-firebase/database` | `nsh-firebase/database` | `nsh-firebase-database` |
| Storage | `@react-native-firebase/storage` | `nsh-firebase/storage` | `nsh-firebase-storage` |
| Messaging | `@react-native-firebase/messaging` | `nsh-firebase/messaging` | `nsh-firebase-messaging` |
| Analytics | `@react-native-firebase/analytics` | `nsh-firebase/analytics` | `nsh-firebase-analytics` |
| Performance | `@react-native-firebase/perf` | `nsh-firebase/perf` (+ `nsh-firebase/monitor`) | `nsh-firebase-perf` |

## App / core

| RNFB | nsh-firebase TSX | Rust | Notes |
|---|---|---|---|
| `initializeApp(options)` | `initializeApp(options)` (`nsh-firebase/core`) | `initialize_app(options)` | Options: `apiKey`, `appId`, `projectId`, `databaseUrl`, `authUrl`, `storageBucket` + optional `emulator` block. `optionsFromConfig` / `from_config` load the same from a config document. |
| `getApp()` / `getApps()` | `getApp()` | `get_app()` | One default app; no named apps. |
| `deleteApp(app)` | not implemented | — | Drop your reference. |
| `SDK_VERSION` | not exported | — | — |
| error with `code` (`auth/...`, `[storage/...]`) | `FirebaseError` with stable codes (`core/*`, `auth/*`, `storage/*`) | same | See the error table at the end. |

## Auth

RNFB persistence (AsyncStorage/KV under the hood) maps to the `store`
argument: the default `StorageStore` persists through `nsh/storage`;
`MemoryStore` is for tests or cache-only apps. Either tier restores the
other's persisted record.

| RNFB | nsh-firebase TSX | Rust | Notes |
|---|---|---|---|
| `getAuth(app)` | `new Auth(app, store?)` | `Auth::new(app, store)` | |
| `createUserWithEmailAndPassword(auth, email, pw)` | `auth.createUserWithEmailAndPassword(email, pw)` | `auth.create_user_with_email_and_password(email, pw)` | Returns the user (not a `UserCredential`). |
| `signInWithEmailAndPassword(auth, email, pw)` | `auth.signInWithEmailAndPassword(email, pw)` | `auth.sign_in_with_email_and_password(email, pw)` | |
| `signInAnonymously(auth)` | `auth.signInAnonymously()` | `auth.sign_in_anonymously()` | Both tiers speak `accounts:signUp` with no credentials (there is no `accounts:signInAnonymously` on the wire). |
| `signInWithCustomToken(auth, token)` | `auth.signInWithCustomToken(token)` | `auth.sign_in_with_custom_token(token)` | |
| `signInWithCredential(auth, cred)` | `auth.signInWithCredential(cred)` | `auth.sign_in_with_credential(cred)` | `Credential` is one of `{kind:"password", email, password}`, `{kind:"custom-token", token}`, `{kind:"id-token", idToken}` — the last replaces provider credentials minted by other libraries (sent to `accounts:verifyCustomToken`; needs Identity Platform in production). |
| `signOut(auth)` | `auth.signOut()` | `auth.sign_out()` | |
| `onAuthStateChanged(auth, cb)` | read the `auth.currentUser` signal; `await auth.authStateReady()` for the first restore | `auth.user()` signal; `auth.state_ready()` / `auth.restore()` | The signal updates through sign-in, refresh and sign-out. |
| `user.getIdToken(force)` | not implemented | — | Refresh is automatic on a schedule, not pull-based: `refresh_due()`, `refresh_now()` (Rust) / `refreshDue()`, `refreshNow()` — clock-driven, single-flight, 5-minute margin. |
| `user.refreshToken` | not exposed | — | RNFB deliberately throws on it too; the token goes to the persistence layer only. |
| `updateProfile`, `sendPasswordResetEmail`, phone/email-link/OAuth providers, MFA | not implemented | — | The pure tier speaks Identity Toolkit REST only; see research.md §3. |
| `connectAuthEmulator(auth, host)` | the app options' `emulator.auth` block | same | Default sandbox: `127.0.0.1:19099`. |

## Realtime Database

Same wire (REST + SSE), so reads and writes behave like RNFB's; what the
SDK socket gives RNFB (presence, `onDisconnect`, offline write queueing)
has no REST surface here and is absent — offline writes reject.

| RNFB | nsh-firebase TSX | Rust | Notes |
|---|---|---|---|
| `getDatabase(app, url?)` | `getDatabase(app)` | `Database::from_app(app, ...)` | One database per app (cached). The emulator switch in the app options wins over `databaseUrl`. |
| `connectDatabaseEmulator(db, host, port)` | the app options' `emulator.database` block | same | Default sandbox: `127.0.0.1:19000`. |
| `ref(db, path?)` / `refFromURL` | `db.ref(path)` | `db.r#ref(path)` | |
| `ref.child(path)` | `ref.child(path)` | `ref.child(path)` | |
| `ref.get()` / `query.once('value')` | `ref.get()` -> `Json \| null` | `ref.get()` | |
| `ref.set(value)` | `ref.set(value)` | `ref.set(value)` | |
| `ref.update(map)` | `ref.update(map)` | `ref.update(map)` | Multi-path PATCH, relative-path keys. |
| `ref.remove()` | `ref.remove()` | `ref.remove()` | |
| `ref.push(value?)` | `ref.push(value)` -> the generated key | `ref.push(value)` -> key | Official `nextPushId` format; RNFB returns a `ThenableReference`, here a key string (set first, then `ref.child(key)` if you need the ref). |
| `ref.transaction(fn, onComplete, applyLocally)` | `ref.transaction(apply)` -> `{committed, snapshot}` | `ref.transaction(apply)` | `apply` returning `undefined` aborts. ETag/`if-match` under the hood, at most 25 rounds. No `applyLocally` (the write is server-truth only). |
| `query.on('value', cb)` / `once` | `ref.onValue()` -> `{ data(), status(), off() }` (signals) or `ref.listen(cb)` | `ref.on_value()` / `ref.listen(...)` | Live listeners are SSE streams, not the SDK socket; reconnect with doubling backoff (1 s -> 30 s) and resync from the server's `put`. |
| `orderByChild/Key/Value`, `startAt`, `endAt`, `equalTo`, `limitToFirst/Last` | same names, same chaining | `order_by_child`, `order_by_key`, `order_by_value`, `start_at`, `end_at`, `equal_to`, `limit_to_first`, `limit_to_last` | Encoded as REST query parameters; `get` sorts client-side (the REST answers unsorted). Invalid queries fail at subscribe time. |
| `onDisconnect()`, `goOffline()`, `goOnline()`, `serverTimestamp()`, `increment()`, persistence (`setPersistenceEnabled`) | not implemented | — | No REST surface; writes while offline reject. Documented in the crate docs. |
| `database/permission-denied` | `core/access-denied` | same | See the error table. |

## Storage

Bytes in and out of memory; RNFB's file-path helpers (`putFile`,
`writeToFile`, `getBlob`, `getStream`) have no equivalent — read/write
the bytes yourself and pass `Uint8Array` / `Vec<u8>`.

| RNFB | nsh-firebase TSX | Rust | Notes |
|---|---|---|---|
| `getStorage(app, bucketUrl?)` | `new FirebaseStorage(app)` | `FirebaseStorage::new(app)` | Bucket from the app options (`storageBucket`); emulator switch wins. |
| `connectStorageEmulator(storage, host, port)` | the app options' `emulator.storage` block | same | Default sandbox: `127.0.0.1:19199`, bucket `demo-nsh.firebasestorage.app`. |
| `ref(storage, path)` | `storage.ref(path)` -> `StorageRef` | `storage.r#ref(path)` | |
| `uploadBytesResumable(ref, bytes, metadata?)` | `ref.upload(bytes, metadata?)` -> `UploadTask` | `ref.upload(bytes, metadata)` -> `UploadTask` | Resumable: 256 KiB slices, `X-Goog-Upload-*` protocol. |
| `task.pause()` / `task.resume()` / `task.cancel()` | same names | `task.pause()`, `task.resume()`, `task.cancel()` | Cancel ends the task in `error` with `storage/canceled`. |
| `task.on('state_changed', cb)` / snapshot | `task.progress` (signal of `{bytesTransferred, totalBytes, state}`); `task.result` promise | `task.progress()` signal; `task.finished()` | State strings: `paused \| running \| done \| error` (RNFB's `TaskState` adds `success`/`canceled`; here done/canceled are `done` and `error`+`storage/canceled`). |
| resume across launches | `task.sessionUrl` -> `ref.resumeUpload(bytes, sessionUrl)` | `task.session_url()` -> `ref.resume_upload(bytes, session_url)` | Persist the session URI; the server's offset query resumes where it stopped. |
| `getDownloadURL(ref)` | `ref.downloadUrl()` | `ref.download_url()` | First entry of the metadata's `downloadTokens`. |
| `getBytes(ref)` | `ref.download()` -> bytes | `ref.download()` | `alt=media` GET. |
| `getMetadata` / `updateMetadata` | `ref.getMetadata()` / `ref.updateMetadata(change)` | `ref.get_metadata()` / `ref.update_metadata(change)` | Only the SDK's writable fields are PATCHed. |
| `list(ref, opts)` / `listAll(ref)` | `ref.list(maxResults?, pageToken?)` -> `{items, prefixes, nextPageToken?}` | `ref.list(...)` | Page by `nextPageToken`; `listAll` is a loop you write. |
| `deleteObject(ref)` | `ref.delete()` | `ref.delete()` | |
| `setMaxUploadRetryTime` etc. | not implemented | — | |
| `storage/unauthorized`, `storage/object-not-found`, `storage/canceled` | same codes, no prefix change needed (`storage/*`) | same | |

## Messaging

**Native tier only.** On the pure tier every call rejects with
`core/unavailable` and the message "Firebase Messaging needs the native
tier: iOS/Android host with firebase.messaging.* handlers"
(`NEEDS_NATIVE_TIER`), and `on(...)` rejects too — a listener that never
fires is the silent no-op the contract forbids. There is no web push
(NSH's web target has no service-worker registration).

The notification-facing half deliberately mirrors nsh's unreleased
`Notifications` module (`permission`, `show`, `close`, `setBadge`,
`on("press")`), so moving to the released module later is one adapter.

| RNFB | nsh-firebase TSX | Rust | Notes |
|---|---|---|---|
| `getMessaging(app)` | `getMessaging()` (default app) | `Messaging::new(host)` | Rust takes the host channel an embedding app injects. |
| `getToken(messaging)` | `messaging.token()` | `messaging.token()` | FCM registration; with the fake fixture project it rejects clearly (`firebase/token: FCM registration failed: ...`) — that is the sandbox's proof, not a failure of the migration. |
| `onTokenRefresh(messaging, cb)` | `messaging.on("token", ({token}) => ...)` | `messaging.on("token", ...)` + `dispatch` | Rust hosts forward host->app events through `dispatch(event, payload)`. |
| `onMessage(messaging, cb)` | `messaging.on("message", (msg) => ...)` | `messaging.on("message", ...)` | Foreground and background messages both arrive as this event; the host buffers what arrives before the app listens (Android replays on `appLoaded`; the nsh event queue holds 256). |
| `setBackgroundMessageHandler` | none — register `on("message")` at startup | none | No handler registration; buffered delivery replaces it. |
| `onNotificationOpenedApp` / `getInitialNotification` | `messaging.on("press", ({id}) => ...)` | same | Taps surface as `press` events with the notification's id; a press that started the app is buffered until the first listener. No separate initial-notification getter — listen at startup. |
| `requestPermission(messaging)` | `messaging.requestPermission()` -> `"default" \| "granted" \| "denied" \| "unavailable"` | `messaging.request_permission()` | RNFB resolves `AuthorizationStatus` numbers; these strings come from nsh's `Notifications` module. Android needs `POST_NOTIFICATIONS` in the host manifest; iOS needs the push entitlements and background modes — both in [setup.md](setup.md). |
| `hasPermission(messaging)` | `messaging.permission()` | `messaging.permission()` | |
| `subscribeToTopic` / `unsubscribeFromTopic` | `messaging.subscribe(topic)` / `messaging.unsubscribe(topic)` | same | |
| `show` (n/a in RNFB) | `messaging.show({id, title, body?, silent?})` -> boolean | `messaging.show(&MessageNotification)` | Local notification through the host's `UNUserNotificationCenter` / `NotificationManager`; same id replaces. |
| `deleteToken`, `sendMessage`, APNs token get/set, `registerDeviceForRemoteMessages`, `setAutoInitEnabled`, delivery-metrics switches | not implemented | — | |

## Analytics

Honest difference: **there is no native Analytics pipeline on the pure
tier.** Events go through a batching queue to a transport you plug in —
the GA4 Measurement Protocol (`mp`) or an HTTP sink (`collector`, here
`tools/collector`). MP delivers *events* only: no sessions, no automatic
IDFA/GAID or geo/device metadata, 25 events and ~130 KB per request
(the queue batches 20). On the native tier the same calls route to the
host's `firebase.analytics.*` handlers and the SDK's real pipeline;
`setDebugViewEnabled(true)` then enables the SDK debug view (a documented
no-op on the pure tier).

| RNFB | nsh-firebase TSX | Rust | Notes |
|---|---|---|---|
| `getAnalytics(app)` | `getAnalytics(app, config?)` | `Analytics::new(...)`; `with_collector`, `with_measurement_protocol`, `recording` constructors | Returns `NativeAnalytics` on a native tier unless `config` forces pure (`transport`/`mp`/`collector`/`forcePure`); otherwise a `QueuedAnalytics`. |
| `logEvent(analytics, name, params)` | `analytics.logEvent(name, params?)` | `analytics.log_event(name, params)` | Client-side validation mirrors RNFB's (`firebase_`/`google_`/`ga_` prefixes reserved, name pattern). |
| `setUserProperty` / `setUserProperties` | `analytics.setUserProperty(key, value)` | `analytics.set_user_property(key, value)` | |
| `setUserId(analytics, id)` | `analytics.setUserId(id \| null)` | `analytics.set_user_id(option)` | |
| `logScreenView(analytics, {screenName, screenClass?})` | `analytics.logScreenView(screenName, screenClass?)` | `analytics.log_screen_view(name, class?)` | Positional args, not a params object. |
| `setConsent(analytics, consentSettings)` | `analytics.setConsent({analyticsStorage, adStorage?, adUserData?, adPersonalization?})` | `analytics.set_consent(Consent { analytics_storage, ad_storage, ad_user_data, ad_personalization })` | RNFB's `ConsentSettings` takes snake_case keys with booleans (`analytics_storage: true`); here the keys are camelCase in TSX / snake_case in Rust and the values are `"granted" \| "denied"`, matching the wire. Denied `analyticsStorage` stops all sends; denied `adPersonalization` rides the batch as MP's `non_personalized_ads: true`. |
| `setAnalyticsCollectionEnabled` | not implemented | — | Use the consent gate. |
| `setDefaultEventParameters`, `getSessionId`, `getAppInstanceId`, `resetAnalyticsData`, `setSessionTimeoutDuration` | not implemented | — | MP has no session/app-instance id without the native SDK's `app_instance_id`. |
| `settings(...)` | `setDebugViewEnabled(bool)` exists instead | `set_debug_view_enabled(bool)` (native tier) | Native debug view switch; pure tier: no-op by documentation. |
| batching/flush (none in RNFB — the SDK owns it) | `QueuedAnalytics` queue: 10 s interval, batch 20, `onSend` hook, drops-oldest at 500 | same (`poll(now)`, `flush()`) | Clock-driven; retries with backoff (1 s base, 30 s cap, 5 attempts). |
| client id (`getGoogleAnalyticsClientId`) | `ensureClientId(storage)` / `CLIENT_ID_KEY` | `ensure_client_id(storage)` | Stable UUID in storage, sent as MP `client_id`. |

## Performance and network monitoring

Honest difference: **perf traces here are local aggregates with a
pluggable transport** — Firebase Performance has no public ingestion
REST, so pure-tier records can never reach the Firebase console (they go
to `tools/collector` or your transport). On the native tier,
`startTrace`/`stopTrace`/`recordHttpMetric` are real SDK traces; trace
metrics/attributes you `putMetric`/`putAttribute` stay in the local ring
(the fixed host-handler shape carries only name and duration across the
channel), and the SDK's automatic HTTP capture for native-made calls
shows in the console, not in this monitor's ring.

| RNFB | nsh-firebase TSX | Rust | Notes |
|---|---|---|---|
| `getPerformance(app)` | `createPerfmon(options)` (or the shared instance pattern) | `Perfmon::new(...)` | |
| `trace(perf, name)` + `t.start()` / `t.putMetric(k, v)` / `t.putAttribute(k, v)` / `t.stop()` | `const t = await perf.startTrace(name)` -> `{putMetric, putAttribute, stop}` | same snake_case | `stop()` returns the `TraceRecord` (pure tier: sent through the transport). |
| `httpMetric(perf, url, method)` + `setHttpResponseCode` + `start/stop` | `perf.recordHttpMetric(event)` with the `NetworkEvent` shape (`method`, `url`, `host`, `path`, `status`, `startedAtMs`, `durationMs`, `ttfbMs`, `requestBytes`, `responseBytes`, `error`, `initiator`) | same | One call with the finished event instead of start/stop bookkeeping; the event shape is shared with `nsh-http`. |
| `newScreenTrace` / `startScreenTrace` | not implemented | — | Use `startTrace("screen-...")` or analytics `logScreenView`. |
| (RNFB has no network monitor) | `nsh-firebase/monitor`: `sharedMonitor`, `instrumentFetch` (drop-in `fetch` wrapper), ring buffer, `monitorRows(events, filter)` — slowest first | `Monitor`, `instrumented_send`, `rows(...)` | Per-host/per-path aggregates: count, error rate, p50/p95 (nearest-rank). The demo's monitor screen (`examples/demo/src/screens/monitor.tsx`) shows it live. |
| delivery | `BatchingTransport` (auto-flush), `HttpLineTransport` (NDJSON to `tools/collector`) | same | Native tier: records route through `firebase.perf.*` handlers when the host answers the probe. |

## Error codes

RNFB code -> nsh-firebase stable code (the facade contract; the same
codes in Rust and TypeScript):

| RNFB | nsh-firebase | Where |
|---|---|---|
| `auth/email-already-in-use`, `auth/user-not-found`, `auth/wrong-password` / `auth/invalid-credential`, `auth/weak-password` | same names — `auth/email-already-in-use`, `auth/user-not-found`, `auth/wrong-password`, `auth/invalid-credential`, `auth/weak-password` | the `auth/*` wire table in both tiers, mapped from the Identity Toolkit's `EMAIL_EXISTS` / `EMAIL_NOT_FOUND` / `INVALID_PASSWORD` / `WEAK_PASSWORD` replies; emulator message text may differ slightly, the code is the contract |
| `database/permission-denied` | `core/access-denied` | RTDB 401/403 |
| `storage/unauthorized`, `storage/object-not-found`, `storage/canceled`, `storage/retry-limit-exceeded`, `storage/quota-exceeded` | same `storage/*` codes | Storage |
| (host refusals, new) | `core/access-denied` (`ERR_ACCESS_DENIED`, undeclared network host or `capabilities.host` name), `core/host-error` (`ERR_NSH_HOST_ERROR`), `core/unavailable` (`ERR_FEATURE_UNAVAILABLE_ON_PLATFORM`), `core/network`, `core/invalid-argument`, `core/timeout`, `core/no-app` | the shared `FirebaseError` type |

Criterion 8's proof: revoke the network capability and every facade call
rejects with `core/access-denied (ERR_ACCESS_DENIED)` — tested in
`packages/nsh-firebase/tests/core.test.tsx` and the `nsh-firebase-core`
crate, evidence in `docs/evidence/core/access-denied.log`.
