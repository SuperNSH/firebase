// Firebase Remote Config for Native Shell TSX apps: defaults, typed
// gets, REST template parsing, and an honest fetch — the pure tier
// refuses with remote-config/unavailable unless a transport supplies a
// template, because the REST API is authorized with an OAuth 2.0
// service-account token, not the web API key, and the emulator suite has
// no Remote Config service. Mirrors crates/nsh-firebase-remote-config.
export {
  getRemoteConfig,
  RemoteConfig,
  NativeRemoteConfig,
  NATIVE_SET_DEFAULTS,
  NATIVE_GET_DEFAULTS,
  NATIVE_FETCH_AND_ACTIVATE,
  NATIVE_GET,
  OAUTH_MESSAGE,
} from "./client";
export type { RemoteConfigClient } from "./client";
export { RC_CODES } from "./codes";
export { parseRemoteConfigTemplate } from "./template";
export type {
  FetchAndActivateResult,
  ParsedCondition,
  ParsedParameter,
  ParsedRemoteConfigTemplate,
  RemoteConfigConfig,
  RemoteConfigDefaults,
  RemoteConfigStatus,
  RemoteConfigTransport,
} from "./types";
