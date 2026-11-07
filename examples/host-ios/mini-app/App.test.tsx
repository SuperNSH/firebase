// Headless run with the test playing the host: the same handler names the
// native NshFirebase facade answers, so the app side of the contract is
// proven without a simulator.
import { expect, open, test } from "nsh/test";

const handlers = {
  "firebase.messaging.token": async () => ({ token: "fake-token-1234567890" }),
  "firebase.messaging.permission": async () => ({ permission: "granted" }),
  "firebase.messaging.requestPermission": async () => ({ permission: "granted" }),
  "firebase.messaging.show": async () => ({ shown: true }),
  "firebase.analytics.logEvent": async () => null,
  "firebase.analytics.logScreenView": async () => null,
  "firebase.perf.startTrace": async ({ name }: { name: string }) => ({ id: `trace-${name}` }),
  "firebase.perf.stopTrace": async () => null,
  "firebase.perf.recordHttpMetric": async () => ({ recorded: true }),
};

test("probes every facade handler and shows the answers", async () => {
  const app = await open("./App.tsx", { host: { handlers } });
  await app.press("role:button[Probe handlers]");
  await app.waitFor("text:token fake-token-1234567890");
  await app.waitFor("text:permission granted");
  await app.waitFor("text:analytics + perf recorded (true)");
});

test("shows the notification it asked the host for", async () => {
  const app = await open("./App.tsx", { host: { handlers } });
  await app.press("role:button[Show a notification]");
  await app.waitFor("text:show true");
});

test("a press event from the host reaches the app", async () => {
  const app = await open("./App.tsx", { host: { handlers } });
  await app.host.send("firebase.messaging.press", { id: "tap-1" });
  await app.waitFor("text:press tap-1");
});
