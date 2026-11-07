// The messaging facade: the same calls and events over the native tier
// and clear failures in the pure tier, mirroring
// crates/nsh-firebase-messaging name for name. The native tier routes
// through the app's tier facade with the firebase.messaging.* handler
// names the native areas register and the host→app events they send.

import { CODES, FirebaseError, getApp, toFirebaseError } from "../core";
import type { FirebaseTier } from "../core";

// The permission strings are the unreleased NSH Notifications module's,
// so the released module later drops in behind the same names.
export type MessagingPermission = "default" | "granted" | "denied" | "unavailable";

export interface NotificationContent {
  id: string;
  title: string;
  body: string;
  silent?: boolean;
}

export interface PressPayload {
  id: string;
}

export interface TokenPayload {
  token: string;
}

export interface RemoteMessageNotification {
  title: string;
  body: string;
}

// The message payload the native areas send on firebase.messaging.message
// (RNFB's serializer shape): message-level fields pulled out, `data`
// riding as it came.
export interface RemoteMessage {
  messageId?: string;
  messageType?: string;
  collapseKey?: string;
  from?: string;
  to?: string;
  sentTime?: number;
  data: Record<string, string>;
  notification: RemoteMessageNotification | null;
}

export type MessagingEvent = "press" | "message" | "token";

export const NEEDS_NATIVE_TIER =
  "Firebase Messaging needs the native tier: iOS/Android host with firebase.messaging.* handlers";

const HOST_EVENT: Record<MessagingEvent, string> = {
  press: "firebase.messaging.press",
  message: "firebase.messaging.message",
  token: "firebase.messaging.tokenRefreshed",
};

const PERMISSIONS: readonly string[] = ["default", "granted", "denied", "unavailable"];

function unavailable(): FirebaseError {
  return new FirebaseError(CODES.unavailable, NEEDS_NATIVE_TIER, "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM");
}

function readPermission(name: string, raw: unknown): MessagingPermission {
  if (typeof raw === "string" && PERMISSIONS.includes(raw)) return raw as MessagingPermission;
  throw new FirebaseError(
    CODES.hostError,
    `\`${name}\` answered \`${String(raw)}\`, which is not a permission name`,
  );
}

export class Messaging {
  constructor(private readonly tier: FirebaseTier) {}

  // The one call path: the pure tier never reaches a host, and a native
  // host answering "no host here" gets the same naming error.
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

  async permission(): Promise<MessagingPermission> {
    const reply = (await this.call("firebase.messaging.permission")) as {
      permission?: unknown;
    } | null;
    return readPermission("firebase.messaging.permission", reply?.permission);
  }

  async requestPermission(): Promise<MessagingPermission> {
    const reply = (await this.call("firebase.messaging.requestPermission")) as {
      permission?: unknown;
    } | null;
    return readPermission("firebase.messaging.requestPermission", reply?.permission);
  }

  // Rejects with the host's own message when the token request fails (the
  // fake project's clear registration failure rides it unchanged).
  async token(): Promise<string> {
    const reply = (await this.call("firebase.messaging.token")) as { token?: unknown } | null;
    if (typeof reply?.token !== "string" || reply.token === "") {
      throw new FirebaseError(CODES.hostError, "`firebase.messaging.token` answered without a token");
    }
    return reply.token;
  }

  // The id and the title must be non-empty, as the Notifications module
  // this mirrors requires; the body may be empty, which it shows as none.
  async show(content: NotificationContent): Promise<boolean> {
    if (
      typeof content?.id !== "string" ||
      content.id === "" ||
      typeof content.title !== "string" ||
      content.title === ""
    ) {
      throw new FirebaseError(CODES.invalidArgument, "messaging.show needs a non-empty id and title");
    }
    const reply = await this.call("firebase.messaging.show", {
      id: content.id,
      title: content.title,
      body: content.body,
      silent: content.silent ?? false,
    });
    // iOS answers {shown}, Android's facade the bare boolean; anything
    // else is "not shown".
    const shown = (reply as { shown?: unknown } | null)?.shown;
    return shown === true || reply === true;
  }

  async close(id: string): Promise<void> {
    if (typeof id !== "string" || id === "") {
      throw new FirebaseError(CODES.invalidArgument, "messaging.close needs a non-empty id");
    }
    await this.call("firebase.messaging.close", { id });
  }

  async setBadge(count: number): Promise<void> {
    if (typeof count !== "number" || !Number.isInteger(count) || count < 0) {
      throw new FirebaseError(CODES.invalidArgument, "messaging.setBadge needs a non-negative integer count");
    }
    await this.call("firebase.messaging.setBadge", { count });
  }

  async subscribe(topic: string): Promise<void> {
    if (typeof topic !== "string" || topic === "") {
      throw new FirebaseError(CODES.invalidArgument, "messaging.subscribe needs a non-empty topic");
    }
    await this.call("firebase.messaging.subscribe", { topic });
  }

  async unsubscribe(topic: string): Promise<void> {
    if (typeof topic !== "string" || topic === "") {
      throw new FirebaseError(CODES.invalidArgument, "messaging.unsubscribe needs a non-empty topic");
    }
    await this.call("firebase.messaging.unsubscribe", { topic });
  }

  // Throws in the pure tier: a listener that never fires is exactly the
  // silent no-op this facade refuses to be.
  on(event: "press", listener: (payload: PressPayload) => void): () => void;
  on(event: "message", listener: (payload: RemoteMessage) => void): () => void;
  on(event: "token", listener: (payload: TokenPayload) => void): () => void;
  on(event: MessagingEvent, listener: (payload: never) => void): () => void {
    const wire = HOST_EVENT[event];
    if (!wire) {
      throw new FirebaseError(
        CODES.invalidArgument,
        `messaging.on takes "press", "message" or "token", not \`${String(event)}\``,
      );
    }
    if (this.tier.kind === "pure") throw unavailable();
    return this.tier.on(wire, listener as (payload: unknown) => void);
  }
}

// The default app's messaging. Instances are cheap wrappers: the
// subscriptions live in the host, so calling this again changes nothing.
export function getMessaging(): Messaging {
  return new Messaging(getApp().tier);
}
