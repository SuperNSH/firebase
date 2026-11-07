// Client-side validation, mirroring React Native Firebase's
// packages/analytics/lib/index.ts (ValidEventName,
// ReservedEventNamePrefixes, ReservedEventNames) as recorded in
// docs/research.md §1.6, and crates/nsh-firebase-analytics/src/validate.rs.

import { CODES, FirebaseError } from "../core/error";

const RESERVED_EVENT_NAMES: readonly string[] = [
  "ad_activeview",
  "ad_click",
  "ad_exposure",
  "ad_query",
  "ad_reward",
  "adunit_exposure",
  "app_background",
  "app_clear_data",
  "app_remove",
  "app_store_refund",
  "app_store_subscription_cancel",
  "app_store_subscription_convert",
  "app_store_subscription_renew",
  "app_update",
  "app_upgrade",
  "dynamic_link_app_open",
  "dynamic_link_app_update",
  "dynamic_link_first_open",
  "error",
  "first_open",
  "first_visit",
  "in_app_purchase",
  "notification_dismiss",
  "notification_foreground",
  "notification_open",
  "notification_receive",
  "os_update",
  "session_start",
  "session_start_with_rollout",
  "user_engagement",
];

const RESERVED_EVENT_NAME_PREFIXES = ["firebase_", "google_", "ga_"] as const;

/** RNFB's ValidEventName: an alphabetic first character, then 1-40
 * alphanumeric-or-underscore characters in total. */
function isValidIdentifier(name: string): boolean {
  if (!/^[a-zA-Z]/.test(name)) return false;
  return name.length <= 40 && /^[a-zA-Z][a-zA-Z0-9_]*$/.test(name);
}

function invalid(message: string): FirebaseError {
  return new FirebaseError(CODES.invalidArgument, message);
}

export function validateEventName(name: string): void {
  if ((RESERVED_EVENT_NAMES as readonly string[]).includes(name)) {
    throw invalid(
      `logEvent: the event name '${name}' is reserved and can not be used`,
    );
  }
  if (
    RESERVED_EVENT_NAME_PREFIXES.some((prefix) => name.startsWith(prefix))
  ) {
    throw invalid(
      `logEvent: the event name '${name}' uses a reserved prefix and can not be used`,
    );
  }
  if (!isValidIdentifier(name)) {
    throw invalid(
      `logEvent: invalid event name '${name}'. Names must start with an alphabetic character and contain 1 to 40 alphanumeric characters or underscores`,
    );
  }
}

type Primitive = string | number | boolean;

function isPrimitive(value: unknown): value is Primitive {
  return (
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  );
}

/** GA4 params are flat: primitive values only, keys shaped like event
 * names. Nested objects, arrays and null are refused here rather than
 * silently flattened by the endpoint. */
export function validateParams(
  params: Record<string, unknown> | undefined,
): void {
  if (params === undefined) return;
  if (typeof params !== "object" || Array.isArray(params) || params === null) {
    throw invalid("logEvent: params expected an object value");
  }
  for (const [key, value] of Object.entries(params)) {
    if (!isValidIdentifier(key)) {
      throw invalid(
        `logEvent: invalid param name '${key}'. Names must start with an alphabetic character and contain 1 to 40 alphanumeric characters or underscores`,
      );
    }
    if (!isPrimitive(value)) {
      throw invalid(
        `logEvent: the param '${key}' must be a string, number or boolean`,
      );
    }
  }
}

/** GA4 reserves the google_ prefix for its own properties; the
 * firebase_/ga_ prefixes are event-name rules, not user-property ones. */
export function validateUserProperty(key: string, value: unknown): void {
  if (key.startsWith("google_")) {
    throw invalid(
      `setUserProperty: the property name '${key}' uses a reserved prefix and can not be used`,
    );
  }
  if (!isValidIdentifier(key)) {
    throw invalid(
      `setUserProperty: invalid property name '${key}'. Names must start with an alphabetic character and contain 1 to 40 alphanumeric characters or underscores`,
    );
  }
  if (!isPrimitive(value)) {
    throw invalid(
      `setUserProperty: the value of '${key}' must be a string, number or boolean`,
    );
  }
}
