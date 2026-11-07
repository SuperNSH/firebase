// The shared error type and the stable codes, mirroring
// crates/nsh-firebase-core/src/error.rs name for name.

export const CODES = {
  accessDenied: "core/access-denied",
  hostError: "core/host-error",
  unavailable: "core/unavailable",
  timeout: "core/timeout",
  network: "core/network",
  invalidArgument: "core/invalid-argument",
  noApp: "core/no-app",
} as const;

export class FirebaseError extends Error {
  readonly code: string;
  readonly nativeCode: string | null;

  constructor(code: string, message: string, nativeCode?: string) {
    super(nativeCode ? `${code} (${nativeCode}): ${message}` : `${code}: ${message}`);
    this.name = "FirebaseError";
    this.code = code;
    this.nativeCode = nativeCode ?? null;
  }
}

const STABLE_BY_NATIVE: Record<string, string> = {
  ERR_ACCESS_DENIED: CODES.accessDenied,
  ERR_NSH_HOST_ERROR: CODES.hostError,
  ERR_FEATURE_UNAVAILABLE_ON_PLATFORM: CODES.unavailable,
  ETIMEDOUT: CODES.timeout,
};

// Maps a platform failure (a refused host call or fetch, a handler that
// threw, no host) to its stable code, keeping the platform's message. An
// unknown platform code stays recorded, as Rust's from_host_code keeps it.
export function toFirebaseError(thrown: unknown): FirebaseError {
  if (thrown instanceof FirebaseError) return thrown;
  const code = (thrown as { code?: string } | null)?.code;
  const raw = thrown as { message?: unknown } | null;
  const message =
    thrown instanceof Error
      ? thrown.message
      : typeof raw?.message === "string"
        ? raw.message
        : String(thrown);
  const stable = code ? STABLE_BY_NATIVE[code] : undefined;
  if (stable) return new FirebaseError(stable, message, code);
  return new FirebaseError(CODES.network, message, code);
}
