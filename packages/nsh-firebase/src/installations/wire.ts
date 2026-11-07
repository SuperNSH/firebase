// The Installations REST wire shapes (docs/research.md §4, verified
// against firebase-js-sdk packages/installations): endpoints, request
// bodies, headers, reply parsing. Mirrors
// crates/nsh-firebase-installations/src/wire.rs name for name.

import { FirebaseError, joinUrl } from "../core";
import { INSTALLATIONS_CODES } from "./error";
import type { StoredInstallation } from "./store";

// The API base the js-SDK's INSTALLATIONS_API_URL pins; no emulator
// serves this product, so there is no base-URL override.
export const INSTALLATIONS_API_URL = "https://firebaseinstallations.googleapis.com/v1";
// The authVersion of the register body and the Authorization scheme of
// the token call (the js-SDK's INTERNAL_AUTH_VERSION).
export const INTERNAL_AUTH_VERSION = "FIS_v2";
// The sdkVersion the register and token bodies carry; keep it equal to
// the Rust side's (env!("CARGO_PKG_VERSION") builds the same "w:0.1.0"),
// both wire tests pin the exact value.
export const SDK_VERSION = "w:0.1.0";

// The native tier's handlers (the contract; declare
// firebase.installations.* in capabilities.host).
export const GET_ID = "firebase.installations.getId";
export const GET_TOKEN = "firebase.installations.getToken";

// One auth token as a reply carried it: the bearer token and its
// lifetime in milliseconds (the reply's expiresIn is seconds with an s
// suffix, "604800s").
export interface AuthToken {
  token: string;
  expiresInMs: number;
}

// projects/{projectId}/installations, the register endpoint and the
// prefix of the token endpoint.
export function installationsEndpoint(projectId: string): string {
  return joinUrl(INSTALLATIONS_API_URL, `projects/${projectId}/installations`);
}

export interface WireRequest {
  url: string;
  headers: Record<string, string>;
  body: string;
}

// POST projects/{projectId}/installations registering fid (the js-SDK's
// createInstallationRequest).
export function registerRequest(
  projectId: string,
  apiKey: string,
  appId: string,
  fid: string,
): WireRequest {
  requireConfig(apiKey, appId);
  return {
    url: installationsEndpoint(projectId),
    headers: baseHeaders(apiKey),
    body: JSON.stringify({
      fid,
      authVersion: INTERNAL_AUTH_VERSION,
      appId,
      sdkVersion: SDK_VERSION,
    }),
  };
}

// POST projects/{projectId}/installations/{fid}/authTokens:generate
// minting an auth token for the registered fid (the js-SDK's
// generateAuthTokenRequest). The refresh token authenticates the call as
// `FIS_v2 {refreshToken}`.
export function tokenRequest(
  projectId: string,
  apiKey: string,
  appId: string,
  fid: string,
  refreshToken: string,
): WireRequest {
  requireConfig(apiKey, appId);
  return {
    url: joinUrl(installationsEndpoint(projectId), `${fid}/authTokens:generate`),
    headers: {
      ...baseHeaders(apiKey),
      Authorization: `${INTERNAL_AUTH_VERSION} ${refreshToken}`,
    },
    body: JSON.stringify({
      installation: { sdkVersion: SDK_VERSION, appId },
    }),
  };
}

function baseHeaders(apiKey: string): Record<string, string> {
  return {
    "Content-Type": "application/json",
    Accept: "application/json",
    "x-goog-api-key": apiKey,
  };
}

function requireConfig(apiKey: string, appId: string): void {
  if (apiKey === "" || appId === "") {
    throw new FirebaseError(
      INSTALLATIONS_CODES.missingAppConfig,
      "the app options need a non-empty apiKey and appId",
    );
  }
}

export interface RegisterReply {
  stored: StoredInstallation;
  auth: AuthToken;
}

// Reads a success reply of the register call: {name?, fid?, refreshToken,
// authToken: {token, expiresIn}}. The reply may omit the fid; the one
// the client generated stands in then, as the js-SDK's
// `responseValue.fid || fid` does.
export function parseRegisterReply(body: string, requestedFid: string): RegisterReply {
  const reply = parseJson(body);
  const refreshToken = string(reply, "refreshToken");
  if (refreshToken === null) throw missing("refreshToken");
  const authToken = reply.authToken;
  if (authToken === null || typeof authToken !== "object") {
    throw missing("authToken");
  }
  const fid = string(reply, "fid") ?? requestedFid;
  return {
    stored: { fid, refreshToken },
    auth: parseAuthToken(authToken as Record<string, unknown>),
  };
}

// Reads a success reply of the token call: {token, expiresIn}.
export function parseTokenReply(body: string): AuthToken {
  return parseAuthToken(parseJson(body));
}

function parseAuthToken(value: Record<string, unknown>): AuthToken {
  const token = typeof value.token === "string" ? value.token : null;
  if (token === null) throw missing("authToken.token");
  const text = typeof value.expiresIn === "string" ? value.expiresIn : null;
  const expiresInMs =
    text !== null && text.endsWith("s") && /^\d+$/.test(text.slice(0, -1))
      ? Number(text.slice(0, -1)) * 1000
      : null;
  if (expiresInMs === null) throw missing("authToken.expiresIn (seconds with an s suffix)");
  return { token, expiresInMs };
}

function parseJson(body: string): Record<string, unknown> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(body);
  } catch {
    throw new FirebaseError(INSTALLATIONS_CODES.invalidResponse, "the reply is not JSON");
  }
  if (parsed === null || typeof parsed !== "object") {
    throw new FirebaseError(INSTALLATIONS_CODES.invalidResponse, "the reply is not JSON");
  }
  return parsed as Record<string, unknown>;
}

function string(record: Record<string, unknown>, name: string): string | null {
  const value = record[name];
  return typeof value === "string" ? value : null;
}

function missing(field: string): FirebaseError {
  return new FirebaseError(
    INSTALLATIONS_CODES.invalidResponse,
    `the reply has no usable ${field}`,
  );
}
