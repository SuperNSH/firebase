// Headless tests of the database on a manual clock with a stubbed
// network: the REST verbs, push ordering, the transaction retry, a
// queried read, and the live listener — one SSE route the test holds and
// releases, the reconnect landing on the clock. The mirror of
// crates/nsh-firebase-database/tests/database_ui.rs.
import { describe, expect, open, test } from "nsh/test";
import type { App, OpenOptions } from "nsh/test";
import { FrameParser, applyEvent } from "../src/database";

const DB = "http://127.0.0.1:19000";
// A nested entry would make `nsh` treat tests/ as the app; this file
// mounts its own exercise UI, next to the App.tsx the gate builds.
const ENTRY = "./database-app.tsx";

// One SSE frame: a put replacing the cache at `/`.
function put(value: unknown): string {
  return `event: put\ndata: ${JSON.stringify({ path: "/", data: value })}\n\n`;
}

type Routes = NonNullable<OpenOptions["network"]>;

async function openDatabase(network: Routes): Promise<App> {
  return open(ENTRY, { clock: {}, network });
}

describe("database", () => {
  test("set then get round trips through the stubbed rest", async () => {
    const app = await openDatabase({
      [`PUT ${DB}/ui-todos/one.json`]: { status: 200, body: { done: false, title: "one" } },
      [`GET ${DB}/ui-todos/one.json`]: { status: 200, body: { done: false, title: "one" } },
    });
    await app.press("id:set");
    await app.waitFor("text:set: ok");
    await app.press("id:get");
    await app.waitFor('text:get: {"done":false,"title":"one"}');
  });

  test("update merges multi paths and get shows the merge", async () => {
    const app = await openDatabase({
      [`PUT ${DB}/ui-multi.json`]: { status: 200, body: "" },
      [`PATCH ${DB}/ui-multi.json`]: { status: 200, body: "" },
      [`GET ${DB}/ui-multi.json`]: { status: 200, body: { a: 9, b: { c: 1, d: 7 } } },
    });
    await app.press("id:update");
    await app.waitFor('text:multi: {"a":9,"b":{"c":1,"d":7}}');
  });

  test("remove deletes and the read back is none", async () => {
    const app = await openDatabase({
      [`DELETE ${DB}/ui-todos/one.json`]: { status: 200, body: "null" },
      [`GET ${DB}/ui-todos/one.json`]: { status: 200, body: "null" },
    });
    await app.press("id:remove");
    await app.waitFor("text:removed: none");
  });

  test("push ids stay ordered within and across milliseconds", async () => {
    const app = await openDatabase({
      [`PUT ${DB}/ui-pushed/*.json`]: { status: 200, body: "" },
    });
    // Two pushes on the same manual clock tick: 20 characters each, the
    // second sorting after the first.
    await app.press("id:push");
    await app.waitFor("text:push: 20 20 true");
    // One millisecond later: the timestamp prefix alone orders it after.
    await app.clock!.runFor(1);
    await app.press("id:push-next");
    await app.waitFor("text:/^push-next: [^ ]+ < [^ ]+ = true$/");
    // The pushes were plain PUTs at the generated children: no server
    // key came back.
    const pushed = app
      .requests()
      .filter((r) => r.method === "PUT" && r.url.startsWith(`${DB}/ui-pushed/`));
    expect(pushed.length).toBe(3);
  });

  test("a transaction rebuilds on 412 and commits", async () => {
    let gets = 0;
    let puts = 0;
    const app = await openDatabase({
      [`GET ${DB}/ui-count.json`]: () => {
        gets += 1;
        return { status: 200, headers: { ETag: `etag-${gets}` }, body: "5" };
      },
      [`PUT ${DB}/ui-count.json`]: () => {
        const which = puts++;
        if (which === 0) {
          // Changed elsewhere: the current value 6 and a fresh ETag.
          return { status: 412, headers: { ETag: "etag-2" }, body: "6" };
        }
        return { status: 200, body: "7" };
      },
    });
    await app.press("id:transaction");
    await app.waitFor("text:transaction: committed=true snapshot=7");
    // Two rounds: two tagged reads, two conditional writes with the ETag
    // each read returned.
    const requests = app.requests();
    const getsOf = requests.filter((r) => r.method === "GET" && r.url.endsWith("/ui-count.json"));
    const putsOf = requests.filter((r) => r.method === "PUT" && r.url.endsWith("/ui-count.json"));
    expect(getsOf.length).toBe(2);
    expect(putsOf.length).toBe(2);
    expect(getsOf[0].headers["x-firebase-etag"]).toBe("true");
    expect(getsOf[1].headers["x-firebase-etag"]).toBe("true");
    expect(putsOf[0].headers["if-match"]).toBe("etag-1");
    expect(putsOf[1].headers["if-match"]).toBe("etag-2");
  });

  test("a queried get encodes the params and sorts children", async () => {
    const app = await openDatabase({
      // The stub matches the URL whole: the encoding and the parameter
      // order must be exactly the documented ones.
      [`GET ${DB}/ui-scores.json?orderBy=%22score%22&limitToFirst=2`]: {
        status: 200,
        // A deliberately unsorted key order: the sort the screen shows
        // is the client's.
        body: { b: { score: 9 }, d: { score: 1 }, a: { score: 5 } },
      },
    });
    await app.press("id:query");
    await app.waitFor("text:query: d a b");
  });

  test("a request no route lists is a network failure", async () => {
    const app = await openDatabase({});
    await app.press("id:get-missing");
    await app.waitFor("text:/^get-missing: core\\/network/");
  });

  test("the listener receives then reconnects and resyncs from the put", async () => {
    let sseCalls = 0;
    const app = await openDatabase({
      [`GET ${DB}/ui-live.json`]: () => {
        sseCalls += 1;
        if (sseCalls === 1) {
          // The first stream delivers the put and then ends: a network
          // loss.
          return { status: 200, body: put({ a: 1 }) };
        }
        // The reconnect delivers the fresh value one and a half seconds
        // after it opens, on the clock.
        return { status: 200, body: put({ a: 2 }), delay: 1_500 };
      },
    });
    // The first stream: the put arrives (the value on screen), the end
    // marks reconnecting attempt 1.
    await app.waitFor('text:live: reconnecting {"a":1}');
    expect(sseCalls).toBe(1);
    // The listen request asked for the event stream, with no query and
    // no auth on this ref.
    const listen = app
      .requests()
      .find((r) => r.method === "GET" && r.url === `${DB}/ui-live.json`);
    expect(listen?.headers["accept"]).toBe("text/event-stream");
    // The clock runs past the backoff's second, the new stream opens,
    // and its fresh put replaces the cache whole: the resync.
    await app.clock!.runFor(3_000);
    expect(sseCalls).toBe(2);
    await app.waitFor('text:live: reconnecting {"a":2}');
    // Off before the next backoff: no third connection opens.
    await app.press("id:off");
    await app.clock!.runFor(2_000);
    expect(sseCalls).toBe(2);
  });

  test("a patch in a later chunk re-renders the listener text", async () => {
    let sseCalls = 0;
    const app = await openDatabase({
      [`GET ${DB}/ui-live.json`]: () => {
        sseCalls += 1;
        if (sseCalls === 1) {
          return { status: 200, body: put({ c: { bar: 2 } }) };
        }
        // A second stream: the patch merges into what the first put
        // left, and the fresh value must reach the screen.
        return {
          status: 200,
          body: `event: patch\ndata: ${JSON.stringify({ path: "/c", data: { foo: 3 } })}\n\n`,
        };
      },
    });
    await app.waitFor('text:live: reconnecting {"c":{"bar":2,"foo":3}}');
  });

  test("a cancel stops the listener without reconnecting", async () => {
    let sseCalls = 0;
    const app = await openDatabase({
      [`GET ${DB}/ui-live.json`]: () => {
        sseCalls += 1;
        return { status: 200, body: "event: cancel\ndata: null\n\n" };
      },
    });
    await app.waitFor("text:live: revoked none");
    // Off was never pressed; the stopped listener opens nothing more.
    await app.clock!.runFor(2_000);
    expect(sseCalls).toBe(1);
  });

  test("a listener on an invalid query fails at subscribe time", async () => {
    const app = await openDatabase({});
    await app.press("id:listen-bad");
    await app.waitFor("text:/^listen-bad: core\/invalid-argument/");
  });

  test("a non-json 200 body is a network failure with the mapped message", async () => {
    const app = await openDatabase({
      [`GET ${DB}/ui-blob.json`]: { status: 200, body: "not-json" },
    });
    await app.press("id:get-bad-json");
    await app.waitFor("text:/^get-bad-json: core\/network: the database sent invalid JSON/");
  });

  test("a deep put and a patch yield a fresh cache reference", () => {
    // The signal set compares references: a cache mutated in place
    // would read right and notify never.
    const initial = { c: { bar: 2 } };
    const afterPut = applyEvent(initial, { kind: "put", path: "/c/x", data: 1 });
    expect(afterPut).not.toBe(initial);
    const afterPatch = applyEvent(afterPut, { kind: "patch", path: "/c", data: { foo: 3 } });
    expect(afterPatch).not.toBe(afterPut);
    expect(afterPatch).toEqual({ c: { bar: 2, x: 1, foo: 3 } });
  });

  test("getDatabase hands back one database per app, carrying the token", async () => {
    const app = await openDatabase({
      // The route matches whole: the token set on the first instance
      // must reach the second, or the URL misses.
      [`GET ${DB}/ui-auth.json?auth=tok-1`]: { status: 200, body: { v: 1 } },
    });
    await app.press("id:shared-db");
    await app.waitFor('text:shared: same=true get={"v":1}');
  });

  test("a path with a space or a question mark is percent-encoded", async () => {
    const app = await openDatabase({
      // The route matches whole: an unencoded path would miss it.
      [`GET ${DB}/ui%20a%3Fb.json`]: { status: 200, body: { v: 1 } },
    });
    await app.press("id:get-encoded");
    await app.waitFor('text:encoded: {"v":1}');
  });

  test("the parser reads lf and crlf framed events alike", () => {
    const lf = new FrameParser().feed(
      `event: put\ndata: ${JSON.stringify({ path: "/a", data: 1 })}\n\n`,
    );
    expect(lf.length).toBe(1);
    expect(lf[0]).toEqual({ kind: "put", path: "/a", data: 1 });
    const crlf = new FrameParser().feed(
      `event: put\r\ndata: ${JSON.stringify({ path: "/b", data: 2 })}\r\n\r\n`,
    );
    expect(crlf.length).toBe(1);
    expect(crlf[0]).toEqual({ kind: "put", path: "/b", data: 2 });
  });

  test("off during the backoff stops the reconnecting", async () => {
    let sseCalls = 0;
    const app = await openDatabase({
      [`GET ${DB}/ui-live.json`]: () => {
        sseCalls += 1;
        return { status: 200, body: put({ a: 1 }) };
      },
    });
    await app.waitFor('text:live: reconnecting {"a":1}');
    await app.press("id:off");
    // The backoff passes; off stopped the listener, so no stream opens.
    await app.clock!.runFor(2_000);
    expect(sseCalls).toBe(1);
    expect(app.texts()).toContain('live: reconnecting {"a":1}');
  });
});
