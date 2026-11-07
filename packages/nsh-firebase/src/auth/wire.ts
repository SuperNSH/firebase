// The Identity Toolkit and Secure Token wire shapes (docs/research.md
// §3): URLs, request bodies, reply parsing, and the base64url read of an
// idToken's claims that flows whose reply has no localId need for the
// uid. Mirrors crates/nsh-firebase-auth/src/wire.rs.

import { FirebaseError } from "../core/error";
import { joinUrl } from "../core/http";
import { AUTH_CODES } from "./error";

export const IDENTITY_TOOLKIT = "identitytoolkit.googleapis.com/v1";
export const SECURE_TOKEN_TOKEN = "securetoken.googleapis.com/v1/token";

// One sign-in reply reduced to what a session needs. expiresInSecs comes
// as a JSON string on both services ("3600").
export interface WireSession {
  idToken: string;
  refreshToken: string;
  expiresInSecs: number;
  localId: string | null;
  email: string | null;
}

// The service paths sit under the base URL as full segments, the shape
// the emulator serves (http://127.0.0.1:19099/identitytoolkit.googleapis
// .com/v1/...) and the real hosts share.
export function identityUrl(base: string, apiKey: string, operation: string): string {
  return `${joinUrl(base, `${IDENTITY_TOOLKIT}/${operation}`)}?key=${apiKey}`;
}

export function refreshUrl(base: string, apiKey: string): string {
  return `${joinUrl(base, SECURE_TOKEN_TOKEN)}?key=${apiKey}`;
}

// Percent-encodes every byte but the unreserved set, so any refresh
// token lands in the form body intact.
export function formEncode(value: string): string {
  const unreserved = /[A-Za-z0-9\-_.~]/;
  let out = "";
  for (const char of value) {
    if (unreserved.test(char)) {
      out += char;
    } else {
      for (const byte of new TextEncoder().encode(char)) {
        out += `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
      }
    }
  }
  return out;
}

export function refreshFormBody(refreshToken: string): string {
  return `grant_type=refresh_token&refresh_token=${formEncode(refreshToken)}`;
}

// Reads a success reply. Identity Toolkit answers camelCase
// (idToken/refreshToken/expiresIn), Secure Token snake_case
// (id_token/refresh_token/expires_in); the emulator may hand Secure
// Token's access_token instead of an idToken on refresh.
export function parseSessionReply(body: string, acceptAccessToken: boolean): WireSession {
  let parsed: unknown;
  try {
    parsed = JSON.parse(body);
  } catch {
    // The same code and message the Rust parse_session_reply gives.
    throw new FirebaseError(AUTH_CODES.invalidResponse, "the reply is not JSON");
  }
  // A valid-JSON non-object has no fields to read: the same
  // invalid-response the Rust Value::Null reply gets.
  if (parsed === null || typeof parsed !== "object") {
    throw missing("idToken");
  }
  const reply = parsed as Record<string, unknown>;
  const string = (names: string[]): string | undefined => {
    for (const name of names) {
      const value = reply[name];
      if (typeof value === "string") return value;
    }
    return undefined;
  };
  const idToken =
    string(["idToken", "id_token"]) ??
    (acceptAccessToken ? string(["access_token"]) : undefined);
  if (idToken === undefined) {
    throw missing("idToken");
  }
  // expiresIn parses like the Rust u64 FromStr: an optional '+', digits
  // only, up to the u64 ceiling. The ceiling is a string bound, so
  // values past Number.MAX_SAFE_INTEGER parse like they do in Rust (to
  // a float).
  const expiresIn = string(["expiresIn", "expires_in"]);
  const expiresInSecs = expiresIn === undefined ? NaN : (u64FromStr(expiresIn) ?? NaN);
  if (Number.isNaN(expiresInSecs)) {
    throw missing("expiresIn (a number-as-string)");
  }
  return {
    idToken,
    refreshToken: string(["refreshToken", "refresh_token"]) ?? "",
    expiresInSecs,
    localId: string(["localId", "local_id"]) ?? null,
    email: string(["email"]) ?? null,
  };
}

function missing(field: string): FirebaseError {
  return new FirebaseError(AUTH_CODES.invalidResponse, `the reply has no usable ${field}`);
}

// The u64 FromStr shape: an optional '+' then digits only, up to the u64
// ceiling (compared as a string bound; leading zeros only shrink the
// value, so equal-length lexicographic order equals numeric order).
const U64_MAX_DIGITS = "18446744073709551615";

function u64FromStr(text: string): number | null {
  const digits = text.startsWith("+") ? text.slice(1) : text;
  if (!/^\d+$/.test(digits)) return null;
  if (
    digits.length > U64_MAX_DIGITS.length ||
    (digits.length === U64_MAX_DIGITS.length && digits > U64_MAX_DIGITS)
  ) {
    return null;
  }
  return Number(text);
}

// The `sub` claim of a JWT's base64url payload: the uid of a flow whose
// reply has no localId (custom token, foreign id token, refresh).
export function uidFromIdToken(idToken: string): string | null {
  const payload = idToken.split(".")[1];
  if (payload === undefined) return null;
  const bytes = base64UrlDecode(payload);
  if (bytes === null) return null;
  try {
    const claims = JSON.parse(new TextDecoder().decode(bytes)) as Record<string, unknown>;
    const uid = claims.sub ?? claims.user_id;
    return typeof uid === "string" ? uid : null;
  } catch {
    return null;
  }
}

// Base64url (RFC 4648 §5), padding optional, no invalid characters.
function base64UrlDecode(input: string): Uint8Array | null {
  const out: number[] = [];
  let bits = 0;
  let pending = 0;
  for (const char of input) {
    let value: number;
    if (char >= "A" && char <= "Z") value = char.charCodeAt(0) - 65;
    else if (char >= "a" && char <= "z") value = char.charCodeAt(0) - 71;
    else if (char >= "0" && char <= "9") value = char.charCodeAt(0) + 4;
    else if (char === "-") value = 62;
    else if (char === "_") value = 63;
    else if (char === "=") break;
    else return null;
    bits = (bits << 6) | value;
    pending += 6;
    if (pending >= 8) {
      pending -= 8;
      out.push((bits >> pending) & 0xff);
    }
  }
  return new Uint8Array(out);
}
