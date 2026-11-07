// swift-tools-version: 6.1
import PackageDescription

// Firebase 13.1.1, the exact pin React Native Firebase ships for SPM
// (packages/app/ios/RNFBFirebase/Package.swift in the reference checkout).
// The only trait in this SDK is "Firestore"; enabling it pulls gRPC and
// abseil into every build, which the six stretch handlers need — the other
// five products (RemoteConfig, Crashlytics, Installations, AppCheck,
// Functions) are not trait-gated. Verified against the SDK's Package.swift:
// the FirebaseFirestore product targets FirebaseFirestoreTarget, which is
// `condition: .when(traits: ["Firestore"])`.
let package = Package(
    name: "NshFirebase",
    platforms: [
        .iOS(.v15),
    ],
    products: [
        .library(name: "NshFirebase", targets: ["NshFirebase"]),
    ],
    dependencies: [
        .package(
            url: "https://github.com/firebase/firebase-ios-sdk.git", exact: "13.1.1",
            traits: ["Firestore"]),
    ],
    targets: [
        .target(
            name: "NshFirebase",
            dependencies: [
                .product(name: "FirebaseCore", package: "firebase-ios-sdk"),
                .product(name: "FirebaseMessaging", package: "firebase-ios-sdk"),
                .product(name: "FirebaseAnalytics", package: "firebase-ios-sdk"),
                // Performance is iOS/tvOS/visionOS only (the SDK's
                // Package.swift); `swift build` on the host compiles the
                // source's `#if os(iOS)` paths without it.
                .product(
                    name: "FirebasePerformance", package: "firebase-ios-sdk",
                    condition: .when(platforms: [.iOS])),
                .product(name: "FirebaseFirestore", package: "firebase-ios-sdk"),
                .product(name: "FirebaseRemoteConfig", package: "firebase-ios-sdk"),
                .product(name: "FirebaseCrashlytics", package: "firebase-ios-sdk"),
                .product(name: "FirebaseInstallations", package: "firebase-ios-sdk"),
                .product(name: "FirebaseAppCheck", package: "firebase-ios-sdk"),
                .product(name: "FirebaseFunctions", package: "firebase-ios-sdk"),
            ],
            // Firebase's ObjC delegates and completion closures predate
            // Sendable; strict checking buys nothing here.
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
