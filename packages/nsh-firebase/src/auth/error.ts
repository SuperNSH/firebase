// The auth stable codes and the mapping from Identity Toolkit and Secure
// Token wire codes to them. Mirrors crates/nsh-firebase-auth/src/error.rs:
// transport failures keep the core's stable codes (core/network,
// core/access-denied); only a server reply maps to an auth/* code, with
// the same messages.

import { FirebaseError } from "../core/error";

export const AUTH_CODES = {
  emailAlreadyInUse: "auth/email-already-in-use",
  userNotFound: "auth/user-not-found",
  wrongPassword: "auth/wrong-password",
  invalidCredential: "auth/invalid-credential",
  invalidEmail: "auth/invalid-email",
  weakPassword: "auth/weak-password",
  userDisabled: "auth/user-disabled",
  operationNotAllowed: "auth/operation-not-allowed",
  tooManyRequests: "auth/too-many-requests",
  invalidCustomToken: "auth/invalid-custom-token",
  customTokenMismatch: "auth/custom-token-mismatch",
  invalidRefreshToken: "auth/invalid-refresh-token",
  userTokenExpired: "auth/user-token-expired",
  projectNotFound: "auth/project-not-found",
  invalidApiKey: "auth/invalid-api-key",
  invalidArgument: "auth/invalid-argument",
  noCurrentUser: "auth/no-current-user",
  refreshInProgress: "auth/refresh-in-progress",
  invalidResponse: "auth/invalid-response",
  internalError: "auth/internal-error",
  requestFailed: "auth/request-failed",
} as const;

// The codes whose session cannot recover: refresh maps them to a
// sign-out. USER_NOT_FOUND arrives here only from Secure Token, where it
// means the account was deleted; identitytoolkit sign-ins say
// EMAIL_NOT_FOUND instead.
export function isSessionLost(code: string): boolean {
  return (
    code === AUTH_CODES.userTokenExpired ||
    code === AUTH_CODES.invalidRefreshToken ||
    code === AUTH_CODES.userNotFound
  );
}

// Builds the error for a non-2xx reply: the wire error body's `message`
// is the machine code, alone or ahead of a human part (`WEAK_PASSWORD :
// Password should be at least 6 characters`).
export function wireError(status: number, message: string): FirebaseError {
  const wire = (message.split(" : ")[0] ?? message).trim();
  const [code, human] = (() => {
    switch (wire) {
      case "EMAIL_EXISTS":
        return [AUTH_CODES.emailAlreadyInUse, "that email is already registered; sign in instead"];
      case "EMAIL_NOT_FOUND":
        return [AUTH_CODES.userNotFound, "no account with that email"];
      case "USER_NOT_FOUND":
        return [AUTH_CODES.userNotFound, "no such account on the server"];
      case "INVALID_PASSWORD":
        return [AUTH_CODES.wrongPassword, "that password is wrong for the account"];
      case "INVALID_LOGIN_CREDENTIALS":
      case "INVALID_IDP_RESPONSE":
        return [AUTH_CODES.invalidCredential, "the server rejected those credentials"];
      case "INVALID_EMAIL":
        return [AUTH_CODES.invalidEmail, "that email is not a valid address"];
      case "WEAK_PASSWORD":
        return [AUTH_CODES.weakPassword, "that password is too weak"];
      case "USER_DISABLED":
        return [AUTH_CODES.userDisabled, "that account is disabled"];
      case "OPERATION_NOT_ALLOWED":
        return [AUTH_CODES.operationNotAllowed, "the sign-in provider is not enabled for this project"];
      case "TOO_MANY_ATTEMPTS_TRY_LATER":
        return [AUTH_CODES.tooManyRequests, "too many attempts from this client; try again later"];
      case "INVALID_CUSTOM_TOKEN":
        return [AUTH_CODES.invalidCustomToken, "the server did not accept that token"];
      case "CREDENTIAL_MISMATCH":
        return [AUTH_CODES.customTokenMismatch, "that token was minted for another project"];
      case "INVALID_REFRESH_TOKEN":
      case "INVALID_GRANT_TYPE":
      case "MISSING_REFRESH_TOKEN":
        return [AUTH_CODES.invalidRefreshToken, "the refresh token is not one this project issued"];
      case "TOKEN_EXPIRED":
        return [AUTH_CODES.userTokenExpired, "the session expired server-side; sign in again"];
      case "PROJECT_NUMBER_MISMATCH":
        return [AUTH_CODES.projectNotFound, "the API key names another project"];
      case "INVALID_API_KEY":
        return [AUTH_CODES.invalidApiKey, "the API key is not valid"];
      default:
        if (status >= 500) return [AUTH_CODES.internalError, "the auth server failed"];
        return [AUTH_CODES.requestFailed, message];
    }
  })();
  return new FirebaseError(code, `${human} (${wire})`);
}

// A parse of the error body a non-2xx reply carries, or null when it has
// none ({error: {code: 400, message: "EMAIL_NOT_FOUND"}}).
export function wireErrorMessage(body: string): string | null {
  try {
    const reply = JSON.parse(body) as { error?: { message?: unknown } };
    const message = reply.error?.message;
    return typeof message === "string" ? message : null;
  } catch {
    return null;
  }
}
