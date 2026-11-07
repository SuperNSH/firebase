// Headless tests of the remote-config on a manual clock: the pure
// tier's defaults and typed reads, template activation over a stubbed
// transport, the honest refusal to fetch without one, and the native
// tier's firebase.remote-config.* routing (the test plays the host).
// No emulator test exists: the emulator suite has no Remote Config
// service. The app root is tests/remote-config-app.tsx; capabilities
// come from tests/nsh.json, the nsh.json next to the entry.
import { describe, expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

const ENTRY = "./remote-config-app.tsx";

const PURE_DEFAULTS_LINE =
  'defaults: {"welcome_message":"hello from defaults","feature_flag_dark_mode":false,"tip_amount":10,"rollout_copy":"app default copy"}';
const VALUES_BEFORE_DEFAULTS = "values: string= bool=false number=0 missing=null group=";
const VALUES_AFTER_DEFAULTS =
  "values: string=hello from defaults bool=false number=10 missing=null group=";
const VALUES_AFTER_ACTIVATION =
  "values: string=Welcome to nsh-firebase bool=true number=25 missing=null group=Pay now";

describe("remote-config", () => {
  test("the pure tier sets, reads and types the defaults", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.waitFor("text:pure client ready");
    await app.press("id:read-defaults");
    await app.waitFor("text:defaults: {}");
    await app.press("id:set-defaults");
    await app.waitFor("text:defaults set");
    await app.waitFor(`text:${PURE_DEFAULTS_LINE}`);
    await app.press("id:read-values");
    await app.waitFor(`text:${VALUES_AFTER_DEFAULTS}`);
  });

  test("unset keys read empty, false, zero and null like the SDKs", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.waitFor("text:pure client ready");
    await app.press("id:read-values");
    await app.waitFor(`text:${VALUES_BEFORE_DEFAULTS}`);
  });

  test("without a transport the fetch rejects with the oauth code and nothing is pretended", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.waitFor("text:pure client ready");
    await app.press("id:fetch");
    await app.waitFor("text:/^fetch failed: remote-config.unavailable/");
    await app.waitFor("text:last fetch: no_fetch_yet");
    await app.press("id:read-values");
    await app.waitFor(`text:${VALUES_BEFORE_DEFAULTS}`);
  });

  test("a fetched template activates over the defaults", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-transport");
    await app.waitFor("text:transport client ready");
    await app.press("id:set-defaults");
    await app.waitFor("text:defaults set");
    await app.press("id:fetch");
    await app.waitFor('text:fetch: {"activated":true,"lastFetchStatus":"success"}');
    await app.waitFor("text:last fetch: success");
    await app.press("id:read-values");
    await app.waitFor(`text:${VALUES_AFTER_ACTIVATION}`);
    await app.press("id:read-defaults");
    await app.waitFor(`text:${PURE_DEFAULTS_LINE}`);
  });

  test("a refusing backend records failure and rejects", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-transport");
    await app.waitFor("text:transport client ready");
    await app.press("id:fail-next");
    await app.waitFor("text:next fetch will fail");
    await app.press("id:fetch");
    await app.waitFor(
      "text:fetch failed: core/network: HTTP 401: the backend refused the fake project",
    );
    await app.waitFor("text:last fetch: failure");
  });

  test("a corrupt template rejects with the template code", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-transport");
    await app.waitFor("text:transport client ready");
    await app.press("id:break-template");
    await app.waitFor("text:next template is broken");
    await app.press("id:fetch");
    await app.waitFor(
      "text:fetch failed: remote-config/invalid-template: conditions.1.name `dup` is a duplicate",
    );
    await app.waitFor("text:last fetch: failure");
  });

  test("defaults that are not a flat map are refused with the stable code", async () => {
    const app: App = await open(ENTRY, { clock: {} });
    await app.waitFor("text:ready");
    await app.press("id:init-pure");
    await app.waitFor("text:pure client ready");
    await app.press("id:set-bad-defaults");
    await app.waitFor(
      "text:set failed: core/invalid-argument: defaults.broken is not a string, number or boolean",
    );
    await app.press("id:read-defaults");
    await app.waitFor("text:defaults: {}");
  });

  test("the native tier routes through the firebase.remote-config.* handlers", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.remote-config.setDefaults": () => ({}),
          "firebase.remote-config.getDefaults": { map: { welcome_message: "from host" } },
          "firebase.remote-config.fetchAndActivate": () => ({
            activated: true,
            lastFetchStatus: "success",
          }),
          "firebase.remote-config.get": ({ key }: { key: string }) => ({
            value: key === "ml_prompt" ? null : `host ${key}`,
          }),
        },
      },
    });
    await app.waitFor("text:ready");
    await app.press("id:init-native");
    await app.waitFor("text:native client ready");
    await app.press("id:set-defaults");
    await app.waitFor("text:defaults set");
    await app.press("id:read-defaults");
    await app.waitFor('text:defaults: {"welcome_message":"from host"}');
    await app.press("id:read-values");
    await app.waitFor(
      "text:values: string=host welcome_message bool=false number=0 missing=null group=host checkout_button_label",
    );
    await app.press("id:fetch");
    await app.waitFor('text:fetch: {"activated":true,"lastFetchStatus":"success"}');
    // The wire calls, in order: the map rides setDefaults, the key rides
    // get, getDefaults and fetchAndActivate carry no arguments — the
    // shapes the native areas' handlers must answer.
    expect(app.host.calls()).toEqual([
      {
        name: "firebase.remote-config.setDefaults",
        data: {
          map: {
            welcome_message: "hello from defaults",
            feature_flag_dark_mode: false,
            tip_amount: 10,
            rollout_copy: "app default copy",
          },
        },
      },
      { name: "firebase.remote-config.getDefaults", data: null },
      { name: "firebase.remote-config.get", data: { key: "welcome_message" } },
      { name: "firebase.remote-config.get", data: { key: "feature_flag_dark_mode" } },
      { name: "firebase.remote-config.get", data: { key: "tip_amount" } },
      { name: "firebase.remote-config.get", data: { key: "ml_prompt" } },
      { name: "firebase.remote-config.get", data: { key: "checkout_button_label" } },
      { name: "firebase.remote-config.fetchAndActivate", data: null },
    ]);
  });

  test("a malformed native answer is a clear host error", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.remote-config.fetchAndActivate": () => ({
            activated: "yes",
            lastFetchStatus: "success",
          }),
        },
      },
    });
    await app.waitFor("text:ready");
    await app.press("id:init-native");
    await app.waitFor("text:native client ready");
    await app.press("id:fetch");
    await app.waitFor(
      "text:fetch failed: core/host-error: the firebase.remote-config.fetchAndActivate handler returned no `activated` boolean and `lastFetchStatus` string",
    );
  });

  test("a host handler that throws surfaces clearly", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          // The backend refusing a fake project, played by the host.
          "firebase.remote-config.fetchAndActivate": () => {
            throw new Error("401: the backend refused the fake project");
          },
        },
      },
    });
    await app.waitFor("text:ready");
    await app.press("id:init-native");
    await app.waitFor("text:native client ready");
    await app.press("id:fetch");
    await app.waitFor("text:/fetch failed: core.host-error/");
  });

  test("an explicit transport forces the pure path even on a native tier", async () => {
    const app: App = await open(ENTRY, {
      clock: {},
      host: {
        handlers: {
          "firebase.remote-config.fetchAndActivate": { activated: false, lastFetchStatus: "never" },
        },
      },
    });
    await app.waitFor("text:ready");
    await app.press("id:init-transport");
    await app.waitFor("text:transport client ready");
    await app.press("id:fetch");
    await app.waitFor('text:fetch: {"activated":true,"lastFetchStatus":"success"}');
    // Nothing reached the host: the transport owned the fetch.
    expect(app.host.calls()).toEqual([]);
  });
});
