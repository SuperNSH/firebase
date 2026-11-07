// The host's fallback bundle: it exercises the firebase.* handlers the native
// module installs, so examples/host-android is testable before examples/demo
// lands. The demo bundle replaces it in assets/nsh when it exists.

import { mount, signal, Text, View } from "nsh";
import { host } from "nsh";

type MessagingMessage = {
  messageId?: string;
  data?: Record<string, string>;
  notification?: { title: string; body: string } | null;
};

const row = {
  alignSelf: "flex-start",
  paddingHorizontal: 14,
  paddingVertical: 8,
  borderRadius: 8,
  backgroundColor: "#0969da",
} as const;

function App() {
  const log = signal<string[]>(["listening for firebase.* events"]);
  const note = (line: string) => log.update((lines) => [...lines.slice(-11), line]);

  // Presses that reach a loaded app before these listeners exist are kept by
  // nsh's own shared event buffer; ones before the load replay from the host's queue.
  host.on<MessagingMessage>("firebase.messaging.message", (m) => {
    note(`message ${m.messageId ?? "(no id)"} data=${JSON.stringify(m.data ?? {})}`);
  });
  host.on<{ token: string }>("firebase.messaging.tokenRefreshed", (e) => {
    note(`tokenRefreshed ${e.token.slice(0, 12)}…`);
  });
  host.on<{ id: string }>("firebase.messaging.press", (e) => {
    note(`press ${e.id}`);
  });

  const call = async (name: string, args?: unknown): Promise<unknown> => {
    try {
      const result = await host.call(name, args ?? null);
      note(`${name} → ${JSON.stringify(result)}`);
      return result;
    } catch (e) {
      const { code, message } = e as Error & { code?: string };
      note(`${name} ✕ ${code ?? "ERR"}: ${message}`);
      return undefined;
    }
  };

  return (
    <View style={{ flex: 1, padding: 20, gap: 10, backgroundColor: "#f6f8fa" }}>
      <Text style={{ fontSize: 20, fontWeight: "700" }}>Firebase handlers</Text>
      <View style={{ flexDirection: "row", gap: 8, flexWrap: "wrap" }}>
        <View role="button" onPress={() => call("firebase.messaging.token")} style={row} activeStyle={{ opacity: 0.7 }}>
          <Text style={{ color: "#ffffff" }}>Token</Text>
        </View>
        <View role="button" onPress={() => call("firebase.messaging.permission")} style={row} activeStyle={{ opacity: 0.7 }}>
          <Text style={{ color: "#ffffff" }}>Permission</Text>
        </View>
        <View role="button" onPress={() => call("firebase.messaging.requestPermission")} style={row} activeStyle={{ opacity: 0.7 }}>
          <Text style={{ color: "#ffffff" }}>Request permission</Text>
        </View>
        <View
          role="button"
          onPress={() => call("firebase.messaging.show", { id: "mini-1", title: "Hello", body: "From the mini app", silent: false })}
          style={row}
          activeStyle={{ opacity: 0.7 }}
        >
          <Text style={{ color: "#ffffff" }}>Show</Text>
        </View>
        <View role="button" onPress={() => call("firebase.messaging.close", { id: "mini-1" })} style={row} activeStyle={{ opacity: 0.7 }}>
          <Text style={{ color: "#ffffff" }}>Close</Text>
        </View>
        <View role="button" onPress={() => call("firebase.messaging.setBadge", { count: 3 })} style={row} activeStyle={{ opacity: 0.7 }}>
          <Text style={{ color: "#ffffff" }}>Badge 3</Text>
        </View>
        <View role="button" onPress={() => call("firebase.messaging.subscribe", { topic: "demo" })} style={row} activeStyle={{ opacity: 0.7 }}>
          <Text style={{ color: "#ffffff" }}>Subscribe demo</Text>
        </View>
        <View role="button" onPress={() => call("firebase.analytics.logEvent", { name: "mini_tap", params: { where: "host" } })} style={row} activeStyle={{ opacity: 0.7 }}>
          <Text style={{ color: "#ffffff" }}>Log event</Text>
        </View>
        <View
          role="button"
          onPress={async () => {
            const started = (await call("firebase.perf.startTrace", { name: "mini-trace" })) as { id?: string };
            if (started?.id) await call("firebase.perf.stopTrace", { id: started.id });
            await call("firebase.perf.recordHttpMetric", {
              id: "evt-mini-1",
              method: "GET",
              url: "http://127.0.0.1:8788/slow",
              host: "127.0.0.1",
              path: "/slow",
              status: 200,
              startedAtMs: 0,
              durationMs: 812,
              ttfbMs: 40,
              requestBytes: 0,
              responseBytes: 512,
              error: null,
              initiator: null,
            });
          }}
          style={row}
          activeStyle={{ opacity: 0.7 }}
        >
          <Text style={{ color: "#ffffff" }}>Trace + metric</Text>
        </View>
      </View>
      <View style={{ marginTop: 8 }}>
        {log().map((line, i) => (
          <Text style={{ fontSize: 12, color: i === log().length - 1 ? "#1f2328" : "#57606a" }}>
            {line}
          </Text>
        ))}
      </View>
    </View>
  );
}

mount(App);
