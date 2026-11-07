// Headless tests of the messaging facade on a manual clock: the test plays
// the native host with the same handler names the native areas register,
// and a run without host options plays the pure tier, whose every call
// must fail naming what is missing.
import { describe, expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

// The entry lives in src/messaging/ because nsh reads the nsh.json next
// to the entry, and grants host functions never without a declaration; a
// tests/ entry would have none. Tests run under capabilities.host
// ["firebase.*"], as on a device.
const ENTRY = "../src/messaging/app.tsx";

const PURE =
  "core/unavailable (ERR_FEATURE_UNAVAILABLE_ON_PLATFORM): Firebase Messaging needs the native tier: iOS/Android host with firebase.messaging.* handlers";

// The reply shapes the native tiers answer with (iOS {shown}, Android's
// bare boolean on show — both must read as acceptance).
const handlers = {
  "firebase.messaging.token": async () => ({ token: "tok-stub-123" }),
  "firebase.messaging.permission": async () => ({ permission: "granted" }),
  "firebase.messaging.requestPermission": async () => ({ permission: "default" }),
  "firebase.messaging.show": async ({ id }: { id: string }) =>
    id === "bare" ? true : id === "hidden" ? { shown: false } : { shown: true },
  "firebase.messaging.close": async () => null,
  "firebase.messaging.setBadge": async () => true,
  "firebase.messaging.subscribe": async () => ({ topic: "news" }),
  "firebase.messaging.unsubscribe": async () => true,
};

describe("messaging", () => {
  test("the native tier routes every call, and the wire names and args match the contract", async () => {
    const app: App = await open(ENTRY, { clock: {}, host: { handlers } });
    await app.press("id:token");
    await app.waitFor("text:token: tok-stub-123");
    await app.press("id:permission");
    await app.waitFor("text:permission: granted");
    await app.press("id:request-permission");
    await app.waitFor("text:request: default");
    await app.press("id:show");
    await app.waitFor("text:show: true");
    await app.press("id:show-hidden");
    await app.waitFor("text:show hidden: false");
    await app.press("id:show-bare");
    await app.waitFor("text:show bare: true");
    // The Notifications module this mirrors shows an empty body as none,
    // but rejects an empty title like an empty id: the body-only shape
    // reaches the host, the empty title is refused before it.
    await app.press("id:show-empty");
    await app.waitFor("text:show empty: true");
    await app.press("id:show-empty-title");
    await app.waitFor(
      "text:show empty title: core/invalid-argument: messaging.show needs a non-empty id and title",
    );
    await app.press("id:close");
    await app.waitFor("text:close: ok");
    await app.press("id:badge");
    await app.waitFor("text:badge: ok");
    await app.press("id:subscribe");
    await app.waitFor("text:sub: ok");
    await app.press("id:unsubscribe");
    await app.waitFor("text:unsub: ok");
    expect(app.host.calls()).toEqual([
      { name: "firebase.messaging.token", data: null },
      { name: "firebase.messaging.permission", data: null },
      { name: "firebase.messaging.requestPermission", data: null },
      {
        name: "firebase.messaging.show",
        data: { id: "chat:42", title: "Ada", body: "Are you coming?", silent: false },
      },
      {
        name: "firebase.messaging.show",
        data: { id: "hidden", title: "Ada", body: "Are you coming?", silent: false },
      },
      {
        name: "firebase.messaging.show",
        data: { id: "bare", title: "Ada", body: "Are you coming?", silent: false },
      },
      {
        name: "firebase.messaging.show",
        data: { id: "quiet", title: "Hi", body: "", silent: false },
      },
      { name: "firebase.messaging.close", data: { id: "chat:42" } },
      { name: "firebase.messaging.setBadge", data: { count: 3 } },
      { name: "firebase.messaging.subscribe", data: { topic: "news" } },
      { name: "firebase.messaging.unsubscribe", data: { topic: "news" } },
    ]);
  });

  test("a failed token request surfaces the host error with its message", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.messaging.token": async () => {
            throw new Error("fcm token unavailable: fake project rejected");
          },
        },
      },
    });
    await app.press("id:token");
    await app.waitFor("text:/^token: core\\/host-error \\(ERR_NSH_HOST_ERROR\\).*fcm token unavailable/");
  });

  test("press, message and token events reach the listeners", async () => {
    const app: App = await open(ENTRY, { clock: {}, host: { handlers } });
    await app.press("id:listen");
    await app.waitFor("text:listen: ok");
    await app.host.send("firebase.messaging.press", { id: "chat:42" });
    await app.waitFor("text:press: chat:42");
    await app.host.send("firebase.messaging.message", {
      messageId: "m1",
      data: { lane: "chat" },
      notification: { title: "Hi", body: "Yo" },
      sentTime: 1234,
    });
    await app.waitFor("text:message: m1 Hi");
    await app.host.send("firebase.messaging.tokenRefreshed", { token: "tok-refreshed-9" });
    await app.waitFor("text:token event: tok-refreshed-9");
  });

  test("an empty topic is refused before the host is reached", async () => {
    const app: App = await open(ENTRY, { clock: {}, host: { handlers } });
    await app.press("id:subscribe-empty");
    await app.waitFor(
      "text:sub empty: core/invalid-argument: messaging.subscribe needs a non-empty topic",
    );
  });

  test("the pure tier fails every call with the error naming what is missing", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    const calls: (readonly [string, string])[] = [
      ["id:token", `token: ${PURE}`],
      ["id:permission", `permission: ${PURE}`],
      ["id:request-permission", `request: ${PURE}`],
      ["id:show", `show: ${PURE}`],
      ["id:show-hidden", `show hidden: ${PURE}`],
      ["id:close", `close: ${PURE}`],
      ["id:badge", `badge: ${PURE}`],
      ["id:subscribe", `sub: ${PURE}`],
      ["id:unsubscribe", `unsub: ${PURE}`],
    ];
    for (const [id, text] of calls) {
      await app.press(id);
      await app.waitFor(`text:${text}`);
    }
    await app.press("id:listen");
    await app.waitFor(`text:listen: ${PURE}`);
  });
});
