// The signed-in user and the credential shapes, mirroring
// crates/nsh-firebase-auth/src/auth.rs's AuthUser and Credential.

// The signed-in user. expiresAtMs and nextRefreshAtMs are absolute
// wall-clock milliseconds (Date.now() on the app's clock, so a manual
// clock moves them only when the test moves it).
export interface AuthUser {
  uid: string;
  email: string | null;
  isAnonymous: boolean;
  idToken: string;
  refreshToken: string;
  expiresAtMs: number;
  nextRefreshAtMs: number;
}

// What signInWithCredential accepts.
export type Credential =
  | { kind: "password"; email: string; password: string }
  | { kind: "custom-token"; token: string }
  | { kind: "id-token"; idToken: string };
