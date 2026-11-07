// Headless tests of the demo's product screens, on a manual clock with a
// stubbed network and host (no real time, no real requests): the shell's
// navigation, the auth form's validation and a stubbed sign-in, the
// database live list, the upload's progress, the messaging pure-tier
// refusal (and its native path), the analytics batch, and the monitor
// over a stubbed collector.
import { describe, expect, open, test } from "nsh/test";
import type { App, OpenOptions, StubRequest, StubResponse } from "nsh/test";

const AUTH = "http://127.0.0.1:19099";
const DB = "http://127.0.0.1:19000";
const OBJECT_BASE = "http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app";
const COLLECTOR = "http://127.0.0.1:18742";
const SESSION = "http://127.0.0.1:19199/upload/demo-session";

const SIGN_IN_URL = `${AUTH}/identitytoolkit.googleapis.com/v1/accounts:signInWithPassword?key=demo-nsh-fake-api-key`;
const START_URL = `${OBJECT_BASE}/o?name=demo%2Fnotes.bin`;
const OBJECT_URL = `${OBJECT_BASE}/o/demo%2Fnotes.bin`;

type Routes = NonNullable<OpenOptions["network"]>;

function session(email: string, uid: string): StubResponse {
  return {
    status: 200,
    body: {
      idToken: "demo-id-token",
      refreshToken: "demo-refresh-token",
      expiresIn: "3600",
      email,
      localId: uid,
      registered: true,
    },
  };
}

// One SSE put frame: the fresh stream's whole value at the ref.
function put(value: unknown): string {
  return `event: put\ndata: ${JSON.stringify({ path: "/", data: value })}\n\n`;
}

// The storage session: the start hands out the session URI, a query
// reports what the server holds, slices record, the finalize answers
// the object's metadata.
let received = 0;

const storageRoutes: Routes = {
  [`POST ${START_URL}`]: () => ({
    status: 200,
    headers: { "X-Goog-Upload-Status": "active", "X-Goog-Upload-URL": SESSION },
    body: "",
  }),
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
          name: "demo/notes.bin",
          bucket: "demo-nsh.firebasestorage.app",
          size: "600000",
          contentType: "application/octet-stream",
          downloadTokens: "demo-tok",
        },
      };
    }
    // The first slice waits on the clock, so the in-flight progress is
    // observable before the run completes.
    const offset = Number(request.headers["x-goog-upload-offset"] ?? 0);
    return {
      status: 200,
      headers: { "X-Goog-Upload-Status": "active" },
      body: "",
      delay: offset === 0 ? 250 : 0,
    };
  },
  [`GET ${OBJECT_URL}?alt=media`]: { status: 200, body: "demo-bytes" },
  [`DELETE ${OBJECT_URL}`]: { status: 200, body: "" },
};

// The collector the monitor screen measures: a slow route the clock has
// to move past, two fast ones and the sink.
const monitorRoutes: Routes = {
  [`GET ${COLLECTOR}/slow?ms=1200`]: { status: 200, body: "slow", delay: 1_200 },
  [`GET ${COLLECTOR}/health`]: { status: 200, body: "ok" },
  [`GET ${COLLECTOR}/assert?contains=demo&count=0`]: { status: 200, body: "ok" },
  [`POST ${COLLECTOR}/collect`]: { status: 200, body: "ok" },
};

async function openScreen(row: string, extra: OpenOptions = {}): Promise<App> {
  return open("../App.tsx", { clock: {}, ...extra });
}

describe("demo", () => {
  test("the home rows open every product page", async () => {
    const app = await open("../App.tsx", { clock: {} });
    for (const route of ["auth", "database", "storage", "messaging", "analytics", "monitor"]) {
      await app.press(`id:row-${route}`);
      await app.waitFor(`id:page-${route}`);
      await app.back();
      await app.waitFor("id:page-home");
    }
  });

  test("the auth form validates, then signs in through the stubbed emulator", async () => {
    const app = await open("../App.tsx", {
      clock: {},
      network: { [`POST ${SIGN_IN_URL}`]: session("demo@demo-nsh.test", "uid-demo-1") },
    });
    await app.press("id:row-auth");
    await app.waitFor("id:page-auth");
    // An empty submit shows the rules, not a server call.
    await app.press("role:button[Sign in]");
    await app.waitFor("text:Enter your email.");
    // A bad address shows the email rule.
    await app.type("id:auth-email", "not-an-email");
    await app.press("role:button[Sign in]");
    await app.waitFor("text:That is not an email address.");
    // A valid form reaches the emulator and flips the currentUser signal.
    await app.type("id:auth-email", "demo@demo-nsh.test");
    await app.type("id:auth-password", "fake-password-1");
    await app.press("role:button[Sign in]");
    await app.waitFor("text:Signed in as uid-demo-1");
    await app.waitFor("text:demo@demo-nsh.test");
    const sent = app.requests().filter((r) => r.url === SIGN_IN_URL);
    expect(sent.length).toBe(1);
    // Signing out clears the signal and the session line.
    await app.press("role:button[Sign out]");
    await app.waitFor("text:Signed out");
  });

  test("a server auth error lands on its field", async () => {
    const app = await open("../App.tsx", {
      clock: {},
      network: {
        [`POST ${SIGN_IN_URL}`]: {
          status: 400,
          body: { error: { code: 400, message: "INVALID_PASSWORD" } },
        },
      },
    });
    await app.press("id:row-auth");
    await app.waitFor("id:page-auth");
    await app.type("id:auth-email", "demo@demo-nsh.test");
    await app.type("id:auth-password", "wrong-password");
    await app.press("role:button[Sign in]");
    await app.waitFor("text:That password is wrong for the account.");
  });

  test("the database screen shows the live value and pushes", async () => {
    const app = await open("../App.tsx", {
      clock: {},
      network: {
        [`GET ${DB}/demo/items.json`]: { status: 200, body: put({ "item-1": { title: "First" } }) },
        [`PUT ${DB}/demo/items/*.json`]: { status: 200, body: "" },
      },
    });
    await app.press("id:row-database");
    await app.waitFor("id:page-database");
    // The listener's first put renders the row; the stub's stream ends
    // after it, which the status line reports honestly.
    await app.waitFor("text:First");
    await app.waitFor("text:1 items · Reconnecting…");
    // The push form writes a child and clears its draft.
    await app.type("id:db-draft", "Second");
    await app.press("id:db-add");
    const pushes = app
      .requests()
      .filter((r) => r.method === "PUT" && r.url.startsWith(`${DB}/demo/items/`));
    expect(pushes.length).toBe(1);
    expect(app.enabled("id:db-add")).toBe(false);
  });

  test("the upload shows its progress, then download and delete run", async () => {
    const app = await open("../App.tsx", { clock: {}, network: storageRoutes });
    await app.press("id:row-storage");
    await app.waitFor("id:page-storage");
    // The task's progress signal drives the bar from the first instant.
    await app.press("id:storage-upload");
    await app.waitFor("text:0 of 600000 bytes · running");
    // The slices and the finalize go out; the metadata lands.
    await app.waitFor("text:Uploaded notes.bin (600000 bytes).");
    const uploads = app.requests().filter((r) => r.url === SESSION);
    // Two full 256 KiB slices, the rest, and the finalize (plus any query).
    expect(uploads.length).toBeGreaterThanOrEqual(4);
    expect(
      uploads.filter((r) => r.headers["x-goog-upload-command"] === "upload, finalize").length,
    ).toBe(1);
    // Download and delete of the same object.
    await app.press("id:storage-download");
    await app.waitFor("text:Downloaded 10 bytes.");
    await app.press("id:storage-delete");
    await app.waitFor("text:Deleted.");
  });

  test("messaging says plainly that the pure tier has no token", async () => {
    const app = await open("../App.tsx", { clock: {} });
    await app.press("id:row-messaging");
    await app.waitFor("id:page-messaging");
    await app.press("id:messaging-check");
    await app.waitFor("text:/^core\\/unavailable/");
    await app.press("id:messaging-request");
    await app.waitFor("text:/^core\\/unavailable/");
  });

  test("messaging reaches a native host when one answers", async () => {
    const app = await open("../App.tsx", {
      clock: {},
      host: {
        handlers: {
          "firebase.messaging.permission": () => ({ permission: "granted" }),
          "firebase.messaging.token": () => ({ token: "demo-fcm-token" }),
        },
      },
    });
    await app.press("id:row-messaging");
    await app.waitFor("id:page-messaging");
    await app.press("id:messaging-check");
    await app.waitFor("text:granted");
    await app.press("id:messaging-request");
    await app.waitFor("text:Token: demo-fcm-token");
  });

  test("analytics queues three events and the flush sends the batch", async () => {
    const app = await open("../App.tsx", { clock: {} });
    await app.press("id:row-analytics");
    await app.waitFor("id:page-analytics");
    await app.waitFor("text:Queued: 0");
    await app.press("id:analytics-log");
    await app.waitFor("text:Queued: 1");
    // The third event fills the batch, and the queue sends it at once.
    await app.press("id:analytics-log");
    await app.press("id:analytics-log");
    await app.waitFor("text:Events sent: 3");
    await app.waitFor("text:Queued: 0");
    // The flush on an empty queue is a no-op.
    await app.press("id:analytics-flush");
    await app.waitFor("text:Events sent: 3");
  });

  test("the monitor renders the measured calls from the stubbed collector", async () => {
    const app = await open("../App.tsx", { clock: {}, network: monitorRoutes });
    await app.press("id:row-monitor");
    await app.waitFor("id:page-monitor");
    // The slow route's delay runs on the clock; the rows follow the ring.
    await app.clock!.runFor(1_300);
    await app.waitFor("text:/slow");
    expect(app.texts()).toContain("/health");
    expect(app.texts()).toContain("/collect");
    expect(app.texts()).toContain("1200 ms");
  });
});
