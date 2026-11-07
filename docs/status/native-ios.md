# native-ios status

Firebase Apple SDK pin: **firebase-ios-sdk 13.1.1**, exact, via SwiftPM,
traits empty (RNFB's shipped SPM pin; see
`~/fo/.refs/react-native-firebase/packages/app/ios/RNFBFirebase/Package.swift`).

## Done

- `fixtures/GoogleService-Info.plist` — obviously fake values (project
  `demo-nsh`, key `fake-ios-key-abc123`), bundle id matching the host app
  (`dev.nsh.firebase.host`). The fake-file note also lives in
  docs/setup-ios.md.
- `native/ios/NshFirebase` — SwiftPM package: `configure(from:)` over
  `FirebaseOptions(contentsOfFile:)` + `FirebaseApp.configure(options:)`, the
  16 contract handlers dispatched by name (messaging token/permission/
  requestPermission/show/close/setBadge/subscribe/unsubscribe, analytics
  logEvent/setUserId/setUserProperty/logScreenView/setConsent, perf
  startTrace/stopTrace/recordHttpMetric), `MessagingDelegate` +
  `UNUserNotificationCenterDelegate` takeover with previous-delegate
  forwarding (the RNFB pattern from
  `packages/messaging/ios/RNFBMessaging/RNFBMessaging+{FIRMessagingDelegate,UNUserNotificationCenter}.m`),
  host→app events `firebase.messaging.message` / `.tokenRefreshed` / `.press`
  via the injected `sendEvent` closure (the host's `app.send`, which queues
  events until the app listens), local shows with the unreleased NSH
  Notifications semantics (`{id,title,body,silent}`, replace-on-same-id,
  `default|granted|denied|unavailable` permissions).
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
  from the app side, with headless tests playing the host.
- `docs/setup-ios.md` — plugin-vs-manual mapping and build commands.

## Facade shapes (the cross-host contract decision)

| Handler | Call | Answer / rejection |
|---|---|---|
| `firebase.perf.startTrace` | `{ name }` | resolves `{ id: string }` |
| `firebase.perf.stopTrace` | `{ id }` | resolves; unknown `id` rejects |
| `firebase.perf.recordHttpMetric` | a `NetworkEvent` (`url`, `method`, `status`, `requestBytes`, `responseBytes` — only `url`+`method` required; nullable fields skipped) | resolves `{ recorded: true }` |

Messaging answers: `token` → `{ token }` or a rejection with the reason;
`permission`/`requestPermission` → `{ permission: "default" | "granted" |
"denied" | "unavailable" }` (never a rejection); `show` → `{ shown: bool }`
(never a rejection); `close`/`setBadge`/`subscribe`/`unsubscribe` resolve or
reject with the reason.

## Verified

- `swift build` of the package passes (macOS host paths). Known non-gating
  warnings: the token API deprecation — the handler uses
  `retrieveFCMToken(forSenderID:)`, RNFB's getToken path, which 13.x
  deprecates for `register(completion:)` (that replacement cannot return a
  token to a `host.call`) — and a few Sendable-conversion warnings from
  Firebase's ObjC completions.
- `xcodebuild -project examples/host-ios/NshFirebaseHost.xcodeproj -scheme
  NshFirebaseHost -sdk iphonesimulator -destination 'generic/platform=iOS
  Simulator' -derivedDataPath examples/host-ios/build build` →
  `** BUILD SUCCEEDED **`, tail in `docs/evidence/native-ios/xcodebuild.log`.
  The iOS compile type-checked the `#if os(iOS)` paths the macOS gate skips
  and caught two real errors in them: `HTTPMetric(url:httpMethod:)` is a
  failable init, and the Performance properties import as
  `isInstrumentationEnabled` / `isDataCollectionEnabled`. The built
  `NshFirebaseHost.app` contains `GoogleService-Info.plist`,
  `nsh/bundle.nshc` (+ `capabilities.json`, `app.json`) and the Firebase
  resource bundles. The `ARCHS=arm64` setting exists because
  NativeShell.xcframework ships an arm64 simulator slice only.
- Mini app TSX gate: `nsh check`, `nsh build` (no warnings), `nsh test`
  3/3 passed — handler names, reply shapes and press delivery proven against
  a test host.

## Not verified (device phase)

- No simulator run, no `xcrun simctl push`, no notification tap — the
  workflow's device verifier does that; the demo bundle
  (`examples/demo/dist/nsh`) is not built yet, so the device phase should
  embed either the demo bundle or this mini app.
- FCM token success with the fake project is expected to fail; the clear
  rejection path is proven headlessly (test host) but not on a device.
- Automatic Performance traces and Analytics debug logging
  (`-FIRDebugEnabled`) are wired but not observed on a device.
