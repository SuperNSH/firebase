// The pluggable transports, mirroring
// crates/nsh-firebase-analytics/src/transport.rs: the in-memory recorder,
// the GA4 Measurement Protocol transport, and the collector transport
// posting NDJSON to tools/collector. All go through the app's fetch.

import { requestText } from "../core/http";
import { CODES, FirebaseError } from "../core/error";
import type { Batch, Transport } from "./types";

export const MP_BASE_URL = "https://www.google-analytics.com";
export const COLLECTOR_DEFAULT_ENDPOINT = "http://127.0.0.1:18742";

/** The GA4 Measurement Protocol request body. Honest limits, per
 * docs/research.md §3: events only — no sessions, no automatic IDFA/GAID
 * or geo/device metadata (that joins only through an app_instance_id the
 * native SDK generated), no rule-based event creation or renaming, no
 * audiences that rely on automatic collection. Requests are capped at 25
 * events and ~130 KB; this queue batches 20 to stay under both, and
 * timestamp_micros may not be more than 72 hours in the past. */
export function mpCollectUrl(
  baseUrl: string,
  apiSecret: string,
  measurementId: string,
): string {
  // Percent-encoded so a credential containing `&` or `=` cannot
  // corrupt the query; mirrors Rust's percent_encode_query.
  return `${baseUrl.replace(/\/+$/, "")}/mp/collect?api_secret=${encodeURIComponent(apiSecret)}&measurement_id=${encodeURIComponent(measurementId)}`;
}

export function mpCollectRequest(
  baseUrl: string,
  apiSecret: string,
  measurementId: string,
  batch: Batch,
): { url: string; body: string } {
  const body: Record<string, unknown> = {
    client_id: batch.clientId,
    user_properties: batch.userProperties,
    non_personalized_ads: batch.nonPersonalizedAds,
    events: batch.events.map((event) => ({
      name: event.name,
      params: event.params,
      timestamp_micros: event.timestampMs * 1000,
    })),
  };
  if (batch.userId !== null) body.user_id = batch.userId;
  return { url: mpCollectUrl(baseUrl, apiSecret, measurementId), body: JSON.stringify(body) };
}

/** One JSON object per event, newline-delimited: identity first, so a
 * collector can assert on a substring of one line. */
export function toCollectorLines(batch: Batch): string {
  return batch.events
    .map((event) => {
      const line: Record<string, unknown> = {
        client_id: batch.clientId,
        user_properties: batch.userProperties,
        name: event.name,
        params: event.params,
        timestamp_ms: event.timestampMs,
      };
      if (batch.userId !== null) line.user_id = batch.userId;
      return JSON.stringify(line);
    })
    .join("\n");
}

async function checkStatus(status: number, what: string): Promise<void> {
  if (status < 200 || status >= 300) {
    throw new FirebaseError(CODES.network, `${what} responded with HTTP ${status}`);
  }
}

/** Records every batch it is handed: the unit tests' transport and the
 * demo's console sink. */
export class InMemoryTransport implements Transport {
  private batches: Batch[] = [];

  send(batch: Batch): Promise<void> {
    this.batches.push(batch);
    return Promise.resolve();
  }

  recorded(): Batch[] {
    return this.batches;
  }

  clear(): void {
    this.batches = [];
  }
}

/** The collector transport: NDJSON lines to `tools/collector`'s
 * /collect, the sandbox's assertion sink. No Firebase account, no GA4. */
export class CollectorTransport implements Transport {
  readonly endpoint: string;

  constructor(endpoint: string = COLLECTOR_DEFAULT_ENDPOINT) {
    this.endpoint = endpoint;
  }

  async send(batch: Batch): Promise<void> {
    const response = await requestText(`${this.endpoint.replace(/\/+$/, "")}/collect`, {
      method: "POST",
      headers: { "Content-Type": "application/x-ndjson" },
      body: toCollectorLines(batch),
    });
    await checkStatus(response.status, "the collector");
  }
}

/** The GA4 Measurement Protocol transport. See mpCollectRequest for the
 * protocol's limits. */
export class MeasurementProtocolTransport implements Transport {
  constructor(
    private readonly apiSecret: string,
    private readonly measurementId: string,
    private readonly baseUrl: string = MP_BASE_URL,
  ) {}

  async send(batch: Batch): Promise<void> {
    const request = mpCollectRequest(this.baseUrl, this.apiSecret, this.measurementId, batch);
    const response = await requestText(request.url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: request.body,
    });
    await checkStatus(response.status, "the Measurement Protocol endpoint");
  }
}
