// The installations stable codes and the mapping from an Installations
// REST error reply to them, mirroring
// crates/nsh-firebase-installations/src/error.rs name for name.
// Transport failures keep the core's stable codes (requestText throws
// them); only a server reply maps to an installations/* code.

import { FirebaseError } from "../core";

export const INSTALLATIONS_CODES = {
  missingAppConfig: "installations/missing-app-config-values",
  requestFailed: "installations/request-failed",
  internalError: "installations/internal-error",
  invalidResponse: "installations/invalid-response",
  registerInProgress: "installations/register-in-progress",
} as const;

// Builds the error for a non-2xx reply. A 4xx body's error object
// ({code, message, status}) goes into the message in the js-SDK's
// request-failed shape, so the server's own code and status stay
// diagnosable; a 5xx is the server's failure, not the request's.
export function replyError(requestName: string, status: number, body: string): FirebaseError {
  if (status >= 500) {
    return new FirebaseError(
      INSTALLATIONS_CODES.internalError,
      `the installations server failed (HTTP ${status})`,
    );
  }
  let parsed: string | null = null;
  try {
    const error = (JSON.parse(body) as { error?: unknown }).error;
    if (error !== null && typeof error === "object") {
      const record = error as { code?: unknown; message?: unknown; status?: unknown };
      if (typeof record.code === "number" && typeof record.message === "string") {
        const wireStatus = typeof record.status === "string" ? record.status : "";
        parsed = `${requestName} request failed with error "${record.code} ${wireStatus}: ${record.message}"`;
      }
    }
  } catch {
    // Not JSON: the no-error-message branch below names the status.
  }
  return new FirebaseError(
    INSTALLATIONS_CODES.requestFailed,
    parsed ?? `the server replied ${status} without an error message`,
  );
}
