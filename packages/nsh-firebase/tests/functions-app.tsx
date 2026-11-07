// The functions exercise UI the tests drive: the callable wire protocol
// (echo, add, backend error, unknown canonical status, missing function,
// details, the legacy `data` answer) and the labelled sandbox flow
// (tests/emulator). Mirrors the component in
// crates/nsh-firebase-functions/tests/functions_ui.rs. All test ids and
// lines carry the `fn` prefix, so the core exercise app can share an
// entry without its selectors colliding.
import { mount, signal, Text, View } from "nsh";
import { demoOptions, initializeApp } from "../src/core";
import { Functions } from "../src/functions";
import type { FunctionsError as WireError } from "../src/functions";

function functionsFor(): Functions {
  const app = initializeApp(demoOptions(), "functions-ui");
  return new Functions(app);
}

// The line a failed call shows: the mapped code with its native code,
// plus what details reached the client (the wire's optional
// error.details).
function render(error: {
  code: string;
  message: string;
  nativeCode?: string | null;
  details?: unknown;
}): string {
  // The callable wire's message already leads with its code; the
    // render shows it once.
    const message = error.message.startsWith(`${error.code}: `)
      ? error.message.slice(error.code.length + 2)
      : error.message;
    const base = `${error.code}${error.nativeCode ? ` (${error.nativeCode})` : ""}: ${message}`;
  return error.details === undefined || error.details === null
    ? base
    : `${base} details=${JSON.stringify(error.details)}`;
}

export function FunctionsExerciseApp() {
  const line = signal("fn: ready");
  const sandbox = signal("fn: sandbox idle");

  const run = (name: string, data?: unknown) => {
    void (async () => {
      try {
        const result = await functionsFor().call(name, data);
        line.set(`fn: ${name} ok ${JSON.stringify(result)}`);
      } catch (error) {
        line.set(`fn: ${name} err ${render(error as WireError)}`);
      }
    })();
  };

  // The labelled sandbox flow (tests/emulator/functions.sandbox.test.tsx):
  // an echo with data, an add, and a function the emulator does not
  // serve, summarized in one canonical line so a wrong wire shape
  // cannot pass.
  const runSandbox = () => {
    void (async () => {
      try {
        const functions = functionsFor();
        const ping = (await functions.call("ping", { from: "sandbox" })) as {
          pong?: unknown;
          sent?: { from?: unknown };
        };
        const add = (await functions.call("add", { a: 2, b: 3 })) as { sum?: unknown };
        const missing = await functions.call("missing").catch(
          (error: WireError) => `err=${error.code}`,
        );
        sandbox.set(
          `fn: sandbox ok ping(pong=${ping.pong === true},from=${String(ping.sent?.from)}) add(sum=${String(add.sum)}) missing(${String(missing)})`,
        );
      } catch (error) {
        sandbox.set(`fn: sandbox err ${render(error as WireError)}`);
      }
    })();
  };

  const button = (testID: string, label: string, onPress: () => void) => (
    <View testID={testID} role="button" onPress={onPress} class="bg-primary p-1 px-2">
      <Text class="text-primary-foreground">{label}</Text>
    </View>
  );

  return (
    <View class="flex-1 gap-1 bg-background p-2">
      <Text>{line()}</Text>
      <Text>{sandbox()}</Text>
      {button("fn-ping", "fn ping", () => run("ping"))}
      {button("fn-add", "fn add", () => run("add", { a: 1, b: 2 }))}
      {button("fn-bad-add", "fn bad add", () => run("add", { a: "x", b: 2 }))}
      {button("fn-weird", "fn weird", () => run("weird"))}
      {button("fn-missing", "fn missing", () => run("missing"))}
      {button("fn-details", "fn details", () => run("details"))}
      {button("fn-legacy", "fn legacy", () => run("legacy"))}
      {button("fn-sandbox", "fn sandbox", runSandbox)}
    </View>
  );
}

mount(FunctionsExerciseApp);
