// Labelled sandbox test: the callable wire protocol against the emulator
// suite's Functions on 127.0.0.1:15001 (project demo-nsh, a demo-
// project: no account, no credentials, no real Google endpoint; the v1
// callables ping and add of emulator/functions/index.js). The repo's
// runner runs it (`bash scripts/test-emulator.sh`); it needs the
// emulator up and emulator/functions/node_modules installed.
import { describe, expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

// The app entry is under tests/, whose nsh.json grants the local
// network hosts this run may reach.
const ENTRY = "../functions-app.tsx";

const BASE = "http://127.0.0.1:15001/demo-nsh/us-central1";

describe("functions sandbox", () => {
  test("pings, adds and misses over the live emulator", async () => {
    // Real network, the system clock: this is the sandbox run.
    const app: App = await open(ENTRY, { network: "live" });
    await app.press("id:fn-sandbox");
    // The functions runtime spawns a node process on the first call; the
    // default 5 s wait is not enough for a cold emulator.
    await app.waitFor(
      "text:/^fn: sandbox ok ping\\(pong=true,from=sandbox\\) add\\(sum=5\\) missing\\(functions\\/not-found\\)$/",
      { timeout: 60000 },
    );
    // The wire talked to the local emulator only: one POST per call,
    // each on the project/region/name path.
    const requests = app.requests();
    expect(requests.length).toBe(3);
    expect(
      requests.every((request) => request.url.startsWith(`${BASE}/`)),
    ).toBe(true);
    expect(
      requests.map((request) => request.url.slice(BASE.length + 1)),
    ).toEqual(["ping", "add", "missing"]);
  });
});
