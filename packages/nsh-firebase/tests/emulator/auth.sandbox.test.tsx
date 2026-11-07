// SANDBOX: runs only by scripts/test-emulator.sh, against the emulator
// suite on 127.0.0.1 (demo-nsh; no real Firebase account). The network
// is live for the app, so the last step asserts every request the app
// made stayed on the emulator's port: the flows build their URLs from
// the app options, and this check holds them to it.
import { describe, expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

// The app root stays the package (its nsh.json); the entry is this
// directory's neighbor.
const ENTRY = "../auth-app.tsx";
const EMULATOR = "http://127.0.0.1:19099/";

describe("auth sandbox (emulator suite)", () => {
  test("signs up, signs in, refreshes and reads the user back", async () => {
    const app: App = await open(ENTRY, { network: "live" });
    // A fresh suite signs up; a running one has the account, so the
    // flow falls through to sign-in.
    await app.press("id:signup");
    await app.waitFor("text:/signup: (ok|exists)/");
    await app.press("id:signin");
    await app.waitFor("text:/signin: ok .+/");
    await app.waitFor("text:/user: .+ email=demo@demo-nsh.test .+/");
    // Refresh now, ahead of the schedule: Secure Token answers with a
    // fresh session for the same user.
    await app.press("id:refresh-now");
    await app.waitFor("text:/refresh: ok .+/");
    // A token no one minted: the refusal surfaces live with the stable
    // code, not an opaque HTTP error.
    await app.press("id:custom");
    await app.waitFor("text:/custom: auth\\/invalid-custom-token/");
    // Restore-on-init over the real Storage: a fresh Auth reads the
    // record the sign-in wrote.
    await app.press("id:reauth");
    await app.waitFor("text:/restored: user: .+ email=demo@demo-nsh.test .+/");
    // The live network never left the emulator.
    for (const request of app.requests()) {
      expect(request.url.startsWith(EMULATOR)).toBe(true);
    }
  });
});
