// The labelled sandbox test against the Realtime Database emulator on
// 127.0.0.1:19000 (project demo-nsh, started by
// `bash scripts/start-emulator.sh`, run by `bash scripts/test-emulator.sh`):
// a write, a read back, a subscribed listener, and a live update written
// by a second client — a second app instance — through the emulator's
// real SSE stream. It talks to the network for real (`network: "live"`);
// nothing in it runs under the headless unit gates.
import { expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

// A nested entry would make `nsh` treat tests/ as the app.
const ENTRY = "../database-app.tsx";

async function openClient(): Promise<App> {
  return open(ENTRY, { network: "live" });
}

test("writes, reads back and receives a live update against the emulator", async () => {
  // The emulator's data for the sandbox paths starts empty, so the
  // listener's initial put is deterministic.
  const watcher: App = await openClient();
  await watcher.waitFor("text:write watch");
  const second: App = await openClient();
  await second.waitFor("text:write watch");

  // Write and read back through the REST API.
  await watcher.press("id:set");
  await watcher.waitFor("text:set: ok");
  await watcher.press("id:get");
  await watcher.waitFor('text:get: {"done":false,"title":"one"}');

  // The listener is live; the delete below arrives on its open stream
  // as a put of null, so the watched path reads none again.
  await watcher.press("id:clear-watch");
  await watcher.waitFor("text:clear-watch: ok");
  await watcher.waitFor("text:/^watch: live none/");

  // The second client writes the watched path; the live update lands in
  // the first app without any poll of the REST API.
  await second.press("id:write-watch");
  await second.waitFor("text:write-watch: ok");
  await watcher.waitFor('text:watch: live "from-second-client"');
});
