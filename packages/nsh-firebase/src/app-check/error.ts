// The app-check stable codes and the exchange reply's mapping to them,
// mirroring crates/nsh-firebase-app-check/src/error.rs name for name.
// Transport failures keep the core's stable codes (requestText throws
// them); only a server reply maps to an app-check/* code.

import { FirebaseError } from "../core";

export const APP_CHECK_CODES = {
  invalidArgument: "app-check/invalid-argument",
  needsAttestation: "app-check/needs-attestation",
  invalidDebugToken: "app-check/invalid-debug-token",
  exchangeFailed: "app-check/exchange-failed",
  invalidResponse: "app-check/invalid-response",
} as const;

// The error a non-2xx exchange reply maps to: the status decides the
// code, and Google's error body ({"error": {"message": ...}}) rides in
// the message for diagnosis, as the client SDKs surface only the status.
export function replyError(status: number, text: string): FirebaseError {
  let detail: string | null = null;
  try {
    const body = JSON.parse(text) as { error?: { message?: unknown } };
    if (typeof body?.error?.message === "string") detail = body.error.message;
  } catch {
    // Not JSON: the fallback below names that.
  }
  const shown = detail ?? "the server gave no error message";
  const [code, why] =
    status >= 400 && status <= 403
      ? ([APP_CHECK_CODES.invalidDebugToken, "the exchange refused the debug token"] as const)
      : ([APP_CHECK_CODES.exchangeFailed, "the exchange failed"] as const);
  return new FirebaseError(code, `${why} (HTTP ${status}: ${shown})`);
}

// The pure tier cannot attest: DeviceCheck/App Attest and Play Integrity
// need a device. getToken without debug:true says so.
export function needsAttestation(): FirebaseError {
  return new FirebaseError(
    APP_CHECK_CODES.needsAttestation,
    "the pure tier has no device attestor (DeviceCheck/App Attest on iOS, Play Integrity on " +
      "Android): call getToken with debug:true and a debug token registered in the Firebase console",
  );
}

// A 2xx reply without a usable {token, ttl}.
export function invalidResponse(message: string): FirebaseError {
  return new FirebaseError(APP_CHECK_CODES.invalidResponse, message);
}
