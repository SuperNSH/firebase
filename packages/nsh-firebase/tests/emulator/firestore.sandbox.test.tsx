// The labelled sandbox test against the Firestore emulator on
// 127.0.0.1:18080 (project demo-nsh, started by
// `bash scripts/start-emulator.sh`, run by `bash scripts/test-emulator.sh`):
// set a doc, get it back, addDoc, runQuery with a where filter,
// deleteDoc, and getDoc answering exists=false. It talks to the network
// for real (`network: "live"`, 127.0.0.1 only per tests/nsh.json);
// nothing in it runs under the headless unit gates.
import { open, test } from "nsh/test";
import type { App } from "nsh/test";

// A nested entry would make `nsh` treat tests/ as the app.
const ENTRY = "../firestore-app.tsx";

test("writes, reads, queries and deletes against the emulator", async () => {
  const app: App = await open(ENTRY, { network: "live" });
  await app.waitFor("text:ready");

  // Set and read back through the v1 REST surface.
  await app.press("id:sb-set");
  await app.waitFor("text:sb-set: ok");
  await app.press("id:sb-get");
  await app.waitFor('text:sb-get: exists=true data={"n":1}');

  // addDoc: the server assigns the id, and the facade reads it from the
  // reply's resource name.
  await app.press("id:sb-add");
  await app.waitFor("text:sb-add: ok");

  // runQuery with a where filter on this run's field: the set doc is
  // among the matches (the added doc's n is 2, so it is not).
  await app.press("id:sb-query");
  await app.waitFor("text:sb-query: has-one=true");

  // deleteDoc, then the read back is exists=false, not an error.
  await app.press("id:sb-delete");
  await app.waitFor("text:sb-delete: ok");
  await app.press("id:sb-get-gone");
  await app.waitFor("text:sb-get-gone: exists=false data=none");
});
