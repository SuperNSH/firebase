// The Crashlytics facade: the explicit calls over the native tier and
// the pure tier's clear rejections, mirroring
// crates/nsh-firebase-crashlytics name for name. There is no honest pure
// tier — Crashlytics has no REST, so a local log-only recorder would be
// a fake crash reporter, not a degraded one; this facade refuses to be
// one. The native SDK already captures crashes automatically, so the
// surface is only the explicit calls (RNFB's crash(),
// checkForUnsentReports() and setCrashlyticsCollectionEnabled() have no
// counterpart: that state machine belongs to the native SDK).

import { CODES, FirebaseError, getApp, toFirebaseError } from "../core";
import type { FirebaseTier } from "../core";

// The SDK's custom-key value types; anything else refuses before the
// host, because it would arrive on the far side of the wire as another
// type than the caller sent.
export type CustomKeyValue = string | number | boolean;

// The non-fatal recordError records: `{code, message, stack?}` on the
// wire. RNFB's recordError takes an Error and strips its stack; the wire
// contract takes the pieces explicitly, so the caller supplies them.
export interface RecordErrorInput {
  code: number;
  message: string;
  stack?: string;
}

// Product-scoped, not core/unavailable: every degradable product reports
// the shared code, Crashlytics refuses with its own.
export const UNAVAILABLE = "crashlytics/unavailable";

export const NEEDS_NATIVE_TIER =
  "Firebase Crashlytics needs the native tier: iOS/Android host with firebase.crashlytics.* handlers (Crashlytics has no REST — no honest pure tier exists)";

const WIRE = {
  log: "firebase.crashlytics.log",
  setUserId: "firebase.crashlytics.setUserId",
  setCustomKey: "firebase.crashlytics.setCustomKey",
  recordError: "firebase.crashlytics.recordError",
  sendUnsentReports: "firebase.crashlytics.sendUnsentReports",
  didCrash: "firebase.crashlytics.didCrashDuringPreviousExecution",
} as const;

function unavailable(): FirebaseError {
  return new FirebaseError(UNAVAILABLE, NEEDS_NATIVE_TIER, "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM");
}

function invalidArgument(message: string): FirebaseError {
  return new FirebaseError(CODES.invalidArgument, message);
}

function isCustomValue(value: unknown): value is CustomKeyValue {
  return typeof value === "string" || typeof value === "number" || typeof value === "boolean";
}

export class Crashlytics {
  constructor(private readonly tier: FirebaseTier) {}

  // The one call path: the pure tier never reaches a host, and a host
  // answering "no crashlytics here" gets the same naming error.
  private async call(name: string, args?: unknown): Promise<unknown> {
    if (this.tier.kind === "pure") throw unavailable();
    try {
      return await this.tier.call(name, args);
    } catch (error) {
      const mapped = toFirebaseError(error);
      if (mapped.code === CODES.unavailable) throw unavailable();
      throw mapped;
    }
  }

  // Coerces the breadcrumb to a string, as RNFB's log does.
  async log(message: string): Promise<void> {
    await this.call(WIRE.log, { message: `${message}` });
  }

  // An empty id clears it, as the SDK's own call does.
  async setUserId(id: string): Promise<void> {
    if (typeof id !== "string") {
      throw invalidArgument("crashlytics.setUserId needs a string id");
    }
    await this.call(WIRE.setUserId, { id });
  }

  async setAttribute(key: string, value: CustomKeyValue): Promise<void> {
    if (!isCustomValue(value)) {
      throw invalidArgument("crashlytics custom keys take a string, number or boolean");
    }
    await this.call(WIRE.setCustomKey, { key, value });
  }

  // One call per key, in the object's order; an empty object calls
  // nothing.
  async setAttributes(attributes: Record<string, CustomKeyValue>): Promise<void> {
    for (const [key, value] of Object.entries(attributes)) {
      await this.setAttribute(key, value);
    }
  }

  async recordError(input: RecordErrorInput): Promise<void> {
    if (
      typeof input?.code !== "number" ||
      !Number.isFinite(input.code) ||
      typeof input?.message !== "string"
    ) {
      throw invalidArgument(
        "crashlytics.recordError needs a finite number code and a string message",
      );
    }
    const args: { code: number; message: string; stack?: string } = {
      code: input.code,
      message: input.message,
    };
    if (input.stack !== undefined) {
      if (typeof input.stack !== "string") {
        throw invalidArgument("crashlytics.recordError needs the stack as a string");
      }
      args.stack = input.stack;
    }
    await this.call(WIRE.recordError, args);
  }

  // What the SDK holds and when it sends is its own state machine; this
  // only asks it to flush now.
  async sendUnsentReports(): Promise<void> {
    await this.call(WIRE.sendUnsentReports);
  }

  // The contract's {crashed}; a reply without the flag is a host error,
  // not a silent false — a silent false would tell the app "no crash"
  // when the host said nothing.
  async didCrashOnPreviousExecution(): Promise<boolean> {
    const reply = (await this.call(WIRE.didCrash)) as { crashed?: unknown } | null;
    if (typeof reply?.crashed !== "boolean") {
      throw new FirebaseError(
        CODES.hostError,
        `\`${WIRE.didCrash}\` answered without a crashed flag`,
      );
    }
    return reply.crashed;
  }
}

// The default app's crashlytics. Instances are cheap wrappers over the
// app's tier; calling this again changes nothing.
export function getCrashlytics(): Crashlytics {
  return new Crashlytics(getApp().tier);
}
