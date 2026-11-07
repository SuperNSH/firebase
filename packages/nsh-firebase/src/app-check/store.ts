// Persistence: the key-value store an app provides (nsh's Storage by
// default, an in-memory one the tests use) and the cache record both
// this package and the Rust crate
// (crates/nsh-firebase-app-check/src/store.rs) persist as one JSON
// shape. Expiry, not removal, is what refreshes the record.

import { Storage } from "nsh";

// Where the App Check keeps its cached token. The calls are promises:
// the flows await them, so an await of getToken covers the stored write.
export interface KeyValueStore {
  get(key: string): Promise<string | null>;
  set(key: string, value: string): Promise<void>;
}

// Keeps entries in process memory; a cache in it does not survive a
// restart. What a cache-only app may pass instead of the device store.
export class MemoryStore implements KeyValueStore {
  private readonly entries = new Map<string, string>();

  async get(key: string): Promise<string | null> {
    return this.entries.get(key) ?? null;
  }

  async set(key: string, value: string): Promise<void> {
    this.entries.set(key, value);
  }
}

// Stores the cache in the app's own storage (nsh/storage), the same
// entries a Rust app's EngineStore writes. Every call swallows platform
// failures, like the Rust EngineStore: a read failure reads as no cache,
// and a write failure keeps what was there — none of it rejects up into
// restore(), which getToken awaits.
export class StorageStore implements KeyValueStore {
  async get(key: string): Promise<string | null> {
    try {
      return await Storage.getItem(key);
    } catch {
      return null;
    }
  }

  async set(key: string, value: string): Promise<void> {
    // A full storage keeps the old record; the cache then just does not
    // survive the launch, which the next exchange repairs.
    try {
      await Storage.setItem(key, value);
    } catch {
      // The record stays as it was.
    }
  }
}

// One name per app, so two apps of a process do not share a cache.
export function storeKey(appName: string): string {
  return `nsh.firebase.app-check.${appName}`;
}

// The cached token: the version, then the fields. Field names are
// camelCase so either tier restores the other's record.
export interface CachedRecord {
  token: string;
  expiresAtMs: number;
}

export function encodeRecord(record: CachedRecord): string {
  return JSON.stringify({ v: 1, token: record.token, expiresAtMs: record.expiresAtMs });
}

// null for anything not the version-1 shape: a corrupt record is treated
// as no cache, not as an error to surface at startup. The Rust read is
// as_u64: an integer at least 0.
export function decodeRecord(text: string): CachedRecord | null {
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
  const token = typeof record.token === "string" ? record.token : null;
  const expiresAtMs =
    typeof record.expiresAtMs === "number" && Number.isInteger(record.expiresAtMs) && record.expiresAtMs >= 0
      ? record.expiresAtMs
      : null;
  if (record.v !== 1 || token === null || expiresAtMs === null) {
    return null;
  }
  return { token, expiresAtMs };
}
