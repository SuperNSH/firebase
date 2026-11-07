// An existing UIKit app that embeds the nsh-firebase demo app and answers
// its `firebase.*` calls through the NshFirebase facade. Modelled on
// ~/fo/.refs/nsh-v0.4.0/examples/embed-ios/main.swift.

import NshFirebase
import NativeShell
import UIKit

final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        // Swizzling is off; FCM needs the APNs token from us.
        application.registerForRemoteNotifications()
        return true
    }

    // This SDK refuses to launch apps without the scene lifecycle, so the
    // window lives in SceneDelegate and the manifest in Info.plist names it.
    func application(
        _ application: UIApplication,
        configurationForConnecting connectingSceneSession: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        let config = UISceneConfiguration(name: "Default", sessionRole: connectingSceneSession.role)
        config.delegateClass = SceneDelegate.self
        return config
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

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene,
        willConnectTo session: UISceneSession,
        options connectionOptions: UIScene.ConnectionOptions
    ) {
        guard let windowScene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: windowScene)
        window.rootViewController = HostController()
        window.makeKeyAndVisible()
        self.window = window
    }
}

UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(AppDelegate.self))
