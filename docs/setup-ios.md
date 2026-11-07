# iOS setup: what the plugin does, done by hand

The reference automation is React Native Firebase's Expo config plugin,
`~/fo/.refs/react-native-firebase/packages/app/plugin/src/ios/`. This repo
owns its hosts, so nothing needs to be rewritten behind our backs — the
plugin's three steps are done once, by hand, in `examples/host-ios`.

**The `fixtures/GoogleService-Info.plist` is fake.** Project `demo-nsh`, API
key `fake-ios-key-abc123`, app id `1:123456789012:ios:abc123def456`. It
initializes the Apple SDK and makes token requests fail with a clear error;
never replace it with a real plist.

## What the Expo plugin automates, and the manual equivalent

1. **Copy the plist into the source root and register it as a resource**
   (`plugin/src/ios/googleServicesPlist.ts` copies the file and calls
   `addResourceFileToGroup`). Here: `project.yml` lists
   `../../fixtures/GoogleService-Info.plist` with `buildPhase: resources`, so
   the fixture is the bundle resource — one copy, no drift. Runtime:
   `Bundle.main.url(forResource: "GoogleService-Info", withExtension: "plist")`
   → `NshFirebase.configure(from:)`, which builds
   `FirebaseOptions(contentsOfFile:)` and calls
   `FirebaseApp.configure(options:)` (RNFB's AppDelegate step).
2. **AppDelegate wiring** (`plugin/src/ios/appDelegate.ts` injects
   `import FirebaseCore` and `FirebaseApp.configure()`). Here the AppDelegate
   is ours (`examples/host-ios/Sources/main.swift`), so there is no injection:
   it calls `registerForRemoteNotifications()`, hands the APNs token to
   `NshFirebase.shared.setAPNSToken`, and forwards
   `didReceiveRemoteNotification` to `NshFirebase.shared.onRemoteMessage`.
   With `FirebaseAppDelegateProxyEnabled = false` in Info.plist nothing
   swizzles in between — the explicit counterpart of RNFB's
   `GULAppDelegateSwizzler` interceptors.
3. **Podfile / dependency management** (`plugin/src/ios/podfile.ts`; Firebase
   13+ is SPM-only, RNFB now rejects `disableSPM`). Here: no CocoaPods at
   all. `native/ios/NshFirebase/Package.swift` pins
   `firebase-ios-sdk` **13.1.1** (the exact SPM pin in RNFB's
   `packages/app/ios/RNFBFirebase/Package.swift`) with the `["Firestore"]`
   trait — the only trait in this SDK; it pulls gRPC and abseil into every
   build, and the stretch handlers need Firestore. The other five stretch
   products (`FirebaseRemoteConfig`, `FirebaseCrashlytics`,
   `FirebaseInstallations`, `FirebaseAppCheck`, `FirebaseFunctions`) are
   plain products, not trait-gated. `project.yml` declares the package as a
   local package dependency next to `NativeShell.xcframework` from the `nsh`
   Homebrew install. First resolve after the trait switch downloads abseil
   and gRPC — several hundred MB more.
4. **Delegates** — RNFB's `packages/messaging/ios/RNFBMessaging/RNFBMessaging+{AppDelegate,FIRMessagingDelegate,UNUserNotificationCenter}.m`
   swap `FIRMessaging.delegate` and `UNUserNotificationCenter.delegate` for
   themselves, keep the previous delegates, and forward. `NshFirebase.configure(from:)`
   does the same swap natively: it stores whatever delegate was set before it
   and forwards `willPresentNotification` and
   `didReceiveNotificationResponse` to it, emitting `firebase.messaging.*`
   events on the way.
5. **Entitlements (push)**: the plugin's RNFB docs ask for the Push
   Notifications capability and background modes. Here `project.yml`
   generates `aps-environment: development` and
   `UIBackgroundModes: [remote-notification]`. Note the limits: a real APNs
   transport needs a real Apple developer account and Firebase project, which
   this sandbox never has. On the simulator the plumbing is proven with
   `xcrun simctl push <udid> payload.apns`; a fake project makes the token
   request fail loudly instead of producing a token.

## Build and run

```bash
# 1. the app bundle the host embeds (the demo area's examples/demo/dist/nsh
#    can replace this once it lands)
cd examples/host-ios/mini-app && nsh build --release --out ../App/nsh

# 2. regenerate the Xcode project (needed after project.yml or sources change)
cd .. && xcodegen generate

# 3. build. The host uses the UIScene life cycle (a SceneDelegate builds
#    the window; the UIApplicationSceneManifest in project.yml names it) —
#    this SDK refuses to launch a legacy-AppDelegate-only app.

xcodebuild -project NshFirebaseHost.xcodeproj \
  -scheme NshFirebaseHost -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath build build
```

`NSH_EMBED_IOS` in `project.yml` points at the Homebrew nsh embedding
framework (`$(brew --prefix nsh)/libexec/embed/ios`); override it there if
your prefix differs. SwiftPM fetches the Firebase SDK on first resolve —
several hundred MB of binary frameworks, be patient.

## The six stretch handlers and what they initialize

The handlers (`firebase.firestore.*`, `firebase.remote-config.*`,
`firebase.crashlytics.*`, `firebase.installations.*`,
`firebase.appCheck.getToken`, `firebase.functions.call`) are registered like
the messaging and analytics ones: the host's `registerHandlers(on:)` loop
covers every name in `NshFirebase.handlerNames`. Nothing extra is needed at
configure time, with one exception: `firebase.appCheck.getToken({debug:true})`
swaps in `AppCheckDebugProviderFactory()` on the first debug call — App Check
requires the provider factory to be set before its first token fetch, so the
first non-debug `getToken` after a debug call still runs on whichever factory
was in place first. Crashlytics, Installations, Remote Config, Firestore and
Functions need no setup call at all: `FirebaseApp.configure` plus the module
imports is the whole integration (the SDK's component discovery does the
rest, the same mechanism RNFB's `RNFBConfig`, `RNFBFirestore` and co. rely
on).
