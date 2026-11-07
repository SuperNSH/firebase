// The instrumented fetch wrapper: a drop-in for the global fetch (same
// `(input, init) => Promise<Response>` shape, `init.signal` passed
// through, so a `nsh-query` `fetch` option can take it) that records one
// NetworkEvent per exchange, even a failed one. Mirrors
// crates/nsh-firebase-perf/src/instrument.rs.

import { NETWORK_INITIATOR, nextEventId, splitHostPath, type NetworkEvent } from "./event";
import type { Monitor } from "./monitor";

// The JS fetch resolves when the response headers are ready (the body
// still streams afterwards), so this side has a ttfb and its durationMs
// covers start-to-headers; the monitor never reads the caller's body,
// which is why responseBytes needs a content-length to be known.
export function instrumentFetch(
  monitor: Monitor,
  fetchImpl: typeof fetch = fetch,
): (input: string | URL | Request, init?: RequestInit) => Promise<Response> {
  return async (input, init) => {
    const url = input instanceof Request ? input.url : String(input);
    const method = (init?.method ?? (input instanceof Request ? input.method : "GET")).toUpperCase();
    const body = init?.body;
    const requestBytes =
      typeof body === "string"
        ? new TextEncoder().encode(body).byteLength
        : body instanceof Uint8Array || body instanceof DataView
          ? body.byteLength
          : null;

    const startedAtMs = Date.now();
    const start = performance.now();
    try {
      const response = await fetchImpl(input, init);
      const ttfbMs = performance.now() - start;
      const contentLength = response.headers.get("content-length");
      monitor.record({
        id: nextEventId(),
        method,
        url,
        host: splitHostPath(url).host,
        path: splitHostPath(url).path,
        status: response.status,
        startedAtMs,
        durationMs: ttfbMs,
        ttfbMs,
        requestBytes,
        responseBytes: contentLength === null ? null : Number(contentLength) || null,
        error: null,
        initiator: NETWORK_INITIATOR,
      });
      return response;
    } catch (thrown) {
      const durationMs = performance.now() - start;
      monitor.record({
        id: nextEventId(),
        method,
        url,
        host: splitHostPath(url).host,
        path: splitHostPath(url).path,
        status: null,
        startedAtMs,
        durationMs,
        ttfbMs: null,
        requestBytes,
        responseBytes: null,
        error: thrown instanceof Error ? thrown.message : String(thrown),
        initiator: NETWORK_INITIATOR,
      });
      throw thrown;
    }
  };
}
