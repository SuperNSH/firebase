// swift-tools-version: 6.1
import PackageDescription

// Firebase 13.1.1, the exact pin React Native Firebase ships for SPM
// (packages/app/ios/RNFBFirebase/Package.swift in the reference checkout).
// Traits are empty: no Firestore here, and its default trait pulls gRPC and
// abseil into every build.
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
            traits: []),
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
            ],
            // Firebase's ObjC delegates and completion closures predate
            // Sendable; strict checking buys nothing here.
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
