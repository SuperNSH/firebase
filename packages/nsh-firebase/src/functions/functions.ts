// The Functions API of one app: httpsCallable over the pure tier's
// fetch, or native routing through the firebase.functions.call handler
// the embedding host registers. Mirrors
// crates/nsh-firebase-functions/src/functions.rs name for name.

import type { FirebaseApp, FirebaseTier } from "../core";
import { CODES, FirebaseError, toFirebaseError } from "../core/error";
import { requestText } from "../core/http";
import {
  callableUrl,
  defaultFunctionsConfig,
  resultFromResponse,
  FUNCTIONS_EMULATOR_PORT,
  NATIVE_CALL,
  type FunctionsConfig,
  type FunctionsEmulatorEndpoint,
} from "./wire";

/** Read again before every call, so a refreshed token is honored. */
export type TokenProvider = () => string | null;

/** One named callable, as httpsCallable(name) hands it out. */
export type HttpsCallable = (data?: unknown) => Promise<unknown>;

/** The Cloud Functions API of one app. */
export class Functions {
  private readonly tier: FirebaseTier;
  private readonly projectId: string;
  private config: Required<Pick<FunctionsConfig, "region">> & FunctionsConfig;
  private auth: TokenProvider | null = null;

  /** The functions of `app`, on the sandbox default: the emulator at
   * 127.0.0.1:15001, region us-central1. */
  constructor(app: FirebaseApp, config?: FunctionsConfig) {
    this.tier = app.tier;
    this.projectId = app.options.projectId;
    const defaults = defaultFunctionsConfig();
    this.config = {
      region: config?.region ?? defaults.region,
      emulator: config && "emulator" in config ? config.emulator : defaults.emulator,
    };
  }

  /** Supplies `Authorization: Bearer <idToken>` for callables that
   * check auth (the SDK's context header). */
  withAuth(auth: TokenProvider): Functions {
    this.auth = auth;
    return this;
  }

  /** Points calls at the Functions emulator (the SDK's
   * connectFunctionsEmulator); a missing host is 127.0.0.1. */
  connectFunctionsEmulator(host?: string, port: number = FUNCTIONS_EMULATOR_PORT): void {
    const endpoint: FunctionsEmulatorEndpoint = { port };
    if (host !== undefined) endpoint.host = host;
    this.config.emulator = endpoint;
  }

  /** The URL a call to `name` would use right now. */
  callableUrl(name: string): string {
    return callableUrl(this.config, this.projectId, name);
  }

  /** httpsCallable(name): the callable reference. */
  httpsCallable(name: string): HttpsCallable {
    return (data?: unknown) => this.call(name, data);
  }

  /** Calls `name` with `data`: through the host handler when one is
   * wired ("when the host answers"), otherwise a plain POST. */
  async call(name: string, data?: unknown): Promise<unknown> {
    if (this.tier.kind === "native") return this.callNative(name, data ?? null);
    return this.callOverFetch(name, data);
  }

  /** The native contract (coordinator-fixed): the handler takes
   * `{name, data}` and answers `{result}`; a refusal or a handler
   * without a result surfaces as its mapped error. */
  private async callNative(name: string, data: unknown): Promise<unknown> {
    try {
      const payload = await this.tier.call(NATIVE_CALL, { name, data });
      if (payload && typeof payload === "object" && "result" in (payload as object)) {
        return (payload as { result: unknown }).result;
      }
      throw new FirebaseError(CODES.hostError, "firebase.functions.call returned no `result`");
    } catch (error) {
      throw toFirebaseError(error);
    }
  }

  private async callOverFetch(name: string, data: unknown): Promise<unknown> {
    // The wire's data field is always present; absent caller data is
    // null (the SDK's encode).
    const body = JSON.stringify({ data: data === undefined ? null : data });
    const headers: Record<string, string> = { "Content-Type": "application/json" };
    const token = this.auth?.();
    if (token) headers.Authorization = `Bearer ${token}`;
    const response = await requestText(this.callableUrl(name), {
      method: "POST",
      headers,
      body,
    });
    return resultFromResponse(response.status, response.text);
  }
}
