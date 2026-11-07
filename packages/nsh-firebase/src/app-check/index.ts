// The App Check facade: the same calls over the native tier and the
// pure tier's debug-token exchange, mirroring
// crates/nsh-firebase-app-check name for name, down to the persisted
// cache record. The native tier routes through the app's tier facade
// with the firebase.appCheck.* handler name the native areas register;
// the pure tier exchanges a debug token registered in a real Firebase
// console and never fakes a token.

import { FirebaseError, getApp, requestText, toFirebaseError } from "../core";
import type { FirebaseApp } from "../core";
import { APP_CHECK_CODES, invalidResponse, needsAttestation, replyError } from "./error";
import { decodeRecord, encodeRecord, storeKey, StorageStore } from "./store";
import type { CachedRecord, KeyValueStore } from "./store";
import { APP_CHECK_BASE, exchangeDebugTokenBody, exchangeDebugTokenUrl, parseTokenReply } from "./wire";

export { APP_CHECK_CODES } from "./error";
export { APP_CHECK_BASE, exchangeDebugTokenUrl } from "./wire";
export { MemoryStore, StorageStore, storeKey } from "./store";
export type { KeyValueStore } from "./store";

// The `firebase.appCheck.*` handler name the native areas register; the
// native tier calls exactly this.
export const NATIVE_GET_TOKEN = "firebase.appCheck.getToken";

export interface AppCheckOptions {
  // The debug token registered in the project's Firebase console (App
  // Check settings). Empty: the debug exchange refuses before any
  // request.
  debugToken?: string;
  // The exchange base (the REST reference's endpoint). The sandbox
  // points it at a local stand-in, never at Google.
  baseUrl?: string;
}

// One token. expiresAtMs is null on the native tier, whose reply carries
// no ttl and whose SDK owns the refresh schedule.
export interface AppCheckToken {
  token: string;
  expiresAtMs: number | null;
}

export class AppCheck {
  private readonly app: FirebaseApp;
  private readonly store: KeyValueStore;
  private readonly debugToken: string;
  private readonly baseUrl: string;
  private cached: CachedRecord | null = null;
  private readonly restored: Promise<void>;

  constructor(app: FirebaseApp, options: AppCheckOptions = {}, store: KeyValueStore = new StorageStore()) {
    this.app = app;
    this.store = store;
    this.debugToken = options.debugToken ?? "";
    this.baseUrl = options.baseUrl ?? APP_CHECK_BASE;
    // The constructor's restore, awaited before the first cache read:
    // the store is async here, while the Rust restore is sync.
    this.restored = this.restore();
  }

  // Re-reads the store into the cache. A record the other tier (or an
  // older launch) wrote restores too; a corrupt one is no cache.
  async restore(): Promise<void> {
    const text = await this.store.get(storeKey(this.app.name));
    this.cached = text === null ? null : decodeRecord(text);
  }

  // The token, from the cache while it lives and from the debug exchange
  // once past its expiry on the app's clock (a manual clock in tests).
  // The native tier routes the contract call instead; debug only names
  // the debug provider there.
  async getToken(call: { debug?: boolean } = {}): Promise<AppCheckToken> {
    if (this.app.tier.kind === "native") {
      const reply = (await this.call(NATIVE_GET_TOKEN, {
        debug: call.debug === true,
      })) as { token?: unknown } | null;
      if (typeof reply?.token !== "string" || reply.token === "") {
        throw invalidResponse(`\`${NATIVE_GET_TOKEN}\` answered without a token`);
      }
      return { token: reply.token, expiresAtMs: null };
    }
    if (call.debug !== true) throw needsAttestation();
    if (this.debugToken.trim() === "") {
      throw new FirebaseError(
        APP_CHECK_CODES.invalidArgument,
        "the debug token is empty: register one in the Firebase console and pass it to " +
          "the App Check options",
      );
    }
    await this.restored;
    const now = Date.now();
    if (this.cached !== null && now < this.cached.expiresAtMs) {
      return { token: this.cached.token, expiresAtMs: this.cached.expiresAtMs };
    }
    const response = await requestText(
      exchangeDebugTokenUrl(
        this.baseUrl,
        this.app.options.projectId,
        this.app.options.appId,
        this.app.options.apiKey,
      ),
      { method: "POST", headers: { "Content-Type": "application/json" }, body: exchangeDebugTokenBody(this.debugToken) },
    );
    // A transport failure keeps the core's stable codes (requestText
    // throws them); only a server reply maps to an app-check code.
    if (!response.ok) throw replyError(response.status, response.text);
    const reply = parseTokenReply(response.text);
    const expiresAtMs = now + reply.ttlMs;
    const record: CachedRecord = { token: reply.token, expiresAtMs };
    this.cached = record;
    await this.store.set(storeKey(this.app.name), encodeRecord(record));
    return { token: reply.token, expiresAtMs };
  }

  private async call(name: string, args: unknown): Promise<unknown> {
    try {
      return await this.app.tier.call(name, args);
    } catch (error) {
      throw toFirebaseError(error);
    }
  }
}

// The default app's App Check. Instances hold the cache, so build one
// and keep it, as with Auth.
export function getAppCheck(app: FirebaseApp = getApp(), options: AppCheckOptions = {}): AppCheck {
  return new AppCheck(app, options);
}
