// The labelled sandbox test: real records against the real
// tools/collector sink on 127.0.0.1:18742. It runs only when the
// collector is up (scripts/test-emulator.sh checks /health first and
// skips otherwise); start it with the one command in
// docs/status/perfmon.md. Unlike the unit tests, this runs on the live
// network and real time — that is the point: the transport really posts,
// the sink really appends, and the assert route answers from its file.
import { describe, expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

const ENTRY = "../perf-app.tsx";

describe("monitor sandbox (collector on 127.0.0.1:18742)", () => {
  test("a queued HTTP metric reaches the collector and is asserted there", async () => {
    const app: App = await open(ENTRY, { network: "live" });
    await app.press("id:queue");
    await app.waitFor("text:queue: queued=1");
    await app.press("id:flush");
    await app.waitFor("text:flush: ok queued=0");
    // The collector holds the record: its own /assert answers from the
    // sink file it appended.
    await app.press("id:assert");
    await app.waitFor("text:/assert evt-q1: \\{.*\"ok\":true.*\"matches\":[1-9][0-9]*.*\\}/");
  });

  test("the collector's slow route records a real duration", async () => {
    const app: App = await open(ENTRY, { network: "live" });
    await app.press("id:fetch-slow");
    // Real time: /slow?ms=900 replies after ~900 ms, and the recorded
    // duration is that order of magnitude (three digits of millis).
    await app.waitFor("text:/slow event: GET .* path=/slow status=200 dur=[1-9][0-9]{2}\\./", {
      timeout: 15000,
    });
  }, 20000);
});
