// The remote-config exercise UI the tests drive: the pure tier's
// defaults and typed reads, template activation over a stub transport
// (and the honest refusal without one), and the native tier's
// firebase.remote-config.* routing. Mirrors the wire shapes the Rust
// integration tests assert in crates/nsh-firebase-remote-config/tests.
import { mount, signal, Text, View } from "nsh";
import {
  getRemoteConfig,
  type RemoteConfigClient,
  type RemoteConfigTransport,
} from "../src/remote-config";
import { CODES, FirebaseError, demoOptions, initializeApp, toFirebaseError } from "../src/core";

const DEFAULTS = {
  welcome_message: "hello from defaults",
  feature_flag_dark_mode: false,
  tip_amount: 10,
  rollout_copy: "app default copy",
};

// The invalid-defaults probe; the shape is refused before any store.
const BAD_DEFAULTS = { broken: {} } as unknown as typeof DEFAULTS;

// A realistic template document, the REST RemoteConfig shape: two
// conditions, five parameters (default, conditional, useInAppDefault,
// dynamic), one parameter group, and a version block.
const TEMPLATE = {
  conditions: [
    { name: "ios_users", expression: "device.os == 'ios'", tagColor: "BLUE" },
    { name: "beta_cohort", expression: "user.property('cohort') == 'beta'" },
  ],
  parameters: {
    welcome_message: {
      defaultValue: { value: "Welcome to nsh-firebase" },
      conditionalValues: { ios_users: { value: "Welcome, iOS user" } },
    },
    feature_flag_dark_mode: { defaultValue: { value: "true" } },
    tip_amount: { defaultValue: { value: "25" }, valueType: "NUMBER" },
    rollout_copy: {
      defaultValue: { useInAppDefault: true },
      conditionalValues: { beta_cohort: { value: "beta copy" } },
    },
    ml_prompt: { defaultValue: { personalizationValue: { personalizationId: "53" } } },
  },
  parameterGroups: {
    checkout: {
      description: "Checkout tuning",
      parameters: { checkout_button_label: { defaultValue: { value: "Pay now" } } },
    },
  },
  version: { versionNumber: "12", updateOrigin: "CONSOLE" },
};

const BROKEN_TEMPLATE = {
  conditions: [
    { name: "dup", expression: "x" },
    { name: "dup", expression: "y" },
  ],
};

// The stub transport stands in for a backend only an OAuth holder could
// call; no real network is touched.
class StubTransport implements RemoteConfigTransport {
  failNext = false;
  breakNext = false;

  async fetchTemplate(): Promise<unknown> {
    if (this.failNext) {
      this.failNext = false;
      throw new FirebaseError(CODES.network, "HTTP 401: the backend refused the fake project");
    }
    if (this.breakNext) {
      this.breakNext = false;
      return BROKEN_TEMPLATE;
    }
    return TEMPLATE;
  }
}

export function RemoteConfigExerciseApp() {
  const state = signal("ready");
  const defaultsLine = signal("defaults: not read");
  const valuesLine = signal("values: not read");
  const fetchLine = signal("fetch: not run");
  const statusLine = signal("last fetch: unknown");
  const stub = new StubTransport();
  let client: RemoteConfigClient | null = null;

  const init = (build: () => RemoteConfigClient, label: string) => {
    try {
      client = build();
      state.set(label);
    } catch (error) {
      state.set(`init: ${toFirebaseError(error).message}`);
    }
  };

  const initPure = () => {
    const app = initializeApp(demoOptions());
    init(() => getRemoteConfig(app, { forcePure: true }), "pure client ready");
  };

  // transport alone (without forcePure) forces the pure path on a native
  // tier too, which the routing test asserts.
  const initTransport = () => {
    const app = initializeApp(demoOptions());
    init(() => getRemoteConfig(app, { transport: stub }), "transport client ready");
  };

  const initNative = () => {
    const app = initializeApp(demoOptions());
    init(() => getRemoteConfig(app), "native client ready");
  };

  const setDefaults = (defaults: typeof DEFAULTS, label: string) => {
    const active = client;
    if (!active) {
      state.set("no client");
      return;
    }
    active
      .setDefaults(defaults)
      .then(() => state.set(label))
      .catch((error: unknown) => state.set(`set failed: ${toFirebaseError(error).message}`));
  };

  const readDefaults = () => {
    const active = client;
    if (!active) {
      state.set("no client");
      return;
    }
    active
      .getDefaults()
      .then((defaults: Record<string, string | number | boolean>) => defaultsLine.set(`defaults: ${JSON.stringify(defaults)}`))
      .catch((error: unknown) =>
        defaultsLine.set(`defaults failed: ${toFirebaseError(error).message}`),
      );
  };

  const readValues = () => {
    const active = client;
    if (!active) {
      state.set("no client");
      return;
    }
    Promise.all([
      active.getString("welcome_message"),
      active.getBoolean("feature_flag_dark_mode"),
      active.getNumber("tip_amount"),
      active.get("ml_prompt"),
      active.getString("checkout_button_label"),
    ])
      .then(([str, bool, num, missing, group]) =>
        valuesLine.set(
          `values: string=${str} bool=${bool} number=${num} missing=${missing} group=${group}`,
        ),
      )
      .catch((error: unknown) =>
        valuesLine.set(`values failed: ${toFirebaseError(error).message}`),
      );
  };

  const fetch = () => {
    const active = client;
    if (!active) {
      state.set("no client");
      return;
    }
    active
      .fetchAndActivate()
      .then((result: { activated: boolean; lastFetchStatus: string }) => {
        fetchLine.set(`fetch: ${JSON.stringify(result)}`);
        statusLine.set(`last fetch: ${active.lastFetchStatus}`);
      })
      .catch((error: unknown) => {
        fetchLine.set(`fetch failed: ${toFirebaseError(error).message}`);
        statusLine.set(`last fetch: ${active.lastFetchStatus}`);
      });
  };

  return (
    <View class="flex-1 gap-1 bg-background p-2">
      <Text testID="state">{state()}</Text>
      <Text testID="defaults">{defaultsLine()}</Text>
      <Text testID="values">{valuesLine()}</Text>
      <Text testID="fetch">{fetchLine()}</Text>
      <Text testID="status">{statusLine()}</Text>
      <View testID="init-pure" role="button" onPress={initPure} class="bg-primary p-1">
        <Text class="text-primary-foreground">init pure</Text>
      </View>
      <View testID="init-transport" role="button" onPress={initTransport} class="bg-primary p-1">
        <Text class="text-primary-foreground">init transport</Text>
      </View>
      <View testID="init-native" role="button" onPress={initNative} class="bg-primary p-1">
        <Text class="text-primary-foreground">init native</Text>
      </View>
      <View
        testID="set-defaults"
        role="button"
        onPress={() => setDefaults(DEFAULTS, "defaults set")}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">set defaults</Text>
      </View>
      <View
        testID="set-bad-defaults"
        role="button"
        onPress={() => setDefaults(BAD_DEFAULTS, "bad defaults set")}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">set bad defaults</Text>
      </View>
      <View testID="read-defaults" role="button" onPress={readDefaults} class="bg-primary p-1">
        <Text class="text-primary-foreground">read defaults</Text>
      </View>
      <View testID="read-values" role="button" onPress={readValues} class="bg-primary p-1">
        <Text class="text-primary-foreground">read values</Text>
      </View>
      <View testID="fetch" role="button" onPress={fetch} class="bg-primary p-1">
        <Text class="text-primary-foreground">fetch and activate</Text>
      </View>
      <View
        testID="fail-next"
        role="button"
        onPress={() => {
          stub.failNext = true;
          state.set("next fetch will fail");
        }}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">fail next</Text>
      </View>
      <View
        testID="break-template"
        role="button"
        onPress={() => {
          stub.breakNext = true;
          state.set("next template is broken");
        }}
        class="bg-primary p-1"
      >
        <Text class="text-primary-foreground">break template</Text>
      </View>
    </View>
  );
}

mount(RemoteConfigExerciseApp);
