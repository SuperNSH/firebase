// Firebase Auth for Native Shell TSX apps, over the Identity Toolkit and
// Secure Token REST the emulator suite also serves (docs/research.md
// §3). Email+password sign-up and sign-in, anonymous, custom token, a
// credential from an ID token another library minted, a clock-based
// refresh schedule (no hidden timers), the currentUser signal, and
// persistence behind KeyValueStore. Mirrors
// crates/nsh-firebase-auth/src/auth.rs behavior name for name, down to
// the persisted JSON.

import { signal } from "nsh";

import { authBaseUrl, CODES, FirebaseError, requestText } from "../core";
import type { FirebaseApp } from "../core";
import { isSessionLost, wireErrorMessage, wireError, AUTH_CODES } from "./error";
import { decodeSession, encodeSession, KeyValueStore, storeKey, StorageStore } from "./store";
import { identityUrl, parseSessionReply, refreshFormBody, refreshUrl, uidFromIdToken } from "./wire";
import type { WireSession } from "./wire";
import type { AuthUser, Credential } from "./user";

export { AUTH_CODES, isSessionLost } from "./error";
export { MemoryStore, StorageStore, storeKey } from "./store";
export type { KeyValueStore } from "./store";
export type { AuthUser, Credential } from "./user";
export { formEncode, identityUrl, refreshUrl, uidFromIdToken } from "./wire";

// Refreshing starts this long before expiry, like the Firebase SDKs'
// five-minute margin.
export const REFRESH_MARGIN_MS = 5 * 60 * 1000;

// The auth for one app. The constructor restores the persisted session,
// so currentUser holds it from the first read; authStateReady resolves
// once that initial restore finished (the store is async here; the Rust
// restore is sync, where state_ready() answers the same question).
export class Auth {
  // The currentUser signal: null while signed out. A binding that reads
  // it updates through every sign-in, refresh and sign-out.
  readonly currentUser = signal<AuthUser | null>(null);

  private readonly app: FirebaseApp;
  private readonly store: KeyValueStore;
  private refreshing = false;
  private readonly ready: Promise<void>;

  constructor(app: FirebaseApp, store: KeyValueStore = new StorageStore()) {
    this.app = app;
    this.store = store;
    this.ready = this.restore();
  }

  // Resolves once the constructor's initial restore finished.
  authStateReady(): Promise<void> {
    return this.ready;
  }

  // Re-reads the store into currentUser. A record the other tier (or an
  // older launch) wrote restores too; a corrupt one is signed out.
  async restore(): Promise<void> {
    const text = await this.store.get(storeKey(this.app.name));
    this.currentUser.set(text === null ? null : decodeSession(text));
  }

  // Creates the account (accounts:signUp) and signs in with it.
  async createUserWithEmailAndPassword(email: string, password: string): Promise<AuthUser> {
    requireText(email, "email");
    requireText(password, "password");
    const session = await this.postIdentity("accounts:signUp", {
      email,
      password,
      returnSecureToken: true,
    });
    return this.adopt(session, false);
  }

  // Signs in with email and password (accounts:signInWithPassword).
  async signInWithEmailAndPassword(email: string, password: string): Promise<AuthUser> {
    requireText(email, "email");
    requireText(password, "password");
    const session = await this.postIdentity("accounts:signInWithPassword", {
      email,
      password,
      returnSecureToken: true,
    });
    return this.adopt(session, false);
  }

  // Signs in anonymously: accounts:signUp with no email and no password,
  // the shape the docs give anonymous sign-up (and the emulator serves);
  // there is no accounts:signInAnonymously.
  async signInAnonymously(): Promise<AuthUser> {
    const session = await this.postIdentity("accounts:signUp", { returnSecureToken: true });
    return this.adopt(session, true);
  }

  // Exchanges a custom token for a session (accounts:signInWithCustomToken).
  async signInWithCustomToken(token: string): Promise<AuthUser> {
    requireText(token, "token");
    const session = await this.postIdentity("accounts:signInWithCustomToken", {
      token,
      returnSecureToken: true,
    });
    return this.adopt(session, false);
  }

  // Signs in with a credential: a password pair, a custom token, or an ID
  // token another library minted for this project (sent to
  // accounts:verifyCustomToken).
  async signInWithCredential(credential: Credential): Promise<AuthUser> {
    switch (credential.kind) {
      case "password":
        return this.signInWithEmailAndPassword(credential.email, credential.password);
      case "custom-token":
        return this.signInWithCustomToken(credential.token);
      case "id-token": {
        requireText(credential.idToken, "idToken");
        const session = await this.postIdentity("accounts:verifyCustomToken", {
          token: credential.idToken,
          returnSecureToken: true,
        });
        return this.adopt(session, false);
      }
    }
  }

  // Whether the current user's token reaches its refresh point on the
  // clock's time now. The schedule-check: call it whenever the app
  // chooses (on start, on a screen, on a timer it owns) and refreshNow()
  // when it answers true.
  refreshDue(): boolean {
    const user = this.currentUser();
    return user !== null && Date.now() >= user.nextRefreshAtMs;
  }

  // When the next refresh is due, in wall-clock milliseconds; null while
  // signed out.
  nextRefreshAtMs(): number | null {
    return this.currentUser()?.nextRefreshAtMs ?? null;
  }

  // Refreshes now (securetoken.googleapis.com/v1/token), whatever the
  // schedule says. One runs at a time: a second concurrent call rejects
  // with auth/refresh-in-progress. A session the server has lost
  // (auth/user-token-expired, auth/invalid-refresh-token,
  // auth/user-not-found) signs the user out: it cannot recover.
  async refreshNow(): Promise<AuthUser> {
    if (this.refreshing) {
      throw new FirebaseError(
        AUTH_CODES.refreshInProgress,
        "a token refresh is already in flight; wait for it",
      );
    }
    this.refreshing = true;
    try {
      const current = this.currentUser();
      if (current === null) {
        throw new FirebaseError(
          AUTH_CODES.noCurrentUser,
          "no user is signed in; nothing to refresh",
        );
      }
      const base = this.authBaseUrl();
      let session: WireSession;
      try {
        const response = await requestText(refreshUrl(base, this.app.options.apiKey), {
          method: "POST",
          headers: { "Content-Type": "application/x-www-form-urlencoded" },
          body: refreshFormBody(current.refreshToken),
        });
        session = this.parseReply(response.status, response.text, true);
      } catch (error) {
        const err = error instanceof FirebaseError ? error : new FirebaseError(CODES.network, String(error));
        if (isSessionLost(err.code)) {
          // The server no longer knows this session; keeping the user
          // would fail every API call.
          this.currentUser.set(null);
          await this.persist(null);
        }
        throw err;
      }
      // The refresh reply names no email and may name no uid and no new
      // refresh token; those carry over from the session it renewed.
      const now = Date.now();
      const expiresAtMs = now + session.expiresInSecs * 1000;
      const user: AuthUser = {
        uid: session.localId ?? uidFromIdToken(session.idToken) ?? current.uid,
        email: current.email,
        isAnonymous: current.isAnonymous,
        idToken: session.idToken,
        refreshToken: session.refreshToken === "" ? current.refreshToken : session.refreshToken,
        expiresAtMs,
        // Mirrors the Rust u64 saturating_sub: a sub-margin expiry clamps
        // at 0 the same way on both tiers.
        nextRefreshAtMs: Math.max(0, expiresAtMs - REFRESH_MARGIN_MS),
      };
      this.currentUser.set(user);
      await this.persist(user);
      return user;
    } finally {
      this.refreshing = false;
    }
  }

  // Signs out: the signal clears and the record leaves the store. These
  // providers have nothing to revoke server-side.
  async signOut(): Promise<void> {
    this.currentUser.set(null);
    await this.persist(null);
  }

  // Puts a fresh session in the signal and the store.
  private async adopt(session: WireSession, isAnonymous: boolean): Promise<AuthUser> {
    const uid = session.localId ?? uidFromIdToken(session.idToken);
    if (uid === null) {
      throw new FirebaseError(
        AUTH_CODES.invalidResponse,
        "neither the reply nor the idToken names the user",
      );
    }
    const now = Date.now();
    const expiresAtMs = now + session.expiresInSecs * 1000;
    const user: AuthUser = {
      uid,
      email: session.email,
      isAnonymous,
      idToken: session.idToken,
      refreshToken: session.refreshToken,
      expiresAtMs,
      // Mirrors the Rust u64 saturating_sub: a sub-margin expiry clamps
      // at 0 the same way on both tiers.
      nextRefreshAtMs: Math.max(0, expiresAtMs - REFRESH_MARGIN_MS),
    };
    this.currentUser.set(user);
    await this.persist(user);
    return user;
  }

  private async persist(user: AuthUser | null): Promise<void> {
    const key = storeKey(this.app.name);
    if (user === null) {
      await this.store.remove(key);
    } else {
      await this.store.set(key, encodeSession(user));
    }
  }

  private async postIdentity(operation: string, body: Record<string, unknown>): Promise<WireSession> {
    const base = this.authBaseUrl();
    const response = await requestText(identityUrl(base, this.app.options.apiKey, operation), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return this.parseReply(response.status, response.text, false);
  }

  // A transport failure keeps the core's stable codes (requestText throws
  // them); only a server reply maps to an auth code.
  private parseReply(status: number, text: string, acceptAccessToken: boolean): WireSession {
    if (status < 200 || status >= 300) {
      const wire = wireErrorMessage(text);
      if (wire !== null) throw wireError(status, wire);
      if (status >= 500) {
        throw new FirebaseError(AUTH_CODES.internalError, `the auth server failed (HTTP ${status})`);
      }
      throw new FirebaseError(
        AUTH_CODES.requestFailed,
        `the server replied ${status} without an error message`,
      );
    }
    return parseSessionReply(text, acceptAccessToken);
  }

  private authBaseUrl(): string {
    const base = authBaseUrl(this.app.options);
    if (base === null) {
      throw new FirebaseError(
        CODES.unavailable,
        "auth has no base URL: set authUrl or the auth emulator switch in the app options",
      );
    }
    return base;
  }
}

function requireText(value: string, what: string): void {
  if (value.trim() === "") {
    throw new FirebaseError(AUTH_CODES.invalidArgument, `${what} is empty`);
  }
}
