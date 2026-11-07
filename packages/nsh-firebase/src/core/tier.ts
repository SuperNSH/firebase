// The tier facade. The native tier wraps `host.call` / `host.post` /
// `host.on` with the firebase.* handler names the app declares under
// capabilities.host; the pure tier is what every other target gets.
// Mirrors crates/nsh-firebase-core/src/host.rs.

import { host } from "nsh";
import { CODES, FirebaseError, toFirebaseError } from "./error";

export interface FirebaseTier {
  readonly kind: "native" | "pure";
  call(name: string, args?: unknown): Promise<unknown>;
  // Fire-and-forget for the products that push (analytics). host.post
  // throws when capabilities.host does not grant the name; the facade
  // maps that to the stable codes.
  post(name: string, args?: unknown): void;
  on(event: string, listener: (payload: unknown) => void): () => void;
}

export class NativeTier implements FirebaseTier {
  readonly kind = "native" as const;

  async call(name: string, args?: unknown): Promise<unknown> {
    try {
      return await host.call(name, args ?? null);
    } catch (error) {
      throw toFirebaseError(error);
    }
  }

  post(name: string, args?: unknown): void {
    try {
      host.post(name, args ?? null);
    } catch (error) {
      throw toFirebaseError(error);
    }
  }

  on(event: string, listener: (payload: unknown) => void): () => void {
    return host.on(event, listener);
  }
}

export class PureTier implements FirebaseTier {
  readonly kind = "pure" as const;

  call(name: string): Promise<never> {
    return Promise.reject(unavailable(name));
  }

  post(name: string): void {
    throw unavailable(name);
  }

  on(): () => void {
    return () => {};
  }
}

function unavailable(name: string): FirebaseError {
  return new FirebaseError(
    CODES.unavailable,
    `no host answers \`${name}\` in the pure tier; use the product's fetch transport`,
    "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
  );
}

// host.available() tells the platform's answer; the handler names
// themselves are the firebase.* entries of capabilities.host, which the
// runtime checks before every call.
export function createTier(): FirebaseTier {
  return host.available() ? new NativeTier() : new PureTier();
}
