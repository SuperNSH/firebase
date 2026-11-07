// The core exercise UI the tests drive: app and config (the demo defaults
// and a loaded config document), the pure-tier transport, the tier
// facade's call/post error mapping and the event bus.
import { mount, signal, Text, View } from "nsh";
import {
  EventBus,
  PureTier,
  databaseBaseUrl,
  demoOptions,
  getApp,
  initializeApp,
  joinUrl,
  optionsFromConfig,
  requestText,
  storageBaseUrl,
  toFirebaseError,
} from "../src/core";

const DB = "http://127.0.0.1:19000";
const DENIED_URL = "https://denied.example.test/.json";

// A config document like the setup step hands over. The fallback URLs are
// deliberately distinct from the emulator ports, so the UI can show which
// one won.
const CONFIG_JSON = `{
  "apiKey": "demo-nsh-fake-api-key",
  "appId": "1:000000000000:web:demo-nsh",
  "projectId": "demo-nsh",
  "databaseUrl": "http://10.9.9.9:17000",
  "authUrl": "http://10.9.9.8:17099",
  "storageBucket": "demo-nsh.firebasestorage.app",
  "emulator": {
    "auth": { "host": "127.0.0.1", "port": 19099 },
    "database": { "host": "127.0.0.1", "port": 19000 },
    "storage": { "port": 19199 }
  }
}`;

async function run(name: string, call: () => Promise<string>) {
  try {
    return await call();
  } catch (error) {
    const err = toFirebaseError(error);
    return `${name}: ${err.code} (${err.nativeCode ?? "-"})`;
  }
}

// host.post throws synchronously (a refusal), so posts cannot share run().
function postNow(name: string, call: () => void): string {
  try {
    call();
    return `${name}: ok`;
  } catch (error) {
    const err = toFirebaseError(error);
    return `${name}: ${err.code} (${err.nativeCode ?? "-"})`;
  }
}

export function CoreExerciseApp() {
  const line = signal("ready");
  const busLine = signal("bus idle");
  const a = signal(0);
  const b = signal(0);
  const bus = new EventBus();
  let bOff: (() => void) | null = null;

  const init = () => {
    const app = initializeApp(demoOptions());
    const again = getApp();
    const options = demoOptions();
    line.set(
      `app ${app.name === again.name ? "same" : "different"} project=${app.options.projectId}` +
        ` db=${databaseBaseUrl(options)} storage=${options.storageBucket}`,
    );
  };

  const loadConfig = (withoutEmulator: boolean) => {
    const document = JSON.parse(CONFIG_JSON) as Record<string, unknown>;
    const options = optionsFromConfig(
      withoutEmulator ? { ...document, emulator: undefined } : document,
    );
    line.set(
      `config db=${databaseBaseUrl(options)} storage=${storageBaseUrl(options)}` +
        ` project=${options.projectId}`,
    );
  };

  const fetchTodos = () =>
    run("fetch", async () => {
      // The base ends in a slash on purpose: joinUrl must not double it,
      // or the stub route never matches (mirrors the Rust TransportApp).
      const response = await requestText(joinUrl(`${DB}/`, "todos.json"));
      return `fetch: status=${response.status} body=${response.text}`;
    }).then((value) => line.set(value));

  const deny = () =>
    run("deny", async () => {
      const response = await requestText(DENIED_URL);
      return `deny: status=${response.status} body=${response.text}`;
    }).then((value) => line.set(value));

  const ping = (label: string, name: string) =>
    run(label, async () => {
      // Each test opens a fresh VM: initialize before the tier's first call.
      initializeApp(demoOptions());
      const result = await getApp().tier.call(name);
      return `${label}: ${JSON.stringify(result)}`;
    }).then((value) => line.set(value));

  const post = (label: string, name: string) => {
    initializeApp(demoOptions());
    line.set(postNow(label, () => getApp().tier.post(name, { name: "demo" })));
  };

  const subscribe = () => {
    bus.on("tick", () => a.set(a() + 1));
    bOff = bus.on("tick", () => b.set(b() + 1));
  };

  const emit = () => {
    bus.emit("tick", null);
    busLine.set(`a=${a()} b=${b()}`);
  };

  return (
    <View class="flex-1 gap-2 bg-background p-4">
      <Text testID="state">{line()}</Text>
      <View testID="init" role="button" onPress={init} class="bg-primary p-2">
        <Text class="text-primary-foreground">init</Text>
      </View>
      <View testID="config" role="button" onPress={() => loadConfig(false)} class="bg-primary p-2">
        <Text class="text-primary-foreground">config</Text>
      </View>
      <View
        testID="config-pure"
        role="button"
        onPress={() => loadConfig(true)}
        class="bg-primary p-2"
      >
        <Text class="text-primary-foreground">config pure</Text>
      </View>
      <View testID="fetch" role="button" onPress={fetchTodos} class="bg-primary p-2">
        <Text class="text-primary-foreground">fetch</Text>
      </View>
      <View testID="deny" role="button" onPress={deny} class="bg-primary p-2">
        <Text class="text-primary-foreground">deny</Text>
      </View>
      <View testID="ping" role="button" onPress={() => ping("ping", "firebase.core.ping")} class="bg-primary p-2">
        <Text class="text-primary-foreground">ping</Text>
      </View>
      <View
        testID="ping-missing"
        role="button"
        onPress={() => ping("missing", "firebase.core.missing")}
        class="bg-primary p-2"
      >
        <Text class="text-primary-foreground">ping missing</Text>
      </View>
      <View
        testID="ping-denied"
        role="button"
        onPress={() => ping("other", "other.ping")}
        class="bg-primary p-2"
      >
        <Text class="text-primary-foreground">ping denied</Text>
      </View>
      <View
        testID="post"
        role="button"
        onPress={() => post("post", "firebase.core.track")}
        class="bg-primary p-2"
      >
        <Text class="text-primary-foreground">post</Text>
      </View>
      <View
        testID="post-denied"
        role="button"
        onPress={() => post("other-post", "other.post")}
        class="bg-primary p-2"
      >
        <Text class="text-primary-foreground">post denied</Text>
      </View>
      <View
        testID="map-unavailable"
        role="button"
        onPress={() => {
          try {
            new PureTier().post("firebase.core.ping");
            line.set("map: unreachable");
          } catch (error) {
            line.set(toFirebaseError(error).message);
          }
        }}
        class="bg-primary p-2"
      >
        <Text class="text-primary-foreground">map unavailable</Text>
      </View>
      <Text testID="bus">{busLine()}</Text>
      <View testID="subscribe" role="button" onPress={subscribe} class="bg-primary p-2">
        <Text class="text-primary-foreground">subscribe</Text>
      </View>
      <View testID="emit" role="button" onPress={emit} class="bg-primary p-2">
        <Text class="text-primary-foreground">emit</Text>
      </View>
      <View testID="off-b" role="button" onPress={() => bOff?.()} class="bg-primary p-2">
        <Text class="text-primary-foreground">off b</Text>
      </View>
    </View>
  );
}

mount(CoreExerciseApp);
