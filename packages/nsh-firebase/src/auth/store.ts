// Persistence: the key-value store an app provides (nsh's Storage by
// default, an in-memory one the tests use) and the session record both
// this package and the Rust crate (crates/nsh-firebase-auth/src/store.rs)
// persist as one JSON shape.

import { Storage } from "nsh";

import type { AuthUser } from "./user";

// Where the auth keeps its session record. The calls are promises: the
// flows await them, so an await of a sign-in covers the persisted write.
export interface KeyValueStore {
  get(key: string): Promise<string | null>;
  set(key: string, value: string): Promise<void>;
  remove(key: string): Promise<void>;
}

// Keeps entries in process memory; a session in it does not survive a
// restart. What the tests use, and what a cache-only app may pass.
export class MemoryStore implements KeyValueStore {
  private readonly entries = new Map<string, string>();

  seed(entries: Record<string, string>): void {
    for (const [key, value] of Object.entries(entries)) this.entries.set(key, value);
  }

  async get(key: string): Promise<string | null> {
    return this.entries.get(key) ?? null;
  }

  async set(key: string, value: string): Promise<void> {
    this.entries.set(key, value);
  }

  async remove(key: string): Promise<void> {
    this.entries.delete(key);
  }
}

// Stores the session in the app's own storage (nsh/storage), the same
// entries a Rust app's EngineStore writes. Every call swallows platform
// failures, like the Rust EngineStore: a read failure reads as signed
// out, and a write failure keeps what was there — none of it rejects up
// into restore(), which an app may never await.
export class StorageStore implements KeyValueStore {
  async get(key: string): Promise<string | null> {
    try {
      return await Storage.getItem(key);
    } catch {
      return null;
    }
  }

  async set(key: string, value: string): Promise<void> {
    // A full storage keeps the old record; the session then just does not
    // survive the launch, which sign-in on the next one repairs.
    try {
      await Storage.setItem(key, value);
    } catch {
      // The record stays as it was.
    }
  }

  async remove(key: string): Promise<void> {
    try {
      await Storage.removeItem(key);
    } catch {
      // The record staying is the signed-in state it came from.
    }
  }
}

// One name per app, so two apps of a process do not share a session.
export function storeKey(appName: string): string {
  return `nsh.firebase.auth.${appName}`;
}

// The persisted record: the version, then the user fields. Field names
// are camelCase so either tier restores the other's record.
export function encodeSession(user: AuthUser): string {
  return JSON.stringify({
    v: 1,
    uid: user.uid,
    email: user.email,
    isAnonymous: user.isAnonymous,
    idToken: user.idToken,
    refreshToken: user.refreshToken,
    expiresAtMs: user.expiresAtMs,
    nextRefreshAtMs: user.nextRefreshAtMs,
  });
}

// null for anything not the version-1 shape: a corrupt record is treated
// as signed out, not as an error to surface at startup. A valid-JSON
// non-object ("null", a number) is corrupt too, like the Rust
// as_object() read.
export function decodeSession(text: string): AuthUser | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  if (parsed === null || typeof parsed !== "object") {
    return null;
  }
  const record = parsed as Record<string, unknown>;
  const string = (name: string): string | null => {
    const value = record[name];
    return typeof value === "string" ? value : null;
  };
  const uid = string("uid");
  const idToken = string("idToken");
  const refreshToken = string("refreshToken");
  // The Rust read is as_u64: an integer at least 0. (Values past the
  // safe range stay a JS-side difference: they are still integers here.)
  const timestamp = (name: string): number | null => {
    const value = record[name];
    return typeof value === "number" && Number.isInteger(value) && value >= 0 ? value : null;
  };
  const expiresAtMs = timestamp("expiresAtMs");
  const nextRefreshAtMs = timestamp("nextRefreshAtMs");
  if (
    record.v !== 1 ||
    uid === null ||
    idToken === null ||
    refreshToken === null ||
    expiresAtMs === null ||
    nextRefreshAtMs === null ||
    typeof record.isAnonymous !== "boolean"
  ) {
    return null;
  }
  return {
    uid,
    email: string("email"),
    isAnonymous: record.isAnonymous,
    idToken,
    refreshToken,
    expiresAtMs,
    nextRefreshAtMs,
  };
}
