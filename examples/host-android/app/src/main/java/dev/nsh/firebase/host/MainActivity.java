package dev.nsh.firebase.host;

import android.app.Activity;
import android.graphics.Color;
import android.graphics.drawable.GradientDrawable;
import android.os.Bundle;
import android.util.Log;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import com.longbridge.nsh.NshView;

import dev.nsh.firebase.NshFirebase;

/**
 * An embedding host, modelled on nsh 0.4.0's examples/embed-android: native UI
 * above an {@code NshView} whose app talks to Firebase through the {@code
 * firebase.*} handlers {@link NshFirebase#register} installs. The status line
 * logs every handler the app calls and every messaging event the host sends.
 */
public final class MainActivity extends Activity {
    private static final String TAG = "nsh-firebase-host";

    private NshView nsh;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        float d = getResources().getDisplayMetrics().density;

        NshFirebase.init(this);

        LinearLayout column = new LinearLayout(this);
        column.setOrientation(LinearLayout.VERTICAL);
        column.setPadding(Math.round(20 * d), Math.round(48 * d), Math.round(20 * d), Math.round(20 * d));
        column.setBackgroundColor(Color.WHITE);

        TextView title = new TextView(this);
        title.setText("nsh-firebase host");
        title.setTextSize(24);
        title.setTextColor(Color.BLACK);
        column.addView(title);

        TextView status = new TextView(this);
        status.setTextSize(12);
        status.setTextColor(0xff57606a);
        status.setText("waiting for the app…");
        ScrollView statusScroll = new ScrollView(this);
        statusScroll.addView(status);
        column.addView(statusScroll, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, Math.round(110 * d)));

        // The grant is the most the app may do: every firebase.* name (a
        // trailing * is a prefix match in nsh's access.rs) and loopback only.
        nsh = new NshView(this);
        nsh.setGrantedCapabilities("{ \"host\": [\"firebase.*\"],"
                + "\"network\": { \"hosts\": [\"127.0.0.1\"] } }");
        nsh.setErrorListener(message -> Log.e(TAG, message));
        NshFirebase.setListener(line -> runOnUiThread(() -> status.append("\n" + line)));
        // Register before the load, so the app's first firebase.* call has a handler.
        NshFirebase.register(nsh);
        // A notification tap that started the app reports before the app listens;
        // the module queues the press until it does.
        NshFirebase.handleNotificationIntent(getIntent());

        FrameLayout frame = new FrameLayout(this);
        GradientDrawable border = new GradientDrawable();
        border.setStroke(Math.round(2 * d), 0xff1a7f37);
        frame.setBackground(border);
        frame.addView(nsh, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        column.addView(frame, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1));

        setContentView(column);
        // Only a loaded bundle drains the host's pending-event queue; a failed
        // load must not flush it into a view that cannot deliver.
        if (nsh.loadAssetBundle("nsh")) NshFirebase.appLoaded();
        else Log.e(TAG, "the nsh bundle failed to load; queued firebase.messaging events stay pending");
    }

    @Override
    protected void onNewIntent(android.content.Intent intent) {
        super.onNewIntent(intent);
        NshFirebase.handleNotificationIntent(intent);
    }

    @Override
    protected void onDestroy() {
        // The static listener holds a lambda capturing this activity.
        NshFirebase.setListener(null);
        nsh.destroy();
        super.onDestroy();
    }
}
