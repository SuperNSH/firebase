// Headless tests of the analytics on a manual clock: batching at
// maxBatchSize, flush on interval, the consent gate, retry then
// success, screen-view shapes, user property merging, validation, the
// native tier's routing, and the collector transport over the stubbed
// network. The app root is tests/analytics-app.tsx; capabilities come
// from tests/nsh.json, the nsh.json next to the entry.
import { describe, expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

const ENTRY = "./analytics-app.tsx";
const COLLECT = "http://127.0.0.1:18742/collect";

describe("analytics", () => {
  test("events batch at maxBatchSize without waiting for the interval", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.waitFor("text:pure client ready");
    await app.press("id:log");
    await app.press("id:log");
    // Two of three: nothing sends yet.
    await app.waitFor("text:pending 2 dropped 0");
    await app.waitFor("text:sent: none");
    await app.press("id:log");
    // The third fills the batch: the size trigger sends without a flush.
    await app.clock!.runFor(1);
    await app.waitUntil(() => sentTextOf(app).startsWith("sent: ["));
    const posts = JSON.parse(sentTextOf(app).replace("sent: ", "")) as Array<{
      name: string;
      params: Record<string, unknown>;
    }>;
    expect(posts.map((post) => post.name)).toEqual(["level_up", "level_up", "level_up"]);
    await app.waitFor("text:pending 0 dropped 0");
  });

  test("events flush on the interval and no earlier", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.press("id:log");
    await app.waitFor("text:pending 1 dropped 0");
    // The interval is 10_000 ms; 9_999 fires nothing.
    await app.clock!.runFor(9_999);
    await app.waitFor("text:sent: none");
    await app.clock!.runFor(1);
    await app.waitUntil(() => sentTextOf(app).startsWith("sent: ["));
    const posts = JSON.parse(sentTextOf(app).replace("sent: ", "")) as Array<{ name: string }>;
    expect(posts.map((post) => post.name)).toEqual(["level_up"]);
  });

  test("denied consent blocks sends until it is reganted", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.press("id:log");
    await app.press("id:deny");
    await app.waitFor("text:consent denied");
    // An event logged while denied is dropped; the queued one is held.
    await app.press("id:log");
    await app.waitFor("text:pending 1 dropped 1");
    await app.clock!.runFor(1);
    await app.press("id:flush");
    await app.waitFor("text:flush ok");
    await app.waitFor("text:sent: none");
    // Regranting resumes: the held event goes out.
    await app.press("id:grant");
    await app.waitFor("text:consent granted");
    await app.press("id:flush");
    await app.waitUntil(() => sentTextOf(app).startsWith("sent: ["));
    expect(sentTextOf(app)).toContain("level_up");
  });

  test("a failed send retries after backoff and then succeeds", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.press("id:log");
    await app.press("id:fail-next");
    await app.waitFor("text:next send will fail");
    await app.press("id:flush");
    await app.waitFor("text:flush failed: core/network: transport down");
    await app.waitFor("text:pending 1 dropped 0");
    // Half of the 1_000 ms backoff: no retry yet.
    await app.clock!.runFor(500);
    await app.waitFor("text:pending 1 dropped 0");
    // The retry timer fires at the backoff's end.
    await app.clock!.runFor(500);
    await app.waitUntil(() => sentTextOf(app).startsWith("sent: ["));
    expect(sentTextOf(app)).toContain("level_up");
    await app.waitFor("text:pending 0 dropped 0");
  });

  test("screen views carry the ga4 param names", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.press("id:screen");
    await app.press("id:flush");
    await app.waitUntil(() => sentTextOf(app).startsWith("sent: ["));
    const posts = JSON.parse(sentTextOf(app).replace("sent: ", "")) as Array<{
      name: string;
      data: { params: Record<string, string> };
    }>;
    expect(posts.length).toBe(1);
    expect(posts[0].name).toBe("screen_view");
    expect(posts[0].data.params).toEqual({
      screen_name: "Inventory",
      screen_class: "InventoryScreen",
    });
  });

  test("user properties merge and the user id rides the batch", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.press("id:prop-hard");
    await app.press("id:prop-easy");
    await app.press("id:user-id");
    await app.waitFor("text:user id set");
    await app.press("id:log");
    await app.press("id:flush");
    await app.waitUntil(() => sentTextOf(app).startsWith("sent: ["));
    const posts = JSON.parse(sentTextOf(app).replace("sent: ", "")) as Array<{
      data: { userProperties: Record<string, { value: string }>; userId: string | null };
    }>;
    expect(posts[0].data.userProperties).toEqual({ difficulty: { value: "easy" } });
    expect(posts[0].data.userId).toBe("user-41");
    // Clearing the user id clears it in the next batch too.
    await app.press("id:user-id-clear");
    await app.press("id:log");
    await app.press("id:flush");
    await app.waitFor("text:pending 0 dropped 0");
    const all = JSON.parse(sentTextOf(app).replace("sent: ", "")) as Array<{
      data: { userId: string | null };
    }>;
    expect(all[1].data.userId).toBe(null);
  });

  test("reserved names are refused with the stable code", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.press("id:log-reserved");
    await app.waitFor(
      "text:logged reserved: core/invalid-argument: logEvent: the event name 'session_start' is reserved and can not be used",
    );
  });

  test("the collector transport posts ndjson to /collect", async () => {
    let hits = 0;
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`POST ${COLLECT}`]: () => {
          hits += 1;
          return { status: 200, body: "{}" };
        },
      },
    });
    await app.waitFor("text:ready");
    await app.press("id:init-collector");
    await app.waitFor("text:collector client ready");
    await app.press("id:log");
    await app.press("id:flush");
    await app.waitFor("text:flush ok");
    expect(hits).toBe(1);
    const [request] = app.requests();
    expect(request.method).toBe("POST");
    expect(request.url).toBe(COLLECT);
    const lines = (request.body ?? "").split("\n");
    expect(lines.length).toBe(1);
    const line = JSON.parse(lines[0]) as Record<string, unknown>;
    expect(line.name).toBe("level_up");
    expect(typeof line.client_id).toBe("string");
    expect(typeof line.timestamp_ms).toBe("number");
  });

  test("the mp transport posts the ga4 body to the collect endpoint", async () => {
    let hits = 0;
    const mpUrl =
      "http://127.0.0.1:18742/mp/collect?api_secret=sb%26secret%3D1&measurement_id=m%3D1";
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        // The credentials contain `&` and `=`: only the percent-encoded
        // URL can match, so the route asserts the encoding. The sandbox
        // base stands in for www.google-analytics.com.
        [`POST ${mpUrl}`]: () => {
          hits += 1;
          return { status: 204, body: "" };
        },
      },
    });
    await app.waitFor("text:ready");
    await app.press("id:init-mp");
    await app.waitFor("text:mp client ready");
    await app.press("id:user-id");
    await app.press("id:log");
    await app.press("id:flush");
    await app.waitFor("text:flush ok");
    expect(hits).toBe(1);
    const [request] = app.requests();
    expect(request.method).toBe("POST");
    expect(request.url).toBe(mpUrl);
    const body = JSON.parse(request.body ?? "{}") as {
      client_id: string;
      user_id: string;
      non_personalized_ads: boolean;
      events: Array<{ name: string; timestamp_micros: number }>;
    };
    expect(typeof body.client_id).toBe("string");
    expect(body.user_id).toBe("user-41");
    expect(body.non_personalized_ads).toBe(false);
    expect(body.events[0].name).toBe("level_up");
    expect(body.events[0].timestamp_micros).toBeGreaterThan(0);
  });

  test("a leftover full batch drains on the next tick without the interval", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    // Six events in one synchronous press: two full batches queue with
    // no timer interleaving, so the drain rule is observable.
    await app.press("id:log6");
    // One tick drains the first batch and the leftover full batch
    // back-to-back; no 10 s wait between them (Rust drains the same way
    // across two polls). Asserted directly: waitFor would advance the
    // clock past the interval and mask the difference.
    await app.clock!.runFor(2);
    expect((sentTextOf(app).match(/level_up/g) ?? []).length).toBe(6);
    expect(app.texts()).toContain("pending 0 dropped 0");
  });

  test("an explicit mp config overrides native routing", async () => {
    let hits = 0;
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.analytics.logEvent": () => "logged",
        },
      },
      network: {
        "POST http://127.0.0.1:18742/mp/collect?api_secret=sb%26secret%3D1&measurement_id=m%3D1":
          () => {
            hits += 1;
            return { status: 204, body: "" };
          },
      },
    });
    await app.waitFor("text:ready");
    await app.press("id:init-mp");
    await app.waitFor("text:mp client ready");
    await app.press("id:log");
    await app.press("id:flush");
    await app.waitFor("text:flush ok");
    // The event went over the mp transport, not to the host handler.
    expect(hits).toBe(1);
    expect(app.host.posts()).toEqual([]);
  });

  test("a throwing onSend hook does not stall the retry schedule", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.press("id:hook-throws");
    await app.waitFor("text:hook will throw");
    await app.press("id:fail-next");
    await app.press("id:log");
    await app.press("id:flush");
    // The hook's throw surfaces instead of the transport error, and the
    // failed batch is held.
    await app.waitFor("text:flush failed: core/network: hook boom");
    await app.waitFor("text:pending 1 dropped 0");
    // The retry still armed: it fires at the 1 s backoff.
    await app.clock!.runFor(1_000);
    await app.waitUntil(() => sentTextOf(app).startsWith("sent: ["));
    expect(sentTextOf(app)).toContain("level_up");
  });

  test("the native tier routes through the firebase.analytics.* handlers", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.analytics.logEvent": () => "logged",
          "firebase.analytics.setDebugViewEnabled": () => "on",
          "firebase.analytics.setConsent": () => "ok",
        },
      },
    });
    await app.waitFor("text:ready");
    await app.press("id:init-native");
    await app.waitFor("text:client ready");
    await app.press("id:log");
    await app.press("id:debug");
    await app.waitFor("text:debug view: on (pure tier: no-op)");
    await app.press("id:deny");
    await app.waitFor("text:consent denied");
    // The posts went to the host handlers, not a queue. setConsent
    // normalizes to all four keys, nulls for unset optionals — the
    // shape Rust's NativeAnalytics posts.
    expect(app.host.posts()).toEqual([
      {
        name: "firebase.analytics.logEvent",
        data: { name: "level_up", params: { difficulty: "hard" } },
      },
      { name: "firebase.analytics.setDebugViewEnabled", data: { enabled: true } },
      {
        name: "firebase.analytics.setConsent",
        data: {
          analyticsStorage: "denied",
          adStorage: "denied",
          adUserData: null,
          adPersonalization: null,
        },
      },
    ]);
    expect(app.texts()).toContain("pending 0 dropped 0");
  });

  test("without a host the native path refuses to build without a transport", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-native");
    await app.waitFor(
      "text:init: core/invalid-argument: no analytics transport configured; pass a transport, mp or collector",
    );
  });
});

function sentTextOf(app: App): string {
  return app.texts().find((text) => text.startsWith("sent: ")) ?? "sent: none";
}
