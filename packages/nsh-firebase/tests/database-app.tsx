// The database exercise UI the tests drive: the REST verbs, push
// ordering, the transaction retry, a queried read and the live listener
// (the mirror of the Rust tests' CrudApp and ListenerApp). Mounted at
// the bottom, like tests/core-app.tsx.
import { mount, signal, Text, View } from "nsh";
import { demoOptions, initializeApp, toFirebaseError } from "../src/core";
import { Database, DatabaseReference, getDatabase } from "../src/database";

// The failure text the screens show: the stable code first.
function failed(error: unknown): string {
  const err = toFirebaseError(error);
  return `${err.code} (${err.nativeCode ?? "-"})`;
}

// The value text the screens show, the way Rust's `show` formats it.
function show(value: unknown): string {
  if (value === null || value === undefined) return "none";
  return JSON.stringify(value);
}

async function run(name: string, call: () => Promise<string>): Promise<string> {
  try {
    return await call();
  } catch (error) {
    return `${name}: ${failed(error)}`;
  }
}

// The same, with the message: for a failure whose text pins behavior
// (the invalid-JSON mapping, an invalid query at subscribe time).
async function runWithMessage(name: string, call: () => Promise<string>): Promise<string> {
  try {
    return await call();
  } catch (error) {
    // A FirebaseError's message already leads with its code.
    return `${name}: ${toFirebaseError(error).message}`;
  }
}

export function DatabaseExerciseApp() {
  const db: Database = getDatabase(initializeApp(demoOptions()));
  const line = signal("ready");
  const lastPush = signal("");
  // The listener's signals: the value and the connection status, bound
  // straight into the text below (a connect at mount, like the Rust
  // ListenerApp). The watch line listens at the sandbox path the
  // emulator runs against.
  const live = db.ref("ui-live").onValue();
  const watch = db.ref("sandbox-db/watch").onValue();

  const set = () =>
    run("set", async () => {
      await db.ref("ui-todos").child("one").set({ done: false, title: "one" });
      return "set: ok";
    }).then((value) => line.set(value));

  const get = () =>
    run("get", async () => {
      const value = await db.ref("ui-todos").child("one").get();
      return `get: ${show(value)}`;
    }).then((value) => line.set(value));

  const update = () =>
    run("multi", async () => {
      const r: DatabaseReference = db.ref("ui-multi");
      await r.set({ a: 1, b: { c: 1 } });
      await r.update({ a: 9, "b/d": 7 });
      return `multi: ${show(await r.get())}`;
    }).then((value) => line.set(value));

  const remove = () =>
    run("removed", async () => {
      const one = db.ref("ui-todos").child("one");
      await one.remove();
      return `removed: ${show(await one.get())}`;
    }).then((value) => line.set(value));

  const push = () =>
    run("push", async () => {
      const pushed = db.ref("ui-pushed");
      // The same clock tick: the second id must sort right after the
      // first.
      const first = await pushed.push({ n: 1 });
      const second = await pushed.push({ n: 2 });
      lastPush.set(second);
      return `push: ${first.length} ${second.length} ${first < second}`;
    }).then((value) => line.set(value));

  const pushNext = () =>
    run("push-next", async () => {
      const id = await db.ref("ui-pushed").push({ n: 3 });
      const previous = lastPush();
      lastPush.set(id);
      return `push-next: ${previous} < ${id} = ${previous < id}`;
    }).then((value) => line.set(value));

  const transaction = () =>
    run("transaction", async () => {
      const result = await db
        .ref("ui-count")
        .transaction((current) => (typeof current === "number" ? current + 1 : 1));
      return `transaction: committed=${result.committed} snapshot=${show(result.snapshot)}`;
    }).then((value) => line.set(value));

  const query = () =>
    run("query", async () => {
      const value = await db
        .ref("ui-scores")
        .orderByChild("score")
        .limitToFirst(2)
        .get();
      return `query: ${value && typeof value === "object" && !Array.isArray(value) ? Object.keys(value).join(" ") : "none"}`;
    }).then((value) => line.set(value));

  const getMissing = () =>
    run("get-missing", async () => {
      const value = await db.ref("ui-nowhere").get();
      return `get-missing: answered ${show(value)}`;
    }).then((value) => line.set(value));

  // What a second client does in the sandbox test: a separate app
  // instance writes the watched path, and the listener here receives it
  // over its own connection. The clear resets the path (the delete
  // arrives on the open stream as a put of null).
  const writeWatch = () =>
    run("write-watch", async () => {
      await db.ref("sandbox-db/watch").set("from-second-client");
      return "write-watch: ok";
    }).then((value) => line.set(value));

  const clearWatch = () =>
    run("clear-watch", async () => {
      await db.ref("sandbox-db").remove();
      return "clear-watch: ok";
    }).then((value) => line.set(value));

  // A filter without orderBy: subscribe time must say so, loudly, not
  // open a stream that silently retries.
  const listenBad = () =>
    runWithMessage("listen-bad", async () => {
      db.ref("ui-bad").limitToFirst(2).onValue();
      return "listen-bad: opened";
    }).then((value) => line.set(value));

  // A 200 body that is not JSON maps to a network failure with the
  // message the Rust side carries.
  const getBadJson = () =>
    runWithMessage("get-bad-json", async () => {
      const value = await db.ref("ui-blob").get();
      return `get-bad-json: ${show(value)}`;
    }).then((value) => line.set(value));

  // getDatabase hands back one database per app: the token set here
  // reaches the second instance, and the read carries ?auth=tok-1.
  const sharedDb = () =>
    run("shared", async () => {
      const app = initializeApp(demoOptions());
      const a = getDatabase(app);
      a.setAuthToken("tok-1");
      const b = getDatabase(app);
      const same = a === b;
      const value = await b.ref("ui-auth").get();
      return `shared: same=${same} get=${show(value)}`;
    }).then((value) => line.set(value));

  // A space and a question mark are legal RTDB keys: the request path
  // percent-encodes them, the slashes stay.
  const getEncoded = () =>
    run("encoded", async () => {
      const value = await db.ref("ui a?b").get();
      return `encoded: ${show(value)}`;
    }).then((value) => line.set(value));

  const button = (id: string, label: string, onPress: () => void) => (
    <View testID={id} role="button" onPress={onPress} class="bg-primary p-2">
      <Text class="text-primary-foreground">{label}</Text>
    </View>
  );

  return (
    <View class="flex-1 gap-2 bg-background p-4">
      <Text testID="state">{line()}</Text>
      <Text testID="live">{`live: ${live.status()} ${show(live.data())}`}</Text>
      <Text testID="watch">{`watch: ${watch.status()} ${show(watch.data())}`}</Text>
      {button("set", "set", set)}
      {button("get", "get", get)}
      {button("update", "update", update)}
      {button("remove", "remove", remove)}
      {button("push", "push", push)}
      {button("push-next", "push next", pushNext)}
      {button("transaction", "transaction", transaction)}
      {button("query", "query", query)}
      {button("get-missing", "get missing", getMissing)}
      {button("write-watch", "write watch", writeWatch)}
      {button("clear-watch", "clear watch", clearWatch)}
      {button("listen-bad", "listen bad", listenBad)}
      {button("get-bad-json", "get bad json", getBadJson)}
      {button("shared-db", "shared db", sharedDb)}
      {button("get-encoded", "get encoded", getEncoded)}
      {button("off", "off", () => live.off())}
    </View>
  );
}

mount(DatabaseExerciseApp);
