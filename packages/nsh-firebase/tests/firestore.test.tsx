// Headless tests of Firestore on a manual clock with a stubbed network:
// the typed-value encoding, the four document calls and the structured
// query with their exact wire shapes, the REST error mapping, the
// validation refusals, and the native tier's contract routing (the test
// plays the host with the same handler names the native areas register).
// The mirror of crates/nsh-firebase-firestore/tests/firestore_ui.rs.
import { describe, expect, open, test } from "nsh/test";
import type { App, OpenOptions } from "nsh/test";
import {
  encodeValue,
  decodeValue,
  errorForResponse,
  idFromName,
  leafPaths,
  mergeMaskQuery,
  type Json,
} from "../src/firestore";

// A nested entry would make `nsh` treat tests/ as the app; this file
// mounts its own exercise UI, next to the App.tsx the gate builds.
const ENTRY = "./firestore-app.tsx";

const FS = "http://127.0.0.1:18080";
const ROOT = `${FS}/v1/projects/demo-nsh/databases/(default)/documents`;

// The document the get route answers, in wire form.
const DOC_WIRE = {
  name: "projects/demo-nsh/databases/(default)/documents/ui-todos/one",
  fields: {
    done: { booleanValue: false },
    title: { stringValue: "one" },
    count: { integerValue: "3" },
  },
};

// The set button's data, in the key order its body assertion needs.
const SET_DATA: Record<string, Json> = {
  done: false,
  title: "one",
  count: 3,
  price: 1.5,
  tags: ["a", 2],
  meta: { deep: 1.5, ok: true, note: null },
};

const SET_BODY =
  '{"fields":{"done":{"booleanValue":false},"title":{"stringValue":"one"},"count":{"integerValue":"3"},"price":{"doubleValue":1.5},"tags":{"arrayValue":{"values":[{"stringValue":"a"},{"integerValue":"2"}]}},"meta":{"mapValue":{"fields":{"deep":{"doubleValue":1.5},"ok":{"booleanValue":true},"note":{"nullValue":"NULL_VALUE"}}}}}}';

const QUERY_REPLY = [
  {
    document: {
      name: "projects/demo-nsh/databases/(default)/documents/ui-cities/SERVER1",
      fields: { pop: { integerValue: "500" } },
    },
    readTime: "2026-01-01T00:00:00.000000001Z",
  },
  {
    document: {
      name: "projects/demo-nsh/databases/(default)/documents/ui-cities/SERVER2",
      fields: { pop: { integerValue: "300" } },
    },
    readTime: "2026-01-01T00:00:00.000000001Z",
  },
  { readTime: "2026-01-01T00:00:00.000000001Z" },
];

type Routes = NonNullable<OpenOptions["network"]>;

async function openPure(network: Routes): Promise<App> {
  return open(ENTRY, { clock: {}, network });
}

describe("firestore", () => {
  test("typed values encode to the wire and decode back", () => {
    expect(encodeValue("one")).toEqual({ stringValue: "one" });
    expect(encodeValue(true)).toEqual({ booleanValue: true });
    expect(encodeValue(null)).toEqual({ nullValue: "NULL_VALUE" });
    // Safe integers take their string form; -0 keeps its sign spelled.
    expect(encodeValue(3)).toEqual({ integerValue: "3" });
    expect(encodeValue(1.5)).toEqual({ doubleValue: 1.5 });
    expect(encodeValue(-0)).toEqual({ doubleValue: "-0" });
    expect(encodeValue(["a", 2])).toEqual({
      arrayValue: { values: [{ stringValue: "a" }, { integerValue: "2" }] },
    });
    expect(encodeValue({ a: true })).toEqual({
      mapValue: { fields: { a: { booleanValue: true } } },
    });
    // A number JSON cannot carry is refused, not silently nulled.
    expect(() => encodeValue(NaN)).toThrow("has no JSON form");
    expect(decodeValue({ stringValue: "one" })).toBe("one");
    expect(decodeValue({ integerValue: "3" })).toBe(3);
    expect(decodeValue({ doubleValue: 1.5 })).toBe(1.5);
    // Past the safe range the float stands.
    expect(decodeValue({ integerValue: "9007199254740993" })).toBe(9007199254740992);
    expect(decodeValue({ nullValue: "NULL_VALUE" })).toBeNull();
    // Absent containers decode empty, as the server omits them.
    expect(decodeValue({ mapValue: {} })).toEqual({});
    expect(decodeValue({ arrayValue: {} })).toEqual([]);
    // Types beyond the seven are an error, not a silent loss.
    expect(() => decodeValue({ timestampValue: "2026-01-01T00:00:00Z" })).toThrow(
      "timestampValue",
    );
    expect(() => decodeValue({ doubleValue: "NaN" })).toThrow("no JSON form");
    // Merge masks: maps descend, arrays and empty maps are leaves.
    const data = { title: "Merged", meta: { deep: 2, list: [1], void: {} } };
    expect(leafPaths(data)).toEqual(["title", "meta.deep", "meta.list", "meta.void"]);
    expect(mergeMaskQuery(leafPaths(data))).toBe(
      "updateMask.fieldPaths=title&updateMask.fieldPaths=meta.deep" +
        "&updateMask.fieldPaths=meta.list&updateMask.fieldPaths=meta.void",
    );
    expect(() => leafPaths({})).toThrow("cannot be empty");
    expect(idFromName("projects/p/databases/(default)/documents/cities/LA")).toBe("LA");
    const denied = errorForResponse(
      403,
      JSON.stringify({
        error: {
          code: 403,
          message: "Missing or insufficient permissions.",
          status: "PERMISSION_DENIED",
        },
      }),
    );
    expect(denied.code).toBe("firestore/permission-denied");
    expect(denied.message).toBe("firestore/permission-denied: Missing or insufficient permissions. (403)");
    expect(errorForResponse(404, "").code).toBe("firestore/not-found");
    expect(errorForResponse(400, "oops").code).toBe("firestore/invalid-argument");
  });

  test("setDoc writes the typed fields document", async () => {
    const app = await openPure({
      [`PATCH ${ROOT}/ui-todos/one`]: { status: 200, body: "" },
    });
    await app.press("id:set");
    await app.waitFor("text:set: ok");
    const patch = app.requests().find((request) => request.method === "PATCH");
    expect(patch?.url).toBe(`${ROOT}/ui-todos/one`);
    expect(patch?.headers["content-type"]).toBe("application/json");
    expect(patch?.body).toBe(SET_BODY);
  });

  test("setDoc with merge writes the leaf-path updateMask", async () => {
    const app = await openPure({
      [`PATCH ${ROOT}/ui-todos/one?updateMask.fieldPaths=title&updateMask.fieldPaths=meta.deep`]:
        { status: 200, body: "" },
    });
    await app.press("id:merge");
    await app.waitFor("text:merge: ok");
    const patch = app.requests().find((request) => request.method === "PATCH");
    expect(patch?.url).toBe(
      `${ROOT}/ui-todos/one?updateMask.fieldPaths=title&updateMask.fieldPaths=meta.deep`,
    );
    expect(patch?.body).toBe(
      '{"fields":{"title":{"stringValue":"Merged"},"meta":{"mapValue":{"fields":{"deep":{"integerValue":"2"}}}}}}',
    );
  });

  test("getDoc decodes a present document and maps 404 to exists false", async () => {
    const app = await openPure({
      [`GET ${ROOT}/ui-todos/one`]: { status: 200, body: DOC_WIRE },
      [`GET ${ROOT}/ui-nowhere/none`]: {
        status: 404,
        body: { error: { code: 404, message: "not there", status: "NOT_FOUND" } },
      },
    });
    await app.press("id:get");
    await app.waitFor('text:get: exists=true data={"done":false,"title":"one","count":3}');
    await app.press("id:missing");
    await app.waitFor("text:missing: exists=false data=none");
  });

  test("addDoc posts to the collection and reads the id from the name", async () => {
    const app = await openPure({
      [`POST ${ROOT}/ui-todos`]: {
        status: 200,
        body: {
          name: "projects/demo-nsh/databases/(default)/documents/ui-todos/SERVER1",
          fields: { title: { stringValue: "Added" } },
        },
      },
    });
    await app.press("id:add");
    await app.waitFor("text:add: SERVER1");
    const post = app.requests().find((request) => request.method === "POST");
    expect(post?.url).toBe(`${ROOT}/ui-todos`);
    expect(post?.body).toBe('{"fields":{"title":{"stringValue":"Added"}}}');
  });

  test("deleteDoc deletes and a 404 is done", async () => {
    const app = await openPure({
      [`DELETE ${ROOT}/ui-todos/one`]: { status: 200, body: "" },
      [`DELETE ${ROOT}/ui-gone/x`]: {
        status: 404,
        body: { error: { code: 404, message: "gone already", status: "NOT_FOUND" } },
      },
    });
    await app.press("id:delete");
    await app.waitFor("text:delete: ok");
    await app.press("id:delete-missing");
    await app.waitFor("text:delete-missing: ok");
  });

  test("runQuery posts the structured query and maps the documents", async () => {
    const app = await openPure({
      [`POST ${ROOT}:runQuery`]: { status: 200, body: QUERY_REPLY },
    });
    await app.press("id:query");
    await app.waitFor("text:query: SERVER1 SERVER2");
    await app.press("id:query-plain");
    await app.waitFor("text:query-plain: 2 docs");
    const posts = app.requests().filter((request) => request.method === "POST");
    expect(posts.length).toBe(2);
    expect(JSON.parse(posts[0]?.body ?? "")).toEqual({
      parent: "projects/demo-nsh/databases/(default)/documents",
      structuredQuery: {
        from: [{ collectionId: "ui-cities" }],
        where: {
          fieldFilter: {
            field: { fieldPath: "state" },
            op: "EQUAL",
            value: { stringValue: "CA" },
          },
        },
        orderBy: [{ field: { fieldPath: "pop" }, direction: "DESCENDING" }],
        limit: 2,
      },
    });
    // The optional keys appear only when set.
    expect(JSON.parse(posts[1]?.body ?? "")).toEqual({
      parent: "projects/demo-nsh/databases/(default)/documents",
      structuredQuery: { from: [{ collectionId: "ui-cities" }], limit: 5 },
    });
  });

  test("a denied read maps to firestore/permission-denied", async () => {
    const app = await openPure({
      [`GET ${ROOT}/ui-denied/x`]: {
        status: 403,
        body: {
          error: {
            code: 403,
            message: "Missing or insufficient permissions.",
            status: "PERMISSION_DENIED",
          },
        },
      },
    });
    await app.press("id:denied");
    await app.waitFor(
      "text:/^denied: firestore\\/permission-denied: Missing or insufficient permissions. \\(403\\)$/",
    );
  });

  test("values the JSON contract cannot carry are invalid-data", async () => {
    const app = await openPure({
      [`GET ${ROOT}/ui-typed/x`]: {
        status: 200,
        body: {
          name: "projects/demo-nsh/databases/(default)/documents/ui-typed/x",
          fields: { when: { timestampValue: "2026-01-01T00:00:00Z" } },
        },
      },
    });
    await app.press("id:typed");
    await app.waitFor(
      "text:/^typed: firestore\\/invalid-data: the value type `timestampValue` has no JSON form/",
    );
    // NaN is refused before any request goes out.
    await app.press("id:nan");
    await app.waitFor("text:nan: firestore/invalid-data: the number NaN has no JSON form");
  });

  test("a non-json body is invalid-response", async () => {
    const app = await openPure({
      [`GET ${ROOT}/ui-badjson/x`]: { status: 200, body: "not-json" },
    });
    await app.press("id:bad-json");
    await app.waitFor(
      "text:bad-json: firestore/invalid-response: the server sent a body that is not JSON",
    );
  });

  test("the auth token rides as a Bearer header", async () => {
    const app = await openPure({
      [`GET ${ROOT}/ui-auth/a`]: {
        status: 200,
        body: {
          name: "projects/demo-nsh/databases/(default)/documents/ui-auth/a",
          fields: { ok: { booleanValue: true } },
        },
      },
    });
    await app.press("id:auth");
    await app.waitFor('text:auth: exists=true data={"ok":true}');
    const get = app.requests().find((request) => request.method === "GET");
    expect(get?.headers.authorization).toBe("Bearer tok-1");
  });

  test("validation refusals happen before any request", async () => {
    const app = await openPure({});
    await app.press("id:bad-path");
    await app.waitFor(
      "text:bad-path: core/invalid-argument: not a document path: `ui-todos` must name collection/document pairs (an even segment count)",
    );
    await app.press("id:merge-empty");
    await app.waitFor(
      "text:merge-empty: core/invalid-argument: a merge write needs at least one field; the REST patch mask cannot be empty",
    );
    await app.press("id:query-in");
    await app.waitFor("text:query-in: core/invalid-argument: `in` needs an array value");
    await app.press("id:query-bad-op");
    await app.waitFor(
      "text:query-bad-op: core/invalid-argument: %%% is not a where op; one of ==, <, <=, >, >=, !=, array-contains, in, not-in, array-contains-any",
    );
    expect(app.requests().length).toBe(0);
  });

  test("the native tier routes the five calls with the contract args", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.firestore.getDoc": async ({ path }: { path: string }) =>
            path === "ui-nowhere/none"
              ? { exists: false, data: null }
              : { exists: true, data: { done: false, title: "one", count: 3 } },
          "firebase.firestore.setDoc": async () => ({}),
          "firebase.firestore.addDoc": async () => ({ id: "NATIVE1" }),
          "firebase.firestore.deleteDoc": async () => ({}),
          "firebase.firestore.runQuery": async () => ({
            docs: [
              { id: "N1", data: { n: 1 } },
              { id: "N2", data: { n: 2 } },
            ],
          }),
        },
      },
    });
    await app.press("id:set");
    await app.waitFor("text:set: ok");
    await app.press("id:merge");
    await app.waitFor("text:merge: ok");
    await app.press("id:get");
    await app.waitFor('text:get: exists=true data={"done":false,"title":"one","count":3}');
    await app.press("id:missing");
    await app.waitFor("text:missing: exists=false data=none");
    await app.press("id:add");
    await app.waitFor("text:add: NATIVE1");
    await app.press("id:delete");
    await app.waitFor("text:delete: ok");
    await app.press("id:delete-missing");
    await app.waitFor("text:delete-missing: ok");
    await app.press("id:query");
    await app.waitFor("text:query: N1 N2");
    // The hosts answered everything: nothing went to fetch, and every
    // call carried exactly the contract's args, `merge` only when set.
    expect(app.requests().length).toBe(0);
    expect(app.host.calls()).toEqual([
      { name: "firebase.firestore.setDoc", data: { path: "ui-todos/one", data: SET_DATA } },
      {
        name: "firebase.firestore.setDoc",
        data: {
          path: "ui-todos/one",
          data: { title: "Merged", meta: { deep: 2 } },
          merge: true,
        },
      },
      { name: "firebase.firestore.getDoc", data: { path: "ui-todos/one" } },
      { name: "firebase.firestore.getDoc", data: { path: "ui-nowhere/none" } },
      {
        name: "firebase.firestore.addDoc",
        data: { collectionPath: "ui-todos", data: { title: "Added" } },
      },
      { name: "firebase.firestore.deleteDoc", data: { path: "ui-todos/one" } },
      { name: "firebase.firestore.deleteDoc", data: { path: "ui-gone/x" } },
      {
        name: "firebase.firestore.runQuery",
        data: {
          collectionPath: "ui-cities",
          where: { field: "state", op: "==", value: "CA" },
          orderBy: [{ field: "pop", direction: "desc" }],
          limit: 2,
        },
      },
    ]);
  });

  test("a native reply without the contract shape is invalid-response", async () => {
    const missingExists: App = await open(ENTRY, {
      clock: {},
      host: { handlers: { "firebase.firestore.getDoc": async () => ({}) } },
    });
    await missingExists.press("id:get");
    // The screen shows the stable code with the native code placeholder,
    // not the message - the run() formatters are code-first.
    await missingExists.waitFor("text:get: firestore/invalid-response (-)");
    const missingData: App = await open(ENTRY, {
      clock: {},
      host: { handlers: { "firebase.firestore.getDoc": async () => ({ exists: true }) } },
    });
    await missingData.press("id:get");
    await missingData.waitFor("text:get: firestore/invalid-response (-)");
    const missingId: App = await open(ENTRY, {
      clock: {},
      host: { handlers: { "firebase.firestore.addDoc": async () => ({}) } },
    });
    await missingId.press("id:add");
    await missingId.waitFor("text:add: firestore/invalid-response (-)");
    const missingDocs: App = await open(ENTRY, {
      clock: {},
      host: { handlers: { "firebase.firestore.runQuery": async () => ({}) } },
    });
    await missingDocs.press("id:query");
    await missingDocs.waitFor("text:query: firestore/invalid-response (-)");
  });
});
