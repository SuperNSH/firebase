// Headless tests of the installations on a manual clock (no real time)
// and a stubbed network (no real requests), with the wire replies the
// real service gives (verified against the firebase-js-sdk). The pure
// tier opens without host options; the native tier opens with the
// handlers the contract fixes the native areas to.
import { describe, expect, open, test } from "nsh/test";
import type { App, StubRequest } from "nsh/test";

// The entry lives in src/installations/ because nsh reads the nsh.json
// next to the entry, and it grants both the network host the REST calls
// need and the firebase.* host names; a tests/ entry would have neither.
const ENTRY = "../src/installations/app.tsx";

const T0 = 1_767_258_000_000;
const REGISTER_URL =
  "https://firebaseinstallations.googleapis.com/v1/projects/demo-nsh/installations";
const TOKEN_GLOB = `${REGISTER_URL}/*/authTokens:generate`;
const STORE_KEY = "nsh.firebase.installations.[DEFAULT]";
// The register reply's token stays valid a week; the refresh margin is
// one hour, so due-ness starts exactly here.
const EXPIRES_AT = T0 + 604_800_000;
const REFRESH_AT = EXPIRES_AT - 3_600_000;
const REFRESHED_EXPIRES_AT = REFRESH_AT + 604_800_000;

interface Route {
  status: number;
  body: unknown;
  error?: string;
}

// The register route: echoes the FID the request carried, as the server
// does, and answers with the reply shape the service gives.
function registerRoute(): (request: StubRequest) => Route {
  return (request: StubRequest): Route => {
    const fid = (JSON.parse(request.body ?? "{}") as { fid?: string }).fid ?? "";
    return {
      status: 200,
      body: {
        name: `projects/demo-nsh/installations/${fid}`,
        fid,
        refreshToken: "refresh-token-1",
        authToken: { token: "auth-token-1", expiresIn: "604800s" },
      },
    };
  };
}

function tokenRoute(token: string): (request: StubRequest) => Route {
  return (): Route => ({
    status: 200,
    body: { token, expiresIn: "604800s" },
  });
}

function routes(
  register: Route | ((request: StubRequest) => Route),
  generate?: (request: StubRequest) => Route,
): Record<string, Route | ((request: StubRequest) => Route)> {
  const table: Record<string, Route | ((request: StubRequest) => Route)> = {
    [`POST ${REGISTER_URL}`]: register,
  };
  if (generate) table[`POST ${TOKEN_GLOB}`] = generate;
  return table;
}

// The engine sends fetch header names lower-cased; read them
// case-insensitively, as HTTP means them.
function header(request: StubRequest, name: string): string | undefined {
  const lower = name.toLowerCase();
  const found = Object.keys(request.headers).find((key) => key.toLowerCase() === lower);
  return found === undefined ? undefined : request.headers[found];
}

const STORED_FID = "c0123456789abcdef-_xyz";
const STORED_RECORD = JSON.stringify({ v: 1, fid: STORED_FID, refreshToken: "stored-refresh" });

describe("installations", () => {
  test("the pure tier registers an FID, persists the record, and serves the cached token until it is due", async () => {
    const app: App = await open(ENTRY, {
      clock: { time: T0 },
      network: routes(registerRoute(), tokenRoute("auth-token-2")),
    });
    await app.press("id:get-id");
    await app.waitFor("text:/^id: [cdef][A-Za-z0-9_-]{21}$/");

    // What went out: the register call with the service's headers and
    // body, and a well-formed locally generated FID.
    const sent = app.requests();
    expect(sent.length).toBe(1);
    expect(sent[0].method).toBe("POST");
    expect(sent[0].url).toBe(REGISTER_URL);
    expect(header(sent[0], "x-goog-api-key")).toBe("demo-nsh-fake-api-key");
    expect(header(sent[0], "Content-Type")).toBe("application/json");
    expect(header(sent[0], "Accept")).toBe("application/json");
    const body = JSON.parse(sent[0].body ?? "{}") as Record<string, string>;
    const fid = body.fid;
    expect(fid).toMatch(/^[cdef][A-Za-z0-9_-]{21}$/);
    expect(body.authVersion).toBe("FIS_v2");
    expect(body.appId).toBe("1:000000000000:web:demo-nsh");
    expect(body.sdkVersion).toBe("w:0.1.0");

    // The record the store now holds is the one a later launch restores,
    // in the camelCase shape the Rust tier reads.
    await app.press("id:reinstall");
    await app.waitFor(`text:restored: fid=${fid} refresh=refresh-token-1`);
    const stored = JSON.parse(app.storage()[STORE_KEY] ?? "{}") as Record<string, unknown>;
    expect(stored).toEqual({ v: 1, fid, refreshToken: "refresh-token-1" });

    // The register reply's token is already cached: no second request.
    await app.press("id:get-token");
    await app.waitFor("text:token: auth-token-1");
    expect(app.requests().length).toBe(1);
    await app.press("id:expires");
    await app.waitFor(`text:expires: ${EXPIRES_AT}`);

    // Due only once the clock passes the one-hour margin before expiry.
    await app.press("id:due");
    await app.waitFor("text:due: false");
    app.clock?.fastForward(604_800_000 - 3_600_000);
    await app.press("id:due");
    await app.waitFor("text:due: true");

    await app.press("id:get-token");
    await app.waitFor("text:token: auth-token-2");
    const generated = app.requests();
    expect(generated.length).toBe(2);
    expect(generated[1].url).toBe(`${REGISTER_URL}/${fid}/authTokens:generate`);
    expect(header(generated[1], "Authorization")).toBe("FIS_v2 refresh-token-1");
    expect(JSON.parse(generated[1].body ?? "{}")).toEqual({
      installation: { sdkVersion: "w:0.1.0", appId: "1:000000000000:web:demo-nsh" },
    });
    await app.press("id:expires");
    await app.waitFor(`text:expires: ${REFRESHED_EXPIRES_AT}`);
  });

  test("a force refresh on an unregistered installation registers first, then mints", async () => {
    const app: App = await open(ENTRY, {
      clock: { time: T0 },
      network: routes(registerRoute(), tokenRoute("auth-token-gen-1")),
    });
    await app.press("id:force");
    await app.waitFor("text:refresh: auth-token-gen-1");
    expect(app.requests().length).toBe(2);
    // The fresh token is cached: a plain get_token stays off the network.
    await app.press("id:get-token");
    await app.waitFor("text:token: auth-token-gen-1");
    expect(app.requests().length).toBe(2);
  });

  test("a restore of the record the Rust tier wrote answers without any request", async () => {
    const app: App = await open(ENTRY, {
      clock: { time: T0 },
      network: {},
      storage: { [STORE_KEY]: STORED_RECORD },
    });
    await app.press("id:reinstall");
    await app.waitFor(`text:restored: fid=${STORED_FID} refresh=stored-refresh`);
    await app.press("id:get-id");
    await app.waitFor(`text:id: ${STORED_FID}`);
    expect(app.requests()).toEqual([]);
  });

  test("the demo- project's refusal maps to a clear request-failed and persists nothing", async () => {
    const app: App = await open(ENTRY, {
      clock: { time: T0 },
      network: routes({
        status: 403,
        body: {
          error: {
            code: 403,
            message: "Firebase Installations service is disabled for this project",
            status: "PERMISSION_DENIED",
          },
        },
      }),
    });
    await app.press("id:get-id");
    await app.waitFor(
      'text:id: installations/request-failed: Create Installation request failed with error "403 PERMISSION_DENIED: Firebase Installations service is disabled for this project"',
    );
    // Nothing was persisted: the next launch starts unregistered.
    await app.press("id:reinstall");
    await app.waitFor("text:restored: fid=none refresh=none");
    expect(app.storage()[STORE_KEY]).toBeUndefined();
  });

  test("a server failure and a transport failure map distinctly", async () => {
    const server: App = await open(ENTRY, {
      clock: { time: T0 },
      network: routes({ status: 500, body: { error: { code: 500, message: "backend" } } }),
    });
    await server.press("id:get-id");
    await server.waitFor(
      "text:id: installations/internal-error: the installations server failed (HTTP 500)",
    );

    const offline: App = await open(ENTRY, {
      clock: { time: T0 },
      network: routes({ status: 0, body: "", error: "the connection was reset" }),
    });
    await offline.press("id:get-id");
    await offline.waitFor("text:id: core/network: the connection was reset");
  });

  test("the native tier routes through the firebase.installations.* handlers", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.installations.getId": async () => ({ id: "native-fid-1" }),
          "firebase.installations.getToken": async () => ({ token: "native-token-1" }),
        },
      },
    });
    await app.press("id:get-id");
    await app.waitFor("text:id: native-fid-1");
    await app.press("id:get-token");
    await app.waitFor("text:token: native-token-1");
    // The contract carries no args, and no REST request leaves the app.
    expect(app.host.calls()).toEqual([
      { name: "firebase.installations.getId", data: null },
      { name: "firebase.installations.getToken", data: null },
    ]);
    expect(app.requests()).toEqual([]);
  });

  test("a native host failure keeps the host error with its message", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.installations.getId": async () => {
            throw new Error("FIS registration failed: fake project rejected");
          },
        },
      },
    });
    await app.press("id:get-id");
    await app.waitFor(
      "text:/^id: core\\/host-error \\(ERR_NSH_HOST_ERROR\\).*FIS registration failed: fake project rejected/",
    );
  });

  test("a native reply without the field is a host error", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.installations.getId": async () => ({ nope: 1 }),
          "firebase.installations.getToken": async () => ({ token: "" }),
        },
      },
    });
    await app.press("id:get-id");
    await app.waitFor(
      "text:id: core/host-error: `firebase.installations.getId` answered without an id",
    );
    await app.press("id:get-token");
    await app.waitFor(
      "text:token: core/host-error: `firebase.installations.getToken` answered without a token",
    );
  });
});
