// The remote-config types, mirroring crates/nsh-firebase-remote-config.

export type RemoteConfigDefaults = Record<string, string | number | boolean>;

export type RemoteConfigStatus = "no_fetch_yet" | "success" | "failure";

export interface FetchAndActivateResult {
  activated: boolean;
  lastFetchStatus: string;
}

// The seam a future OAuth holder plugs in: wherever it can authorize, it
// returns a template document (the REST RemoteConfig JSON shape) and the
// client parses, validates and activates it. The pure tier ships none —
// the REST API is authorized with an OAuth 2.0 service-account token,
// not the web API key.
export interface RemoteConfigTransport {
  fetchTemplate(): Promise<unknown>;
}

export interface RemoteConfigConfig {
  transport?: RemoteConfigTransport;
  // Skips native routing even when the host answers.
  forcePure?: boolean;
}

export interface ParsedCondition {
  name: string;
  expression: string;
  tagColor: string | null;
}

export interface ParsedParameter {
  // The `{value}` string of defaultValue; null for useInAppDefault, a
  // dynamic value, or no default at all.
  defaultValue: string | null;
  // Condition name -> `{value}` string; recorded, never evaluated.
  conditionalValues: Record<string, string>;
  description: string | null;
  valueType: string | null;
}

export interface ParsedRemoteConfigTemplate {
  conditions: ParsedCondition[];
  parameters: Record<string, ParsedParameter>;
  version: Record<string, unknown> | null;
  resolvedValues: Record<string, string>;
}

// One surface over both tiers: every call that can cross the host
// channel is async, so a screen needs no per-tier branches.
export interface RemoteConfigClient {
  readonly lastFetchStatus: RemoteConfigStatus;
  setDefaults(defaults: RemoteConfigDefaults): Promise<void>;
  getDefaults(): Promise<RemoteConfigDefaults>;
  fetchAndActivate(): Promise<FetchAndActivateResult>;
  get(key: string): Promise<string | null>;
  getString(key: string): Promise<string>;
  getBoolean(key: string): Promise<boolean>;
  getNumber(key: string): Promise<number>;
}
