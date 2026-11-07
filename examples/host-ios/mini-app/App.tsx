// The fallback bundle for the iOS host: it exercises the `firebase.*`
// handlers from the app side, so the host is testable before the full demo
// bundle lands. The host grants `host: ["firebase.*"]`, this nsh.json
// declares the same names.
import { host, mount, signal, Text, View } from "nsh";

function App() {
  const lines = signal<string[]>([]);
  const add = (line: string) => lines.set([...lines(), line]);

  // Presses of notifications that started the app queue in the channel
  // until this listener exists, so registering at startup loses nothing.
  host.on("firebase.messaging.press", (event) => {
    add(`press ${(event as { id: string }).id}`);
  });
  host.on("firebase.messaging.tokenRefreshed", (event) => {
    add(`token refreshed ${(event as { token: string }).token}`);
  });
  host.on("firebase.messaging.message", (event) => {
    const message = event as { messageId?: string; notification?: { title?: string } };
    add(`message ${message.messageId ?? "(no id)"} ${message.notification?.title ?? ""}`);
  });

  const probe = async () => {
    try {
      const token = (await host.call("firebase.messaging.token")) as { token: string };
      add(`token ${token.token}`);
    } catch (error) {
      // The fake project cannot fetch a real token; the failure must say why.
      add(`token failed: ${(error as { message?: string }).message ?? String(error)}`);
    }
    const permission = (await host.call("firebase.messaging.permission")) as {
      permission: string;
    };
    add(`permission ${permission.permission}`);
    await host.call("firebase.analytics.logEvent", {
      name: "mini_app_open",
      params: { tier: "native-host" },
    });
    await host.call("firebase.analytics.logScreenView", { name: "mini", class: "App" });
    const trace = (await host.call("firebase.perf.startTrace", { name: "mini_boot" })) as {
      id: string;
    };
    await host.call("firebase.perf.stopTrace", { id: trace.id });
    const metric = (await host.call("firebase.perf.recordHttpMetric", {
      url: "https://demo-nsh.firebaseio.com/.json",
      method: "GET",
      status: 200,
      requestBytes: 12,
      responseBytes: 3456,
    })) as { recorded: boolean };
    add(`analytics + perf recorded (${metric.recorded})`);
  };

  const askPermission = async () => {
    const result = (await host.call("firebase.messaging.requestPermission")) as {
      permission: string;
    };
    add(`permission ${result.permission}`);
  };

  const showNotification = async () => {
    const shown = (await host.call("firebase.messaging.show", {
      id: "mini-1",
      title: "From the mini app",
      body: "Shown through UNUserNotificationCenter",
    })) as { shown: boolean };
    add(`show ${shown.shown}`);
  };

  return (
    <View class="flex-1 gap-3 p-4">
      <Text class="text-lg font-bold">nsh-firebase mini app</Text>
      <Text role="button" class="bg-primary text-primary-foreground rounded px-3 py-2 self-start" onPress={probe}>
        Probe handlers
      </Text>
      <Text role="button" class="bg-muted text-foreground rounded px-3 py-2 self-start" onPress={askPermission}>
        Request permission
      </Text>
      <Text role="button" class="bg-muted text-foreground rounded px-3 py-2 self-start" onPress={showNotification}>
        Show a notification
      </Text>
      <Each of={lines()}>
        {(line) => <Text class="text-xs text-muted-foreground">{line()}</Text>}
      </Each>
    </View>
  );
}

mount(App);
