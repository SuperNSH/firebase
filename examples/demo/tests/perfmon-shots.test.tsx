// The monitor screen's look, on a real platform that draws: the route
// driven with the collector's responses (stubs with the same status,
// headers and 1200 ms slow-route delay — no real internet), screenshots
// in both appearances into docs/evidence/perfmon. Run from the repo
// root with a target that draws:
//   nsh test examples/demo/tests/perfmon-shots.test.tsx --target macos
// Headless runs draw nothing, so a screenshot step needs `--target`.
import { expect, open, test } from "nsh/test";

const COLLECTOR = "http://127.0.0.1:18742";

const collectorRoutes = {
  [`GET ${COLLECTOR}/slow?ms=1200`]: {
    status: 200,
    headers: { "content-length": "2" },
    body: "ok",
    delay: 1200,
  },
  [`GET ${COLLECTOR}/health`]: { status: 200, body: { ok: true } },
  [`GET ${COLLECTOR}/assert?contains=demo&count=0`]: {
    status: 200,
    body: { ok: true, count: null, matches: 0 },
  },
  [`POST ${COLLECTOR}/collect`]: { status: 200, body: { ok: true, appended: 1, total: 1 } },
};

// Absolute, whatever the run's cwd: this file lives three levels under
// the repository root.
function evidencePath(name: string): string {
  return new URL(`../../../docs/evidence/perfmon/${name}`, import.meta.url).pathname;
}

test("the monitor screen, slowest call on top, in both appearances", async () => {
  // Entries are relative to this file's directory; the app root is the
  // demo's, whose nsh.json grants the loopback network.
  const app = await open("../App.tsx", { colorScheme: "light", network: collectorRoutes });

  await app.press("id:row-monitor");
  await app.waitFor("id:page-monitor");
  // The screen measures on mount. A real target runs on the real clock,
  // so the slow route's duration is a real ~1200 ms, not an exact one;
  // the re-sorted list puts it on top whatever its exact value.
  await app.waitFor("id:call-0 >> text:/\\/slow/");
  await app.waitFor("text:/[1-9][0-9]{3} ms/");
  // The fast routes answer in single-digit milliseconds.
  await app.waitFor("text:/[1-9] ms/");
  expect(app.count("id:call-0")).toBe(1);
  // The filters are visible and reversible: Errors narrows to the
  // failures (none here), which is a filtered empty — the ring still
  // holds the calls — and All brings the rows back.
  await app.press("id:filter-errors");
  await app.waitFor("text:No calls match this filter.");
  await app.press("id:filter-all");
  await app.waitFor("id:call-0 >> text:/\\/slow/");
  await app.screenshot(evidencePath("monitor-light.png"));

  await app.setColorScheme("dark");
  await app.screenshot(evidencePath("monitor-dark.png"));
});
