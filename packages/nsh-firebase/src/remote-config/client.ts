// The remote-config clients, mirroring crates/nsh-firebase-remote-config/
// src/client.rs. RemoteConfig owns the pure tier (defaults, typed reads,
// template activation through a pluggable transport, an honest refusal to
// fetch without one); NativeRemoteConfig does the same work through the
// firebase.remote-config.* handlers an embedding host registers.

import { getApp, type FirebaseApp } from "../core/app";
import { CODES, FirebaseError, toFirebaseError } from "../core/error";
import type { FirebaseTier } from "../core/tier";
import { RC_CODES } from "./codes";
import { parseRemoteConfigTemplate } from "./template";
import type {
  FetchAndActivateResult,
  ParsedRemoteConfigTemplate,
  RemoteConfigClient,
  RemoteConfigConfig,
  RemoteConfigDefaults,
  RemoteConfigStatus,
  RemoteConfigTransport,
} from "./types";

/** The firebase.remote-config.* handler names the native areas register. */
export const NATIVE_SET_DEFAULTS = "firebase.remote-config.setDefaults";
export const NATIVE_GET_DEFAULTS = "firebase.remote-config.getDefaults";
export const NATIVE_FETCH_AND_ACTIVATE = "firebase.remote-config.fetchAndActivate";
export const NATIVE_GET = "firebase.remote-config.get";

// The truthy spellings the web SDK's asBoolean accepts, lowercased.
const SDK_BOOL_TRUE = ["1", "true", "t", "yes", "y", "on"];

// Why the pure tier refuses to fetch, named on the error.
export const OAUTH_MESSAGE =
  "Remote Config fetch needs an OAuth 2.0 access token from a service account " +
  "(scope https://www.googleapis.com/auth/firebase.remoteconfig); the web API key " +
  "cannot authorize firebaseremoteconfig.googleapis.com and the emulator suite has " +
  "no Remote Config service, so nothing was fetched — supply a transport";

function toBoolean(value: string | null): boolean {
  return value === null ? false : SDK_BOOL_TRUE.includes(value.toLowerCase());
}

function toNumber(value: string | null): number {
  if (value === null) return 0;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function stringDefault(value: string | number | boolean): string {
  return typeof value === "string" ? value : String(value);
}

// Rejects anything but a flat map of string, number or boolean — RNFB's
// ConfigDefaults shape — before a value is stored or sent.
function validateDefaults(defaults: RemoteConfigDefaults): void {
  if (typeof defaults !== "object" || defaults === null || Array.isArray(defaults)) {
    throw new FirebaseError(CODES.invalidArgument, "defaults is not an object");
  }
  for (const [key, value] of Object.entries(defaults)) {
    if (
      typeof value !== "string" &&
      typeof value !== "number" &&
      typeof value !== "boolean"
    ) {
      throw new FirebaseError(
        CODES.invalidArgument,
        `defaults.${key} is not a string, number or boolean`,
      );
    }
  }
}

function hostError(handler: string, problem: string): FirebaseError {
  return new FirebaseError(CODES.hostError, `the ${handler} handler returned ${problem}`);
}

function objectOf(result: unknown, handler: string): Record<string, unknown> {
  if (typeof result !== "object" || result === null) {
    throw hostError(handler, "a non-object answer");
  }
  return result as Record<string, unknown>;
}

function mapOf(result: unknown, handler: string): RemoteConfigDefaults {
  const map = objectOf(result, handler)["map"];
  if (typeof map !== "object" || map === null) {
    throw hostError(handler, "no `map` object");
  }
  return map as RemoteConfigDefaults;
}

/** The pure tier's client. Reads resolve the activated template values
 * first, then the app defaults; anything else is unset. State is
 * in-memory only — nothing here persists across a restart. */
export class RemoteConfig implements RemoteConfigClient {
  private defaults: RemoteConfigDefaults = {};
  private active: Record<string, string> = {};
  private status: RemoteConfigStatus = "no_fetch_yet";
  private fetchedAtMs: number | null = null;

  constructor(private readonly transport: RemoteConfigTransport | null) {}

  get lastFetchStatus(): RemoteConfigStatus {
    return this.status;
  }

  // Follows the app's clock: real time in production, the manual test
  // clock under nsh test.
  get lastFetchTimeMs(): number | null {
    return this.fetchedAtMs;
  }

  // Replaces the whole defaults map (the JS SDK's setDefaults
  // semantics: later calls do not merge).
  async setDefaults(defaults: RemoteConfigDefaults): Promise<void> {
    validateDefaults(defaults);
    this.defaults = { ...defaults };
  }

  async getDefaults(): Promise<RemoteConfigDefaults> {
    return { ...this.defaults };
  }

  // The activated template value, else the default's string form, else
  // nothing.
  async get(key: string): Promise<string | null> {
    if (key === "") throw new FirebaseError(CODES.invalidArgument, "the key is empty");
    if (key in this.active) return this.active[key];
    if (key in this.defaults) return stringDefault(this.defaults[key]);
    return null;
  }

  async getString(key: string): Promise<string> {
    return (await this.get(key)) ?? "";
  }

  async getBoolean(key: string): Promise<boolean> {
    return toBoolean(await this.get(key));
  }

  async getNumber(key: string): Promise<number> {
    return toNumber(await this.get(key));
  }

  // Fetches through the transport, validates the document and activates
  // its resolved values. Without one this refuses with
  // remote-config/unavailable: no fake fetch is attempted, because the
  // REST API is OAuth-token-only and the emulator suite has no Remote
  // Config service.
  async fetchAndActivate(): Promise<FetchAndActivateResult> {
    if (this.transport === null) {
      throw new FirebaseError(RC_CODES.unavailable, OAUTH_MESSAGE);
    }
    let document: unknown;
    try {
      document = await this.transport.fetchTemplate();
    } catch (error) {
      this.recordFailure();
      throw toFirebaseError(error);
    }
    let template: ParsedRemoteConfigTemplate;
    try {
      template = parseRemoteConfigTemplate(document);
    } catch (error) {
      this.recordFailure();
      throw error;
    }
    this.active = { ...template.resolvedValues };
    this.status = "success";
    this.fetchedAtMs = Date.now();
    return { activated: true, lastFetchStatus: this.status };
  }

  private recordFailure(): void {
    this.status = "failure";
    this.fetchedAtMs = Date.now();
  }
}

/** The native tier's client: validated arguments, then a host call per
 * the fixed contract. The typed reads coerce the wire value locally, as
 * the native SDKs do; the fetch status is the local echo of the last
 * result the host returned. */
export class NativeRemoteConfig implements RemoteConfigClient {
  private status: RemoteConfigStatus = "no_fetch_yet";

  constructor(private readonly tier: FirebaseTier) {}

  get lastFetchStatus(): RemoteConfigStatus {
    return this.status;
  }

  async setDefaults(defaults: RemoteConfigDefaults): Promise<void> {
    validateDefaults(defaults);
    await this.tier.call(NATIVE_SET_DEFAULTS, { map: defaults });
  }

  async getDefaults(): Promise<RemoteConfigDefaults> {
    const result = await this.tier.call(NATIVE_GET_DEFAULTS);
    return mapOf(result, NATIVE_GET_DEFAULTS);
  }

  async fetchAndActivate(): Promise<FetchAndActivateResult> {
    const result = objectOf(
      await this.tier.call(NATIVE_FETCH_AND_ACTIVATE),
      NATIVE_FETCH_AND_ACTIVATE,
    );
    const activated = result["activated"];
    const lastFetchStatus = result["lastFetchStatus"];
    if (typeof activated !== "boolean" || typeof lastFetchStatus !== "string") {
      throw hostError(
        NATIVE_FETCH_AND_ACTIVATE,
        "no `activated` boolean and `lastFetchStatus` string",
      );
    }
    this.status = lastFetchStatus as RemoteConfigStatus;
    return { activated, lastFetchStatus };
  }

  async get(key: string): Promise<string | null> {
    if (key === "") throw new FirebaseError(CODES.invalidArgument, "the key is empty");
    const result = objectOf(await this.tier.call(NATIVE_GET, { key }), NATIVE_GET);
    const value = result["value"];
    if (value === null || value === undefined) return null;
    if (typeof value !== "string") throw hostError(NATIVE_GET, "a non-string `value`");
    return value;
  }

  async getString(key: string): Promise<string> {
    return (await this.get(key)) ?? "";
  }

  async getBoolean(key: string): Promise<boolean> {
    return toBoolean(await this.get(key));
  }

  async getNumber(key: string): Promise<number> {
    return toNumber(await this.get(key));
  }
}

/** The remote config for `app`: native routing when the host answers and
 * no transport was configured, the honest pure path otherwise (mirrors
 * RemoteConfig::new / NativeRemoteConfig in Rust, where the caller picks). */
export function getRemoteConfig(
  app: FirebaseApp = getApp(),
  config: RemoteConfigConfig = {},
): RemoteConfigClient {
  if (app.tier.kind === "native" && config.transport === undefined && config.forcePure !== true) {
    return new NativeRemoteConfig(app.tier);
  }
  return new RemoteConfig(config.transport ?? null);
}
