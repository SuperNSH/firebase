// Labelled sandbox test: the whole storage round trip against the
// emulator suite's Storage on 127.0.0.1:19199 (project demo-nsh, a
// demo- project: no account, no credentials, no real Google endpoint).
// The repo's runner runs it (`bash scripts/test-emulator.sh`); it needs
// the emulator up and its permissive rules loaded.
import { describe, expect, open, test } from "nsh/test";
import type { App, StubRequest } from "nsh/test";

// The app entry is under tests/, whose nsh.json grants the local
// network hosts this run may reach.
const ENTRY = "../storage-app.tsx";

const OBJECT_BASE = "http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app";

// The engine sends fetch header names lower-cased; read them
// case-insensitively, as HTTP means them.
function header(request: StubRequest, name: string): string | undefined {
  const lower = name.toLowerCase();
  const found = Object.keys(request.headers).find((key) => key.toLowerCase() === lower);
  return found === undefined ? undefined : request.headers[found];
}

describe("storage sandbox", () => {
  test("uploads the fixture with progress, reads its URL, downloads and deletes it", async () => {
    // Real network, the system clock: this is the sandbox run.
    const app: App = await open(ENTRY, { network: "live" });
    await app.press("id:st-sandbox");
    await app.waitFor("text:st: sandbox ok size=36 roundtrip=true token=true");
    // The progress events the upload emitted: the last one names every
    // byte in state done.
    expect(app.texts().some((text) => text.endsWith("36/36 done"))).toBe(true);
    // The wire talked to the local emulator only: one start, slices
    // on the session URI, every path under the demo-nsh bucket.
    const requests = app.requests();
    expect(
      requests.filter((request) => header(request, "X-Goog-Upload-Command") === "start").length,
    ).toBe(1);
    expect(
      requests.every(
        (request) =>
          request.url.startsWith(OBJECT_BASE) ||
          request.url.startsWith("http://127.0.0.1:19199/upload/"),
      ),
    ).toBe(true);
  });
});
