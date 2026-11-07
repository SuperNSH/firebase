package dev.nsh.firebase;

import com.google.firebase.messaging.FirebaseMessagingService;
import com.google.firebase.messaging.RemoteMessage;

/**
 * Routes FCM into {@link NshFirebase}: token refreshes and messages become
 * {@code firebase.messaging.tokenRefreshed} and {@code firebase.messaging.message}
 * events, queued when the app is not listening. RNFB splits this work across a
 * service and a c2dm receiver; here the service alone sees every delivery.
 */
public class NshFirebaseMessagingService extends FirebaseMessagingService {
    @Override
    public void onNewToken(String token) {
        NshFirebase.onTokenRefreshed(getApplicationContext(), token);
    }

    @Override
    public void onMessageReceived(RemoteMessage message) {
        NshFirebase.onMessageReceived(getApplicationContext(), message);
    }

    @Override
    public void onSendError(String messageId, Exception error) {
        NshFirebase.report("sendError " + messageId + ": " + error.getMessage());
    }

    @Override
    public void onDeletedMessages() {
        NshFirebase.report("onDeletedMessages");
    }
}
