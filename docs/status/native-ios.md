# native-ios status

Firebase Apple SDK pin: **firebase-ios-sdk 13.1.1**, exact, via SwiftPM,
traits `["Firestore"]` (RNFB's shipped SPM pin; see
`~/fo/.refs/react-native-firebase/packages/app/ios/RNFBFirebase/Package.swift`.
Firestore is the SDK's only trait — the `FirebaseFirestore` product targets
`FirebaseFirestoreTarget`, gated on it in the SDK's Package.swift — so the
stretch handlers need it enabled, which pulls gRPC/abseil into the build).

## Done

- `fixtures/GoogleService-Info.plist` — obviously fake values (project
  `demo-nsh`, key `fake-ios-key-abc123`), bundle id matching the host app
  (`dev.nsh.firebase.host`). The fake-file note also lives in
  docs/setup-ios.md.
- `native/ios/NshFirebase` — SwiftPM package: `configure(from:)` over
  `FirebaseOptions(contentsOfFile:)` + `FirebaseApp.configure(options:)`, the
  35 contract handlers dispatched by name (messaging token/permission/
  requestPermission/show/close/setBadge/subscribe/unsubscribe, analytics
  logEvent/setUserId/setUserProperty/logScreenView/setConsent, perf
  startTrace/stopTrace/recordHttpMetric, plus the stretch handlers below),
  `MessagingDelegate` +
  `UNUserNotificationCenterDelegate` takeover with previous-delegate
  forwarding (the RNFB pattern from
  `packages/messaging/ios/RNFBMessaging/RNFBMessaging+{FIRMessagingDelegate,UNUserNotificationCenter}.m`),
  host→app events `firebase.messaging.message` / `.tokenRefreshed` / `.press`
  via the injected `sendEvent` closure (the host's `app.send`, which queues
  events until the app listens), local shows with the unreleased NSH
  Notifications semantics (`{id,title,body,silent}`, replace-on-same-id,
  `default|granted|denied|unavailable` permissions).
- **Stretch handlers** (19 new names, same reply/reject shape as the
  existing 16 — rejections use a `firebase/<product>: <reason>` code):
  firestore getDoc/setDoc/addDoc/deleteDoc/runQuery, remote-config
  setDefaults/getDefaults/fetchAndActivate/get, crashlytics
  log/setUserId/setCustomKey/recordError/sendUnsentReports/
  didCrashDuringPreviousExecution, installations getId/getToken,
  appCheck.getToken, functions.call.
- `examples/host-ios` — xcodegen-generated app (NshFirebaseHost.xcodeproj)
  embedding `NativeShell.xcframework` and the NshFirebase package;
  `main.swift` app delegate forwards APNs token and remote notifications;
  `HostController` configures Firebase from the fake plist, registers the
  handlers (`registerHandlers(on:)`), grants
  `network: [127.0.0.1]` + `host: ["firebase.*"]`, loads `nsh/bundle.nshc`,
  and shows a scrolling status label of what fired. `FirebaseAppDelegateProxyEnabled=false`
  (explicit wiring, no swizzling); `UIBackgroundModes: remote-notification`;
  `aps-environment: development` entitlement.
- `examples/host-ios/mini-app` — fallback TSX bundle (the demo bundle's
  replacement until `examples/demo/dist/nsh` lands), probing every handler
  from the app side, with headless tests playing the host. It does not call
  the stretch handlers yet — the facades' native routing tests prove that
  wire on CI.
- `docs/setup-ios.md` — plugin-vs-manual mapping and build commands,
  updated for the trait switch and the stretch handlers' setup notes.

## Facade shapes (the cross-host contract decision)

| Handler | Call | Answer / rejection |
|---|---|---|
| `firebase.perf.startTrace` | `{ name }` | resolves `{ id: string }` |
| `firebase.perf.stopTrace` | `{ id }` | resolves; unknown `id` rejects |
| `firebase.perf.recordHttpMetric` | a `NetworkEvent` (`url`, `method`, `status`, `requestBytes`, `responseBytes` — only `url`+`method` required; nullable fields skipped) | resolves `{ recorded: true }` |
| `firebase.firestore.getDoc` | `{ path }` | resolves `{ exists: bool, data: object\|null }`; a backend error rejects |
| `firebase.firestore.setDoc` | `{ path, data, merge? }` | resolves `{}` |
| `firebase.firestore.addDoc` | `{ collectionPath, data }` | resolves `{ id }` |
| `firebase.firestore.deleteDoc` | `{ path }` | resolves `{}` |
| `firebase.firestore.runQuery` | `{ collectionPath, where?, orderBy?, limit? }` | resolves `{ docs: [{ id, data }] }` |
| `firebase.remote-config.setDefaults` | `{ map }` | resolves `{}` |
| `firebase.remote-config.getDefaults` | — | resolves `{ map }` (read from the SDK, source `default`) |
| `firebase.remote-config.fetchAndActivate` | — | resolves `{ activated: bool, lastFetchStatus: string }`; a refused fetch rejects |
| `firebase.remote-config.get` | `{ key }` | resolves `{ value: string\|null }` (null when the source is `static`) |
| `firebase.crashlytics.log` | `{ message }` | resolves `{}` |
| `firebase.crashlytics.setUserId` | `{ id }` | resolves `{}` |
| `firebase.crashlytics.setCustomKey` | `{ key, value }` | resolves `{}` |
| `firebase.crashlytics.recordError` | `{ code, message, stack? }` | resolves `{}` |
| `firebase.crashlytics.sendUnsentReports` | — | resolves `{}` |
| `firebase.crashlytics.didCrashDuringPreviousExecution` | — | resolves `{ crashed: bool }` |
| `firebase.installations.getId` | — | resolves `{ id }` or rejects (`firebase/installations: …`) |
| `firebase.installations.getToken` | — | resolves `{ token }` or rejects |
| `firebase.appCheck.getToken` | `{ debug? }` | resolves `{ token }` or rejects; `debug: true` swaps in `AppCheckDebugProviderFactory` once, before the first fetch |
| `firebase.functions.call` | `{ name, data? }` | resolves `{ result }` or rejects |

runQuery's `where` is one condition `{ field, op, value }` with op one of
`==`, `!=`, `<`, `<=`, `>`, `>=`, `array-contains`, `in`, `not-in`,
`array-contains-any` (the last three need a value array); `orderBy` is
`{ field, direction? }` (`asc` default, `desc`); `limit` is a number. Both
hosts implement exactly this encoding; the pure-tier facades' native routing
needs to match it.

Messaging answers: `token` → `{ token }` or a rejection with the reason;
`permission`/`requestPermission` → `{ permission: "default" | "granted" |
"denied" | "unavailable" }` (never a rejection); `show` → `{ shown: bool }`
(never a rejection); `close`/`setBadge`/`subscribe`/`unsubscribe` resolve or
reject with the reason.

## Verified

- Every API name the stretch handlers call was read out of the resolved
  13.1.1 checkout at `examples/host-ios/build/SourcePackages/checkouts/firebase-ios-sdk`:
  `FIRDocumentReference.h` (`getDocument(completion:)`,
  `setData(_:completion:)`, `setData(_:merge:completion:)`,
  `delete(completion:)`, `addDocument(data:completion:)`),
  `FIRQuery.h` (`whereField(_:…)` for all ten ops, `order(by:)`,
  `order(by:descending:)`, `limit(to:)`, `getDocuments(completion:)`),
  `FIRFirestore.h` (`firestore()`, `document(_:)`, `collection(_:)`),
  `FIRRemoteConfig.h` (`remoteConfig()`, `setDefaults(_:)`, `allKeys(from:)`,
  `configValue(forKey:source:)`, `fetchAndActivate(completion:)`,
  `lastFetchStatus`, the `RemoteConfigFetchStatus` /
  `RemoteConfigFetchAndActivateStatus` cases, `jsonValue`),
  `FIRCrashlytics.h` (`crashlytics()`, `log(_:)`, `setUserID(_:)`,
  `setCustomValue(_:forKey:)`, `record(error:)`, `sendUnsentReports()`,
  `didCrashDuringPreviousExecution()`), `FIRInstallations.h`
  (`installations()`, `installationID(completion:)`,
  `authToken(forcingRefresh:completion:)` with `authToken`),
  `FIRAppCheck.h` + `FIRAppCheckDebugProviderFactory.h` + `FIRAppCheckToken.h`
  (`appCheck()`, `setAppCheckProviderFactory(_:)`,
  `token(forcingRefresh:completion:)`, `AppCheckDebugProviderFactory`,
  `token.token`), and `FirebaseFunctions/Sources/*.swift` (`functions()`,
  `httpsCallable(_:)`, `call(_:completion:)` with `HTTPSCallableResult.data`).
  The `.static`/`.default` case spelling is the official Swift reference
  (firebase.google.com/docs/reference/swift/firebaseremoteconfig —
  RemoteConfigSource).
- The six product names were checked against the same checkout's
  `Package.swift` products list: `FirebaseFirestore` (trait-gated),
  `FirebaseRemoteConfig`, `FirebaseCrashlytics`, `FirebaseInstallations`,
  `FirebaseAppCheck`, `FirebaseFunctions` — exact spellings, no trait
  surprises. `import FirebaseFirestore` is the documented module: the
  product's `FirebaseFirestoreTarget` is a wrapper whose real dependency is
  the `FirebaseFirestore` Swift target (the checkout's own
  `docs/FirestoreSPM.md` documents the chain).

## Not verified (compile + device phase)

- **The compile of the stretch handlers has not run anywhere yet.** The
  package built cleanly before the stretch additions (macOS host paths +
  `xcodebuild` against the simulator SDK — tails in
  `docs/evidence/native-ios/xcodebuild.log`), and the mini app's TSX gate
  passed (3/3), but the new Swift code and the `["Firestore"]` trait switch
  have no build behind them: this machine is on battery (no builds allowed),
  and CI's iOS build is blocked because the org has no macOS runners. The
  first `swift build` / `xcodebuild` must re-resolve SwiftPM (abseil + gRPC,
  several hundred MB) and is where any name-level mistake would surface.
- No simulator run, no `xcrun simctl push`, no notification tap — the
  workflow's device verifier does that; the demo bundle
  (`examples/demo/dist/nsh`) is not built yet, so the device phase should
  embed either the demo bundle or this mini app.
- FCM token success with the fake project is expected to fail; the clear
  rejection path is proven headlessly (test host) but not on a device. The
  stretch handlers' rejections (installations, remote-config fetch,
  app-check token, functions call) are expected to surface the same way and
  are likewise not yet observed on a device.
- Automatic Performance traces and Analytics debug logging
  (`-FIRDebugEnabled`) are wired but not observed on a device.
