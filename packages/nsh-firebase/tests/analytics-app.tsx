// The analytics exercise UI the tests drive: the pure tier's batching
// queue over a recording transport (and the collector transport over
// the stubbed network), consent, retry, validation, and the native
// tier's firebase.analytics.* routing. Mirrors the Rust CollectorApp in
// crates/nsh-firebase-analytics/tests/analytics_ui.rs.
import { mount, signal, Text, View } from "nsh";
import {
  COLLECTOR_DEFAULT_ENDPOINT,
  QueuedAnalytics,
  RecordingTransport,
  getAnalytics,
  type Analytics,
  type AnalyticsConfig,
} from "../src/analytics";
import { demoOptions, initializeApp, toFirebaseError } from "../src/core";

// Small on purpose, so a test fills a batch with three presses and
// waits out a short interval and backoff.
const CONFIG: AnalyticsConfig = {
  flushIntervalMs: 10_000,
  maxBatchSize: 3,
  maxQueueSize: 12,
  retryBaseMs: 1_000,
  retryMaxMs: 8_000,
  maxRetries: 3,
};

export function AnalyticsExerciseApp() {
  const state = signal("ready");
  const pending = signal("pending 0 dropped 0");
  const sent = signal("sent: none");
  const consent = signal("consent granted");
  const health = signal("collector: unknown");
  const assertLine = signal("assert: not run");
  const recording = new RecordingTransport();
  let analytics: Analytics | null = null;

  recording.onSend = () => {
    sent.set(`sent: ${JSON.stringify(recording.posts)}`);
  };

  const describe = (client: Analytics | null, kind: string) => {
    if (client instanceof QueuedAnalytics) {
      // The hook fires after the poll settled, when the queue's own
      // bookkeeping (in-flight, retries) is final.
      client.onSend = () => {
        pending.set(`pending ${client.pending} dropped ${client.dropped}`);
      };
      pending.set(`pending ${client.pending} dropped ${client.dropped}`);
    }
    state.set(kind);
  };

  const initPure = () => {
    const app = initializeApp(demoOptions());
    analytics = getAnalytics(app, { transport: recording, ...CONFIG });
    describe(analytics, "pure client ready");
  };

  const initCollector = () => {
    const app = initializeApp(demoOptions());
    analytics = getAnalytics(app, { collector: { endpoint: COLLECTOR_DEFAULT_ENDPOINT }, ...CONFIG });
    describe(analytics, "collector client ready");
  };

  const initMp = () => {
    const app = initializeApp(demoOptions());
    // The sandbox points the MP transport at a local stand-in, never at
    // Google; the credentials are fake AND contain query-hostile
    // characters, so the test sees the percent-encoding.
    analytics = getAnalytics(app, {
      mp: { baseUrl: COLLECTOR_DEFAULT_ENDPOINT, apiSecret: "sb&secret=1", measurementId: "m=1" },
      ...CONFIG,
    });
    describe(analytics, "mp client ready");
  };

  const initNative = () => {
    const app = initializeApp(demoOptions());
    try {
      analytics = getAnalytics(app);
      describe(analytics, "client ready");
    } catch (error) {
      state.set(`init: ${toFirebaseError(error).message}`);
    }
  };

  const queue = (): QueuedAnalytics | null => (analytics instanceof QueuedAnalytics ? analytics : null);

  const act = (label: string, run: (client: Analytics) => void) => {
    if (!analytics) {
      state.set("no client");
      return;
    }
    try {
      run(analytics);
      if (label !== "") state.set(label);
    } catch (error) {
      // FirebaseError's message already carries the stable code.
      state.set(`${label}: ${toFirebaseError(error).message}`);
    }
    const client = queue();
    if (client) pending.set(`pending ${client.pending} dropped ${client.dropped}`);
  };

  const flush = () => {
    const client = queue();
    if (!client) {
      state.set("flush: the native tier sends at once, nothing to flush");
      return;
    }
    client
      .flush()
      .then(() => {
        state.set("flush ok");
        pending.set(`pending ${client.pending} dropped ${client.dropped}`);
      })
      .catch((error: unknown) => {
        state.set(`flush failed: ${toFirebaseError(error).message}`);
      });
  };

  // The sandbox flow: only meaningful while tools/collector answers on
  // 127.0.0.1:18742 (perfmon owns the process).
  const SANDBOX_EVENTS = ["sandbox_alpha", "sandbox_beta", "sandbox_gamma"] as const;

  const sbHealth = () => {
    fetch(`${COLLECTOR_DEFAULT_ENDPOINT}/health`)
      .then((response) =>
        health.set(response.ok ? "collector: up" : `collector: down (HTTP ${response.status})`),
      )
      .catch((error: unknown) => {
        health.set(`collector: unreachable: ${toFirebaseError(error).message}`);
      });
  };

  const sbSend3 = () => {
    if (!analytics) {
      state.set("no client");
      return;
    }
    for (const name of SANDBOX_EVENTS) {
      analytics.logEvent(name, { source: "sandbox" });
    }
    const client = queue();
    if (client) pending.set(`pending ${client.pending} dropped ${client.dropped}`);
    state.set("sandbox events queued");
  };

  const sbAssert = () => {
    Promise.all(
      SANDBOX_EVENTS.map(async (name) => {
        const response = await fetch(
          `${COLLECTOR_DEFAULT_ENDPOINT}/assert?contains=${name}&count=1`,
        );
        const body = (await response.json()) as { ok?: boolean };
        return `${name.split("_")[1]}:${body.ok === true ? "true" : "false"}`;
      }),
    )
      .then((parts) => assertLine.set(`assert ${parts.join(" ")}`))
      .catch((error: unknown) => {
        assertLine.set(`assert unreachable: ${toFirebaseError(error).message}`);
      });
  };

  return (
    <View class="flex-1 gap-1 bg-background p-2">
      <Text testID="state">{state()}</Text>
      <Text testID="pending">{pending()}</Text>
      <Text testID="sent">{sent()}</Text>
      <Text testID="consent">{consent()}</Text>
      <Text testID="health">{health()}</Text>
      <Text testID="assert">{assertLine()}</Text>
      <View testID="init-pure" role="button" onPress={initPure} class="bg-primary p-1">
        <Text class="text-primary-foreground">init pure</Text>
      </View>
      <View testID="init-collector" role="button" onPress={initCollector} class="bg-primary p-1">
        <Text class="text-primary-foreground">init collector</Text>
      </View>
      <View testID="init-mp" role="button" onPress={initMp} class="bg-primary p-1">
        <Text class="text-primary-foreground">init mp</Text>
      </View>
      <View testID="init-native" role="button" onPress={initNative} class="bg-primary p-1">
        <Text class="text-primary-foreground">init native</Text>
      </View>
      <View
        testID="log"
        role="button"
        onPress={() => act("", (client) => client.logEvent("level_up", { difficulty: "hard" }))}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">log</Text>
      </View>
      <View
        testID="log6"
        role="button"
        onPress={() =>
          act("", (client) => {
            // Synchronous: no timer can interleave, so the queue holds
            // two full batches at once and the drain rule is observable.
            for (let i = 0; i < 6; i += 1) {
              client.logEvent("level_up", { difficulty: "hard" });
            }
          })
        }
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">log 6</Text>
      </View>
      <View
        testID="log-reserved"
        role="button"
        onPress={() => act("logged reserved", (client) => client.logEvent("session_start"))}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">log reserved</Text>
      </View>
      <View
        testID="screen"
        role="button"
        onPress={() => act("", (client) => client.logScreenView("Inventory", "InventoryScreen"))}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">screen</Text>
      </View>
      <View
        testID="prop-hard"
        role="button"
        onPress={() => act("", (client) => client.setUserProperty("difficulty", "hard"))}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">prop hard</Text>
      </View>
      <View
        testID="prop-easy"
        role="button"
        onPress={() => act("", (client) => client.setUserProperty("difficulty", "easy"))}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">prop easy</Text>
      </View>
      <View
        testID="user-id"
        role="button"
        onPress={() => act("user id set", (client) => client.setUserId("user-41"))}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">user id</Text>
      </View>
      <View
        testID="user-id-clear"
        role="button"
        onPress={() => act("user id cleared", (client) => client.setUserId(null))}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">user id clear</Text>
      </View>
      <View
        testID="deny"
        role="button"
        onPress={() => {
          act("", (client) =>
            client.setConsent({ analyticsStorage: "denied", adStorage: "denied" }),
          );
          consent.set("consent denied");
        }}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">deny</Text>
      </View>
      <View
        testID="grant"
        role="button"
        onPress={() => {
          act("", (client) => client.setConsent({ analyticsStorage: "granted" }));
          consent.set("consent granted");
        }}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">grant</Text>
      </View>
      <View
        testID="debug"
        role="button"
        onPress={() => act("debug view: on (pure tier: no-op)", (client) => client.setDebugViewEnabled(true))}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">debug</Text>
      </View>
      <View
        testID="fail-next"
        role="button"
        onPress={() => {
          recording.failNext = true;
          state.set("next send will fail");
        }}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">fail next</Text>
      </View>
      <View
        testID="hook-throws"
        role="button"
        onPress={() => {
          const client = queue();
          if (!client) {
            state.set("no client");
            return;
          }
          client.onSend = () => {
            throw new Error("hook boom");
          };
          state.set("hook will throw");
        }}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">hook throws</Text>
      </View>
      <View testID="flush" role="button" onPress={flush} class="bg-primary p-1">
        <Text class="text-primary-foreground">flush</Text>
      </View>
      <View testID="sb-health" role="button" onPress={sbHealth} class="bg-primary p-1">
        <Text class="text-primary-foreground">sandbox health</Text>
      </View>
      <View testID="sb-send3" role="button" onPress={sbSend3} class="bg-primary p-1">
        <Text class="text-primary-foreground">sandbox send 3</Text>
      </View>
      <View testID="sb-assert" role="button" onPress={sbAssert} class="bg-primary p-1">
        <Text class="text-primary-foreground">sandbox assert</Text>
      </View>
    </View>
  );
}

mount(AnalyticsExerciseApp);
