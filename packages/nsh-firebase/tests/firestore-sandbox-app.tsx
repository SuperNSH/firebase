// The live-network sandbox entry: only the six buttons the emulator
// sandbox test presses, so every pressable sits on a real screen. The
// headless unit gates drive ../firestore-app.tsx instead. Network stays
// 127.0.0.1-only per tests/nsh.json.
import { mount, signal, Text, View } from "nsh";
import { demoOptions, initializeApp, toFirebaseError } from "../src/core";
import { firestore, type DocSnapshot } from "../src/firestore";

function failed(error: unknown): string {
  const err = toFirebaseError(error);
  return `${err.code} (${err.nativeCode ?? "-"})`;
}

function show(snapshot: DocSnapshot): string {
  const data = snapshot.exists ? JSON.stringify(snapshot.data) : "none";
  return `exists=${snapshot.exists} data=${data}`;
}

export function FirestoreSandboxApp() {
  const fs = firestore(initializeApp(demoOptions()));
  const line = signal("ready");

  const run = (name: string, call: () => Promise<string>): void => {
    call()
      .then((value) => line.set(value))
      .catch((error: unknown) => {
        const err = toFirebaseError(error);
        line.set(`${name}: ${err.code} (${err.nativeCode ?? "-"})`);
      });
  };

  const sandboxSet = () =>
    run("sb-set", async () => {
      await fs.setDoc("sandbox-fs/one", { n: 1 });
      return "sb-set: ok";
    });

  const sandboxGet = () =>
    run("sb-get", async () => {
      const snapshot = await fs.getDoc("sandbox-fs/one");
      return `sb-get: ${show(snapshot)}`;
    });

  const sandboxAdd = () =>
    run("sb-add", async () => {
      const id = await fs.addDoc("sandbox-fs", { n: 2 });
      return `sb-add: ${id.length > 0 ? "ok" : "empty"}`;
    });

  const sandboxQuery = () =>
    run("sb-query", async () => {
      const docs = await fs.runQuery({
        collectionPath: "sandbox-fs",
        where: { field: "n", op: "==", value: 1 },
        limit: 5,
      });
      return `sb-query: has-one=${docs.some((doc) => doc.id === "one")}`;
    });

  const sandboxDelete = () =>
    run("sb-delete", async () => {
      await fs.deleteDoc("sandbox-fs/one");
      return "sb-delete: ok";
    });

  const sandboxGetGone = () =>
    run("sb-get-gone", async () => {
      const snapshot = await fs.getDoc("sandbox-fs/one");
      return `sb-get-gone: ${show(snapshot)}`;
    });

  const button = (id: string, label: string, onPress: () => void) => (
    <View testID={id} role="button" onPress={onPress} class="bg-primary p-2">
      <Text class="text-primary-foreground">{label}</Text>
    </View>
  );

  return (
    <View class="flex-1 gap-2 bg-background p-4">
      <Text testID="state">{line()}</Text>
      {button("sb-set", "sb set", sandboxSet)}
      {button("sb-get", "sb get", sandboxGet)}
      {button("sb-add", "sb add", sandboxAdd)}
      {button("sb-query", "sb query", sandboxQuery)}
      {button("sb-delete", "sb delete", sandboxDelete)}
      {button("sb-get-gone", "sb get gone", sandboxGetGone)}
    </View>
  );
}

mount(FirestoreSandboxApp);
