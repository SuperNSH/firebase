// Headless tests of storage on a manual clock with a stubbed network:
// the resumable upload's wire traffic and progress (including an
// interrupted-then-resumed run, pause and cancel), the download with its
// Firebase auth header, download URLs, metadata, list paging, delete and
// the missing object's stable error.
import { describe, expect, open, test } from "nsh/test";
import type { App, StubRequest, StubResponse } from "nsh/test";

// The app root is the package root: a nested entry would make `nsh`
// treat tests/ as the app and lose its nsh.json.
const ENTRY = "./storage-app.tsx";

const OBJECT_BASE = "http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app";
// The start URL the client builds for the upload path (st/big.bin).
const START_URL = `${OBJECT_BASE}/o?name=st%2Fbig.bin`;
// The session URI the start stub hands out; what `st-resume` saves.
const SESSION = "http://127.0.0.1:19199/upload/st-session";
const FINAL_METADATA = {
  name: "st/big.bin",
  bucket: "demo-nsh.firebasestorage.app",
  generation: "1",
  metageneration: "1",
  size: "600000",
  contentType: "application/octet-stream",
  downloadTokens: "st-tok",
};

// What the fake server recorded of the session so far.
interface UploadState {
  received: number;
  uploads: { command: string; length: number; offset: number }[];
  failAt: number | null;
  delay: number;
  denyFinalize: boolean;
}

// The session route: a query reports what the server holds, slices
// record themselves and answer active (final with metadata on the last).
function uploadRoutes(state: UploadState): Record<string, (request: StubRequest) => StubResponse> {
  return {
    [`POST ${START_URL}`]: () => ({
      status: 200,
      headers: { "X-Goog-Upload-Status": "active", "X-Goog-Upload-URL": SESSION },
      body: "",
    }),
    [`POST ${SESSION}`]: (request: StubRequest): StubResponse => {
      const command = header(request, "X-Goog-Upload-Command") ?? "";
      if (command === "query") {
        return {
          status: 200,
          headers: {
            "X-Goog-Upload-Status": "active",
            "X-Goog-Upload-Size-Received": String(state.received),
          },
          body: "",
        };
      }
      const offset = Number(header(request, "X-Goog-Upload-Offset"));
      if (state.failAt === offset) {
        state.failAt = null;
        return { error: "connection reset" };
      }
      // A denied finalize answers 403 with an `x-goog-upload-status:
      // final` header: the HTTP status is read first.
      if (command === "upload, finalize" && state.denyFinalize) {
        return {
          status: 403,
          headers: { "X-Goog-Upload-Status": "final" },
          body: { error: { code: 403, message: "Permission denied. No WRITE permission." } },
        };
      }
      const length = (request.body ?? "").length;
      state.received += length;
      state.uploads.push({ command, length, offset });
      const finalized = command === "upload, finalize";
      return {
        status: 200,
        headers: { "X-Goog-Upload-Status": finalized ? "final" : "active" },
        body: finalized ? JSON.stringify(FINAL_METADATA) : "",
        delay: finalized ? 0 : state.delay,
      };
    },
  };
}

// The engine sends fetch header names lower-cased; read them
// case-insensitively, as HTTP means them.
function header(request: StubRequest, name: string): string | undefined {
  const lower = name.toLowerCase();
  const found = Object.keys(request.headers).find((key) => key.toLowerCase() === lower);
  return found === undefined ? undefined : request.headers[found];
}

// The progress log line ends with the latest transition.
function progressEndsWith(app: App, suffix: string): boolean {
  return app.texts().some((text) => text.endsWith(suffix));
}

async function waitForProgress(app: App, suffix: string): Promise<void> {
  await app.waitUntil(() => progressEndsWith(app, suffix));
}

describe("storage", () => {
  test("uploads in 256 KiB slices with progress", async () => {
    const state: UploadState = { received: 0, uploads: [], failAt: null, delay: 0, denyFinalize: false };
    const app: App = await open(ENTRY, { clock: {}, network: uploadRoutes(state) });
    await app.press("id:st-upload");
    await waitForProgress(app, "600000/600000 done");
    await app.waitFor("text:st: finished ok st/big.bin size=600000");

    const requests = app.requests();
    expect(requests.length).toBe(5); // start, query, three slices
    const start = requests[0];
    expect(start.method).toBe("POST");
    expect(start.url).toBe(START_URL);
    expect(header(start, "X-Goog-Upload-Protocol")).toBe("resumable");
    expect(header(start, "X-Goog-Upload-Command")).toBe("start");
    expect(header(start, "X-Goog-Upload-Header-Content-Length")).toBe("600000");
    expect(header(start, "X-Goog-Upload-Header-Content-Type")).toBe("application/octet-stream");
    expect(header(start, "Content-Type")).toBe("application/json; charset=utf-8");
    expect(JSON.parse(start.body ?? "{}")).toEqual({
      name: "st/big.bin",
      contentType: "application/octet-stream",
    });
    expect(header(requests[1], "X-Goog-Upload-Command")).toBe("query");
    expect(state.uploads).toEqual([
      { command: "upload", length: 256 * 1024, offset: 0 },
      { command: "upload", length: 256 * 1024, offset: 256 * 1024 },
      {
        command: "upload, finalize",
        length: 600000 - 2 * 256 * 1024,
        offset: 2 * 256 * 1024,
      },
    ]);
    expect(state.received).toBe(600000);
  });

  test("an interrupted upload resumes from the saved session id", async () => {
    const state: UploadState = { received: 0, uploads: [], failAt: 256 * 1024, delay: 0, denyFinalize: false };
    const app: App = await open(ENTRY, { clock: {}, network: uploadRoutes(state) });
    await app.press("id:st-upload");
    await waitForProgress(app, "262144/600000 error");
    await app.waitFor("text:st: finished err core/network");
    // The first slice landed server-side even though its answer was lost.
    expect(state.received).toBe(256 * 1024);

    state.failAt = null;
    // Save the session URI the task got, then resume by that saved id
    // (as a restarted process would).
    await app.press("id:st-save");
    await app.waitFor("text:st: session http://127.0.0.1:19199/upload/st-session");
    await app.press("id:st-resume");
    await waitForProgress(app, "600000/600000 done");
    await app.waitFor("text:st: finished ok st/big.bin size=600000");

    const requests = app.requests();
    // One start only; the resume opens with a query, not a new session.
    expect(requests.filter((request) => request.url === START_URL).length).toBe(1);
    expect(header(requests[4], "X-Goog-Upload-Command")).toBe("query");
    expect(state.uploads).toEqual([
      { command: "upload", length: 256 * 1024, offset: 0 },
      { command: "upload", length: 256 * 1024, offset: 256 * 1024 },
      {
        command: "upload, finalize",
        length: 600000 - 2 * 256 * 1024,
        offset: 2 * 256 * 1024,
      },
    ]);
  });

  test("pause holds and resume continues", async () => {
    const state: UploadState = { received: 0, uploads: [], failAt: null, delay: 1000, denyFinalize: false };
    const app: App = await open(ENTRY, { clock: {}, network: uploadRoutes(state) });
    await app.press("id:st-upload");
    // The first slice is in flight; pausing stops the loop at the next
    // boundary without it ever sending a second slice.
    await app.press("id:st-pause");
    await waitForProgress(app, "262144/600000 paused");
    // Nothing is in flight any more: the clock moving alone must not
    // advance the upload.
    await app.wait(10000);
    expect(progressEndsWith(app, "262144/600000 paused")).toBe(true);
    expect(state.received).toBe(256 * 1024);

    await app.press("id:st-resume");
    await waitForProgress(app, "600000/600000 done");
    await app.waitFor("text:st: finished ok st/big.bin size=600000");
    expect(state.uploads.map((upload) => upload.command)).toEqual([
      "upload",
      "upload",
      "upload, finalize",
    ]);
    expect(state.received).toBe(600000);
  });

  test("cancel ends in error with the canceled code", async () => {
    const state: UploadState = { received: 0, uploads: [], failAt: null, delay: 1000, denyFinalize: false };
    const app: App = await open(ENTRY, { clock: {}, network: uploadRoutes(state) });
    await app.press("id:st-upload");
    await waitForProgress(app, "0/600000 running");
    await app.press("id:st-cancel");
    await waitForProgress(app, "262144/600000 error");
    await app.waitFor("text:st: finished err storage/canceled");
    // The in-flight slice still landed, and nothing more was sent.
    await app.wait(10000);
    expect(state.received).toBe(256 * 1024);
    expect(state.uploads.length).toBe(1);
  });

  test("a denied finalize is an error despite its final header", async () => {
    const state: UploadState = {
      received: 0,
      uploads: [],
      failAt: null,
      delay: 0,
      denyFinalize: true,
    };
    const app: App = await open(ENTRY, { clock: {}, network: uploadRoutes(state) });
    await app.press("id:st-upload");
    await waitForProgress(app, "524288/600000 error");
    await app.waitFor("text:st: finished err storage/unauthorized");
  });

  test("a query without its size header errors instead of restarting", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`POST ${START_URL}`]: () => ({
          status: 200,
          headers: { "X-Goog-Upload-Status": "active", "X-Goog-Upload-URL": SESSION },
          body: "",
        }),
        [`POST ${SESSION}`]: (request: StubRequest): StubResponse => {
          // The query omits X-Goog-Upload-Size-Received entirely: the
          // client must not read that as "the server holds 0 bytes".
          return { status: 200, headers: { "X-Goog-Upload-Status": "active" }, body: "" };
        },
      },
    });
    await app.press("id:st-upload");
    await waitForProgress(app, "0/600000 error");
    await app.waitFor("text:st: finished err storage/retry-limit-exceeded");
    // The run stopped at the query: no slice went out.
    expect(app.requests().length).toBe(2);
  });

  test("download compares bytes and carries the Firebase auth header", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`GET ${OBJECT_BASE}/o/st%2Ffile.txt?alt=media`]: () => ({
          status: 200,
          body: "file-bytes",
        }),
        [`GET ${OBJECT_BASE}/o/st%2Fsecret.txt?alt=media`]: () => ({
          status: 200,
          body: "secret-bytes",
        }),
      },
    });
    await app.press("id:st-download");
    await app.waitFor("text:st: download ok 10");
    await app.press("id:st-auth-download");
    await app.waitFor("text:st: auth download ok 12");
    const secret = app
      .requests()
      .find((request) => request.url === `${OBJECT_BASE}/o/st%2Fsecret.txt?alt=media`);
    expect(secret === undefined ? undefined : header(secret, "Authorization")).toBe("Firebase st-id-token");
    const plain = app
      .requests()
      .find((request) => request.url === `${OBJECT_BASE}/o/st%2Ffile.txt?alt=media`);
    expect(plain === undefined ? undefined : header(plain, "Authorization")).toBeUndefined();
  });

  test("the download URL comes from the metadata's first token", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`GET ${OBJECT_BASE}/o/st%2Ffile.txt`]: () => ({
          status: 200,
          body: {
            name: "st/file.txt",
            bucket: "demo-nsh.firebasestorage.app",
            downloadTokens: "st-tok-1,st-tok-2",
            contentType: "text/plain",
          },
        }),
      },
    });
    await app.press("id:st-url");
    await app.waitFor(
      `text:st: url ${OBJECT_BASE}/o/st%2Ffile.txt?alt=media&token=st-tok-1`,
    );
  });

  test("metadata round-trips and the patch sends writable fields only", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`GET ${OBJECT_BASE}/o/st%2Ffile.txt`]: () => ({
          status: 200,
          body: {
            name: "st/file.txt",
            size: "11",
            contentType: "text/plain",
            metageneration: "1",
          },
        }),
        [`PATCH ${OBJECT_BASE}/o/st%2Ffile.txt`]: () => ({
          status: 200,
          body: {
            name: "st/file.txt",
            contentType: "text/updated",
            metageneration: "2",
          },
        }),
      },
    });
    await app.press("id:st-metadata");
    await app.waitFor("text:st: metadata ct=text/plain size=11");
    await app.press("id:st-patch");
    await app.waitFor("text:st: patched ct=text/updated gen=2");
    const patch = app
      .requests()
      .find((request) => request.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ contentType: "text/updated" });
  });

  test("list walks pages by nextPageToken", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`${OBJECT_BASE}/o?prefix=st%2F&delimiter=/&maxResults=2`]: () => ({
          status: 200,
          body: {
            prefixes: [],
            items: [{ name: "st/a.txt" }, { name: "st/b.txt" }],
            nextPageToken: "t2",
          },
        }),
        [`${OBJECT_BASE}/o?prefix=st%2F&delimiter=/&pageToken=t2&maxResults=2`]: () => ({
          status: 200,
          body: { prefixes: [], items: [{ name: "st/c.txt" }] },
        }),
      },
    });
    await app.press("id:st-list");
    await app.waitFor("text:st: page1=2 next=t2 page2=1");
  });

  test("delete succeeds and a missing object maps to its stable code", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`DELETE ${OBJECT_BASE}/o/st%2Ffile.txt`]: () => ({ status: 204, body: "" }),
        [`GET ${OBJECT_BASE}/o/st%2Fgone.txt`]: () => ({
          status: 404,
          body: { error: { code: 404, message: "No such object" } },
        }),
      },
    });
    await app.press("id:st-delete");
    await app.waitFor("text:st: deleted ok");
    await app.press("id:st-missing");
    await app.waitFor("text:st: err storage/object-not-found");
    const requests = app.requests();
    expect(requests[0].method).toBe("DELETE");
    expect(requests[0].url).toBe(`${OBJECT_BASE}/o/st%2Ffile.txt`);
  });
});
