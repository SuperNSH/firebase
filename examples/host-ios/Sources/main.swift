// An existing UIKit app that embeds the nsh-firebase demo app and answers
// its `firebase.*` calls through the NshFirebase facade. Modelled on
// ~/fo/.refs/nsh-v0.4.0/examples/embed-ios/main.swift.

import NshFirebase
import NativeShell
import UIKit

final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        // Swizzling is off; FCM needs the APNs token from us.
        application.registerForRemoteNotifications()
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = HostController()
        window.makeKeyAndVisible()
        self.window = window
        return true
    }

    func application(
        _ application: UIApplication,
        didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data
    ) {
        NshFirebase.shared.setAPNSToken(deviceToken)
    }

    func application(
        _ application: UIApplication,
        didFailToRegisterForRemoteNotificationsWithError error: Error
    ) {
        NshFirebase.shared.apnsRegistrationFailed(error)
    }

    // Every remote notification arrives here — Apple calls this in the
    // foreground too — and the facade skips the alerting ones willPresent
    // already emitted.
    func application(
        _ application: UIApplication,
        didReceiveRemoteNotification userInfo: [AnyHashable: Any],
        fetchCompletionHandler completionHandler: @escaping (UIBackgroundFetchResult) -> Void
    ) {
        NshFirebase.shared.onRemoteMessage(userInfo)
        completionHandler(.newData)
    }
}

UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(AppDelegate.self))
