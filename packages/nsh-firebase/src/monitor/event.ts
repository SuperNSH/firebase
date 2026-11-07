// The NetworkEvent shared type, fixed by docs/PROMPT.md and mirrored by
// crates/nsh-firebase-perf/src/event.rs with the same JSON. nsh-http and
// tools/collector exchange these lines too, so the fields and their
// nulls are a contract, not a convenience.

export interface NetworkEvent {
  id: string;
  method: string;
  url: string;
  host: string;
  path: string;
  status: number | null;
  startedAtMs: number;
  durationMs: number;
  ttfbMs: number | null;
  requestBytes: number | null;
  responseBytes: number | null;
  error: string | null;
  initiator: string | null;
}

// The initiator instrumentFetch puts on its events.
export const NETWORK_INITIATOR = "firebase-fetch";

// Splits a URL into host (authority, port kept, userinfo dropped) and
// path (a leading `/`, no query or hash; `/` when the URL has none), the
// same split the Rust side does by hand.
export function splitHostPath(url: string): { host: string; path: string } {
  const schemeEnd = url.indexOf("://");
  if (schemeEnd < 0) return { host: "", path: "" };
  const rest = url.slice(schemeEnd + 3);
  const authorityEnd = rest.search(/[/?#]/) < 0 ? rest.length : rest.search(/[/?#]/);
  const authority = rest.slice(0, authorityEnd);
  const host = authority.includes("@") ? authority.slice(authority.lastIndexOf("@") + 1) : authority;
  const tail = rest.slice(authorityEnd);
  const cut = tail.search(/[?#]/);
  if (tail === "" || cut === 0) return { host, path: "/" };
  return { host, path: cut < 0 ? tail : tail.slice(0, cut) };
}

// An exchange that failed: it reports an error, or its status is outside
// 2xx. The aggregates' errorRate uses this rule.
export function isNetworkError(event: NetworkEvent): boolean {
  return event.error !== null || (event.status !== null && !(event.status >= 200 && event.status < 300));
}

// The event id: crypto.randomUUID(), per the contract; the Rust side
// has no UUID crate and builds evt-<ms>-<n> ids, and both are plain
// strings.
export function nextEventId(): string {
  return crypto.randomUUID();
}
