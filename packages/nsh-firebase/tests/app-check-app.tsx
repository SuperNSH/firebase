// The app-check exercise UI the tests drive: the pure tier's debug
// exchange over the stubbed network (a local stand-in base), the native
// tier's firebase.appCheck.getToken routing, the ttl cache against the
// manual clock, and every failure mapped to its stable code. The entry
// is this file; capabilities come from tests/nsh.json (network
// 127.0.0.1, host firebase.*).
import { mount, signal, Text, View } from "nsh";
import { demoOptions, initializeApp, toFirebaseError } from "../src/core";
import { APP_CHECK_BASE, exchangeDebugTokenUrl, getAppCheck } from "../src/app-check";
import type { AppCheck } from "../src/app-check";

// The sandbox stand-in for firebaseappcheck.googleapis.com, like the
// analytics tests' collector endpoint: inside tests/nsh.json's grants.
const SANDBOX_BASE = "http://127.0.0.1:19191/v1";
const FAKE_DEBUG_TOKEN = "demo-nsh-fake-debug-token";

// FirebaseError's message already carries the stable code and the native
// code, so the tests match on exactly what the facade produced.
function describeError(error: unknown): string {
  return toFirebaseError(error).message;
}

export function AppCheckExerciseApp() {
  const line = signal("ready");
  const tokenLine = signal("token: not called");
  const expiresLine = signal("expires: none");
  const urlLine = signal("url: not built");
  let client: AppCheck | null = null;

  // Each test opens a fresh VM: initialize before the facade's first call.
  const app = initializeApp(demoOptions());

  const init = (baseUrl?: string) => {
    client = getAppCheck(app, { debugToken: FAKE_DEBUG_TOKEN, baseUrl });
    line.set("client ready");
  };

  const initEmpty = () => {
    client = getAppCheck(app, {});
    line.set("client ready without a debug token");
  };

  const getToken = (label: string, call: { debug?: boolean }) => {
    if (!client) {
      tokenLine.set("token: no client");
      return;
    }
    client
      .getToken(call)
      .then((result) => {
        tokenLine.set(`token: ${result.token}`);
        expiresLine.set(`expires: ${result.expiresAtMs === null ? "host" : result.expiresAtMs}`);
      })
      .catch((error: unknown) => {
        tokenLine.set(`${label}: ${describeError(error)}`);
        expiresLine.set("expires: none");
      });
  };

  const buildUrl = () => {
    urlLine.set(`url: ${exchangeDebugTokenUrl(APP_CHECK_BASE, app.options.projectId, app.options.appId, app.options.apiKey)}`);
  };

  const button = (id: string, label: string, onPress: () => void) => (
    <View testID={id} role="button" onPress={onPress} class="bg-primary p-2">
      <Text class="text-primary-foreground">{label}</Text>
    </View>
  );

  return (
    <View class="flex-1 gap-2 bg-background p-4">
      <Text testID="state">{line()}</Text>
      <Text testID="token">{tokenLine()}</Text>
      <Text testID="expires">{expiresLine()}</Text>
      <Text testID="url">{urlLine()}</Text>
      {button("init", "init", () => init())}
      {button("init-sandbox", "init sandbox", () => init(SANDBOX_BASE))}
      {button("init-empty", "init empty", initEmpty)}
      {button("token", "token", () => getToken("token", { debug: true }))}
      {button("token-attested", "attested", () => getToken("attested", {}))}
      {button("url", "url", buildUrl)}
    </View>
  );
}

mount(AppCheckExerciseApp);
