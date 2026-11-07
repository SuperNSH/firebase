// Live listeners over the fetch's streaming body: one SSE connection
// per listen, its events applied to a per-listener cache, and the
// honest behavior on network loss. Mirrors
// crates/nsh-firebase-database/src/listener.rs.
//
// On network loss — the stream ends, fails, or never answered with a
// success status — the listener reports "reconnecting", waits
// reconnectDelayMs and opens a new stream. The server answers a fresh
// stream with a put at `/` — the ref's whole, current value — so the
// cache is rebuilt from the server's truth, not patched on top of what
// the client guessed while offline. Locally issued writes do not queue
// offline: a write that cannot reach the server rejects. `cancel` or
// `auth_revoked`, or a stream answered with a failure status, stops
// reconnecting: the caller re-authenticates and listens again. This
// pure tier has no server-side `onDisconnect`: what the SDK registers
// there (a write or a presence flip when the connection drops) has no
// documented REST surface, and is not faked.

import { FrameParser, applyEvent } from "./sse";
import type { Json } from "./query";

/** A listener's connection state (the strings the Rust side labels). */
export type ListenStatus = "connecting" | "live" | "reconnecting" | "revoked";

/** What a listener reports: the ref's current value (none before the
 * first put) and the connection's state. */
export type Listener = (value: Json | null, status: ListenStatus) => void;

/** Stops a listener: its open stream is cancelled and no reconnect
 * fires. */
export interface ListenHandle {
  off(): void;
}

export interface ReconnectPolicy {
  baseMs: number;
  capMs: number;
}

/** The reconnect waits: one second after the first loss, doubling, at
 * thirty seconds at most (the SDK's order of magnitude, without its
 * jitter, to stay deterministic under a test clock). A stream that
 * opens resets the count. */
export const DEFAULT_POLICY: ReconnectPolicy = { baseMs: 1_000, capMs: 30_000 };

/** The wait before reconnect number `attempt` (1, 2, ...). */
export function reconnectDelayMs(attempt: number, policy: ReconnectPolicy = DEFAULT_POLICY): number {
  const shift = Math.min(attempt - 1, 31);
  return Math.min(policy.baseMs * 2 ** shift, policy.capMs);
}

export interface ListenOptions {
  /** The request URL, rebuilt per attempt so a refreshed auth token
   * lands in the reconnect's URL. */
  url: () => string;
  listener: Listener;
  policy?: ReconnectPolicy;
}

/** Starts a listener: connects at once, applies every stream event to a
 * per-listener cache, reports the value and the status through
 * `listener`, and reconnects per the policy after a loss. Off with the
 * returned handle. */
export function startListen(options: ListenOptions): ListenHandle {
  const policy = options.policy ?? DEFAULT_POLICY;
  const state = {
    cancelled: false,
    stopped: false,
    attempts: 0,
    cache: null as Json | null,
    status: "connecting" as ListenStatus,
    controller: null as AbortController | null,
    timer: undefined as ReturnType<typeof setTimeout> | undefined,
  };
  const report = () => options.listener(state.cache, state.status);
  // The self-stop paths share off()'s cleanup: without the abort, a
  // stream the listener has already given up stays open.
  const revoke = (): void => {
    state.stopped = true;
    state.status = "revoked";
    if (state.timer !== undefined) clearTimeout(state.timer);
    state.controller?.abort();
    report();
  };
  const connect = async (): Promise<void> => {
    if (state.cancelled || state.stopped) return;
    report();
    const controller = new AbortController();
    state.controller = controller;
    try {
      const response = await fetch(options.url(), {
        headers: { Accept: "text/event-stream" },
        signal: controller.signal,
      });
      if (state.cancelled || state.stopped) return;
      if (!response.ok || !response.body) {
        // A failure status (permission denied, auth rejected) is not a
        // network loss: retrying cannot succeed.
        revoke();
        return;
      }
      state.attempts = 0;
      state.status = "live";
      report();
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      const parser = new FrameParser();
      for (;;) {
        const { done, value } = await reader.read();
        if (state.cancelled || state.stopped) return;
        if (done) break;
        const events = parser.feed(decoder.decode(value, { stream: true }));
        if (events.length === 0) continue;
        for (const event of events) {
          if (event.kind === "cancel" || event.kind === "auth-revoked") {
            revoke();
          } else if (event.kind !== "keep-alive") {
            state.cache = applyEvent(state.cache, event);
          }
        }
        if (state.stopped) return;
        report();
      }
    } catch {
      // Aborted (off) or the stream broke: the loss path below.
    }
    lost();
  };
  const lost = (): void => {
    if (state.cancelled || state.stopped) return;
    state.attempts += 1;
    state.status = "reconnecting";
    report();
    state.timer = setTimeout(connect, reconnectDelayMs(state.attempts, policy));
  };
  connect();
  return {
    off(): void {
      state.cancelled = true;
      state.stopped = true;
      if (state.timer !== undefined) clearTimeout(state.timer);
      state.controller?.abort();
    },
  };
}
