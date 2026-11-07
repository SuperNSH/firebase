// Headless tests of the auth flows: every step goes through the exercise
// UI on a manual clock (no real time) and a stubbed network (no real
// requests), with the wire replies the emulator and the real services
// give. Mirrors crates/nsh-firebase-auth/tests/auth_ui.rs. The sandbox
// run against the live emulator suite is tests/emulator/auth.sandbox.test.tsx,
// run by scripts/test-emulator.sh only.
import { describe, expect, open, test } from "nsh/test";
import type { App, StubRequest } from "nsh/test";

// The manual clock's fixed instant; every expected timestamp derives
// from it.
const T0 = 1_767_258_000_000;
const SIGN_UP_URL =
  "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/accounts:signUp?key=demo-nsh-fake-api-key";
const SIGN_IN_URL =
  "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/accounts:signInWithPassword?key=demo-nsh-fake-api-key";
const CUSTOM_URL =
  "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/accounts:signInWithCustomToken?key=demo-nsh-fake-api-key";
const VERIFY_URL =
  "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/accounts:verifyCustomToken?key=demo-nsh-fake-api-key";
const REFRESH_URL =
  "http://127.0.0.1:19099/securetoken.googleapis.com/v1/token?key=demo-nsh-fake-api-key";
const STORE_KEY = "nsh.firebase.auth.[DEFAULT]";

// The entry is this file's neighbor; the app root stays the package
// (its nsh.json), which the entry's parent directory decides.
const ENTRY = "./auth-app.tsx";

// A stub reply: error (no response at all) instead of a body means the
// request fails, which is what the network-refused test hands out.
interface Route {
  status: number;
  body?: string;
  error?: string;
}

function okSession(
  idToken: string,
  refreshToken: string,
  email: string,
  uid: string,
): Route {
  return {
    status: 200,
    body: JSON.stringify({
      idToken,
      refreshToken,
      expiresIn: "3600",
      email,
      localId: uid,
      registered: true,
    }),
  };
}

function wireError(wire: string): Route {
  return {
    status: 400,
    body: JSON.stringify({ error: { code: 400, message: wire } }),
  };
}

// A JWT whose payload carries the claims; the code under test reads only
// the payload, so header and signature are fixed text.
function jwt(claims: Record<string, unknown>): string {
  const table = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
  const encode = (text: string): string => {
    const bytes = new TextEncoder().encode(text);
    let out = "";
    for (let at = 0; at < bytes.length; at += 3) {
      const b = [bytes[at], bytes[at + 1] ?? 0, bytes[at + 2] ?? 0];
      const n = (b[0] << 16) | (b[1] << 8) | b[2];
      out += table[(n >> 18) & 63];
      out += table[(n >> 12) & 63];
      if (at + 1 < bytes.length) out += table[(n >> 6) & 63];
      if (at + 2 < bytes.length) out += table[n & 63];
    }
    return out;
  };
  return `header.${encode(JSON.stringify(claims))}.signature`;
}

function sentRequests(app: App): string[] {
  return app.requests().map((request: StubRequest) => `${request.method} ${request.url} body=${request.body ?? ""}`);
}

async function started(
  network: Record<string, Route | ((request: StubRequest) => Route)>,
  storage?: Record<string, string>,
): Promise<App> {
  return open(ENTRY, { clock: { time: T0 }, network, storage: storage ?? null });
}

describe("auth", () => {
  test("sign up signs in and persists the record on the manual clock", async () => {
    const app = await started({
      [`POST ${SIGN_UP_URL}`]: () => okSession("id-token-1", "refresh-1", "demo@demo-nsh.test", "uid-1"),
    });
    await app.press("id:signup");
    await app.waitFor("text:signup: ok uid-1");
    await app.waitFor(
      "text:user: uid-1 email=demo@demo-nsh.test anon=false token=id-token-1 refresh=refresh-1 " +
        `expires=${T0 + 3_600_000} next=${T0 + 3_600_000 - 300_000}`,
    );
    // What the flow sent: JSON with returnSecureToken, the API key in the
    // query, the emulator path under the base URL.
    const sent = sentRequests(app);
    expect(sent.length).toBe(1);
    expect(sent[0]).toMatch(`POST ${SIGN_UP_URL} `);
    expect(sent[0]).toMatch('"email":"demo@demo-nsh.test"');
    expect(sent[0]).toMatch('"returnSecureToken":true');
    // The record the store now holds is what a later launch restores.
    await app.press("id:reauth");
    await app.waitFor(
      "text:restored: user: uid-1 email=demo@demo-nsh.test anon=false token=id-token-1 refresh=refresh-1 " +
        `expires=${T0 + 3_600_000} next=${T0 + 3_600_000 - 300_000}`,
    );
  });

  test("sign in maps the wire errors and keeps the user signed out", async () => {
    const cases: [string, string, string][] = [
      ["EMAIL_NOT_FOUND", "auth/user-not-found", "no account with that email"],
      ["INVALID_PASSWORD", "auth/wrong-password", "that password is wrong for the account"],
    ];
    for (const [wire, stable, human] of cases) {
      const app = await started({ [`POST ${SIGN_IN_URL}`]: () => wireError(wire) });
      await app.press("id:signin");
      await app.waitFor(`text:signin: ${stable}: ${human} (${wire})`);
      await app.waitFor("text:user: none");
    }
  });

  test("empty arguments fail before any request is made", async () => {
    const app = await started({
      [`POST ${SIGN_IN_URL}`]: () => ({ status: 200, body: "the stub must never answer" }),
    });
    await app.press("id:empty");
    await app.waitFor("text:empty: auth/invalid-argument: email is empty");
  });

  test("anonymous sign in sends no email and no password", async () => {
    const app = await started({
      [`POST ${SIGN_UP_URL}`]: () => ({
        status: 200,
        body: JSON.stringify({
          idToken: "anon-token",
          refreshToken: "anon-refresh",
          expiresIn: "3600",
          localId: "anon-uid",
        }),
      }),
    });
    await app.press("id:anon");
    await app.waitFor("text:anon: ok anon-uid");
    await app.waitFor(
      "text:user: anon-uid email=none anon=true token=anon-token refresh=anon-refresh " +
        `expires=${T0 + 3_600_000} next=${T0 + 3_600_000 - 300_000}`,
    );
    const sent = sentRequests(app);
    expect(sent.length).toBe(1);
    // The anonymous shape: returnSecureToken only, no credentials.
    expect(sent[0]).toMatch('"returnSecureToken":true');
    expect(sent[0].includes("password")).toBe(false);
  });

  test("a custom token signs in with the uid from the token claims", async () => {
    const token = jwt({ sub: "custom-uid", provider_id: "custom" });
    const app = await started({
      [`POST ${CUSTOM_URL}`]: () => ({
        status: 200,
        body: JSON.stringify({ idToken: token, refreshToken: "custom-refresh", expiresIn: "3600" }),
      }),
    });
    await app.press("id:custom");
    await app.waitFor("text:custom: ok custom-uid");
    // No email on a custom-token session: the uid came from the sub claim.
    await app.waitFor(
      `text:user: custom-uid email=none anon=false token=${token} refresh=custom-refresh ` +
        `expires=${T0 + 3_600_000} next=${T0 + 3_600_000 - 300_000}`,
    );
  });

  test("a credential from a foreign id token posts verify custom token", async () => {
    const app = await started({
      [`POST ${VERIFY_URL}`]: () => ({
        status: 200,
        body: JSON.stringify({
          idToken: jwt({ sub: "foreign-uid", email: "other@library.test" }),
          refreshToken: "foreign-refresh",
          expiresIn: "3600",
        }),
      }),
    });
    await app.press("id:credential");
    await app.waitFor("text:credential: ok foreign-uid");
    const sent = sentRequests(app);
    expect(sent.length).toBe(1);
    expect(sent[0]).toMatch(`POST ${VERIFY_URL} `);
    expect(sent[0]).toMatch('"token":"foreign-id-token"');
  });

  test("the refresh schedule follows the moved clock", async () => {
    const app = await started({
      [`POST ${SIGN_IN_URL}`]: () => okSession("id-token-1", "refresh-1", "demo@demo-nsh.test", "uid-1"),
      [`POST ${REFRESH_URL}`]: () => ({
        status: 200,
        body: JSON.stringify({
          access_token: "id-token-2",
          expires_in: "3600",
          token_type: "Bearer",
          refresh_token: "refresh-2",
          user_id: "uid-1",
          project_id: "demo-nsh",
        }),
      }),
    });
    await app.press("id:signin");
    await app.waitFor("text:signin: ok uid-1");
    // The schedule-check: not due at once, due once the clock moves past
    // the stored absolute refresh time (55 minutes after sign-in).
    await app.press("id:due");
    await app.waitFor("text:due: false");
    app.clock?.setSystemTime(T0 + 3_300_000);
    await app.press("id:due");
    await app.waitFor("text:due: true");
    await app.press("id:refresh-now");
    await app.waitFor("text:refresh: ok uid-1");
    const at = T0 + 3_300_000;
    await app.waitFor(
      "text:user: uid-1 email=demo@demo-nsh.test anon=false token=id-token-2 refresh=refresh-2 " +
        `expires=${at + 3_600_000} next=${at + 3_600_000 - 300_000}`,
    );
    const sent = sentRequests(app);
    expect(sent.length).toBe(2);
    expect(sent[1]).toMatch(`POST ${REFRESH_URL} `);
    expect(sent[1]).toMatch("grant_type=refresh_token");
    expect(sent[1]).toMatch("refresh_token=refresh-1");
  });

  test("a lost session maps to user token expired and signs out", async () => {
    const app = await started({
      [`POST ${SIGN_IN_URL}`]: () => okSession("id-token-1", "refresh-1", "demo@demo-nsh.test", "uid-1"),
      [`POST ${REFRESH_URL}`]: () => wireError("TOKEN_EXPIRED"),
    });
    await app.press("id:signin");
    await app.waitFor("text:signin: ok uid-1");
    app.clock?.setSystemTime(T0 + 3_300_000);
    await app.press("id:refresh-now");
    await app.waitFor(
      "text:refresh: auth/user-token-expired: the session expired server-side; sign in again (TOKEN_EXPIRED)",
    );
    // The session cannot recover, so the user is gone from the signal,
    // the store (a reauth restores nothing), and a further refresh has
    // nothing to work with.
    await app.waitFor("text:user: none");
    expect(app.storage()[STORE_KEY]).toBeUndefined();
    await app.press("id:refresh-now");
    await app.waitFor("text:refresh: auth/no-current-user: no user is signed in; nothing to refresh");
  });

  test("a network failure keeps the core network code", async () => {
    const app = await started({
      [`POST ${SIGN_IN_URL}`]: () => ({ status: 200, error: "the connection was reset" }),
    });
    await app.press("id:signin");
    // The stable code is what the facade owns; the message carries the
    // engine's own failure wording (the Rust stub delivers it raw).
    await app.waitFor("text:signin: core/network: Network request failed: the connection was reset");
  });

  test("sign out clears the signal and the store", async () => {
    const app = await started({
      [`POST ${SIGN_IN_URL}`]: () => okSession("id-token-1", "refresh-1", "demo@demo-nsh.test", "uid-1"),
    });
    await app.press("id:signin");
    await app.waitFor("text:signin: ok uid-1");
    // The record is in the app's storage while signed in.
    expect(app.storage()[STORE_KEY]).toMatch('"uid":"uid-1"');
    await app.press("id:signout");
    await app.waitFor("text:signed out");
    await app.waitFor("text:user: none");
    expect(app.storage()[STORE_KEY]).toBeUndefined();
    // The record left the store with it.
    await app.press("id:reauth");
    await app.waitFor("text:restored: user: none");
    await app.press("id:refresh-now");
    await app.waitFor("text:refresh: auth/no-current-user: no user is signed in; nothing to refresh");
  });

  test("a second auth restores the record another tier wrote", async () => {
    // The camelCase record the Rust side writes, handed to this side
    // verbatim: either tier restores the other's session record.
    const app = await started({}, {
      [STORE_KEY]: JSON.stringify({
        v: 1,
        uid: "stored-uid",
        email: "stored@demo-nsh.test",
        isAnonymous: false,
        idToken: "stored-id-token",
        refreshToken: "stored-refresh",
        expiresAtMs: 1_999_999_999_999,
        nextRefreshAtMs: 1_999_999_699_999,
      }),
    });
    await app.waitFor(
      "text:user: stored-uid email=stored@demo-nsh.test anon=false token=stored-id-token " +
        "refresh=stored-refresh expires=1999999999999 next=1999999699999",
    );
  });

  test("expiresIn parses like the Rust u64: plus sign and u64 ceiling", async () => {
    // "+3600" is a valid u64 FromStr parse; the safe-integer guard
    // rejected it.
    const plus = await started({
      [`POST ${SIGN_IN_URL}`]: () => ({
        status: 200,
        body: JSON.stringify({
          idToken: "t",
          refreshToken: "r",
          expiresIn: "+3600",
          localId: "uid-plus",
        }),
      }),
    });
    await plus.press("id:signin");
    await plus.waitFor(
      `text:user: uid-plus email=none anon=false token=t refresh=r ` +
        `expires=${T0 + 3_600_000} next=${T0 + 3_600_000 - 300_000}`,
    );

    // Exactly u64::MAX is valid in Rust too.
    const max = await started({
      [`POST ${SIGN_IN_URL}`]: () => ({
        status: 200,
        body: JSON.stringify({
          idToken: "t",
          refreshToken: "r",
          expiresIn: "18446744073709551615",
          localId: "uid-max",
        }),
      }),
    });
    await max.press("id:signin");
    await max.waitFor("text:signin: ok uid-max");

    // One past the ceiling is an invalid response, as the Rust parse is.
    const over = await started({
      [`POST ${SIGN_IN_URL}`]: () => ({
        status: 200,
        body: JSON.stringify({
          idToken: "t",
          refreshToken: "r",
          expiresIn: "18446744073709551616",
          localId: "uid-over",
        }),
      }),
    });
    await over.press("id:signin");
    await over.waitFor(
      "text:signin: auth/invalid-response: the reply has no usable expiresIn (a number-as-string)",
    );
  });

  test("a seeded null record restores as signed out, not a crash", async () => {
    // decodeSession("null") must read as a corrupt record: the reauth's
    // authStateReady resolves and the line lands only when restore did
    // not throw.
    const app = await started({}, { [STORE_KEY]: "null" });
    await app.press("id:reauth");
    await app.waitFor("text:restored: user: none");
  });

  test("a deleted user signs out on refresh", async () => {
    // USER_NOT_FOUND reaches refresh only from Secure Token, where it
    // means the account was deleted server-side: the session cannot
    // recover, so the user must not stay signed in.
    const app = await started({
      [`POST ${SIGN_IN_URL}`]: () => okSession("id-token-1", "refresh-1", "demo@demo-nsh.test", "uid-1"),
      [`POST ${REFRESH_URL}`]: () => wireError("USER_NOT_FOUND"),
    });
    await app.press("id:signin");
    await app.waitFor("text:signin: ok uid-1");
    await app.press("id:refresh-now");
    await app.waitFor("text:refresh: auth/user-not-found: no such account on the server (USER_NOT_FOUND)");
    await app.waitFor("text:user: none");
    expect(app.storage()[STORE_KEY]).toBeUndefined();
  });

  test("a non-JSON success reply is an auth/invalid-response", async () => {
    const app = await started({
      [`POST ${SIGN_IN_URL}`]: () => ({ status: 200, body: "<html>busy</html>" }),
    });
    await app.press("id:signin");
    await app.waitFor("text:signin: auth/invalid-response: the reply is not JSON");
    await app.waitFor("text:user: none");
  });

  test("a record with a negative timestamp restores as signed out", async () => {
    // The Rust read is as_u64: a negative (or fractional) timestamp is a
    // corrupt record, not a signed-in user.
    const app = await started({}, {
      [STORE_KEY]: JSON.stringify({
        v: 1,
        uid: "negative-uid",
        email: "n@demo-nsh.test",
        isAnonymous: false,
        idToken: "t",
        refreshToken: "r",
        expiresAtMs: -5,
        nextRefreshAtMs: -6,
      }),
    });
    await app.press("id:reauth");
    await app.waitFor("text:restored: user: none");
  });

  test("a JSON null success reply is an auth/invalid-response too", async () => {
    const app = await started({
      [`POST ${SIGN_IN_URL}`]: () => ({ status: 200, body: "null" }),
    });
    await app.press("id:signin");
    await app.waitFor("text:signin: auth/invalid-response: the reply has no usable idToken");
    await app.waitFor("text:user: none");
  });

  test("an expiresIn of zero is accepted and one past u64 is not", async () => {
    const zero = await started({
      [`POST ${SIGN_IN_URL}`]: () => ({
        status: 200,
        body: JSON.stringify({ idToken: "t", refreshToken: "r", expiresIn: "0", localId: "uid-0" }),
      }),
    });
    await zero.press("id:signin");
    await zero.waitFor(
      `text:user: uid-0 email=none anon=false token=t refresh=r expires=${T0} next=${T0 - 300_000}`,
    );

    const huge = await started({
      [`POST ${SIGN_IN_URL}`]: () => ({
        status: 200,
        body: JSON.stringify({
          idToken: "t",
          refreshToken: "r",
          expiresIn: "99999999999999999999",
          localId: "uid-x",
        }),
      }),
    });
    await huge.press("id:signin");
    await huge.waitFor(
      "text:signin: auth/invalid-response: the reply has no usable expiresIn (a number-as-string)",
    );
    await huge.waitFor("text:user: none");
  });

  test("sign up refuses an already registered email", async () => {
    const app = await started({ [`POST ${SIGN_UP_URL}`]: () => wireError("EMAIL_EXISTS") });
    await app.press("id:signup");
    // The app prints "exists" only when the error carried the stable
    // auth/email-already-in-use code, so this still pins the mapping.
    await app.waitFor("text:signup: exists");
    await app.waitFor("text:user: none");
  });
});
