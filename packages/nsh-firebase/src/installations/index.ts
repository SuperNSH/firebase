// Firebase Installations for Native Shell TSX apps, over the
// Installations REST (docs/research.md §4). A locally generated FID (the
// js-SDK's 22-character base64url shape), the register call,
// authTokens:generate with the FIS_v2 refresh scheme, {fid, refreshToken}
// persisted behind KeyValueStore and restored on init, and the auth
// token cached in memory with the js-SDK's one-hour expiry margin. No
// emulator serves this product and a fake demo- project is refused by
// the real endpoint, so the sandbox proof is the request/response shape
// against a stubbed network plus the clear installations/request-failed
// mapping — the same honest criterion the messaging area proved for its
// token request. Mirrors crates/nsh-firebase-installations behavior name
// for name, down to the persisted JSON.

import { CODES, FirebaseError, getApp, requestText, toFirebaseError } from "../core";
import type { FirebaseApp, FirebaseTier } from "../core";

import { INSTALLATIONS_CODES, replyError } from "./error";
import { generateFid } from "./fid";
import {
  decodeInstallation,
  encodeInstallation,
  storeKey,
  StorageStore,
} from "./store";
import type { KeyValueStore, StoredInstallation } from "./store";
import {
  GET_ID,
  GET_TOKEN,
  parseRegisterReply,
  parseTokenReply,
  registerRequest,
  tokenRequest,
} from "./wire";

export { INSTALLATIONS_CODES, replyError } from "./error";
export {
  MemoryStore,
  StorageStore,
  storeKey,
  encodeInstallation,
  decodeInstallation,
} from "./store";
export type { KeyValueStore, StoredInstallation } from "./store";
export { VALID_FID_PATTERN, generateFid, isValidFid } from "./fid";
export {
  GET_ID,
  GET_TOKEN,
  INSTALLATIONS_API_URL,
  INTERNAL_AUTH_VERSION,
  SDK_VERSION,
  installationsEndpoint,
  parseRegisterReply,
  parseTokenReply,
  registerRequest,
  tokenRequest,
} from "./wire";
export type { AuthToken, RegisterReply, WireRequest } from "./wire";

// A cached token goes stale this long before its expiry, like the
// js-SDK's one-hour TOKEN_EXPIRATION_BUFFER.
export const TOKEN_EXPIRATION_BUFFER_MS = 60 * 60 * 1000;

// The installations of one app. The constructor restores the persisted
// record, so id() holds it once the promise restored() resolves (the
// store is async here; the Rust restore is sync, where restore_ready()
// answers the same question).
export class Installations {
  private readonly app: FirebaseApp;
  private readonly store: KeyValueStore;
  private readonly tier: FirebaseTier;
  private readonly ready: Promise<void>;
  private stored: StoredInstallation | null = null;
  private cached: { token: string; expiresAtMs: number } | null = null;
  private registering = false;

  constructor(
    app: FirebaseApp,
    store: KeyValueStore = new StorageStore(),
    tier: FirebaseTier = app.tier,
  ) {
    this.app = app;
    this.store = store;
    this.tier = tier;
    this.ready = this.restore();
  }

  // Resolves once the constructor's initial restore finished.
  restored(): Promise<void> {
    return this.ready;
  }

  // Re-reads the store into the in-memory record. A record the other
  // tier (or an older launch) wrote restores too; a corrupt one is
  // treated as unregistered.
  async restore(): Promise<void> {
    const text = await this.store.get(storeKey(this.app.name));
    this.stored = text === null ? null : decodeInstallation(text);
  }

  // The registered FID, null while unregistered (the pure tier's record;
  // the native tier's SDK id is not persisted here).
  id(): string | null {
    return this.stored?.fid ?? null;
  }

  // The refresh token of the registered installation, null while
  // unregistered.
  refreshToken(): string | null {
    return this.stored?.refreshToken ?? null;
  }

  // The installation id: registers on first use in the pure tier, routes
  // through the host in the native tier. Under a fake demo- project the
  // register call refuses server-side and this rejects with a clear
  // installations/request-failed.
  async getId(): Promise<string> {
    if (this.tier.kind === "native") {
      return this.callHost(GET_ID, "id", "an id");
    }
    await this.ready;
    if (this.stored !== null) return this.stored.fid;
    return (await this.register()).fid;
  }

  // An installation auth token: the cached one while it stays fresh past
  // TOKEN_EXPIRATION_BUFFER_MS, a minted one on forceRefresh or expiry.
  // Routes through the host in the native tier (no args, no cache there:
  // the SDK keeps its own).
  async getToken(forceRefresh = false): Promise<string> {
    if (this.tier.kind === "native") {
      return this.callHost(GET_TOKEN, "token", "a token");
    }
    await this.ready;
    if (this.stored === null) {
      await this.register();
    }
    if (
      !forceRefresh &&
      this.cached !== null &&
      Date.now() < this.cached.expiresAtMs - TOKEN_EXPIRATION_BUFFER_MS
    ) {
      return this.cached.token;
    }
    const stored = this.stored;
    if (stored === null) {
      throw new FirebaseError(
        INSTALLATIONS_CODES.invalidResponse,
        "the registration vanished mid-flight; register again",
      );
    }
    const request = tokenRequest(
      this.app.options.projectId,
      this.app.options.apiKey,
      this.app.options.appId,
      stored.fid,
      stored.refreshToken,
    );
    const response = await requestText(request.url, {
      method: "POST",
      headers: request.headers,
      body: request.body,
    });
    const auth = this.parseReply(
      "Generate Auth Token",
      response.status,
      response.text,
      parseTokenReply,
    );
    this.cached = { token: auth.token, expiresAtMs: Date.now() + auth.expiresInMs };
    return auth.token;
  }

  // Whether the cached token reaches its refresh point on the clock's
  // time now (or there is none). The schedule-check: call it whenever
  // the app chooses and getToken() when it answers true.
  tokenDue(): boolean {
    return this.cached === null || Date.now() >= this.cached.expiresAtMs - TOKEN_EXPIRATION_BUFFER_MS;
  }

  // When the cached token expires, in wall-clock milliseconds; null
  // while nothing is cached.
  tokenExpiresAtMs(): number | null {
    return this.cached?.expiresAtMs ?? null;
  }

  // POST projects/{projectId}/installations with a generated FID,
  // persisting {fid, refreshToken} and caching the reply's auth token.
  // One runs at a time: a second concurrent call rejects with
  // installations/register-in-progress.
  private async register(): Promise<StoredInstallation> {
    if (this.registering) {
      throw new FirebaseError(
        INSTALLATIONS_CODES.registerInProgress,
        "an installation registration is already in flight; wait for it",
      );
    }
    this.registering = true;
    try {
      const fid = generateFid();
      const request = registerRequest(
        this.app.options.projectId,
        this.app.options.apiKey,
        this.app.options.appId,
        fid,
      );
      const response = await requestText(request.url, {
        method: "POST",
        headers: request.headers,
        body: request.body,
      });
      const { stored, auth } = this.parseReply(
        "Create Installation",
        response.status,
        response.text,
        (body) => parseRegisterReply(body, fid),
      );
      this.stored = stored;
      await this.store.set(storeKey(this.app.name), encodeInstallation(stored));
      this.cached = { token: auth.token, expiresAtMs: Date.now() + auth.expiresInMs };
      return stored;
    } finally {
      this.registering = false;
    }
  }

  // A transport failure keeps the core's stable codes (requestText
  // throws them); only a server reply maps to an installations code.
  private parseReply<T>(
    requestName: string,
    status: number,
    body: string,
    parse: (body: string) => T,
  ): T {
    if (status < 200 || status >= 300) {
      throw replyError(requestName, status, body);
    }
    return parse(body);
  }

  // The native tier's path: the host answers {id} or {token}; anything
  // else (a missing field, a refusal, a failed handler) is the host's
  // own error, unchanged — the fake project's rejection rides it.
  private async callHost(name: string, field: string, noun: string): Promise<string> {
    let reply: unknown;
    try {
      reply = await this.tier.call(name);
    } catch (error) {
      throw toFirebaseError(error);
    }
    const value = (reply as Record<string, unknown> | null)?.[field];
    if (typeof value !== "string" || value === "") {
      throw new FirebaseError(CODES.hostError, `\`${name}\` answered without ${noun}`);
    }
    return value;
  }
}

// One installations per app: the cache and the restore read live in the
// instance, so calling this again hands back the same one (what RNFB's
// getOrCreateModularInstance does).
const byApp = new Map<string, Installations>();

export function getInstallations(app: FirebaseApp = getApp()): Installations {
  const existing = byApp.get(app.name);
  if (existing) return existing;
  const created = new Installations(app);
  byApp.set(app.name, created);
  return created;
}
