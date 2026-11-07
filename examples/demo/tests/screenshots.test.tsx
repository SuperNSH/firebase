// Vision evidence for the demo (criterion 7 corroboration + 10): walks
// home → auth → database → storage → analytics → monitor on a real
// platform, saving a PNG per route to docs/evidence/demo/macos-<route>.png.
// Run: nsh test tests/screenshots.test.tsx --target macos.
//
// Two apps: the first stubs the wire (the database's SSE stream and the
// storage session, the shapes the emulator serves) for home, auth,
// database, storage and analytics — auth and analytics are local-only on
// top of that; the second runs the live network so the monitor's calls
// go out for real against the collector on 127.0.0.1:18742
// (tools/collector), slow route visibly on top.
import { open, test } from "nsh/test";
import type { OpenOptions, StubRequest, StubResponse } from "nsh/test";

const DB = "http://127.0.0.1:19000";
const SESSION = "http://127.0.0.1:19199/upload/demo-session";
const OBJECT_BASE = "http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app";

function put(value: unknown): string {
  return `event: put\ndata: ${JSON.stringify({ path: "/", data: value })}\n\n`;
}

let received = 0;

type Routes = NonNullable<OpenOptions["network"]>;

const stubbed: Routes = {
  [`GET ${DB}/demo/items.json`]: {
    status: 200,
    body: put({
      "item-1": { title: "Read the design guides" },
      "item-2": { title: "Write the first test" },
    }),
  },
  [`PUT ${DB}/demo/items/*.json`]: { status: 200, body: "" },
  [`POST ${OBJECT_BASE}/o?name=demo%2Fnotes.bin`]: {
    status: 200,
    headers: { "X-Goog-Upload-Status": "active", "X-Goog-Upload-URL": SESSION },
    body: "",
  },
  [`POST ${SESSION}`]: (request: StubRequest): StubResponse => {
    const command = (request.headers["x-goog-upload-command"] ?? "") as string;
    if (command === "query") {
      // The resume probe: what the server already holds.
      return {
        status: 200,
        headers: {
          "X-Goog-Upload-Status": "active",
          "X-Goog-Upload-Size-Received": String(received),
        },
        body: "",
      };
    }
    received += ((request.body ?? "") as string).length;
    if (command === "upload, finalize") {
      return {
        status: 200,
        headers: { "X-Goog-Upload-Status": "final" },
        body: {
          name: "notes.bin",
          bucket: "demo-nsh.firebasestorage.app",
          size: "600000",
          contentType: "application/octet-stream",
        },
      };
    }
    return { status: 200, headers: { "X-Goog-Upload-Status": "active" }, body: "" };
  },
};

test("home, auth, database, storage and analytics render", async () => {
  const app = await open("../App.tsx", { network: stubbed });

  await app.screenshot("../../docs/evidence/demo/macos-home.png");

  // Auth: the form filled in, not yet submitted.
  await app.press("id:row-auth");
  await app.waitFor("id:page-auth");
  await app.type("id:auth-email", "demo@demo-nsh.test");
  await app.type("id:auth-password", "fake-password-1");
  await app.screenshot("../../docs/evidence/demo/macos-auth.png");
  await app.back();
  await app.waitFor("id:page-home");

  // Database: the live list from the seeded stream.
  await app.press("id:row-database");
  await app.waitFor("id:page-database");
  await app.waitFor("text:Read the design guides");
  await app.type("id:db-draft", "Ship the demo");
  await app.screenshot("../../docs/evidence/demo/macos-database.png");
  await app.back();
  await app.waitFor("id:page-home");

  // Storage: a finished upload, the read buttons enabled.
  await app.press("id:row-storage");
  await app.waitFor("id:page-storage");
  await app.press("id:storage-upload");
  await app.waitFor("text:Uploaded notes.bin (600000 bytes).");
  await app.screenshot("../../docs/evidence/demo/macos-storage.png");
  await app.back();
  await app.waitFor("id:page-home");

  // Analytics: three events fill the batch, the queue sends it.
  await app.press("id:row-analytics");
  await app.waitFor("id:page-analytics");
  await app.press("id:analytics-log");
  await app.press("id:analytics-log");
  await app.press("id:analytics-log");
  await app.waitFor("text:Events sent: 3");
  await app.screenshot("../../docs/evidence/demo/macos-analytics.png");
});

test("the monitor measures the live collector, slow route on top", async () => {
  const app = await open("../App.tsx", { network: "live" });
  await app.press("id:row-monitor");
  await app.waitFor("id:page-monitor");
  // The slow route takes 1.2 s for real; the ring fills as the calls land.
  await app.waitFor("text:/4 calls/");
  await app.waitFor("text:/slow");
  await app.screenshot("../../docs/evidence/demo/macos-monitor.png");
});
