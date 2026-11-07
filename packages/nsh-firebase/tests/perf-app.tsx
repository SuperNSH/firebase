// The perf exercise UI the tests drive: instrumented fetches, the ring
// buffer, the per-host/path aggregates, the view-model filters, a trace
// on the clock, an HTTP metric from a monitor event, and the pure-tier
// transport flush. The capabilities come from tests/nsh.json next to
// this entry (network hosts 127.0.0.1, host firebase.*), which is what
// lets the tests' stubbed loopback fetches through at all; the tiered
// facade runs native when the test plays firebase.perf.* handlers and
// falls back to the transport when it plays none or one that throws.
import { mount, signal, Text, View } from "nsh";
import { Monitor, aggregate, aggregatesByHost, aggregatesByPath, instrumentFetch, monitorRows } from "../src/monitor";
import type { NetworkEvent } from "../src/monitor";
import { BatchingTransport, HttpLineTransport, createPerfmon } from "../src/perf";
import { createTier, PureTier, requestText } from "../src/core";

const SINK = "http://127.0.0.1:18742/collect";

// The fixed group the aggregate and rows buttons show, so the numbers on
// screen are pinned by the tests.
function fixedEvents(): NetworkEvent[] {
  const base: Omit<NetworkEvent, "id" | "url" | "host" | "path" | "durationMs" | "status"> = {
    method: "GET",
    startedAtMs: 1,
    ttfbMs: null,
    requestBytes: null,
    responseBytes: null,
    error: null,
    initiator: null,
  };
  const make = (id: string, host: string, path: string, durationMs: number, status: number | null): NetworkEvent => ({
    ...base,
    id,
    url: `http://${host}${path}`,
    host,
    path,
    durationMs,
    status,
  });
  return [
    make("a", "api.test", "/list", 10, 200),
    make("b", "api.test", "/list", 300, 200),
    make("c", "cdn.test", "/img", 120, 200),
    make("d", "api.test", "/slow-route", 900, 503),
    make("e", "api.test", "/list", 200, null),
  ];
}

function eventLine(event: NetworkEvent | null): string {
  if (!event) return "event: none";
  return (
    `event: ${event.method} ${event.url} host=${event.host} path=${event.path} status=${event.status}` +
    ` dur=${event.durationMs.toFixed(1)} ttfb=${event.ttfbMs === null ? "null" : event.ttfbMs.toFixed(1)}` +
    ` req=${event.requestBytes} res=${event.responseBytes} err=${event.error === null ? "null" : "set"}` +
    ` initiator=${event.initiator}`
  );
}

export function PerfExerciseApp() {
  const line = signal("ready");
  const ringLine = signal("ring: idle");
  const monitor = new Monitor();
  // The pure-tier facade: a batching sink transport the flush button
  // delivers through.
  const sink = new BatchingTransport(new HttpLineTransport(SINK), 4);
  const pure = createPerfmon({ tier: new PureTier(), transport: sink });
  // The tier the app actually runs with: over the host when there is
  // one, so the fallback test can see a host that grants nothing.
  const tiered = createPerfmon({ tier: createTier(), transport: sink });
  let running: { putMetric: (name: string, value: number) => void; putAttribute: (name: string, value: string) => void; stop: () => Promise<unknown> } | null = null;

  const lastEvent = () => monitor.events()[monitor.length - 1] ?? null;
  const messageOf = (thrown: unknown) => (thrown instanceof Error ? thrown.message : String(thrown));

  const fetchTodos = (label: string, url: string, init?: RequestInit) =>
    (async () => {
      const response = await instrumentFetch(monitor)(url, init);
      await response.text();
      return eventLine(lastEvent());
    })().then((value) => line.set(`${label} ${value}`));

  const fetchFailing = (label: string, url: string) =>
    instrumentFetch(monitor)(url)
      .then(
        () => eventLine(lastEvent()),
        (thrown: unknown) => `${eventLine(lastEvent())} msg=${messageOf(thrown)}`,
      )
      .then((value) => line.set(`${label} ${value}`));

  // The sandbox run only: the collector's deliberately slow route, live.
  const fetchSlow = () => void fetchTodos("slow", `${SINK.replace("/collect", "/slow")}?ms=900`);

  // The sandbox run only: ask the real collector what it holds.
  const assertSink = (contains: string, count: number) =>
    requestText(`http://127.0.0.1:18742/assert?contains=${encodeURIComponent(contains)}&count=${count}`)
      .then((response) => line.set(`assert ${contains}: ${response.text}`))
      .catch((thrown: unknown) => line.set(`assert: ${thrown instanceof Error ? thrown.message : String(thrown)}`));

  const ring = () => {
    const small = new Monitor({ capacity: 3 });
    for (const n of ["e1", "e2", "e3", "e4", "e5"]) {
      small.record({
        id: n,
        method: "GET",
        url: `http://api.test/${n}`,
        host: "api.test",
        path: `/${n}`,
        status: 200,
        startedAtMs: 1,
        durationMs: 1,
        ttfbMs: null,
        requestBytes: null,
        responseBytes: null,
        error: null,
        initiator: null,
      });
    }
    const kept = small.events();
    ringLine.set(`ring: ${kept.length} kept first=${kept[0].id} last=${kept[kept.length - 1].id}`);
  };

  const agg = () => {
    const events = fixedEvents();
    const byHost = aggregatesByHost(events);
    const byPath = aggregatesByPath(events);
    const api = byHost.get("api.test")!;
    const list = byPath.get("/list")!;
    const one = aggregate(events.filter((event) => event.host === "cdn.test"));
    line.set(
      `agg: api.test n=${api.count} err=${api.errorRate.toFixed(2)} p50=${api.p50Ms} p95=${api.p95Ms}` +
        ` list.n=${list.count} list.p50=${list.p50Ms} cdn=${one.p50Ms}`,
    );
  };

  const rows = (filterPath?: string, minDurationMs?: number) => {
    const filter = {
      ...(filterPath !== undefined ? { path: filterPath } : {}),
      ...(minDurationMs !== undefined ? { minDurationMs } : {}),
    };
    const ids = monitorRows(fixedEvents(), filter)
      .map((event) => event.id)
      .join(",");
    line.set(`rows: ${ids}`);
  };

  // The trace runs on the tiered facade: native when the test plays
  // firebase.perf.* handlers, the transport when it plays none.
  const startTrace = () =>
    tiered
      .startTrace("screen-load")
      .then((trace) => {
        running = trace;
        trace.putMetric("first_paint", 80);
        trace.putAttribute("screen", "monitor");
        line.set("trace: started");
      })
      .catch((thrown: unknown) => line.set(`trace: ${thrown instanceof Error ? thrown.message : String(thrown)}`));

  const stopTrace = () =>
    (running ? running.stop() : Promise.reject(new Error("no trace")))
      .then((record: unknown) => {
        const r = record as { durationMs: number; metrics: Record<string, number>; attributes: Record<string, string> };
        line.set(
          `trace: name=screen-load dur=${r.durationMs.toFixed(1)} fp=${r.metrics.first_paint} screen=${r.attributes.screen}`,
        );
        running = null;
      })
      .catch((thrown: unknown) => line.set(`trace: ${thrown instanceof Error ? thrown.message : String(thrown)}`));

  const metricFromEvent = (facade: "pure" | "tiered") => {
    const event: NetworkEvent = {
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
    };
    const facade_ = facade === "pure" ? pure : tiered;
    facade_
      .recordHttpMetric(event)
      .then(() => line.set(`metric: ${facade} records=${pure.records().length + tiered.records().length}`))
      .catch((thrown: unknown) => line.set(`metric: ${thrown instanceof Error ? thrown.message : String(thrown)}`));
  };

  const flush = () =>
    pure
      .flush()
      .then(() => line.set(`flush: ok queued=${sink.queued}`))
      .catch((thrown: unknown) => line.set(`flush: ${thrown instanceof Error ? thrown.message : String(thrown)}`));

  const queue = () =>
    pure
      .recordHttpMetric({
        id: "evt-q1",
        method: "GET",
        url: "http://api.test/queued",
        host: "api.test",
        path: "/queued",
        status: 200,
        startedAtMs: 1,
        durationMs: 5,
        ttfbMs: null,
        requestBytes: null,
        responseBytes: 3,
        error: null,
        initiator: "firebase-fetch",
      })
      .then(() => line.set(`queue: queued=${sink.queued}`));

  return (
    <View class="flex-1 gap-2 bg-background p-4">
      <Text testID="state">{line()}</Text>
      <Text testID="ring-state">{ringLine()}</Text>
      <View
        testID="fetch"
        role="button"
        onPress={() => void fetchTodos("fetch", "http://127.0.0.1:19000/todos.json")}
        class="bg-primary p-2"
      >
        <Text class="text-primary-foreground">fetch</Text>
      </View>
      <View
        testID="fetch-post"
        role="button"
        onPress={() =>
          void fetchTodos("post", "http://127.0.0.1:19000/todos.json", {
            method: "POST",
            body: "hello world",
          })
        }
        class="bg-primary p-2"
      >
        <Text class="text-primary-foreground">fetch post</Text>
      </View>
      <View
        testID="fetch-fail"
        role="button"
        onPress={() => void fetchFailing("fail", "http://127.0.0.1:19000/missing.json")}
        class="bg-primary p-2"
      >
        <Text class="text-primary-foreground">fetch fail</Text>
      </View>
      <View testID="fetch-slow" role="button" onPress={fetchSlow} class="bg-primary p-2">
        <Text class="text-primary-foreground">fetch slow</Text>
      </View>
      <View
        testID="assert"
        role="button"
        onPress={() => void assertSink("evt-q1", 1)}
        class="bg-primary p-2"
      >
        <Text class="text-primary-foreground">assert</Text>
      </View>
      <View testID="ring" role="button" onPress={ring} class="bg-primary p-2">
        <Text class="text-primary-foreground">ring</Text>
      </View>
      <View testID="agg" role="button" onPress={agg} class="bg-primary p-2">
        <Text class="text-primary-foreground">aggregates</Text>
      </View>
      <View testID="rows" role="button" onPress={() => rows()} class="bg-primary p-2">
        <Text class="text-primary-foreground">rows</Text>
      </View>
      <View testID="rows-slow" role="button" onPress={() => rows(undefined, 100)} class="bg-primary p-2">
        <Text class="text-primary-foreground">rows slow</Text>
      </View>
      <View testID="rows-path" role="button" onPress={() => rows("list")} class="bg-primary p-2">
        <Text class="text-primary-foreground">rows path</Text>
      </View>
      <View testID="trace-start" role="button" onPress={() => void startTrace()} class="bg-primary p-2">
        <Text class="text-primary-foreground">trace start</Text>
      </View>
      <View testID="trace-stop" role="button" onPress={() => void stopTrace()} class="bg-primary p-2">
        <Text class="text-primary-foreground">trace stop</Text>
      </View>
      <View testID="metric-pure" role="button" onPress={() => metricFromEvent("pure")} class="bg-primary p-2">
        <Text class="text-primary-foreground">metric pure</Text>
      </View>
      <View testID="metric-tiered" role="button" onPress={() => metricFromEvent("tiered")} class="bg-primary p-2">
        <Text class="text-primary-foreground">metric tiered</Text>
      </View>
      <View testID="queue" role="button" onPress={() => void queue()} class="bg-primary p-2">
        <Text class="text-primary-foreground">queue</Text>
      </View>
      <View testID="flush" role="button" onPress={() => void flush()} class="bg-primary p-2">
        <Text class="text-primary-foreground">flush</Text>
      </View>
    </View>
  );
}

mount(PerfExerciseApp);
