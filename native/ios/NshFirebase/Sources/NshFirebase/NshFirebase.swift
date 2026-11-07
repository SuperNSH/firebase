//
//  NshFirebase.swift
//  The native tier of nsh-firebase on iOS: the Firebase Apple SDK wired
//  to an NSH app's `host` channel. The embedding host registers the handlers
//  on its `NshAppView` (it owns the framework; this package cannot link it)
//  and forwards its own notification delegates here, the way
//  React Native Firebase's RNFBAppDelegate and RNFBMessaging+UNUserNotificationCenter
//  take the delegates over and forward.
//
//  The handler and event names are this repo's native-tier messaging
//  contract; the notification-facing half keeps the shape of the unreleased
//  NSH `Notifications` module (documented in docs/research.md §5, from
//  `~/fo/.refs/nsh-main`): `default | granted | denied | unavailable`
//  permissions and `{ id, title, body, silent }` shows.
//

import FirebaseAnalytics
import FirebaseAppCheck
import FirebaseCore
import FirebaseCrashlytics
import FirebaseFirestore
import FirebaseFunctions
import FirebaseInstallations
import FirebaseMessaging
import FirebaseRemoteConfig
import Foundation
    @preconcurrency import UserNotifications
#if os(iOS)
    import FirebasePerformance
    import UIKit
#endif

@MainActor
public final class NshFirebase: NSObject, UNUserNotificationCenterDelegate, MessagingDelegate {
    /// Process-wide: `FirebaseApp`, the FCM delegate and the notification
    /// center's delegate are singletons of the process, so the facade is one.
    public static let shared = NshFirebase()

    /// Where host→app events go: the host sets this to `app.send(_,_)`
    /// before configuring, so presses queue in the view's channel until the
    /// app listens (`app.send` keeps them even before a bundle loads).
    public var sendEvent: (@MainActor (String, Any?) -> Void)?

    /// One line per handler call or event sent, for the host's status view.
    public var onActivity: (@MainActor (String) -> Void)?

    /// The names this facade answers. The host registers each on its
    /// `NshAppView` and routes the call to `dispatch(name:data:reply:)`.
    public static let handlerNames: [String] = [
        "firebase.messaging.token",
        "firebase.messaging.permission",
        "firebase.messaging.requestPermission",
        "firebase.messaging.show",
        "firebase.messaging.close",
        "firebase.messaging.setBadge",
        "firebase.messaging.subscribe",
        "firebase.messaging.unsubscribe",
        "firebase.analytics.logEvent",
        "firebase.analytics.setUserId",
        "firebase.analytics.setUserProperty",
        "firebase.analytics.logScreenView",
        "firebase.analytics.setConsent",
        "firebase.perf.startTrace",
        "firebase.perf.stopTrace",
        "firebase.perf.recordHttpMetric",
        "firebase.firestore.getDoc",
        "firebase.firestore.setDoc",
        "firebase.firestore.addDoc",
        "firebase.firestore.deleteDoc",
        "firebase.firestore.runQuery",
        "firebase.remote-config.setDefaults",
        "firebase.remote-config.getDefaults",
        "firebase.remote-config.fetchAndActivate",
        "firebase.remote-config.get",
        "firebase.crashlytics.log",
        "firebase.crashlytics.setUserId",
        "firebase.crashlytics.setCustomKey",
        "firebase.crashlytics.recordError",
        "firebase.crashlytics.sendUnsentReports",
        "firebase.crashlytics.didCrashDuringPreviousExecution",
        "firebase.installations.getId",
        "firebase.installations.getToken",
        "firebase.appCheck.getToken",
        "firebase.functions.call",
    ]

    /// Delegates that were set before ours; RNFB keeps and forwards to the
    /// previous ones instead of dropping them.
    private nonisolated(unsafe) var previousCenterDelegate: UNUserNotificationCenterDelegate?
    private nonisolated(unsafe) var previousMessagingDelegate: MessagingDelegate?

    /// iOS 18.0 calls `willPresent` twice per notification (Apple bug,
    /// developer.apple.com/forums/thread/761597); FlutterFire drops the
    /// repeat by remembering the last message id. Touched only from the
    /// center's serialized delegate callbacks.
    private nonisolated(unsafe) var lastWillPresentMessageId: String?

    #if os(iOS)
        private var traces: [String: Trace] = [:]
    #endif

    /// Configures Firebase from a GoogleService-Info.plist and takes over the
    /// FCM and notification-center delegates. Call once, before loading a
    /// bundle, on the main thread.
    public func configure(from plist: URL) throws {
        guard FirebaseApp.app() == nil else { return }
        guard let options = FirebaseOptions(contentsOfFile: plist.path) else {
            throw CocoaError(.fileReadCorruptFile, userInfo: [
                NSLocalizedDescriptionKey: "not a readable GoogleService-Info.plist: \(plist.path)",
            ])
        }
        FirebaseApp.configure(options: options)

        // Automatic traces and NSURLSession capture are on by default; set
        // them so an Info.plist flag cannot silently turn them off.
        #if os(iOS)
            Performance.sharedInstance().isInstrumentationEnabled = true
            Performance.sharedInstance().isDataCollectionEnabled = true
        #endif

        previousMessagingDelegate = Messaging.messaging().delegate
        Messaging.messaging().delegate = self
        let center = UNUserNotificationCenter.current()
        previousCenterDelegate = center.delegate
        center.delegate = self
        log("configured")
    }

    // MARK: App delegate hooks (the host owns the UIApplicationDelegate)

    /// The APNs token, once registration succeeds; FCM needs it to register.
    public func setAPNSToken(_ deviceToken: Data) {
        Messaging.messaging().setAPNSToken(deviceToken, type: .unknown)
        log("apns token set")
    }

    public func apnsRegistrationFailed(_ error: Error) {
        log("apns registration failed: \(error.localizedDescription)")
    }

    /// A remote notification delivered through the app delegate's
    /// `didReceiveRemoteNotification`, which Apple calls in the foreground
    /// as well as the background. RNFB's app-delegate branch reports only
    /// FCM messages (`gcm.message_id`), and the center's delegate owns the
    /// visible-alert ones, so only the rest emit here.
    public func onRemoteMessage(_ userInfo: [AnyHashable: Any]) {
        guard userInfo["gcm.message_id"] != nil else { return }
        #if os(iOS)
            // FlutterFire's foreground branch: willPresent owns plain alerts
            // while the app is visible (including its re-call on the way
            // back to the foreground after a tap-open). An alert carrying
            // content-available is skipped by willPresent, so it still
            // emits here.
            let aps = userInfo["aps"] as? [String: Any]
            let alerting = aps?["alert"] != nil
            let contentAvailable = (aps?["content-available"] as? NSNumber)?.boolValue == true
            if alerting, !contentAvailable, UIApplication.shared.applicationState != .background {
                return
            }
        #endif
        Task { @MainActor in
            self.emitMessage(userInfo)
        }
    }

    // MARK: The host channel

    /// Answers one registered host call. `reply` goes to the app's promise:
    /// give it once, from any thread; a rejection surfaces as
    /// `ERR_NSH_HOST_ERROR` with the message.
    public func dispatch(
        name: String, data: Any?,
        reply: @escaping (_ value: Any?, _ error: String?) -> Void
    ) {
        log(name)
        switch name {
        case "firebase.messaging.token":
            // RNFB's getToken goes through retrieveFCMTokenForSenderID: with
            // the configured sender id. 13.x deprecates it for
            // `register(completion:)`, which cannot hand back a token to a
            // `host.call`, so the deprecation warning is expected.
            guard let senderId = FirebaseApp.app()?.options.gcmSenderID else {
                reply(nil, "Firebase is not configured; call configure(from:) first")
                return
            }
            Messaging.messaging().retrieveFCMToken(forSenderID: senderId) { token, error in
                if let token {
                    reply(["token": token], nil)
                } else {
                    reply(nil, "fcm token unavailable: \(error?.localizedDescription ?? "no error given")")
                }
            }
        case "firebase.messaging.permission":
            permission(ask: false, reply)
        case "firebase.messaging.requestPermission":
            permission(ask: true, reply)
        case "firebase.messaging.show":
            show(data, reply)
        case "firebase.messaging.close":
            close(data, reply)
        case "firebase.messaging.setBadge":
            setBadge(data, reply)
        case "firebase.messaging.subscribe":
            topic(data, reply, subscribe: true)
        case "firebase.messaging.unsubscribe":
            topic(data, reply, subscribe: false)
        case "firebase.analytics.logEvent":
            let params = (data as? [String: Any])?["params"] as? [String: Any]
            guard let eventName = string(data, "name") else {
                reply(nil, "firebase.analytics.logEvent needs a name"); return
            }
            Analytics.logEvent(eventName, parameters: params)
            reply(nil, nil)
        case "firebase.analytics.setUserId":
            let userId = (data as? [String: Any])?["userId"] as? String
            Analytics.setUserID(userId)
            reply(nil, nil)
        case "firebase.analytics.setUserProperty":
            guard let propertyName = string(data, "name") else {
                reply(nil, "firebase.analytics.setUserProperty needs a name"); return
            }
            let value = (data as? [String: Any])?["value"] as? String
            Analytics.setUserProperty(value, forName: propertyName)
            reply(nil, nil)
        case "firebase.analytics.logScreenView":
            var params: [String: Any] = [:]
            if let screenName = string(data, "name") { params[AnalyticsParameterScreenName] = screenName }
            if let screenClass = string(data, "class") { params[AnalyticsParameterScreenClass] = screenClass }
            Analytics.logEvent(AnalyticsEventScreenView, parameters: params)
            reply(["logged": true], nil)
        case "firebase.analytics.setConsent":
            setConsent(data, reply)
        case "firebase.perf.startTrace":
            #if os(iOS)
                guard let traceName = string(data, "name") else {
                    reply(nil, "firebase.perf.startTrace needs a name"); return
                }
                guard let trace = Performance.startTrace(name: traceName) else {
                    reply(nil, "Firebase Performance could not start trace \(traceName)"); return
                }
                let id = UUID().uuidString
                traces[id] = trace
                reply(["id": id], nil)
            #else
                reply(nil, "Firebase Performance is iOS-only")
            #endif
        case "firebase.perf.stopTrace":
            #if os(iOS)
                let id = string(data, "id") ?? ""
                guard let trace = traces.removeValue(forKey: id) else {
                    reply(nil, "unknown trace id \(id)"); return
                }
                trace.stop()
                reply(nil, nil)
            #else
                reply(nil, "Firebase Performance is iOS-only")
            #endif
        case "firebase.perf.recordHttpMetric":
            #if os(iOS)
                recordHttpMetric(data, reply)
            #else
                reply(nil, "Firebase Performance is iOS-only")
            #endif
        case "firebase.firestore.getDoc",
             "firebase.firestore.setDoc",
             "firebase.firestore.addDoc",
             "firebase.firestore.deleteDoc",
             "firebase.firestore.runQuery":
            firestore(name, data, reply)
        case "firebase.remote-config.setDefaults",
             "firebase.remote-config.getDefaults",
             "firebase.remote-config.fetchAndActivate",
             "firebase.remote-config.get":
            remoteConfig(name, data, reply)
        case "firebase.crashlytics.log",
             "firebase.crashlytics.setUserId",
             "firebase.crashlytics.setCustomKey",
             "firebase.crashlytics.recordError",
             "firebase.crashlytics.sendUnsentReports",
             "firebase.crashlytics.didCrashDuringPreviousExecution":
            crashlytics(name, data, reply)
        case "firebase.installations.getId":
            Installations.installations().installationID { fid, error in
                if let fid {
                    reply(["id": fid], nil)
                } else {
                    // The fake project has no backend: this rejection is the
                    // honest proof the request was made, like the FCM token's.
                    reply(nil, "firebase/installations: getId failed: \(error?.localizedDescription ?? "no error given")")
                }
            }
        case "firebase.installations.getToken":
            // authTokenWithCompletion is the forcing-refresh-false form;
            // the labeled variant does not survive the ObjC importer.
            Installations.installations().authToken { result, error in
                if let result {
                    reply(["token": result.authToken], nil)
                } else {
                    reply(nil, "firebase/installations: getToken failed: \(error?.localizedDescription ?? "no error given")")
                }
            }
        case "firebase.appCheck.getToken":
            appCheckGetToken(data, reply)
        case "firebase.functions.call":
            functionsCall(data, reply)
        default:
            reply(nil, "no handler for \(name)")
        }
    }

    // MARK: Messaging handlers

    /// The permission names are the unreleased NSH Notifications module's:
    /// not determined → `default`, denied → `denied`, any authorization
    /// (including provisional and ephemeral) → `granted`.
    private func permission(ask: Bool, _ reply: @escaping (Any?, String?) -> Void) {
        let center = UNUserNotificationCenter.current()
        let report: (UNNotificationSettings) -> Void = { settings in
            let permission: String
            switch settings.authorizationStatus {
            case .notDetermined: permission = "default"
            case .denied: permission = "denied"
            default: permission = "granted"
            }
            reply(["permission": permission], nil)
        }
        if ask {
            center.requestAuthorization(options: [.alert, .sound, .badge]) { _, error in
                if let error {
                    // `unavailable` is a normal resolution, as the
                    // Notifications module answers when the framework fails.
                    Task { @MainActor in self.log(error.localizedDescription) }
                    reply(["permission": "unavailable"], nil)
                } else {
                    center.getNotificationSettings { report($0) }
                }
            }
        } else {
            center.getNotificationSettings { report($0) }
        }
    }

    /// Shows a local notification under `id`; adding the same id again
    /// replaces the old one, as the Notifications module's `show` does.
    private func show(_ data: Any?, _ reply: @escaping (Any?, String?) -> Void) {
        let args = data as? [String: Any] ?? [:]
        guard let id = args["id"] as? String,
            let title = args["title"] as? String,
            let body = args["body"] as? String
        else {
            reply(nil, "firebase.messaging.show needs id, title and body"); return
        }
        let silent = args["silent"] as? Bool ?? false
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        if !silent { content.sound = .default }
        let request = UNNotificationRequest(identifier: id, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request) { error in
            if let error {
                // `shown: false` is the answer, not a rejection — show must
                // resolve either way, as the Notifications module's does.
                Task { @MainActor in self.log(error.localizedDescription) }
                reply(["shown": false], nil)
            } else {
                reply(["shown": true], nil)
            }
        }
    }

    private func close(_ data: Any?, _ reply: @escaping (Any?, String?) -> Void) {
        guard let id = string(data, "id") else {
            reply(nil, "firebase.messaging.close needs an id"); return
        }
        let center = UNUserNotificationCenter.current()
        center.removePendingNotificationRequests(withIdentifiers: [id])
        center.removeDeliveredNotifications(withIdentifiers: [id])
        reply(nil, nil)
    }

    private func setBadge(_ data: Any?, _ reply: @escaping (Any?, String?) -> Void) {
        let count = (data as? [String: Any])?["count"] as? Int ?? 0
        let center = UNUserNotificationCenter.current()
        // `setBadgeCount` is iOS 16+ (macOS 13); the application property
        // covers older iOS, on the main thread as it requires.
        if #available(iOS 16.0, macOS 13.0, *) {
            center.setBadgeCount(count) { error in
                reply(nil, error?.localizedDescription)
            }
        } else {
            #if os(iOS)
                UIApplication.shared.applicationIconBadgeNumber = count
            #endif
            reply(nil, nil)
        }
    }

    private func topic(_ data: Any?, _ reply: @escaping (Any?, String?) -> Void, subscribe: Bool) {
        guard let name = string(data, "topic") else {
            reply(nil, "topic handlers need a topic"); return
        }
        let done: (Error?) -> Void = { error in
            if let error {
                reply(nil, "fcm topic \(name): \(error.localizedDescription)")
            } else {
                reply(["topic": name], nil)
            }
        }
        let messaging = Messaging.messaging()
        if subscribe {
            messaging.subscribe(toTopic: name, completion: done)
        } else {
            messaging.unsubscribe(fromTopic: name, completion: done)
        }
    }

    // MARK: Analytics handlers

    private func setConsent(_ data: Any?, _ reply: @escaping (Any?, String?) -> Void) {
        let args = data as? [String: Any] ?? [:]
        var consent: [ConsentType: ConsentStatus] = [:]
        // A missing key stays out: `setConsent` persists only what it gets.
        for (key, type) in ["adStorage": ConsentType.adStorage,
            "analyticsStorage": ConsentType.analyticsStorage,
            "adUserData": ConsentType.adUserData,
            "adPersonalization": ConsentType.adPersonalization] {
            guard let granted = args[key] as? Bool else { continue }
            consent[type] = granted ? .granted : .denied
        }
        Analytics.setConsent(consent)
        reply(["set": consent.count], nil)
    }

    // MARK: Performance handlers

    #if os(iOS)
        /// Records one already-completed HTTP metric from a `NetworkEvent`
        /// (the cross-host shape: `status`, `requestBytes`, `responseBytes`):
        /// the caller measured, we report.
        private func recordHttpMetric(_ data: Any?, _ reply: @escaping (Any?, String?) -> Void) {
            let args = data as? [String: Any] ?? [:]
            guard let urlString = args["url"] as? String, let url = URL(string: urlString),
                let methodRaw = args["method"] as? String
            else {
                reply(nil, "firebase.perf.recordHttpMetric needs a url and a method"); return
            }
            let methods: [String: HTTPMethod] = [
                "GET": .get, "POST": .post, "PUT": .put, "DELETE": .delete,
                "HEAD": .head, "PATCH": .patch, "OPTIONS": .options, "TRACE": .trace,
                "CONNECT": .connect,
            ]
            guard let method = methods[methodRaw.uppercased()] else {
                reply(nil, "unknown http method \(methodRaw)"); return
            }
            guard let metric = HTTPMetric(url: url, httpMethod: method) else {
                reply(nil, "Firebase Performance could not create a metric for \(urlString)")
                return
            }
            if let status = args["status"] as? Int { metric.responseCode = status }
            if let sent = args["requestBytes"] as? Int { metric.requestPayloadSize = sent }
            if let received = args["responseBytes"] as? Int { metric.responsePayloadSize = received }
            metric.start()
            metric.stop()
            reply(["recorded": true], nil)
        }
    #endif

    // MARK: Stretch handlers (Firestore, Remote Config, Crashlytics,
    // Installations, App Check, Functions)

    /// Set once the App Check provider factory was swapped; the factory must
    /// be in place before the first token fetch (the SDK keeps whichever
    /// provider was created first).
    private static var appCheckFactorySet = false

    private func firestore(_ name: String, _ data: Any?, _ reply: @escaping (Any?, String?) -> Void) {
        let args = data as? [String: Any] ?? [:]
        let db = Firestore.firestore()
        switch name {
        case "firebase.firestore.getDoc":
            guard let path = string(data, "path") else {
                reply(nil, "firebase.firestore.getDoc needs a path"); return
            }
            db.document(path).getDocument { snapshot, error in
                if let error {
                    reply(nil, "firebase/firestore: getDoc \(path) failed: \(error.localizedDescription)")
                    return
                }
                // A missing document resolves with exists:false, data:null —
                // it is not an error (Firestore's own semantics).
                reply([
                    "exists": snapshot?.exists ?? false,
                    "data": snapshot?.data() ?? NSNull(),
                ], nil)
            }
        case "firebase.firestore.setDoc":
            guard let path = string(data, "path"),
                let docData = args["data"] as? [String: Any]
            else {
                reply(nil, "firebase.firestore.setDoc needs a path and a data object"); return
            }
            let merge = args["merge"] as? Bool ?? false
            let done: (Error?) -> Void = { error in
                if let error {
                    reply(nil, "firebase/firestore: setDoc \(path) failed: \(error.localizedDescription)")
                } else {
                    reply([:], nil)
                }
            }
            merge
                ? db.document(path).setData(docData, merge: true, completion: done)
                : db.document(path).setData(docData, completion: done)
        case "firebase.firestore.addDoc":
            guard let collectionPath = string(data, "collectionPath"),
                let docData = args["data"] as? [String: Any]
            else {
                reply(nil, "firebase.firestore.addDoc needs a collectionPath and a data object"); return
            }
            // addDocument's completion carries only the error; the
            // reference comes back synchronously.
            let reference = db.collection(collectionPath).addDocument(data: docData) { error in
                if let error {
                    reply(nil, "firebase/firestore: addDoc \(collectionPath) failed: \(error.localizedDescription)")
                } else {
                    reply(["id": reference.documentID], nil)
                }
            }
        case "firebase.firestore.deleteDoc":
            guard let path = string(data, "path") else {
                reply(nil, "firebase.firestore.deleteDoc needs a path"); return
            }
            db.document(path).delete { error in
                if let error {
                    reply(nil, "firebase/firestore: deleteDoc \(path) failed: \(error.localizedDescription)")
                } else {
                    reply([:], nil)
                }
            }
        case "firebase.firestore.runQuery":
            guard let collectionPath = string(data, "collectionPath") else {
                reply(nil, "firebase.firestore.runQuery needs a collectionPath"); return
            }
            var query: Query = db.collection(collectionPath)
            if let cond = args["where"] as? [String: Any] {
                guard let field = cond["field"] as? String, let op = cond["op"] as? String else {
                    reply(nil, "firebase/firestore: runQuery where needs a field and an op"); return
                }
                // A JSON null (or a missing value) compares against Firestore null.
                let value = cond["value"] ?? NSNull()
                switch op {
                case "==": query = query.whereField(field, isEqualTo: value)
                case "!=": query = query.whereField(field, isNotEqualTo: value)
                case "<": query = query.whereField(field, isLessThan: value)
                case "<=": query = query.whereField(field, isLessThanOrEqualTo: value)
                case ">": query = query.whereField(field, isGreaterThan: value)
                case ">=": query = query.whereField(field, isGreaterThanOrEqualTo: value)
                case "array-contains": query = query.whereField(field, arrayContains: value)
                case "in", "not-in", "array-contains-any":
                    guard let values = value as? [Any] else {
                        reply(nil, "firebase/firestore: where op \(op) needs an array value"); return
                    }
                    switch op {
                    case "in": query = query.whereField(field, in: values)
                    case "not-in": query = query.whereField(field, notIn: values)
                    default: query = query.whereField(field, arrayContainsAny: values)
                    }
                default:
                    reply(nil, "firebase/firestore: unknown where op \(op)"); return
                }
            }
            if let order = args["orderBy"] as? [String: Any], let field = order["field"] as? String {
                query = order["direction"] as? String == "desc"
                    ? query.order(by: field, descending: true)
                    : query.order(by: field)
            }
            if let limit = args["limit"] as? NSNumber {
                query = query.limit(to: limit.intValue)
            }
            query.getDocuments { snapshot, error in
                if let error {
                    reply(nil, "firebase/firestore: runQuery \(collectionPath) failed: \(error.localizedDescription)")
                    return
                }
                let docs = snapshot?.documents.map { doc in
                    ["id": doc.documentID, "data": doc.data()]
                } ?? []
                reply(["docs": docs], nil)
            }
        default:
            reply(nil, "no handler for \(name)")
        }
    }

    private func remoteConfig(_ name: String, _ data: Any?, _ reply: @escaping (Any?, String?) -> Void) {
        let config = RemoteConfig.remoteConfig()
        switch name {
        case "firebase.remote-config.setDefaults":
            guard let map = (data as? [String: Any])?["map"] as? [String: Any] else {
                reply(nil, "firebase.remote-config.setDefaults needs a map"); return
            }
            // setDefaults takes NSString-keyed NSObject values.
            let nsDefaults = map.mapValues { ($0 as? NSObject) ?? NSString(string: "\($0)") }
            config.setDefaults(nsDefaults)
            reply([:], nil)
        case "firebase.remote-config.getDefaults":
            var map: [String: Any] = [:]
            for key in config.allKeys(from: .default) {
                // jsonValue hands back the stored type (string/number/bool),
                // not the string view.
                map[key] = config.configValue(forKey: key, source: .default).jsonValue ?? NSNull()
            }
            reply(["map": map], nil)
        case "firebase.remote-config.fetchAndActivate":
            config.fetchAndActivate { status, error in
                if let error {
                    // The fake project's backend refuses: the rejection is the
                    // proof, the way the FCM and installations tokens reject.
                    reply(nil, "firebase/remote-config: fetchAndActivate failed: \(error.localizedDescription)")
                    return
                }
                reply([
                    "activated": status != .error,
                    "lastFetchStatus": self.fetchStatusName(config.lastFetchStatus),
                ], nil)
            }
        case "firebase.remote-config.get":
            guard let key = string(data, "key") else {
                reply(nil, "firebase.remote-config.get needs a key"); return
            }
            let value = config.configValue(forKey: key)
            // Static source = neither the backend nor a default ever set it:
            // null, not an empty string.
            reply(["value": value.source == .static ? nil : value.stringValue], nil)
        default:
            reply(nil, "no handler for \(name)")
        }
    }

    /// The fetch status names the facade uses (`no-fetch-yet` mirrors the
    /// SDK's LAST_FETCH_STATUS_NO_FETCH_YET). Nonisolated: the fetchAndActivate
    /// completion runs off the main actor.
    nonisolated private func fetchStatusName(_ status: RemoteConfigFetchStatus) -> String {
        switch status {
        case .success: return "success"
        case .failure: return "failure"
        case .throttled: return "throttled"
        case .noFetchYet: return "no-fetch-yet"
        @unknown default: return "unknown"
        }
    }

    private func crashlytics(_ name: String, _ data: Any?, _ reply: @escaping (Any?, String?) -> Void) {
        let args = data as? [String: Any] ?? [:]
        let crashlytics = Crashlytics.crashlytics()
        switch name {
        case "firebase.crashlytics.log":
            guard let message = args["message"] as? String else {
                reply(nil, "firebase.crashlytics.log needs a message"); return
            }
            crashlytics.log(message)
            reply([:], nil)
        case "firebase.crashlytics.setUserId":
            crashlytics.setUserID(args["id"] as? String)
            reply([:], nil)
        case "firebase.crashlytics.setCustomKey":
            guard let key = args["key"] as? String else {
                reply(nil, "firebase.crashlytics.setCustomKey needs a key"); return
            }
            // Scalars ride as-is (NSString/NSNumber); null clears the key,
            // which is setCustomValue(_:forKey:) passing nil.
            crashlytics.setCustomValue(args["value"], forKey: key)
            reply([:], nil)
        case "firebase.crashlytics.recordError":
            guard let message = args["message"] as? String else {
                reply(nil, "firebase.crashlytics.recordError needs a message"); return
            }
            let code = (args["code"] as? NSNumber)?.intValue ?? 0
            // A JS stack is not a native one: it rides as a breadcrumb log
            // next to the synthesized NSError, RNFB's
            // recordJavaScriptError shape (NativeRNFBTurboCrashlytics.java).
            if let stack = args["stack"] as? String {
                crashlytics.log(stack)
            }
            crashlytics.record(error: NSError(
                domain: "dev.nsh.firebase.crashlytics", code: code,
                userInfo: [NSLocalizedDescriptionKey: message]))
            reply([:], nil)
        case "firebase.crashlytics.sendUnsentReports":
            crashlytics.sendUnsentReports()
            reply([:], nil)
        case "firebase.crashlytics.didCrashDuringPreviousExecution":
            reply(["crashed": crashlytics.didCrashDuringPreviousExecution()], nil)
        default:
            reply(nil, "no handler for \(name)")
        }
    }

    private func appCheckGetToken(_ data: Any?, _ reply: @escaping (Any?, String?) -> Void) {
        // The debug provider only prints a token the backend accepts in debug
        // mode; under the fake project the fetch itself still fails and the
        // handler rejects.
        if (data as? [String: Any])?["debug"] as? Bool == true, !Self.appCheckFactorySet {
            AppCheck.setAppCheckProviderFactory(AppCheckDebugProviderFactory())
            Self.appCheckFactorySet = true
        }
        AppCheck.appCheck().token(forcingRefresh: false) { token, error in
            if let token {
                reply(["token": token.token], nil)
            } else {
                reply(nil, "firebase/app-check: getToken failed: \(error?.localizedDescription ?? "no error given")")
            }
        }
    }

    private func functionsCall(_ data: Any?, _ reply: @escaping (Any?, String?) -> Void) {
        guard let name = string(data, "name") else {
            reply(nil, "firebase.functions.call needs a name"); return
        }
        let payload = (data as? [String: Any])?["data"]
        // Default instance = us-central1; a named region is not in the
        // handler contract.
        Functions.functions().httpsCallable(name).call(payload) { result, error in
            if let error {
                reply(nil, "firebase/functions: call \(name) failed: \(error.localizedDescription)")
                return
            }
            reply(["result": result?.data ?? NSNull()], nil)
        }
    }

    // MARK: Incoming FCM messages

    /// The dictionary the app gets on `firebase.messaging.message`, after
    /// RNFB's RNFBMessagingSerializerStorage.remoteMessageUserInfoToDict:
    /// message-level fields pulled out, `data` carrying everything that is
    /// neither those nor the notification part (`aps`, `gcm.*`, `google.*`).
    /// Omitted next to RNFB: `category` and `threadId` (aps), the
    /// `contentAvailable`/`mutableContent` flags, the alert loc-keys, and
    /// the whole `notification.ios` sub-object (badge, subtitle, sound); the
    /// TSX RemoteMessage carries this narrower shape.
    private func messageDict(_ userInfo: [AnyHashable: Any]) -> [String: Any] {
        var message: [String: Any] = [:]
        var data: [String: Any] = [:]
        for (key, value) in userInfo {
            let name = key as? String ?? String(describing: key)
            switch name {
            case "gcm.message_id", "google.message_id", "message_id":
                message["messageId"] = value
            case "message_type":
                message["messageType"] = value
            case "collapse_key":
                message["collapseKey"] = value
            case "from", "google.c.sender.id":
                message["from"] = value
            case "to", "google.to":
                message["to"] = value
            case "google.c.a.ts":
                if let seconds = sentTimeSeconds(value) {
                    message["sentTime"] = NSNumber(value: seconds * 1000)
                }
            default:
                if name != "aps" && !name.hasPrefix("gcm.") && !name.hasPrefix("google.") {
                    data[name] = value
                }
            }
        }
        message["data"] = data
        var notification: [String: Any] = [:]
        if let aps = userInfo["aps"] as? [String: Any] {
            if let alert = aps["alert"] as? String {
                notification["title"] = alert
            } else if let alert = aps["alert"] as? [String: Any] {
                if let title = alert["title"] { notification["title"] = title }
                if let body = alert["body"] { notification["body"] = body }
            }
        }
        if !notification.isEmpty { message["notification"] = notification }
        return message
    }

    /// `google.c.a.ts` is seconds, as a string or a number; RNFB keeps only a
    /// decimal integer and converts to milliseconds.
    private func sentTimeSeconds(_ value: Any) -> Int64? {
        let text: String?
        switch value {
        case let string as String: text = string
        case let number as NSNumber: text = number.stringValue
        default: text = nil
        }
        guard let text, !text.isEmpty,
            text.unicodeScalars.allSatisfy({ (48...57).contains($0.value) })
        else { return nil }
        let seconds = Int64(text) ?? 0
        return (seconds > 0 && seconds <= Int64.max / 1000) ? seconds : nil
    }

    private func emitMessage(_ userInfo: [AnyHashable: Any]) {
        sendEvent?("firebase.messaging.message", messageDict(userInfo))
        log("message received")
    }

    private func emitPress(_ identifier: String) {
        sendEvent?("firebase.messaging.press", ["id": identifier])
        log("press \(identifier)")
    }

    // MARK: UNUserNotificationCenterDelegate

    /// Called on the center's queue, not the main actor. The completion must
    /// run exactly once, so forward only when the previous delegate really
    /// implemented the method (RNFB's `respondsToSelector:` guards).
    nonisolated private func forwardWillPresent(
        _ center: UNUserNotificationCenter, _ notification: UNNotification,
        _ completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) -> Bool {
        guard
            let previous = previousCenterDelegate,
            previous.responds(
                to: #selector(
                    UNUserNotificationCenterDelegate.userNotificationCenter(
                        _:willPresent:withCompletionHandler:)))
        else { return false }
        previous.userNotificationCenter?(
            center, willPresent: notification, withCompletionHandler: completionHandler)
        return true
    }

    /// Foreground presentation of a notification, ours or an FCM one: an FCM
    /// alert replays the Notifications module's banner + list + badge (sound
    /// rides the payload), and data-only ones arrive through the app delegate
    /// instead, as RNFB documents.
    nonisolated public func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        let userInfo = notification.request.content.userInfo
        let contentAvailable = (userInfo["aps"] as? [String: Any])?["content-available"]
        if userInfo["gcm.message_id"] != nil, (contentAvailable as? NSNumber)?.boolValue != true {
            // An alert delivered in the foreground: the app delegate's
            // `didReceiveRemoteNotification` owns data-only ones, so skip
            // them here (RNFB skips them for the same reason).
            let messageId = userInfo["gcm.message_id"] as? String
            var repeated = false
            if #available(iOS 18.0, *) {
                if #unavailable(iOS 18.1) {
                    if messageId == lastWillPresentMessageId {
                        repeated = true
                    } else {
                        lastWillPresentMessageId = messageId
                    }
                }
            }
            if !repeated {
                Task { @MainActor in self.emitMessage(userInfo) }
            }
        }
        if forwardWillPresent(center, notification, completionHandler) { return }
        // The Notifications module's presentation set: badge, sound, list,
        // banner.
        completionHandler([.badge, .sound, .list, .banner])
    }

    /// A tap on a notification, including one that started the app: the
    /// identifier rides `firebase.messaging.press` (the `{id}` shape), and
    /// `app.send` holds it until the app listens.
    nonisolated public func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let identifier = response.notification.request.identifier
        Task { @MainActor in self.emitPress(identifier) }
        if
            let previous = previousCenterDelegate,
            previous.responds(
                to: #selector(
                    UNUserNotificationCenterDelegate.userNotificationCenter(
                        _:didReceive:withCompletionHandler:)))
        {
            previous.userNotificationCenter?(
                center, didReceive: response, withCompletionHandler: completionHandler)
        } else {
            completionHandler()
        }
    }

    // MARK: MessagingDelegate

    nonisolated public func messaging(
        _ messaging: Messaging, didReceiveRegistrationToken fcmToken: String?
    ) {
        let token = fcmToken ?? ""
        Task { @MainActor in
            self.sendEvent?("firebase.messaging.tokenRefreshed", ["token": token])
            self.log("token refreshed")
        }
        previousMessagingDelegate?.messaging?(messaging, didReceiveRegistrationToken: fcmToken)
    }

    private func log(_ line: String) {
        onActivity?(line)
    }

    private func string(_ data: Any?, _ key: String) -> String? {
        (data as? [String: Any])?[key] as? String
    }
}
