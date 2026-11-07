// The Realtime Database's Server-Sent Events stream: the frame format
// of https://firebase.google.com/docs/database/rest/retrieve-data
// ("Streaming from the REST API") — `event:`/`data:` lines, the events
// put, patch, keep-alive, cancel and auth_revoked, and the
// `{path, data}` payload — and the listener cache those events apply
// to. Mirrors crates/nsh-firebase-database/src/sse.rs.

import type { Json } from "./query";

/** One event of the stream. */
export type SseEvent =
  | { kind: "put"; path: string; data: Json }
  | { kind: "patch"; path: string; data: Json }
  | { kind: "keep-alive" }
  | { kind: "cancel" }
  | { kind: "auth-revoked" };

/** A frame-splitting parser: fed every chunk of the stream as it
 * arrives, it gives the complete events. Chunks split anywhere,
 * including inside a frame or a multi-byte character (the caller
 * decodes with a streaming TextDecoder, which carries the partial
 * character). */
export class FrameParser {
  private buffer = "";

  /** Feeds one decoded chunk, giving every event a complete frame
   * carries. */
  feed(chunk: string): SseEvent[] {
    this.buffer += chunk;
    const events: SseEvent[] = [];
    for (;;) {
      const end = frameEnd(this.buffer);
      if (end === null) break;
      const frame = this.buffer.slice(0, end + 1);
      this.buffer = this.buffer.slice(end + 1);
      const event = parseFrame(frame);
      if (event) events.push(event);
    }
    return events;
  }
}

/** Where a frame ends and its delimiter does: the earliest `\n\n` or
 * `\r\n\r\n`. Frames end past the first newline of the delimiter. */
function frameEnd(buffer: string): number | null {
  for (let at = 0; at + 1 < buffer.length; at++) {
    if (buffer[at] === "\n" && buffer[at + 1] === "\n") return at + 1;
    if (
      buffer[at] === "\r" &&
      buffer[at + 1] === "\n" &&
      buffer[at + 2] === "\r" &&
      buffer[at + 3] === "\n"
    ) {
      return at + 3;
    }
  }
  return null;
}

/** One frame's event: `event:` and `data:` lines (`:` comments are
 * keep-alives of another kind). A `\r` left by a `\r\n` delimiter goes,
 * as Rust's `str::lines` strips it. An unknown event or an unreadable
 * payload is ignored, as the format's forward compatibility asks. */
function parseFrame(frame: string): SseEvent | null {
  let name = "";
  let data = "";
  for (const raw of frame.split("\n")) {
    const line = raw.replace(/\r$/, "");
    if (line.startsWith(":")) continue;
    if (line.startsWith("event:")) {
      name = line.slice("event:".length).replace(/^ +/, "");
    } else if (line.startsWith("data:")) {
      if (data) data += "\n";
      data += line.slice("data:".length).replace(/^ +/, "");
    }
  }
  if (name === "put" || name === "patch") {
    let payload: unknown;
    try {
      payload = JSON.parse(data);
    } catch {
      return null;
    }
    const path = readString(payload, "path");
    if (path === undefined || !hasOwn(payload, "data")) return null;
    return {
      kind: name === "put" ? "put" : "patch",
      path,
      data: (payload as { data: Json }).data,
    };
  }
  if (name === "keep-alive") return { kind: "keep-alive" };
  if (name === "cancel") return { kind: "cancel" };
  if (name === "auth_revoked") return { kind: "auth-revoked" };
  return null;
}

function readString(payload: unknown, key: string): string | undefined {
  if (hasOwn(payload, key) && typeof (payload as Record<string, unknown>)[key] === "string") {
    return (payload as Record<string, string>)[key];
  }
  return undefined;
}

function hasOwn(payload: unknown, key: string): boolean {
  return typeof payload === "object" && payload !== null && Object.hasOwn(payload, key);
}

/** Applies `event` to the listener's cache: the value of the ref the
 * stream listens at. put at `/` replaces it whole (that is how a
 * reconnect resyncs), put deeper sets one path (a null deletes), patch
 * merges keys. Returns the cache to store. */
export function applyEvent(cache: Json | null, event: SseEvent): Json | null {
  if (event.kind === "put") return setAt(cache, event.path, event.data);
  if (event.kind === "patch") {
    if (event.data === null || typeof event.data !== "object" || Array.isArray(event.data)) {
      return cache;
    }
    let at = cache;
    for (const [key, value] of Object.entries(event.data)) {
      at = setAt(at, `${event.path}/${key}`, value);
    }
    return at;
  }
  return cache;
}

/** Sets `value` at `path` (relative to the listen URL), creating the
 * objects along it (a scalar in the way becomes an object); a null
 * value removes what is there. The cache is copied before it is
 * touched, so every applied event yields a fresh reference — a signal
 * set to the same object would compare equal and report no change. */
function setAt(cache: Json | null, path: string, value: Json): Json | null {
  const parts = path.split("/").filter((part) => part !== "");
  if (parts.length === 0) return value === null ? null : value;
  const root: Record<string, Json> =
    cache !== null && typeof cache === "object" && !Array.isArray(cache)
      ? { ...cache }
      : {};
  let at: Record<string, Json> = root;
  for (const part of parts.slice(0, -1)) {
    const next = { ...objectAt(at[part]) };
    at[part] = next;
    at = next;
  }
  const last = parts[parts.length - 1];
  if (value === null) {
    delete at[last];
  } else {
    at[last] = value;
  }
  return root;
}

/** `at` as an object, a scalar in the way becoming an empty one. */
function objectAt(value: Json): Record<string, Json> {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value
    : {};
}
