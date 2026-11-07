// The Firestore exercise UI the tests drive: documents CRUD and the
// structured query over one app instance, so the same buttons run the
// pure tier (network routes) and the native tier (host handlers).
// Mounted at the bottom, like tests/database-app.tsx.
import { mount, signal, Text, View } from "nsh";
import { demoOptions, initializeApp, toFirebaseError } from "../src/core";
import { firestore, type DocSnapshot, type Json, type WhereOp } from "../src/firestore";

// The failure text the screens show: the stable code first.
function failed(error: unknown): string {
  const err = toFirebaseError(error);
  return `${err.code} (${err.nativeCode ?? "-"})`;
}

// The same, with the message: for a failure whose text pins behavior
// (the typed-value refusals, the validation messages).
function failedWithMessage(error: unknown): string {
  // A FirebaseError's message already leads with its code.
  return toFirebaseError(error).message;
}

// The snapshot text the screens show.
function show(snapshot: DocSnapshot): string {
  const data = snapshot.data === null ? "none" : JSON.stringify(snapshot.data);
  return `exists=${snapshot.exists} data=${data}`;
}

async function run(name: string, call: () => Promise<string>): Promise<string> {
  try {
    return await call();
  } catch (error) {
    return `${name}: ${failed(error)}`;
  }
}

async function runMessage(name: string, call: () => Promise<string>): Promise<string> {
  try {
    return await call();
  } catch (error) {
    return `${name}: ${failedWithMessage(error)}`;
  }
}

// The data the set button writes, in the key order its test asserts.
const SET_DATA: Record<string, Json> = {
  done: false,
  title: "one",
  count: 3,
  price: 1.5,
  tags: ["a", 2],
  meta: { deep: 1.5, ok: true, note: null },
};

export function FirestoreExerciseApp() {
  const fs = firestore(initializeApp(demoOptions()));
  const line = signal("ready");

  const set = () =>
    run("set", async () => {
      await fs.setDoc("ui-todos/one", SET_DATA);
      return "set: ok";
    }).then((value) => line.set(value));

  const merge = () =>
    run("merge", async () => {
      await fs.setDoc("ui-todos/one", { title: "Merged", meta: { deep: 2 } }, true);
      return "merge: ok";
    }).then((value) => line.set(value));

  const get = () =>
    run("get", async () => {
      const snapshot = await fs.getDoc("ui-todos/one");
      return `get: ${show(snapshot)}`;
    }).then((value) => line.set(value));

  const missing = () =>
    run("missing", async () => {
      const snapshot = await fs.getDoc("ui-nowhere/none");
      return `missing: ${show(snapshot)}`;
    }).then((value) => line.set(value));

  const add = () =>
    run("add", async () => {
      const id = await fs.addDoc("ui-todos", { title: "Added" });
      return `add: ${id}`;
    }).then((value) => line.set(value));

  const remove = () =>
    run("delete", async () => {
      await fs.deleteDoc("ui-todos/one");
      return "delete: ok";
    }).then((value) => line.set(value));

  const removeMissing = () =>
    run("delete-missing", async () => {
      // An absent document deletes done: Firestore deletes are
      // idempotent.
      await fs.deleteDoc("ui-gone/x");
      return "delete-missing: ok";
    }).then((value) => line.set(value));

  const query = () =>
    run("query", async () => {
      const docs = await fs.runQuery({
        collectionPath: "ui-cities",
        where: { field: "state", op: "==", value: "CA" },
        orderBy: [{ field: "pop", direction: "desc" }],
        limit: 2,
      });
      return `query: ${docs.map((doc) => doc.id).join(" ")}`;
    }).then((value) => line.set(value));

  const queryPlain = () =>
    run("query-plain", async () => {
      const docs = await fs.runQuery({ collectionPath: "ui-cities", limit: 5 });
      return `query-plain: ${docs.length} docs`;
    }).then((value) => line.set(value));

  const denied = () =>
    runMessage("denied", async () => {
      const snapshot = await fs.getDoc("ui-denied/x");
      return `denied: ${show(snapshot)}`;
    }).then((value) => line.set(value));

  const badPath = () =>
    runMessage("bad-path", async () => {
      const snapshot = await fs.getDoc("ui-todos");
      return `bad-path: ${show(snapshot)}`;
    }).then((value) => line.set(value));

  const typed = () =>
    runMessage("typed", async () => {
      const snapshot = await fs.getDoc("ui-typed/x");
      return `typed: ${show(snapshot)}`;
    }).then((value) => line.set(value));

  const auth = () =>
    run("auth", async () => {
      fs.setAuthToken("tok-1");
      const snapshot = await fs.getDoc("ui-auth/a");
      return `auth: ${show(snapshot)}`;
    }).then((value) => line.set(value));

  const badJson = () =>
    runMessage("bad-json", async () => {
      const snapshot = await fs.getDoc("ui-badjson/x");
      return `bad-json: ${show(snapshot)}`;
    }).then((value) => line.set(value));

  const nan = () =>
    runMessage("nan", async () => {
      await fs.setDoc("ui-bad/x", { n: NaN });
      return "nan: ok";
    }).then((value) => line.set(value));

  const mergeEmpty = () =>
    runMessage("merge-empty", async () => {
      await fs.setDoc("ui-todos/one", {}, true);
      return "merge-empty: ok";
    }).then((value) => line.set(value));

  const queryInScalar = () =>
    runMessage("query-in", async () => {
      const docs = await fs.runQuery({
        collectionPath: "ui-cities",
        where: { field: "state", op: "in", value: "CA" },
      });
      return `query-in: ${docs.length} docs`;
    }).then((value) => line.set(value));

  const queryBadOp = () =>
    runMessage("query-bad-op", async () => {
      const docs = await fs.runQuery({
        collectionPath: "ui-cities",
        where: { field: "state", op: "%%%" as WhereOp, value: "CA" },
      });
      return `query-bad-op: ${docs.length} docs`;
    }).then((value) => line.set(value));

  const button = (id: string, label: string, onPress: () => void) => (
    <View testID={id} role="button" onPress={onPress} class="bg-primary p-2">
      <Text class="text-primary-foreground">{label}</Text>
    </View>
  );

  return (
    <View class="flex-1 gap-2 bg-background p-4">
      <Text testID="state">{line()}</Text>
      {button("set", "set", set)}
      {button("merge", "merge", merge)}
      {button("get", "get", get)}
      {button("missing", "missing", missing)}
      {button("add", "add", add)}
      {button("delete", "delete", remove)}
      {button("delete-missing", "delete missing", removeMissing)}
      {button("query", "query", query)}
      {button("query-plain", "query plain", queryPlain)}
      {button("denied", "denied", denied)}
      {button("bad-path", "bad path", badPath)}
      {button("typed", "typed", typed)}
      {button("auth", "auth", auth)}
      {button("bad-json", "bad json", badJson)}
      {button("nan", "nan", nan)}
      {button("merge-empty", "merge empty", mergeEmpty)}
      {button("query-in", "query in", queryInScalar)}
      {button("query-bad-op", "query bad op", queryBadOp)}
    </View>
  );
}

mount(FirestoreExerciseApp);
