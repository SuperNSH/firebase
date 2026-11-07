// Headless tests of the monitor and perf packages on a manual clock with
// stubbed routes: event fields, ring eviction, aggregates and view
// model, trace durations, HTTP metrics from events, the batched
// transport flush, the granted native path (the played firebase.perf.*
// handlers take the trace calls and the recordHttpMetric post), and the
// native-tier fallback when the played handler refuses. The capabilities
// are tests/nsh.json, next to this entry: it grants the loopback network
// the stubs answer on and the firebase.* host names the played handlers
// sit behind.
import { describe, expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

const ENTRY = "./perf-app.tsx";
const TODOS = "http://127.0.0.1:19000/todos.json";
const SINK = "http://127.0.0.1:18742/collect";

const TODO_BODY = JSON.stringify([{ id: 1, title: "one" }]);

describe("perf/monitor", () => {
  test("instrumentFetch records every field of a fetched event", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`GET ${TODOS}`]: {
          status: 200,
          headers: { "content-length": String(TODO_BODY.length) },
          body: TODO_BODY,
        },
        [`POST ${TODOS}`]: { status: 201, body: { ok: true } },
      },
    });
    await app.press("id:fetch");
    await app.waitFor(
      `text:/fetch event: GET ${TODOS.replaceAll(".", "\\.")} host=127\\.0\\.0\\.1:19000 path=/todos\\.json` +
        ` status=200 dur=[0-9.]+ ttfb=[0-9.]+ req=null res=${TODO_BODY.length} err=null initiator=firebase-fetch/`,
    );
    // The wrapper handed back the caller's Response untouched, and a
    // string body is counted in bytes.
    await app.press("id:fetch-post");
    await app.waitFor(
      `text:/post event: POST ${TODOS.replaceAll(".", "\\.")} .* status=201 .* req=11 .* initiator=firebase-fetch/`,
    );
  });

  test("a delayed response times on the manual clock", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`GET ${TODOS}`]: { status: 200, headers: { "content-length": "2" }, body: "[]", delay: 120 },
      },
    });
    await app.press("id:fetch");
    await app.clock!.fastForward(120);
    await app.waitFor("text:/dur=120\\.0 ttfb=120\\.0/");
  });

  test("a failed exchange records an error event with no status", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.press("id:fetch-fail");
    await app.waitFor(
      "text:/fail event: GET http://127\\.0\\.0\\.1:19000/missing\\.json host=127\\.0\\.0\\.1:19000" +
        " path=/missing\\.json status=null dur=[0-9.]+ ttfb=null req=null res=null err=set initiator=firebase-fetch/",
    );
  });

  test("the ring evicts the oldest past the capacity", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.press("id:ring");
    await app.waitFor("text:ring: 3 kept first=e3 last=e5");
  });

  test("aggregates compute count, errorRate, p50 and p95 per host and path", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.press("id:agg");
    // api.test keeps a, b, d, e (d's 503 is the error, e's null status
    // is not); p50 and p95 are nearest-rank over the sorted durations.
    await app.waitFor(
      "text:agg: api.test n=4 err=0.25 p50=200 p95=900 list.n=3 list.p50=200 cdn=120",
    );
  });

  test("the view model sorts slowest first and filters", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.press("id:rows");
    await app.waitFor("text:rows: d,b,e,c,a");
    await app.press("id:rows-slow");
    await app.waitFor("text:rows: d,b,e,c");
    await app.press("id:rows-path");
    await app.waitFor("text:rows: b,e,a");
  });

  test("a trace records its metric, attribute and clock duration", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      network: { [`POST ${SINK}`]: { status: 200, body: { ok: true } } },
    });
    await app.press("id:trace-start");
    await app.waitFor("text:trace: started");
    await app.clock!.fastForward(120);
    await app.press("id:trace-stop");
    await app.waitFor("text:trace: name=screen-load dur=120.0 fp=80 screen=monitor");
  });

  test("an HTTP metric builds from a monitor event and flushes as one ndjson line", async () => {
    let sinkBody: string | null = null;
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`POST ${SINK}`]: (request) => {
          sinkBody = request.body ?? null;
          return { status: 200, body: { ok: true } };
        },
      },
    });
    await app.press("id:queue");
    await app.waitFor("text:queue: queued=1");
    // Below the batch size nothing is sent.
    expect(app.requests().map((request) => request.url)).toEqual([]);
    await app.press("id:flush");
    await app.waitFor("text:flush: ok queued=0");
    expect(app.requests().map((request) => request.url)).toEqual([SINK]);
    expect(sinkBody).toContain('"type":"http"');
    expect(sinkBody).toContain('"host":"api.test"');
    expect(sinkBody).toContain('"path":"/queued"');
    expect(sinkBody).not.toContain("\n");
  });

  test("a native capture that refuses falls back to the transport", async () => {
    let sinkBodies: string[] = [];
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          // The name is granted (tests/nsh.json), so the probe reaches
          // this handler; its throw is what rejects the probe, and the
          // transport takes the record instead.
          "firebase.perf.startTrace": () => {
            throw new Error("capture unavailable on this host");
          },
          // Listed so posts() can prove no recordHttpMetric crossed.
          "firebase.perf.recordHttpMetric": () => "recorded",
        },
      },
      network: {
        [`POST ${SINK}`]: (request) => {
          sinkBodies.push(request.body ?? "");
          return { status: 200, body: { ok: true } };
        },
      },
    });
    await app.press("id:metric-tiered");
    await app.waitFor("text:/metric: tiered records=[0-9]+/");
    // No post reached the played host (recordHttpMetric is listed, so
    // posts() would show one), and the record waits in the batching
    // transport until the flush.
    expect(app.host.posts()).toEqual([]);
    expect(sinkBodies).toEqual([]);
    await app.press("id:flush");
    await app.waitFor("text:flush: ok queued=0");
    expect(sinkBodies).toHaveLength(1);
    expect(sinkBodies[0]).toContain('"host":"127.0.0.1:19000"');
    expect(sinkBodies[0]).toContain('"initiator":"firebase-fetch"');
  });

  test("with firebase.perf.* handlers played, the trace calls and the metric post cross", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          // The granted native tier, played: the probe's throwaway
          // trace, the real trace's start and stop, and the metric post
          // all reach these handlers.
          "firebase.perf.startTrace": ({ name }: { name: string }) => ({ id: `native-${name}` }),
          "firebase.perf.stopTrace": ({ id }: { id: string }) => ({ stopped: id }),
          "firebase.perf.recordHttpMetric": () => "recorded",
        },
      },
    });
    // The metric crosses as the recordHttpMetric post, the event itself
    // as the args object; the probe made the first start/stop pair.
    await app.press("id:metric-tiered");
    await app.waitFor("text:/metric: tiered records=[0-9]+/");
    expect(app.host.posts()).toEqual([
      {
        name: "firebase.perf.recordHttpMetric",
        data: {
          id: "evt-metric",
          method: "GET",
          url: "http://127.0.0.1:19000/todos.json",
          host: "127.0.0.1:19000",
          path: "/todos.json",
          status: 200,
          startedAtMs: 1,
          durationMs: 120,
          ttfbMs: 40,
          requestBytes: null,
          responseBytes: 24,
          error: null,
          initiator: "firebase-fetch",
        },
      },
    ]);
    expect(app.host.calls().map((call) => call.name)).toEqual([
      "firebase.perf.startTrace",
      "firebase.perf.stopTrace",
    ]);
    expect(app.host.calls()[0].data).toEqual({ name: "__nsh_probe__" });

    // The trace runs natively: its start is the third call, its stop
    // the fourth, carrying the handler's trace id.
    await app.press("id:trace-start");
    await app.waitFor("text:trace: started");
    await app.press("id:trace-stop");
    await app.waitFor("text:/trace: name=screen-load dur=[0-9.]+ fp=80 screen=monitor/");
    const calls = app.host.calls();
    expect(calls.map((call) => call.name)).toEqual([
      "firebase.perf.startTrace",
      "firebase.perf.stopTrace",
      "firebase.perf.startTrace",
      "firebase.perf.stopTrace",
    ]);
    expect(calls[2].data).toEqual({ name: "screen-load" });
    expect(calls[3].data).toEqual({ id: "native-screen-load" });
  });
});
