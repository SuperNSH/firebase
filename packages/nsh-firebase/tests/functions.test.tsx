// Headless tests of functions on a manual clock with a stubbed network:
// the request URL and body shape, result mapping (both `data` and legacy
// `result` answers), the backend error mapping to the documented
// functions/* codes, the emulator's missing-function shape, and the
// native routing through firebase.functions.call with played host
// handlers. No real time and no real network.
import { describe, expect, open, test } from "nsh/test";
import type { App, StubRequest, StubResponse } from "nsh/test";

// The app root is the package root: a nested entry would make `nsh`
// treat tests/ as the app and lose its nsh.json.
const ENTRY = "./functions-app.tsx";

const BASE = "http://127.0.0.1:15001/demo-nsh/us-central1";
const url = (name: string) => `${BASE}/${name}`;
const NATIVE_CALL = "firebase.functions.call";

function json(status: number, body: unknown): StubResponse {
  return { status, body: JSON.stringify(body) };
}

// The add route answers by body: the good call, and the bad call whose
// backend throws HttpsError("invalid-argument") — each the shape the
// real emulator's firebase-functions v1 runtime sends.
function wireRoutes(): Record<string, (request: StubRequest) => StubResponse> {
  return {
    [`POST ${url("add")}`]: (request: StubRequest) =>
      (request.body ?? "").includes('"a":1')
        ? json(200, { result: { sum: 3 } })
        : json(400, {
            error: { status: "INVALID_ARGUMENT", message: "a and b must be numbers" },
          }),
    [`POST ${url("ping")}`]: () => json(200, { result: { pong: true, sent: null } }),
    // A 200 answer may still carry an error body: the canonical name
    // decides, and an unknown one is internal.
    [`POST ${url("weird")}`]: () => json(200, { error: { status: "WEIRD", message: "m" } }),
    // The emulator's missing-function answer: 404 with a plain-text
    // body, which is not JSON.
    [`POST ${url("missing")}`]: () => ({
      status: 404,
      body: "Function us-central1-missing does not exist, valid functions are: ping, add",
    }),
    [`POST ${url("details")}`]: () =>
      json(400, {
        error: { status: "FAILED_PRECONDITION", message: "no", details: { quota: 9 } },
      }),
    // The legacy `data` key: the SDK reads data before result.
    [`POST ${url("legacy")}`]: () => json(200, { data: { old: 1 } }),
  };
}

// The engine sends fetch header names lower-cased; read them
// case-insensitively, as HTTP means them.
function header(request: StubRequest, name: string): string | undefined {
  const lower = name.toLowerCase();
  const found = Object.keys(request.headers).find((key) => key.toLowerCase() === lower);
  return found === undefined ? undefined : request.headers[found];
}

describe("functions", () => {
  test("the request speaks the callable wire protocol and maps the result", async () => {
    const app: App = await open(ENTRY, { clock: {}, network: wireRoutes() });
    await app.press("id:fn-add");
    await app.waitFor('text:/^fn: add ok \\{"sum":3\\}$/');

    const requests = app.requests();
    expect(requests.length).toBe(1);
    expect(requests[0].method).toBe("POST");
    expect(requests[0].url).toBe(url("add"));
    expect(header(requests[0], "Content-Type")).toBe("application/json");
    expect(JSON.parse(requests[0].body ?? "{}")).toEqual({ data: { a: 1, b: 2 } });
  });

  test("absent data is null in the body", async () => {
    const app: App = await open(ENTRY, { clock: {}, network: wireRoutes() });
    await app.press("id:fn-ping");
    await app.waitFor('text:/^fn: ping ok \\{"pong":true,"sent":null\\}$/');

    const requests = app.requests();
    expect(requests.length).toBe(1);
    expect(requests[0].url).toBe(url("ping"));
    expect(JSON.parse(requests[0].body ?? "{}")).toEqual({ data: null });
  });

  test("a backend error maps to the functions code with its message", async () => {
    const app: App = await open(ENTRY, { clock: {}, network: wireRoutes() });
    await app.press("id:fn-bad-add");
    await app.waitFor(
      "text:/^fn: add err functions\\/invalid-argument: a and b must be numbers \\[400\\]$/",
    );
  });

  test("an unknown canonical status is internal, read even on a 200", async () => {
    const app: App = await open(ENTRY, { clock: {}, network: wireRoutes() });
    await app.press("id:fn-weird");
    await app.waitFor(
      "text:/^fn: weird err functions\\/internal: Unknown backend error status: WEIRD \\[200\\]$/",
    );
  });

  test("the emulator's missing function is functions/not-found", async () => {
    const app: App = await open(ENTRY, { clock: {}, network: wireRoutes() });
    await app.press("id:fn-missing");
    await app.waitFor("text:/^fn: missing err functions\\/not-found: not-found \\[404\\]$/");
  });

  test("the wire's optional error.details reaches the error", async () => {
    const app: App = await open(ENTRY, { clock: {}, network: wireRoutes() });
    await app.press("id:fn-details");
    await app.waitFor(
      'text:/^fn: details err functions\\/failed-precondition: no \\[400\\] details=\\{"quota":9\\}$/',
    );
  });

  test("a data-shaped answer maps too: data is read before result", async () => {
    const app: App = await open(ENTRY, { clock: {}, network: wireRoutes() });
    await app.press("id:fn-legacy");
    await app.waitFor('text:/^fn: legacy ok \\{"old":1\\}$/');
  });

  test("the native tier routes through firebase.functions.call per the contract", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          [NATIVE_CALL]: async ({ name, data }: { name: string; data: unknown }) =>
            name === "add" ? { result: { sum: 9 } } : { result: { pong: true, sent: data } },
        },
      },
    });
    await app.press("id:fn-add");
    await app.waitFor('text:/^fn: add ok \\{"sum":9\\}$/');
    await app.press("id:fn-ping");
    await app.waitFor('text:/^fn: ping ok \\{"pong":true,"sent":null\\}$/');
    // The handler took the contract's {name, data} shape, absent data as
    // null; a fetch would have failed the ok-lines above, since this run
    // grants no network routes.
    expect(app.host.calls()).toEqual([
      { name: NATIVE_CALL, data: { name: "add", data: { a: 1, b: 2 } } },
      { name: NATIVE_CALL, data: { name: "ping", data: null } },
    ]);
  });

  test("a failed native handler surfaces the host error with its message", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          [NATIVE_CALL]: async () => {
            throw new Error("handler boom");
          },
        },
      },
    });
    await app.press("id:fn-add");
    await app.waitFor(
      "text:/^fn: add err core\\/host-error \\(ERR_NSH_HOST_ERROR\\): handler boom$/",
    );
  });
});
