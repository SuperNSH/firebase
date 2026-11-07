// Headless tests of the crashlytics facade on a manual clock: the test
// plays the native host with the same handler names the native areas
// register, and a run without host options plays the pure tier, whose
// every call must reject with crashlytics/unavailable — there is no
// honest pure tier for crash reporting, so nothing degrades and nothing
// buffers. The entry lives in src/crashlytics/ because nsh reads the
// nsh.json next to the entry, and grants host functions never without a
// declaration.
import { describe, expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

const ENTRY = "../src/crashlytics/app.tsx";

const PURE =
  "crashlytics/unavailable (ERR_FEATURE_UNAVAILABLE_ON_PLATFORM): Firebase Crashlytics needs the native tier: iOS/Android host with firebase.crashlytics.* handlers (Crashlytics has no REST — no honest pure tier exists)";

// The reply shapes the native tiers answer with: the act-only calls
// resolve {}, didCrash carries {crashed}.
const handlers = {
  "firebase.crashlytics.log": async () => ({}),
  "firebase.crashlytics.setUserId": async () => ({}),
  "firebase.crashlytics.setCustomKey": async () => ({}),
  "firebase.crashlytics.recordError": async () => ({}),
  "firebase.crashlytics.sendUnsentReports": async () => ({}),
  "firebase.crashlytics.didCrashDuringPreviousExecution": async () => ({ crashed: true }),
};

describe("crashlytics", () => {
  test("the native tier routes every call, and the wire names and args match the contract", async () => {
    const app: App = await open(ENTRY, { clock: {}, host: { handlers } });
    await app.press("id:log");
    await app.waitFor("text:log: ok");
    await app.press("id:user-id");
    await app.waitFor("text:user id: ok");
    await app.press("id:key");
    await app.waitFor("text:key: ok");
    await app.press("id:keys");
    await app.waitFor("text:keys: ok");
    await app.press("id:record-error");
    await app.waitFor("text:record error: ok");
    await app.press("id:record-error-stack");
    await app.waitFor("text:record error stack: ok");
    await app.press("id:send");
    await app.waitFor("text:send: ok");
    await app.press("id:did-crash");
    await app.waitFor("text:did crash: true");
    // setAttributes expanded to one setCustomKey per key in order; the
    // stack rides only when given; the no-arg calls carry null.
    expect(app.host.calls()).toEqual([
      { name: "firebase.crashlytics.log", data: { message: "payment screen shown" } },
      { name: "firebase.crashlytics.setUserId", data: { id: "user-41" } },
      { name: "firebase.crashlytics.setCustomKey", data: { key: "difficulty", value: "hard" } },
      { name: "firebase.crashlytics.setCustomKey", data: { key: "level", value: 7 } },
      { name: "firebase.crashlytics.setCustomKey", data: { key: "paid", value: true } },
      {
        name: "firebase.crashlytics.recordError",
        data: { code: 401, message: "payment declined" },
      },
      {
        name: "firebase.crashlytics.recordError",
        data: { code: 500, message: "boom", stack: "at run (app.tsx:1)" },
      },
      { name: "firebase.crashlytics.sendUnsentReports", data: null },
      { name: "firebase.crashlytics.didCrashDuringPreviousExecution", data: null },
    ]);
  });

  test("didCrashDuringPreviousExecution reports a clean previous run too", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          ...handlers,
          "firebase.crashlytics.didCrashDuringPreviousExecution": async () => ({ crashed: false }),
        },
      },
    });
    await app.press("id:did-crash");
    await app.waitFor("text:did crash: false");
  });

  test("a reply without a crashed flag is a host error, not a silent false", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          ...handlers,
          "firebase.crashlytics.didCrashDuringPreviousExecution": async () => ({}),
        },
      },
    });
    await app.press("id:did-crash");
    await app.waitFor(
      "text:did crash: core/host-error: `firebase.crashlytics.didCrashDuringPreviousExecution` answered without a crashed flag",
    );
  });

  test("a failing handler surfaces the host error with its message", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.crashlytics.recordError": async () => {
            throw new Error("crashlytics not initialized: fake project");
          },
        },
      },
    });
    await app.press("id:record-error");
    await app.waitFor(
      "text:/^record error: core\\/host-error \\(ERR_NSH_HOST_ERROR\\).*crashlytics not initialized/",
    );
  });

  test("a non-scalar custom key value is refused before the host", async () => {
    const app: App = await open(ENTRY, { clock: {}, host: { handlers } });
    await app.press("id:key-bad");
    await app.waitFor(
      "text:key bad: core/invalid-argument: crashlytics custom keys take a string, number or boolean",
    );
    expect(app.host.calls()).toEqual([]);
  });

  test("the pure tier rejects every call with the error naming what is missing", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    const calls: (readonly [string, string])[] = [
      ["id:log", `log: ${PURE}`],
      ["id:user-id", `user id: ${PURE}`],
      ["id:key", `key: ${PURE}`],
      ["id:keys", `keys: ${PURE}`],
      ["id:record-error", `record error: ${PURE}`],
      ["id:record-error-stack", `record error stack: ${PURE}`],
      ["id:send", `send: ${PURE}`],
      ["id:did-crash", `did crash: ${PURE}`],
    ];
    for (const [id, text] of calls) {
      await app.press(id);
      await app.waitFor(`text:${text}`);
    }
    // Local validation still runs before the tier refusal: a bad custom
    // value is the caller's bug, whatever tier this is.
    await app.press("id:key-bad");
    await app.waitFor(
      "text:key bad: core/invalid-argument: crashlytics custom keys take a string, number or boolean",
    );
  });
});
