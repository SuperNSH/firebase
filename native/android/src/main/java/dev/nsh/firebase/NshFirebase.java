package dev.nsh.firebase;

import android.Manifest;
import android.app.Activity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.ContextWrapper;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import com.google.firebase.FirebaseApp;
import com.google.firebase.FirebaseOptions;
import com.google.firebase.analytics.FirebaseAnalytics;
import com.google.firebase.appcheck.FirebaseAppCheck;
import com.google.firebase.appcheck.debug.DebugAppCheckProviderFactory;
import com.google.firebase.crashlytics.FirebaseCrashlytics;
import com.google.firebase.firestore.DocumentSnapshot;
import com.google.firebase.firestore.FirebaseFirestore;
import com.google.firebase.firestore.Query;
import com.google.firebase.firestore.SetOptions;
import com.google.firebase.functions.FirebaseFunctions;
import com.google.firebase.installations.FirebaseInstallations;
import com.google.firebase.messaging.FirebaseMessaging;
import com.google.firebase.perf.FirebasePerformance;
import com.google.firebase.perf.metrics.HttpMetric;
import com.google.firebase.perf.metrics.Trace;
import com.google.firebase.remoteconfig.FirebaseRemoteConfig;
import com.google.firebase.remoteconfig.FirebaseRemoteConfigValue;

import org.json.JSONArray;
import org.json.JSONObject;

import java.lang.ref.WeakReference;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.EnumMap;
import java.util.HashMap;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.UUID;

/**
 * The native Firebase tier for an NSH embedding host. {@link #init} sets up
 * the Firebase app and {@link #register} installs the {@code firebase.*}
 * host handlers on an {@code NshView}; the app reaches them with {@code
 * host.call}/{@code host.post} and hears {@code firebase.messaging.message},
 * {@code firebase.messaging.tokenRefreshed} and {@code firebase.messaging.press}
 * as {@code host.on} events.
 *
 * <p>Permission, show, close, setBadge and press buffering follow the
 * semantics of nsh's unreleased {@code Notifications} module (nsh-main
 * {@code Notifications.java}): one channel, showing again with the same id
 * replaces, a press opens the launcher activity with the id in an extra and
 * presses wait in a queue until the app listens.
 */
public final class NshFirebase {
    private static final String TAG = "NshFirebase";
    private static final String CHANNEL = "dev.nsh.firebase.notifications";
    private static final String PRESS_EXTRA = "dev.nsh.firebase.NOTIFICATION";
    private static final String PREFS = "dev.nsh.firebase.messaging";
    private static final String PREF_ASKED = "asked";
    private static final String PREF_BADGE = "badge";
    private static final String PREF_PENDING = "pending";
    /** RNFB's store caps the same queue at 100 (ReactNativeFirebaseMessagingStoreImpl). */
    private static final int PENDING_MAX = 100;
    /** The id of every notification, the app's id being its tag. */
    private static final int NOTIFICATION_ID = 1;
    private static final String SILENT_GROUP = "dev.nsh.firebase.silent";

    /**
     * The fake fixture values (fixtures/google-services.json, project demo-nsh).
     * init(Context) reads assets/google-services.json when the host packs one;
     * these defaults keep the module usable without it.
     */
    private static FirebaseOptions defaultOptions() {
        return new FirebaseOptions.Builder()
                .setApplicationId("1:123456789012:android:abc123def456")
                .setApiKey("fake-android-key-abc123")
                .setProjectId("demo-nsh")
                .setGcmSenderId("123456789012")
                .setStorageBucket("demo-nsh.appspot.com")
                .build();
    }

    private static WeakReference<Context> appContext;
    private static WeakReference<com.longbridge.nsh.NshView> viewRef;
    private static final Handler main = new Handler(Looper.getMainLooper());
    private static final Map<String, Trace> traces = new HashMap<>();
    /** Set once the App Check provider factory was swapped; it must be in place before the first token fetch. */
    private static boolean appCheckFactorySet;
    private static Listener listener;

    /** Host-side hook (the demo's status line), not part of the app contract. */
    public interface Listener {
        void onEvent(String line);
    }

    public static void setListener(Listener value) {
        listener = value;
    }

    static void report(String line) {
        Log.d(TAG, line);
        Listener l = listener;
        if (l != null) l.onEvent(line);
    }

    private NshFirebase() {}

    /**
     * Initializes the default FirebaseApp from the host's {@code
     * google-services.json} asset, or from the bundled fake fixture values.
     * No google-services Gradle plugin and no real credentials: the fixture's
     * fake key reaches Firebase's network only when something calls out, and
     * such failures surface through the handlers' rejections.
     */
    public static synchronized boolean init(Context context) {
        // FCM's auto-posted background notifications go to the channel the
        // manifest meta-data names, so it must exist before any delivery.
        ensureChannel(context);
        if (!FirebaseApp.getApps(context).isEmpty()) return true;
        FirebaseOptions options = optionsFromAssets(context);
        if (options == null) options = defaultOptions();
        FirebaseApp app = FirebaseApp.initializeApp(context, options);
        report(app != null ? "FirebaseApp initialized (project " + options.getProjectId() + ")" : "FirebaseApp was null");
        return app != null;
    }

    /** Idempotent: the manifest meta-data points FCM's own posts at this channel. */
    private static void ensureChannel(Context context) {
        if (Build.VERSION.SDK_INT < 26) return;
        NotificationManager manager = manager(context);
        if (manager.getNotificationChannel(CHANNEL) != null) return;
        NotificationChannel channel = new NotificationChannel(CHANNEL, "Notifications", NotificationManager.IMPORTANCE_HIGH);
        channel.setShowBadge(true);
        manager.createNotificationChannel(channel);
    }

    /** The host packs fixtures/google-services.json into assets as the plugin would. */
    private static FirebaseOptions optionsFromAssets(Context context) {
        try (java.io.InputStream in = context.getAssets().open("google-services.json")) {
            byte[] buffer = new byte[8192];
            StringBuilder text = new StringBuilder();
            int n;
            while ((n = in.read(buffer)) > 0) text.append(new String(buffer, 0, n));
            JSONObject json = new JSONObject(text.toString());
            JSONObject info = json.getJSONObject("project_info");
            JSONObject client = json.getJSONArray("client").getJSONObject(0);
            JSONObject clientInfo = client.getJSONObject("client_info");
            String apiKey = client.getJSONArray("api_key").getJSONObject(0).getString("current_key");
            return new FirebaseOptions.Builder()
                    .setApplicationId(clientInfo.getString("mobilesdk_app_id"))
                    .setApiKey(apiKey)
                    .setProjectId(info.getString("project_id"))
                    .setGcmSenderId(info.getString("project_number"))
                    .setStorageBucket(info.optString("storage_bucket", null))
                    .build();
        } catch (Exception e) {
            return null;
        }
    }

    /**
     * Installs the {@code firebase.*} handlers on the view and replays
     * messages and presses that arrived while the app was not listening.
     * Call before {@code loadAssetBundle}.
     */
    public static void register(com.longbridge.nsh.NshView nsh) {
        viewRef = new WeakReference<>(nsh);
        appContext = new WeakReference<>(nsh.getContext().getApplicationContext());

        String[] names = {
                "firebase.messaging.token", "firebase.messaging.permission",
                "firebase.messaging.requestPermission", "firebase.messaging.show",
                "firebase.messaging.close", "firebase.messaging.setBadge",
                "firebase.messaging.subscribe", "firebase.messaging.unsubscribe",
                "firebase.analytics.logEvent", "firebase.analytics.setUserId",
                "firebase.analytics.setUserProperty", "firebase.analytics.logScreenView",
                "firebase.analytics.setConsent",
                "firebase.perf.startTrace", "firebase.perf.stopTrace",
                "firebase.perf.recordHttpMetric",
                "firebase.firestore.getDoc", "firebase.firestore.setDoc",
                "firebase.firestore.addDoc", "firebase.firestore.deleteDoc",
                "firebase.firestore.runQuery",
                "firebase.remote-config.setDefaults", "firebase.remote-config.getDefaults",
                "firebase.remote-config.fetchAndActivate", "firebase.remote-config.get",
                "firebase.crashlytics.log", "firebase.crashlytics.setUserId",
                "firebase.crashlytics.setCustomKey", "firebase.crashlytics.recordError",
                "firebase.crashlytics.sendUnsentReports",
                "firebase.crashlytics.didCrashDuringPreviousExecution",
                "firebase.installations.getId", "firebase.installations.getToken",
                "firebase.appCheck.getToken",
                "firebase.functions.call",
        };
        for (String name : names) nsh.handle(name, (data, reply) -> handle(name, data, reply));
    }

    /**
     * The host calls this after {@code loadAssetBundle}: a queue that could
     * not go out before the load drains now. Events for a loaded app without
     * a listener are covered by nsh's own buffer (256 events in all, oldest
     * dropped first).
     */
    public static void appLoaded() {
        main.post(NshFirebase::drainPending);
    }

    private static void handle(String name, Object data, com.longbridge.nsh.NshHostReply reply) {
        Map<?, ?> args = data instanceof Map ? (Map<?, ?>) data : null;
        Context context = appContext != null ? appContext.get() : null;
        report("handle " + name);
        main.post(() -> dispatch(name, args, context, reply));
    }

    private static void dispatch(String name, Map<?, ?> args, Context context,
                                 com.longbridge.nsh.NshHostReply reply) {
        try {
            switch (name) {
                case "firebase.messaging.token": token(reply); break;
                case "firebase.messaging.permission":
                    reply.resolve(map("permission", permission(context)));
                    break;
                case "firebase.messaging.requestPermission":
                    requestPermission(context, reply);
                    break;
                case "firebase.messaging.show": {
                    boolean shown = show(context, str(args, "id"), str(args, "title"),
                            str(args, "body"), bool(args, "silent"));
                    reply.resolve(shown);
                    break;
                }
                case "firebase.messaging.close": {
                    manager(context).cancel(str(args, "id"), NOTIFICATION_ID);
                    reply.resolve(Boolean.TRUE);
                    break;
                }
                case "firebase.messaging.setBadge": {
                    int count = (int) number(args, "count");
                    prefs(context).edit().putInt(PREF_BADGE, count).apply();
                    reply.resolve(Boolean.TRUE);
                    break;
                }
                case "firebase.messaging.subscribe": topic(args, reply, true); break;
                case "firebase.messaging.unsubscribe": topic(args, reply, false); break;
                case "firebase.analytics.logEvent": {
                    Bundle params = bundle(args == null ? null : args.get("params"));
                    analytics(context).logEvent(str(args, "name"), params);
                    reply.resolve(Boolean.TRUE);
                    break;
                }
                case "firebase.analytics.setUserId": {
                    analytics(context).setUserId(str(args, "userId"));
                    reply.resolve(Boolean.TRUE);
                    break;
                }
                case "firebase.analytics.setUserProperty": {
                    analytics(context).setUserProperty(str(args, "name"), str(args, "value"));
                    reply.resolve(Boolean.TRUE);
                    break;
                }
                case "firebase.analytics.logScreenView": {
                    Bundle params = new Bundle();
                    params.putString(FirebaseAnalytics.Param.SCREEN_NAME, str(args, "screenName"));
                    String screenClass = args == null ? null : (String) args.get("screenClass");
                    if (screenClass != null) params.putString(FirebaseAnalytics.Param.SCREEN_CLASS, screenClass);
                    analytics(context).logEvent(FirebaseAnalytics.Event.SCREEN_VIEW, params);
                    reply.resolve(Boolean.TRUE);
                    break;
                }
                case "firebase.analytics.setConsent": {
                    Map<FirebaseAnalytics.ConsentType, FirebaseAnalytics.ConsentStatus> consent =
                            new EnumMap<>(FirebaseAnalytics.ConsentType.class);
                    putConsent(consent, args, "analyticsStorage", FirebaseAnalytics.ConsentType.ANALYTICS_STORAGE);
                    putConsent(consent, args, "adStorage", FirebaseAnalytics.ConsentType.AD_STORAGE);
                    putConsent(consent, args, "adUserData", FirebaseAnalytics.ConsentType.AD_USER_DATA);
                    putConsent(consent, args, "adPersonalization", FirebaseAnalytics.ConsentType.AD_PERSONALIZATION);
                    analytics(context).setConsent(consent);
                    reply.resolve(Boolean.TRUE);
                    break;
                }
                case "firebase.perf.startTrace": {
                    // startTrace is static: the trace starts whether or not a caller kept it.
                    Trace trace = FirebasePerformance.startTrace(str(args, "name"));
                    String id = UUID.randomUUID().toString();
                    traces.put(id, trace);
                    reply.resolve(map("id", id));
                    break;
                }
                case "firebase.perf.stopTrace": {
                    String id = str(args, "id");
                    Trace trace = traces.remove(id);
                    if (trace == null) {
                        reply.reject("firebase/perf: no open trace with id " + id);
                        break;
                    }
                    trace.stop();
                    reply.resolve(Boolean.TRUE);
                    break;
                }
                case "firebase.perf.recordHttpMetric": recordHttpMetric(args, context, reply); break;
                case "firebase.firestore.getDoc": firestoreGetDoc(args, reply); break;
                case "firebase.firestore.setDoc": firestoreSetDoc(args, reply); break;
                case "firebase.firestore.addDoc": firestoreAddDoc(args, reply); break;
                case "firebase.firestore.deleteDoc": firestoreDeleteDoc(args, reply); break;
                case "firebase.firestore.runQuery": firestoreRunQuery(args, reply); break;
                case "firebase.remote-config.setDefaults": remoteConfigSetDefaults(args, reply); break;
                case "firebase.remote-config.getDefaults": remoteConfigGetDefaults(reply); break;
                case "firebase.remote-config.fetchAndActivate": remoteConfigFetchAndActivate(reply); break;
                case "firebase.remote-config.get": remoteConfigGet(args, reply); break;
                case "firebase.crashlytics.log":
                    FirebaseCrashlytics.getInstance().log(str(args, "message"));
                    reply.resolve(Boolean.TRUE);
                    break;
                case "firebase.crashlytics.setUserId":
                    FirebaseCrashlytics.getInstance().setUserId(str(args, "id"));
                    reply.resolve(Boolean.TRUE);
                    break;
                case "firebase.crashlytics.setCustomKey":
                    crashlyticsSetCustomKey(args);
                    reply.resolve(Boolean.TRUE);
                    break;
                case "firebase.crashlytics.recordError": crashlyticsRecordError(args); reply.resolve(Boolean.TRUE); break;
                case "firebase.crashlytics.sendUnsentReports":
                    FirebaseCrashlytics.getInstance().sendUnsentReports();
                    reply.resolve(Boolean.TRUE);
                    break;
                case "firebase.crashlytics.didCrashDuringPreviousExecution":
                    reply.resolve(map("crashed",
                            FirebaseCrashlytics.getInstance().didCrashOnPreviousExecution()));
                    break;
                case "firebase.installations.getId":
                    FirebaseInstallations.getInstance().getId()
                            .addOnSuccessListener(id -> reply.resolve(map("id", id)))
                            .addOnFailureListener(e -> reply.reject(
                                    "firebase/installations: getId failed: " + e.getMessage()));
                    break;
                case "firebase.installations.getToken":
                    // The fake project has no backend: this rejection is the
                    // honest proof the request was made, like the FCM token's.
                    FirebaseInstallations.getInstance().getToken(false)
                            .addOnSuccessListener(result -> reply.resolve(map("token", result.getToken())))
                            .addOnFailureListener(e -> reply.reject(
                                    "firebase/installations: getToken failed: " + e.getMessage()));
                    break;
                case "firebase.appCheck.getToken": appCheckGetToken(args, reply); break;
                case "firebase.functions.call": functionsCall(args, reply); break;
                default: reply.reject("firebase/unimplemented: " + name);
            }
        } catch (Exception e) {
            reply.reject("firebase/" + name + " failed: " + e.getMessage());
        }
    }

    /** The token request is the sandbox's proof point: with the fake project it must reject clearly. */
    private static void token(com.longbridge.nsh.NshHostReply reply) {
        FirebaseMessaging messaging = FirebaseMessaging.getInstance();
        messaging.getToken().addOnSuccessListener(token -> reply.resolve(map("token", token)))
                .addOnFailureListener(e -> reply.reject("firebase/token: FCM registration failed: " + e.getMessage()));
    }

    private static void topic(Map<?, ?> args, com.longbridge.nsh.NshHostReply reply, boolean subscribe) {
        String topicName = str(args, "topic");
        FirebaseMessaging messaging = FirebaseMessaging.getInstance();
        com.google.android.gms.tasks.Task<Void> task = subscribe
                ? messaging.subscribeToTopic(topicName)
                : messaging.unsubscribeFromTopic(topicName);
        task.addOnSuccessListener(v -> reply.resolve(Boolean.TRUE))
                .addOnFailureListener(e -> reply.reject("firebase/topics: " + (subscribe ? "subscribe " : "unsubscribe ") + topicName + " failed: " + e.getMessage()));
    }

    /**
     * Records a finished NetworkEvent. HttpMetric's public API cannot take an
     * explicit duration, so the event's sizes and status go in and the timing
     * the SDK measures is its own; real call durations come from the SDK's
     * automatic capture.
     */
    private static void recordHttpMetric(Map<?, ?> event, Context context, com.longbridge.nsh.NshHostReply reply) {
        String url = str(event, "url");
        if (url.isEmpty()) {
            reply.reject("firebase/perf: recordHttpMetric needs a NetworkEvent with a url");
            return;
        }
        HttpMetric metric = performance(context).newHttpMetric(url, str(event, "method", "GET"));
        Number status = (Number) event.get("status");
        if (status != null) metric.setHttpResponseCode(status.intValue());
        Number requestBytes = (Number) event.get("requestBytes");
        if (requestBytes != null) metric.setRequestPayloadSize(requestBytes.longValue());
        Number responseBytes = (Number) event.get("responseBytes");
        if (responseBytes != null) metric.setResponsePayloadSize(responseBytes.longValue());
        metric.start();
        metric.stop();
        reply.resolve(Boolean.TRUE);
    }

    // --- Stretch handlers: Firestore, Remote Config, Crashlytics, Installations, App Check, Functions ---
    //
    // Every failure resolves through reply.reject with a firebase/<product>:
    // prefix, and under the fake project anything that reaches Google's
    // network rejects — that rejection is the sandbox's proof, not a bug.

    private static void firestoreGetDoc(Map<?, ?> args, com.longbridge.nsh.NshHostReply reply) {
        String path = str(args, "path");
        if (path.isEmpty()) {
            reply.reject("firebase/firestore: getDoc needs a path");
            return;
        }
        FirebaseFirestore.getInstance().document(path).get()
                .addOnSuccessListener(doc -> {
                    Map<String, Object> out = new HashMap<>();
                    out.put("exists", doc.exists());
                    // A missing document resolves (exists:false, data:null);
                    // it is Firestore's own semantics, not an error.
                    out.put("data", doc.getData());
                    reply.resolve(out);
                })
                .addOnFailureListener(e -> reply.reject(
                        "firebase/firestore: getDoc " + path + " failed: " + e.getMessage()));
    }

    private static void firestoreSetDoc(Map<?, ?> args, com.longbridge.nsh.NshHostReply reply) {
        String path = str(args, "path");
        Object data = args == null ? null : args.get("data");
        if (path.isEmpty() || !(data instanceof Map)) {
            reply.reject("firebase/firestore: setDoc needs a path and a data object");
            return;
        }
        @SuppressWarnings("unchecked")
        com.google.android.gms.tasks.Task<Void> task = bool(args, "merge")
                ? FirebaseFirestore.getInstance().document(path).set((Map<String, Object>) data, SetOptions.merge())
                : FirebaseFirestore.getInstance().document(path).set((Map<String, Object>) data);
        task.addOnSuccessListener(v -> reply.resolve(Boolean.TRUE))
                .addOnFailureListener(e -> reply.reject(
                        "firebase/firestore: setDoc " + path + " failed: " + e.getMessage()));
    }

    private static void firestoreAddDoc(Map<?, ?> args, com.longbridge.nsh.NshHostReply reply) {
        String collectionPath = str(args, "collectionPath");
        Object data = args == null ? null : args.get("data");
        if (collectionPath.isEmpty() || !(data instanceof Map)) {
            reply.reject("firebase/firestore: addDoc needs a collectionPath and a data object");
            return;
        }
        @SuppressWarnings("unchecked")
        com.google.android.gms.tasks.Task<com.google.firebase.firestore.DocumentReference> task =
                FirebaseFirestore.getInstance().collection(collectionPath).add((Map<String, Object>) data);
        task.addOnSuccessListener(ref -> reply.resolve(map("id", ref.getId())))
                .addOnFailureListener(e -> reply.reject(
                        "firebase/firestore: addDoc " + collectionPath + " failed: " + e.getMessage()));
    }

    private static void firestoreDeleteDoc(Map<?, ?> args, com.longbridge.nsh.NshHostReply reply) {
        String path = str(args, "path");
        if (path.isEmpty()) {
            reply.reject("firebase/firestore: deleteDoc needs a path");
            return;
        }
        FirebaseFirestore.getInstance().document(path).delete()
                .addOnSuccessListener(v -> reply.resolve(Boolean.TRUE))
                .addOnFailureListener(e -> reply.reject(
                        "firebase/firestore: deleteDoc " + path + " failed: " + e.getMessage()));
    }

    /**
     * `where` is one condition: `{field, op, value}` with an op of `==`, `!=`,
     * `<`, `<=`, `>`, `>=`, `array-contains`, `in`, `not-in` or
     * `array-contains-any`; `orderBy` is `{field, direction?}` with
     * `asc`/`desc`; `limit` is a number.
     */
    private static void firestoreRunQuery(Map<?, ?> args, com.longbridge.nsh.NshHostReply reply) {
        String collectionPath = str(args, "collectionPath");
        if (collectionPath.isEmpty()) {
            reply.reject("firebase/firestore: runQuery needs a collectionPath");
            return;
        }
        Query query = FirebaseFirestore.getInstance().collection(collectionPath);
        Map<?, ?> cond = mapArg(args, "where");
        if (cond != null) {
            String field = str(cond, "field");
            String op = str(cond, "op");
            Object value = cond.get("value");
            switch (op) {
                case "==": query = query.whereEqualTo(field, value); break;
                case "!=": query = query.whereNotEqualTo(field, value); break;
                case "<": query = query.whereLessThan(field, value); break;
                case "<=": query = query.whereLessThanOrEqualTo(field, value); break;
                case ">": query = query.whereGreaterThan(field, value); break;
                case ">=": query = query.whereGreaterThanOrEqualTo(field, value); break;
                case "array-contains": query = query.whereArrayContains(field, value); break;
                case "in": query = query.whereIn(field, objectList(value)); break;
                case "not-in": query = query.whereNotIn(field, objectList(value)); break;
                case "array-contains-any": query = query.whereArrayContainsAny(field, objectList(value)); break;
                default:
                    reply.reject("firebase/firestore: unknown where op " + op);
                    return;
            }
        }
        Map<?, ?> order = mapArg(args, "orderBy");
        if (order != null && !str(order, "field").isEmpty()) {
            query = query.orderBy(str(order, "field"),
                    "desc".equals(str(order, "direction"))
                            ? Query.Direction.DESCENDING : Query.Direction.ASCENDING);
        }
        Number limit = (Number) (args == null ? null : args.get("limit"));
        if (limit != null) query = query.limit(limit.intValue());
        query.get()
                .addOnSuccessListener(snapshot -> {
                    List<Map<String, Object>> docs = new ArrayList<>();
                    for (DocumentSnapshot doc : snapshot.getDocuments()) {
                        Map<String, Object> entry = new HashMap<>();
                        entry.put("id", doc.getId());
                        entry.put("data", doc.getData());
                        docs.add(entry);
                    }
                    reply.resolve(map("docs", docs));
                })
                .addOnFailureListener(e -> reply.reject(
                        "firebase/firestore: runQuery " + collectionPath + " failed: " + e.getMessage()));
    }

    private static void remoteConfigSetDefaults(Map<?, ?> args, com.longbridge.nsh.NshHostReply reply) {
        Map<?, ?> raw = mapArg(args, "map");
        if (raw == null) {
            reply.reject("firebase/remote-config: setDefaults needs a map");
            return;
        }
        Map<String, Object> defaults = new HashMap<>();
        for (Map.Entry<?, ?> entry : raw.entrySet()) {
            Object value = entry.getValue();
            // The defaults map takes concrete values; a null default would
            // read back as unset, so it stays out.
            if (value != null && value != JSONObject.NULL) {
                defaults.put(String.valueOf(entry.getKey()), value);
            }
        }
        FirebaseRemoteConfig.getInstance().setDefaultsAsync(defaults)
                .addOnSuccessListener(v -> reply.resolve(Boolean.TRUE))
                .addOnFailureListener(e -> reply.reject(
                        "firebase/remote-config: setDefaults failed: " + e.getMessage()));
    }

    /**
     * Read back from the SDK (source DEFAULT), not from a module-side copy.
     * The SDK's canonical view of a default is its string form, so the type
     * is re-parsed and a default stored as string "5" reads back as number 5.
     */
    private static void remoteConfigGetDefaults(com.longbridge.nsh.NshHostReply reply) {
        Map<String, Object> map = new HashMap<>();
        for (Map.Entry<String, FirebaseRemoteConfigValue> entry
                : FirebaseRemoteConfig.getInstance().getAll().entrySet()) {
            if (entry.getValue().getSource() != FirebaseRemoteConfig.VALUE_SOURCE_DEFAULT) continue;
            map.put(entry.getKey(), canonicalJsonValue(entry.getValue().asString()));
        }
        reply.resolve(map("map", map));
    }

    private static void remoteConfigFetchAndActivate(com.longbridge.nsh.NshHostReply reply) {
        FirebaseRemoteConfig.getInstance().fetchAndActivate()
                .addOnSuccessListener(activated -> {
                    Map<String, Object> out = new HashMap<>();
                    out.put("activated", activated);
                    out.put("lastFetchStatus", fetchStatusName(
                            FirebaseRemoteConfig.getInstance().getInfo().getLastFetchStatus()));
                    reply.resolve(out);
                })
                .addOnFailureListener(e -> reply.reject(
                        "firebase/remote-config: fetchAndActivate failed: " + e.getMessage()));
    }

    private static void remoteConfigGet(Map<?, ?> args, com.longbridge.nsh.NshHostReply reply) {
        FirebaseRemoteConfigValue value =
                FirebaseRemoteConfig.getInstance().getValue(str(args, "key"));
        // Static source = neither the backend nor a default ever set it: null.
        boolean unset = value.getSource() == FirebaseRemoteConfig.VALUE_SOURCE_STATIC;
        reply.resolve(map("value", unset ? null : value.asString()));
    }

    private static void crashlyticsSetCustomKey(Map<?, ?> args) {
        String key = str(args, "key");
        Object value = args == null ? null : args.get("value");
        FirebaseCrashlytics crashlytics = FirebaseCrashlytics.getInstance();
        if (value instanceof Boolean) {
            crashlytics.setCustomKey(key, (Boolean) value);
        } else if (value instanceof Number) {
            double d = ((Number) value).doubleValue();
            if (d == Math.rint(d) && !Double.isInfinite(d) && Math.abs(d) < 9.2e18) {
                crashlytics.setCustomKey(key, (long) d);
            } else {
                crashlytics.setCustomKey(key, d);
            }
        } else {
            crashlytics.setCustomKey(key, value == null ? "" : String.valueOf(value));
        }
    }

    /**
     * A JS error is not a Java one: the code and message ride in a named
     * synthetic exception, the optional stack string is parsed into frames,
     * and the handler's own Java stack is dropped — RNFB's
     * recordJavaScriptError shape (NativeRNFBTurboCrashlytics.java).
     */
    private static void crashlyticsRecordError(Map<?, ?> args) {
        String message = str(args, "message");
        long code = number(args, "code");
        RecordedError error = new RecordedError(code, message);
        String stack = str(args, "stack", "");
        error.setStackTrace(stack.isEmpty() ? new StackTraceElement[0] : stackFrames(stack));
        FirebaseCrashlytics.getInstance().recordException(error);
    }

    private static void appCheckGetToken(Map<?, ?> args, com.longbridge.nsh.NshHostReply reply) {
        // The debug provider only produces a token the backend accepts in
        // debug mode; under the fake project the fetch itself still fails.
        if (bool(args, "debug") && !appCheckFactorySet) {
            // installAppCheckProviderFactory is the instance method
            // FlutterFire's plugin uses on this BoM version.
            FirebaseAppCheck.getInstance()
                    .installAppCheckProviderFactory(DebugAppCheckProviderFactory.getInstance());
            appCheckFactorySet = true;
        }
        FirebaseAppCheck.getInstance().getAppCheckToken(false)
                .addOnSuccessListener(token -> reply.resolve(map("token", token.getToken())))
                .addOnFailureListener(e -> reply.reject(
                        "firebase/app-check: getToken failed: " + e.getMessage()));
    }

    private static void functionsCall(Map<?, ?> args, com.longbridge.nsh.NshHostReply reply) {
        String name = str(args, "name");
        if (name.isEmpty()) {
            reply.reject("firebase/functions: call needs a name");
            return;
        }
        // Default instance = us-central1; a named region is not in the
        // handler contract.
        FirebaseFunctions.getInstance().getHttpsCallable(name)
                .call(args == null ? null : args.get("data"))
                .addOnSuccessListener(result -> reply.resolve(map("result", result.getData())))
                .addOnFailureListener(e -> reply.reject(
                        "firebase/functions: call " + name + " failed: " + e.getMessage()));
    }

    /** `no-fetch-yet` mirrors LAST_FETCH_STATUS_NO_FETCH_YET. */
    private static String fetchStatusName(int status) {
        if (status == FirebaseRemoteConfig.LAST_FETCH_STATUS_SUCCESS) return "success";
        if (status == FirebaseRemoteConfig.LAST_FETCH_STATUS_FAILURE) return "failure";
        if (status == FirebaseRemoteConfig.LAST_FETCH_STATUS_THROTTLED) return "throttled";
        return "no-fetch-yet";
    }

    /** "true"/"false" → boolean, integers → Long, decimals → Double, else the string. */
    private static Object canonicalJsonValue(String raw) {
        if (raw.equals("true")) return Boolean.TRUE;
        if (raw.equals("false")) return Boolean.FALSE;
        try {
            return Long.parseLong(raw);
        } catch (NumberFormatException notALong) {
            try {
                return Double.parseDouble(raw);
            } catch (NumberFormatException notADouble) {
                return raw;
            }
        }
    }

    private static Map<?, ?> mapArg(Map<?, ?> args, String key) {
        Object value = args == null ? null : args.get(key);
        return value instanceof Map ? (Map<?, ?>) value : null;
    }

    @SuppressWarnings("unchecked")
    private static List<Object> objectList(Object value) {
        return value instanceof List ? (List<Object>) value : new ArrayList<>();
    }

    private static StackTraceElement[] stackFrames(String stack) {
        List<StackTraceElement> frames = new ArrayList<>();
        for (String line : stack.split("\n")) {
            line = line.trim();
            if (line.startsWith("at ")) line = line.substring(3);
            int open = line.indexOf('(');
            int close = line.lastIndexOf(')');
            if (open >= 0 && close > open) {
                frames.add(new StackTraceElement("", line.substring(0, open).trim(),
                        line.substring(open + 1, close), -1));
            } else if (!line.isEmpty()) {
                frames.add(new StackTraceElement("", line, null, -1));
            }
        }
        return frames.toArray(new StackTraceElement[0]);
    }

    /** A named type so the Crashlytics report says where the error came from. */
    private static final class RecordedError extends Exception {
        RecordedError(long code, String message) {
            super("firebase/" + code + ": " + message);
        }
    }

    // --- Messaging delivery: foreground send, background queue, replay on register ---

    static void onTokenRefreshed(Context context, String token) {
        report("token refreshed");
        // One pending refresh: an older token is stale by the time it replays.
        main.post(() -> {
            try {
                sendOrQueue(context, "firebase.messaging.tokenRefreshed", new JSONObject().put("token", token));
            } catch (Exception e) {
                Log.w(TAG, "cannot deliver token refresh", e);
            }
        });
    }

    static void onMessageReceived(Context context, com.google.firebase.messaging.RemoteMessage message) {
        JSONObject payload = new JSONObject();
        try {
            if (message.getMessageId() != null) payload.put("messageId", message.getMessageId());
            payload.put("data", new JSONObject(message.getData()));
            if (message.getNotification() != null) {
                JSONObject notification = new JSONObject();
                notification.put("title", message.getNotification().getTitle() == null ? "" : message.getNotification().getTitle());
                notification.put("body", message.getNotification().getBody() == null ? "" : message.getNotification().getBody());
                payload.put("notification", notification);
            } else {
                payload.put("notification", JSONObject.NULL);
            }
            if (message.getSentTime() > 0) payload.put("sentTime", message.getSentTime());
        } catch (Exception e) {
            Log.w(TAG, "cannot serialize the message", e);
            return;
        }

        // A message with a notification block always shows: FCM delivers those
        // to backgrounded apps without calling this service, so showing here
        // covers the foreground case the SDK leaves to the app.
        if (message.getNotification() != null) {
            String id = message.getData().containsKey("id")
                    ? message.getData().get("id")
                    : (message.getMessageId() != null ? message.getMessageId() : String.valueOf(message.getSentTime()));
            main.post(() -> show(context, id,
                    message.getNotification().getTitle() == null ? "" : message.getNotification().getTitle(),
                    message.getNotification().getBody() == null ? "" : message.getNotification().getBody(),
                    false));
        }
        main.post(() -> sendOrQueue(context, "firebase.messaging.message", payload));
    }

    /**
     * Called by the host activity from {@code onCreate} and {@code
     * onNewIntent}, per the unreleased module's contract. First forwards to
     * {@code NshView.handleNotificationIntent} when this build of the AAR has
     * it (0.4.0 does not yet), then reports the press this module's own
     * notifications carry.
     */
    public static void handleNotificationIntent(Intent intent) {
        com.longbridge.nsh.NshView nsh = viewRef != null ? viewRef.get() : null;
        if (nsh != null) {
            try {
                Method m = nsh.getClass().getMethod("handleNotificationIntent", Intent.class);
                m.invoke(nsh, intent);
            } catch (NoSuchMethodException expected) {
                // 0.4.0's NshView has no notification handling yet.
            } catch (Exception e) {
                Log.w(TAG, "NshView.handleNotificationIntent failed", e);
            }
        }
        if (intent == null) return;
        String id = intent.getStringExtra(PRESS_EXTRA);
        // FCM's auto-posted background notifications never reach the service;
        // their launch intent carries google.message_id instead (RNFB's
        // receiver and getInitialNotification key on exactly this extra).
        String extra = PRESS_EXTRA;
        if (id == null) {
            id = intent.getStringExtra("google.message_id");
            extra = "google.message_id";
        }
        if (id == null) return;
        // Once: an activity recreated with the same intent was not pressed again.
        intent.removeExtra(extra);
        Context context = appContext != null ? appContext.get() : null;
        if (context == null) return;
        JSONObject payload = new JSONObject();
        try {
            payload.put("id", id);
        } catch (Exception e) {
            return;
        }
        main.post(() -> sendOrQueue(context, "firebase.messaging.press", payload));
    }

    /** Delivers if the app listens; otherwise the event waits in prefs for register(). */
    private static void sendOrQueue(Context context, String name, JSONObject payload) {
        com.longbridge.nsh.NshView nsh = viewRef != null ? viewRef.get() : null;
        Map<String, Object> body = toMap(payload);
        if (nsh == null || !nsh.send(name, body)) enqueue(context, name, payload);
    }

    private static void drainPending() {
        Context context = appContext != null ? appContext.get() : null;
        if (context == null) return;
        JSONArray pending = readPending(context);
        JSONArray remaining = new JSONArray();
        com.longbridge.nsh.NshView nsh = viewRef != null ? viewRef.get() : null;
        for (int i = 0; pending != null && i < pending.length(); i++) {
            JSONObject event = pending.optJSONObject(i);
            if (event == null) continue;
            String name = event.optString("name");
            JSONObject payload = event.optJSONObject("payload");
            if (payload == null) continue;
            boolean delivered = nsh != null && nsh.send(name, toMap(payload));
            if (!delivered) remaining.put(event);
        }
        if (remaining.length() != (pending == null ? 0 : pending.length())) {
            prefs(context).edit().putString(PREF_PENDING, remaining.toString()).apply();
        }
    }

    private static void enqueue(Context context, String name, JSONObject payload) {
        try {
            JSONArray pending = readPending(context);
            // Only the newest token refresh is worth replaying.
            if (name.equals("firebase.messaging.tokenRefreshed")) {
                JSONArray filtered = new JSONArray();
                for (int i = 0; i < pending.length(); i++) {
                    JSONObject existing = pending.optJSONObject(i);
                    if (existing != null && !name.equals(existing.optString("name"))) filtered.put(existing);
                }
                pending = filtered;
            }
            JSONObject event = new JSONObject();
            event.put("name", name);
            event.put("payload", payload);
            pending.put(event);
            while (pending.length() > PENDING_MAX) pending.remove(0);
            prefs(context).edit().putString(PREF_PENDING, pending.toString()).apply();
            report("queued " + name + " (" + pending.length() + " pending)");
        } catch (Exception e) {
            Log.w(TAG, "cannot queue the event", e);
        }
    }

    private static JSONArray readPending(Context context) {
        String raw = prefs(context).getString(PREF_PENDING, null);
        try {
            return raw == null ? new JSONArray() : new JSONArray(raw);
        } catch (Exception e) {
            return new JSONArray();
        }
    }

    private static Map<String, Object> toMap(JSONObject object) {
        Map<String, Object> out = new HashMap<>();
        for (Iterator<String> it = object.keys(); it.hasNext(); ) {
            String key = it.next();
            Object value = object.opt(key);
            out.put(key, value == JSONObject.NULL ? null : value);
        }
        return out;
    }

    // --- Permission, show and badge: the unreleased Notifications module's semantics ---

    /** `default` until the user was asked on Android 13; otherwise the system's answer. */
    private static String permission(Context context) {
        if (Build.VERSION.SDK_INT >= 33
                && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            boolean asked = prefs(context).getBoolean(PREF_ASKED, false);
            return asked || !declared(context) ? "denied" : "default";
        }
        return manager(context).areNotificationsEnabled() ? "granted" : "denied";
    }

    private static boolean declared(Context context) {
        try {
            String[] permissions = context.getPackageManager()
                    .getPackageInfo(context.getPackageName(), PackageManager.GET_PERMISSIONS).requestedPermissions;
            if (permissions != null) {
                for (String permission : permissions) {
                    if (Manifest.permission.POST_NOTIFICATIONS.equals(permission)) return true;
                }
            }
        } catch (PackageManager.NameNotFoundException e) {
            Log.w(TAG, "cannot read the app's permissions", e);
        }
        return false;
    }

    private static void requestPermission(Context context, com.longbridge.nsh.NshHostReply reply) {
        Activity activity = activity(context);
        if (activity == null || !permission(context).equals("default")) {
            reply.resolve(map("permission", permission(context)));
            return;
        }
        Results results = results(activity);
        results.waiting.add(() -> reply.resolve(map("permission", permission(context))));
        if (results.waiting.size() == 1) {
            results.requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, Results.CODE);
        }
    }

    @SuppressWarnings("deprecation")
    private static Results results(Activity activity) {
        android.app.FragmentManager fragments = activity.getFragmentManager();
        Results results = (Results) fragments.findFragmentByTag("dev.nsh.firebase.Results");
        if (results == null) {
            results = new Results();
            // A host call can arrive while the activity is stopped, where a
            // plain commitNow throws and would reject a legitimate call.
            fragments.beginTransaction().add(results, "dev.nsh.firebase.Results").commitNowAllowingStateLoss();
        }
        return results;
    }

    /** Gets the system's answer to the permission prompt, across configuration changes. */
    @SuppressWarnings("deprecation")
    public static final class Results extends android.app.Fragment {
        static final int CODE = 0x4f10;
        final java.util.List<Runnable> waiting = new java.util.ArrayList<>();

        @Override
        public void onCreate(Bundle state) {
            super.onCreate(state);
            setRetainInstance(true);
        }

        @Override
        public void onRequestPermissionsResult(int code, String[] permissions, int[] answers) {
            if (code != CODE) {
                super.onRequestPermissionsResult(code, permissions, answers);
                return;
            }
            // Empty: the prompt was cancelled, so the user can still be asked.
            if (answers.length > 0 && getActivity() != null) {
                prefs(getActivity()).edit().putBoolean(PREF_ASKED, true).apply();
            }
            java.util.List<Runnable> answered = new java.util.ArrayList<>(waiting);
            waiting.clear();
            for (Runnable r : answered) r.run();
        }
    }

    @SuppressWarnings("deprecation")
    private static boolean show(Context context, String id, String title, String body, boolean silent) {
        if (!permission(context).equals("granted")) return false;
        NotificationManager manager = manager(context);
        Notification.Builder builder;
        if (Build.VERSION.SDK_INT >= 26) {
            ensureChannel(context);
            builder = new Notification.Builder(context, CHANNEL);
        } else {
            builder = new Notification.Builder(context);
            builder.setPriority(Notification.PRIORITY_HIGH);
            builder.setDefaults(silent ? Notification.DEFAULT_LIGHTS : Notification.DEFAULT_ALL);
        }
        if (silent && Build.VERSION.SDK_INT >= 26) {
            // Grouped under a never-summarized parent so the channel's sound does not play.
            builder.setGroup(SILENT_GROUP).setGroupAlertBehavior(Notification.GROUP_ALERT_SUMMARY);
        }
        int icon = context.getApplicationInfo().icon;
        builder.setSmallIcon(icon != 0 ? icon : android.R.drawable.ic_dialog_info)
                .setContentTitle(title)
                .setAutoCancel(true)
                .setShowWhen(true);
        if (body != null && !body.isEmpty()) builder.setContentText(body);
        int badge = prefs(context).getInt(PREF_BADGE, 0);
        if (badge > 0) builder.setNumber(badge);
        Intent launch = context.getPackageManager().getLaunchIntentForPackage(context.getPackageName());
        if (launch != null) {
            launch.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
            launch.putExtra(PRESS_EXTRA, id);
            builder.setContentIntent(PendingIntent.getActivity(context, id.hashCode(), launch,
                    PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE));
        }
        try {
            manager.notify(id, NOTIFICATION_ID, builder.build());
            report("show " + id);
            return true;
        } catch (RuntimeException e) {
            Log.w(TAG, "cannot show a notification", e);
            return false;
        }
    }

    // --- Small helpers ---

    private static FirebaseAnalytics analytics(Context context) {
        if (FirebaseApp.getApps(context).isEmpty()) {
            throw new IllegalStateException("call NshFirebase.init before the firebase.analytics handlers");
        }
        return FirebaseAnalytics.getInstance(context);
    }

    private static FirebasePerformance performance(Context context) {
        if (FirebaseApp.getApps(context).isEmpty()) {
            throw new IllegalStateException("call NshFirebase.init before the firebase.perf handlers");
        }
        return FirebasePerformance.getInstance();
    }

    private static void putConsent(Map<FirebaseAnalytics.ConsentType, FirebaseAnalytics.ConsentStatus> consent,
                                   Map<?, ?> args, String key, FirebaseAnalytics.ConsentType type) {
        String value = args == null ? null : (String) args.get(key);
        if (value == null) return;
        consent.put(type, "granted".equals(value)
                ? FirebaseAnalytics.ConsentStatus.GRANTED : FirebaseAnalytics.ConsentStatus.DENIED);
    }

    private static Bundle bundle(Object value) {
        Bundle out = new Bundle();
        if (!(value instanceof Map)) return out;
        for (Map.Entry<?, ?> entry : ((Map<?, ?>) value).entrySet()) {
            String key = String.valueOf(entry.getKey());
            Object v = entry.getValue();
            if (v instanceof String) out.putString(key, (String) v);
            else if (v instanceof Number) out.putDouble(key, ((Number) v).doubleValue());
            else if (v instanceof Boolean) out.putBoolean(key, (Boolean) v);
            else if (v != null) out.putString(key, String.valueOf(v));
        }
        return out;
    }

    private static NotificationManager manager(Context context) {
        return (NotificationManager) context.getSystemService(Context.NOTIFICATION_SERVICE);
    }

    private static SharedPreferences prefs(Context context) {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    private static Activity activity(Context context) {
        while (context instanceof ContextWrapper) {
            if (context instanceof Activity) return (Activity) context;
            context = ((ContextWrapper) context).getBaseContext();
        }
        return null;
    }

    private static String str(Map<?, ?> args, String key) { return str(args, key, ""); }

    private static String str(Map<?, ?> args, String key, String fallback) {
        Object value = args == null ? null : args.get(key);
        return value instanceof String ? (String) value : fallback;
    }

    private static boolean bool(Map<?, ?> args, String key) {
        Object value = args == null ? null : args.get(key);
        return value instanceof Boolean ? (Boolean) value : false;
    }

    private static long number(Map<?, ?> args, String key) {
        Object value = args == null ? null : args.get(key);
        return value instanceof Number ? ((Number) value).longValue() : 0;
    }

    private static Map<String, Object> map(String key, Object value) {
        Map<String, Object> out = new HashMap<>();
        out.put(key, value);
        return out;
    }
}
