// Persistence: the key-value store an app provides (nsh's Storage by
// default, an in-memory one the tests use) and the installation record
// both this package and the Rust crate
// (crates/nsh-firebase-installations/src/store.rs) persist as one JSON
// shape. Only {fid, refreshToken} is persisted; the auth token stays in
// process memory.

import { Storage } from "nsh";

// Where the installations keep their record. The calls are promises: the
// flows await them, so an await of getId covers the persisted write.
export interface KeyValueStore {
  get(key: string): Promise<string | null>;
  set(key: string, value: string): Promise<void>;
  remove(key: string): Promise<void>;
}

// Keeps entries in process memory; a record in it does not survive a
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

// Stores the record in the app's own storage (nsh/storage), the same
// entries a Rust app's EngineStore writes. Every call swallows platform
// failures, like the Rust EngineStore: a read failure reads as
// unregistered, and a write failure keeps what was there — none of it
// rejects up into restore(), which an app may never await.
export class StorageStore implements KeyValueStore {
  async get(key: string): Promise<string | null> {
    try {
      return await Storage.getItem(key);
    } catch {
      return null;
    }
  }

  async set(key: string, value: string): Promise<void> {
    // A full storage keeps the old record; the FID then just does not
    // survive the launch, which register on the next one repairs.
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
      // The record staying is the registered state it came from.
    }
  }
}

// One name per app, so two apps of a process do not share a record.
export function storeKey(appName: string): string {
  return `nsh.firebase.installations.${appName}`;
}

// The persisted record: the version, then what the flows cannot rebuild.
// Field names are camelCase so either tier restores the other's record.
export interface StoredInstallation {
  fid: string;
  refreshToken: string;
}

export function encodeInstallation(stored: StoredInstallation): string {
  return JSON.stringify({ v: 1, fid: stored.fid, refreshToken: stored.refreshToken });
}

// null for anything not the version-1 shape: a corrupt record is treated
// as unregistered, not as an error to surface at startup. A valid-JSON
// non-object ("null", a number) is corrupt too, like the Rust
// as_object() read.
export function decodeInstallation(text: string): StoredInstallation | null {
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
  const fid = string("fid");
  const refreshToken = string("refreshToken");
  if (record.v !== 1 || fid === null || refreshToken === null) {
    return null;
  }
  return { fid, refreshToken };
}
