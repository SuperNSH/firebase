package dev.nsh.firebase.host;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.os.Bundle;
import com.google.firebase.messaging.RemoteMessage;
import dev.nsh.firebase.NshFirebase;

/**
 * Sandbox stand-in for Google Play services' delivery: `adb shell am broadcast`
 * cannot hold the c2dm SEND permission, so this receiver turns the broadcast's
 * extras into the RemoteMessage the messaging service would have received.
 */
public class DebugPushReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context context, Intent intent) {
        Bundle extras = intent.getExtras();
        NshFirebase.onMessageReceived(
                context.getApplicationContext(), new RemoteMessage(extras == null ? new Bundle() : extras));
    }
}
