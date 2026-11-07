// Headless tests of the core on a manual clock with a stubbed network.
// The criterion-8 test is the last one: a request to a host the app's
// capabilities do not declare must reject with a clear ERR_ACCESS_DENIED
// through the facade, before the network sees it.
import { describe, expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

const DB = "http://127.0.0.1:19000";
// The app root is the package root: a nested entry would make `nsh` treat
// tests/ as the app and lose its nsh.json.
const ENTRY = "../App.tsx";

describe("core", () => {
  test("initializes with the demo config", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.press("id:init");
    await app.waitFor(
      "text:app same project=demo-nsh db=http://127.0.0.1:19000 storage=demo-nsh.firebasestorage.app",
    );
  });

  test("the pure tier goes through the stubbed network", async () => {
    let hits = 0;
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`GET ${DB}/todos.json`]: () => {
          hits += 1;
          return { status: 200, body: [{ id: 1, title: "one" }] };
        },
      },
    });
    await app.press("id:fetch");
    await app.waitFor(`text:fetch: status=200 body=[{"id":1,"title":"one"}]`);
    expect(hits).toBe(1);
    expect(app.requests().map((r) => `${r.method} ${r.url}`)).toEqual([`GET ${DB}/todos.json`]);
  });

  test("a host call resolves, and failures map to stable codes", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.core.ping": async () => ({ pong: true }),
          "firebase.core.track": () => "tracked",
        },
      },
    });
    await app.press("id:ping");
    await app.waitFor(`text:ping: {"pong":true}`);
    await app.press("id:ping-missing");
    await app.waitFor("text:missing: core/host-error (ERR_NSH_HOST_ERROR)");
    // `other.ping` is outside capabilities.host ["firebase.*"]: the
    // runtime itself refuses the call before any handler is reached.
    await app.press("id:ping-denied");
    await app.waitFor("text:other: core/access-denied (ERR_ACCESS_DENIED)");
    await app.press("id:post");
    await app.waitFor("text:post: ok");
    // "post: ok" only proves host.post did not throw; this proves the
    // facade actually handed the post to the host.
    expect(app.host.posts()).toEqual([{ name: "firebase.core.track", data: { name: "demo" } }]);
    await app.press("id:post-denied");
    await app.waitFor("text:other-post: core/access-denied (ERR_ACCESS_DENIED)");
    await app.press("id:map-unavailable");
    await app.waitFor(
      "text:core/unavailable (ERR_FEATURE_UNAVAILABLE_ON_PLATFORM): no host answers `firebase.core.ping` in the pure tier; use the product's fetch transport",
    );
  });

  test("a config document loads with the emulator switch winning", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.press("id:config");
    await app.waitFor("text:config db=http://127.0.0.1:19000 storage=http://127.0.0.1:19199 project=demo-nsh");
    await app.press("id:config-pure");
    // Without the emulator block the document's own URLs stand, distinct
    // from any emulator port on purpose; the storage REST host is
    // unprefixed, the bucket goes in the path.
    await app.waitFor(
      "text:config db=http://10.9.9.9:17000 storage=https://firebasestorage.googleapis.com/v0/b/demo-nsh.firebasestorage.app/o project=demo-nsh",
    );
  });

  test("the event bus fans out and off stops a listener", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.press("id:subscribe");
    await app.press("id:emit");
    await app.waitFor("text:a=1 b=1");
    await app.press("id:emit");
    await app.waitFor("text:a=2 b=2");
    await app.press("id:off-b");
    await app.press("id:emit");
    await app.waitFor("text:a=3 b=2");
  });

  test("a refused network capability rejects with ERR_ACCESS_DENIED through the facade", async () => {
    let hits = 0;
    const app: App = await open(ENTRY, {
      clock: {},
      network: {
        [`GET https://denied.example.test/.json`]: () => {
          hits += 1;
          return { status: 200, body: "the stub must never answer" };
        },
      },
    });
    await app.press("id:deny");
    await app.waitFor("text:deny: core/access-denied (ERR_ACCESS_DENIED)");
    expect(hits).toBe(0);
  });
});
