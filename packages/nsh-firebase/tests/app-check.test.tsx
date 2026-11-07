// Headless tests of the App Check facade on a manual clock and a
// stubbed network: the debug exchange's request shape, the {token, ttl}
// mapping, the cache before/after expiry, the failure mapping, and the
// native tier's routing through the played host handlers. The emulator
// suite has no App Check service and demo- projects refuse, so there is
// deliberately no sandbox test — the honest native proof is the SDK's
// debug logging on a device (docs/research.md §4).
import { describe, expect, open, test } from "nsh/test";
import type { App, StubRequest, StubResponse } from "nsh/test";

// The manual clock's fixed instant; every expected timestamp derives
// from it.
const T0 = 1_767_258_000_000;
const EXCHANGE_URL =
  "http://127.0.0.1:19191/v1/projects/demo-nsh/apps/1:000000000000:web:demo-nsh:exchangeDebugToken?key=demo-nsh-fake-api-key";
const REAL_URL =
  "https://firebaseappcheck.googleapis.com/v1/projects/demo-nsh/apps/1:000000000000:web:demo-nsh:exchangeDebugToken?key=demo-nsh-fake-api-key";
const STORE_KEY = "nsh.firebase.app-check.[DEFAULT]";
const TOKEN = "demo-nsh-fake-debug-token";

// The entry is this file's neighbor; the app root stays the package
// (its nsh.json), which the entry's parent directory decides.
const ENTRY = "./app-check-app.tsx";

type Route = StubResponse | ((request: StubRequest) => StubResponse);

// The 200 reply the exchange gives, one fresh token per call.
function exchangeHits(): Record<string, Route> {
  let hits = 0;
  return {
    [`POST ${EXCHANGE_URL}`]: () => {
      hits += 1;
      return {
        status: 200,
        body: JSON.stringify({ token: `fresh-token-${hits}`, ttl: "1800s" }),
      };
    },
  };
}

function refusal(status: number, message: string, wireStatus: string): Route {
  return {
    status,
    body: JSON.stringify({ error: { code: status, message, status: wireStatus } }),
  };
}

interface StartedOptions {
  network?: Record<string, Route>;
  storage?: Record<string, string> | null;
  host?: { handlers: Record<string, unknown | ((data: unknown) => unknown)> };
}

async function started(options: StartedOptions = {}): Promise<App> {
  return open(ENTRY, {
    clock: { time: T0 },
    network: options.network ?? null,
    storage: options.storage ?? null,
    host: options.host ?? null,
  });
}

describe("app-check", () => {
  test("the debug exchange request matches the wire and caches its ttl", async () => {
    const app = await started({ network: exchangeHits() });
    await app.press("id:init-sandbox");
    await app.press("id:token");
    await app.waitFor("text:token: fresh-token-1");
    await app.waitFor(`text:expires: ${T0 + 1_800_000}`);
    // The request: the debug token in the body, the api key in the
    // query, JSON content type, and no X-Firebase-AppCheck header (that
    // header belongs to calls to protected backends, not this exchange).
    const sent = app.requests();
    expect(sent.length).toBe(1);
    expect(sent[0].method).toBe("POST");
    expect(sent[0].url).toBe(EXCHANGE_URL);
    expect(sent[0].headers["Content-Type"]).toBe("application/json");
    expect(
      Object.keys(sent[0].headers).some((name) => name.toLowerCase() === "x-firebase-appcheck"),
    ).toBe(false);
    expect(sent[0].body).toBe(`{"debug_token":"${TOKEN}"}`);
    // The record the store now holds is what a later launch restores.
    const stored = JSON.parse(app.storage()[STORE_KEY] ?? "{}") as Record<string, unknown>;
    expect(stored).toEqual({ v: 1, token: "fresh-token-1", expiresAtMs: T0 + 1_800_000 });
  });

  test("the cache answers before expiry and refreshes once past it", async () => {
    const app = await started({ network: exchangeHits() });
    await app.press("id:init-sandbox");
    await app.press("id:token");
    await app.waitFor("text:token: fresh-token-1");
    // One millisecond inside the ttl: still the cache, no second request.
    app.clock?.setSystemTime(T0 + 1_799_999);
    await app.press("id:token");
    await app.waitFor("text:token: fresh-token-1");
    expect(app.requests().length).toBe(1);
    // At the expiry (equality counts as past it): the exchange runs.
    app.clock?.setSystemTime(T0 + 1_800_000);
    await app.press("id:token");
    await app.waitFor("text:token: fresh-token-2");
    await app.waitFor(`text:expires: ${T0 + 3_600_000}`);
    expect(app.requests().length).toBe(2);
    expect(app.storage()[STORE_KEY]).toContain('"token":"fresh-token-2"');
  });

  test("a stored record restores and answers with no request", async () => {
    const app = await started({
      storage: {
        [STORE_KEY]: JSON.stringify({
          v: 1,
          token: "stored-token",
          expiresAtMs: T0 + 999_999,
        }),
      },
    });
    await app.press("id:init-sandbox");
    await app.press("id:token");
    await app.waitFor("text:token: stored-token");
    await app.waitFor(`text:expires: ${T0 + 999_999}`);
    expect(app.requests().length).toBe(0);
  });

  test("a corrupt or expired record is exchanged over", async () => {
    const corrupt = await started({
      network: exchangeHits(),
      storage: { [STORE_KEY]: "not json" },
    });
    await corrupt.press("id:init-sandbox");
    await corrupt.press("id:token");
    await corrupt.waitFor("text:token: fresh-token-1");
    expect(corrupt.requests().length).toBe(1);

    const expired = await started({
      network: exchangeHits(),
      storage: {
        [STORE_KEY]: JSON.stringify({ v: 1, token: "stale-token", expiresAtMs: T0 - 1 }),
      },
    });
    await expired.press("id:init-sandbox");
    await expired.press("id:token");
    await expired.waitFor("text:token: fresh-token-1");
    expect(expired.requests().length).toBe(1);
  });

  test("getToken without debug names the missing device attestors", async () => {
    const app = await started({ network: exchangeHits() });
    await app.press("id:init-sandbox");
    await app.press("id:token-attested");
    await app.waitFor(
      "text:attested: app-check/needs-attestation: the pure tier has no device attestor " +
        "(DeviceCheck/App Attest on iOS, Play Integrity on Android): call getToken with " +
        "debug:true and a debug token registered in the Firebase console",
    );
    expect(app.requests().length).toBe(0);
  });

  test("an empty debug token fails before any request", async () => {
    const app = await started({
      network: { [`POST ${EXCHANGE_URL}`]: { status: 200, body: "the stub must never answer" } },
    });
    await app.press("id:init-empty");
    await app.waitFor("text:client ready without a debug token");
    await app.press("id:token");
    await app.waitFor(
      "text:token: app-check/invalid-argument: the debug token is empty: register one in the " +
        "Firebase console and pass it to the App Check options",
    );
    expect(app.requests().length).toBe(0);
  });

  test("a refusal maps to invalid-debug-token with the server's message", async () => {
    const app = await started({
      network: {
        [`POST ${EXCHANGE_URL}`]: refusal(
          403,
          "App attestation failed: debug token not registered",
          "PERMISSION_DENIED",
        ),
      },
    });
    await app.press("id:init-sandbox");
    await app.press("id:token");
    await app.waitFor(
      "text:token: app-check/invalid-debug-token: the exchange refused the debug token " +
        "(HTTP 403: App attestation failed: debug token not registered)",
    );
    // Nothing is cached from a failed exchange.
    expect(app.storage()[STORE_KEY]).toBeUndefined();
  });

  test("a 500 maps to exchange-failed and a bad ttl to invalid-response", async () => {
    const failed = await started({
      network: { [`POST ${EXCHANGE_URL}`]: { status: 500, body: "{}" } },
    });
    await failed.press("id:init-sandbox");
    await failed.press("id:token");
    await failed.waitFor(
      "text:token: app-check/exchange-failed: the exchange failed (HTTP 500: the server gave " +
        "no error message)",
    );

    const badTtl = await started({
      network: {
        [`POST ${EXCHANGE_URL}`]: { status: 200, body: JSON.stringify({ token: "tok", ttl: "soon" }) },
      },
    });
    await badTtl.press("id:init-sandbox");
    await badTtl.press("id:token");
    await badTtl.waitFor(
      'text:token: app-check/invalid-response: the reply has no usable ttl (a Protobuf Duration like "1800s")',
    );
    expect(badTtl.storage()[STORE_KEY]).toBeUndefined();
  });

  test("a network failure keeps the core network code", async () => {
    const app = await started({
      network: { [`POST ${EXCHANGE_URL}`]: { status: 200, error: "the connection was reset" } },
    });
    await app.press("id:init-sandbox");
    await app.press("id:token");
    // The stable code is what the facade owns; the message carries the
    // engine's own failure wording.
    await app.waitFor("text:/^token: core\\/network .*connection was reset/");
    expect(app.storage()[STORE_KEY]).toBeUndefined();
  });

  test("the native tier routes the contract call with the debug flag", async () => {
    const app = await started({
      host: {
        handlers: {
          // The debug flag decides the host's provider; the reply is the
          // contract's {token}.
          "firebase.appCheck.getToken": async ({ debug }: { debug: boolean }) =>
            debug ? { token: "native-debug-token" } : { token: "native-attested-token" },
        },
      },
    });
    await app.press("id:init");
    await app.press("id:token");
    await app.waitFor("text:token: native-debug-token");
    await app.waitFor("text:expires: host");
    // Attested routing on the native tier is the host's provider choice,
    // never the pure tier's refusal.
    await app.press("id:token-attested");
    await app.waitFor("text:token: native-attested-token");
    expect(app.host.calls()).toEqual([
      { name: "firebase.appCheck.getToken", data: { debug: true } },
      { name: "firebase.appCheck.getToken", data: { debug: false } },
    ]);
    expect(app.requests().length).toBe(0);
  });

  test("a native reply without a token is a host error", async () => {
    const app = await started({
      host: {
        handlers: {
          "firebase.appCheck.getToken": async () => ({}),
        },
      },
    });
    await app.press("id:init");
    await app.press("id:token");
    await app.waitFor(
      "text:token: app-check/invalid-response: `firebase.appCheck.getToken` answered without a token",
    );
  });

  test("a native handler failure surfaces with its message", async () => {
    const app = await started({
      host: {
        handlers: {
          "firebase.appCheck.getToken": async () => {
            throw new Error("app check debug exchange failed: the fake project refuses");
          },
        },
      },
    });
    await app.press("id:init");
    await app.press("id:token");
    await app.waitFor(
      "text:/^token: core\\/host-error \\(ERR_NSH_HOST_ERROR\\).*the fake project refuses/",
    );
  });

  test("the default exchange URL pins the real endpoint", async () => {
    const app = await started();
    await app.press("id:url");
    await app.waitFor(`text:url: ${REAL_URL}`);
  });
});
