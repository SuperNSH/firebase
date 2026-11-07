// The analytics data shapes, mirroring crates/nsh-firebase-analytics/
// src/model.rs name for name. Wire formats are snake_case (GA4
// Measurement Protocol's), the TS API is camelCase.

/** `granted` or `denied`, as setConsent takes them. */
export type ConsentStatus = "granted" | "denied";

export interface Consent {
  analyticsStorage: ConsentStatus;
  adStorage?: ConsentStatus;
  adUserData?: ConsentStatus;
  adPersonalization?: ConsentStatus;
}

export interface AnalyticsEvent {
  name: string;
  params: Record<string, string | number | boolean>;
  timestampMs: number;
}

/** What a Transport receives: one drained slice of the queue. */
export interface Batch {
  clientId: string;
  userId: string | null;
  /** MP-shaped: `{"key": {"value": ...}}`. */
  userProperties: Record<string, { value: string | number | boolean }>;
  /** From consent: an ad-personalization denial forces this true in the
   * MP body. */
  nonPersonalizedAds: boolean;
  events: AnalyticsEvent[];
}

/** The queue's sizing and retry policy. 20 per batch stays under the
 * Measurement Protocol's 25-event request limit. */
export interface AnalyticsConfig {
  transport?: Transport;
  /** GA4 Measurement Protocol credentials; builds the MP transport. */
  mp?: { baseUrl?: string; apiSecret: string; measurementId: string };
  /** The collector sink (`tools/collector`); builds the collector
   * transport posting NDJSON to `<endpoint>/collect`. */
  collector?: { endpoint: string };
  /** Use the queued pure path even when a native host answers. */
  forcePure?: boolean;
  flushIntervalMs?: number;
  maxBatchSize?: number;
  maxQueueSize?: number;
  retryBaseMs?: number;
  retryMaxMs?: number;
  maxRetries?: number;
  /** Client-id persistence; the app passes its platform store. */
  storage?: AnalyticsKeyValueStorage;
}

export const DEFAULT_FLUSH_INTERVAL_MS = 10_000;
export const DEFAULT_MAX_BATCH_SIZE = 20;
export const DEFAULT_MAX_QUEUE_SIZE = 500;
export const DEFAULT_RETRY_BASE_MS = 1_000;
export const DEFAULT_RETRY_MAX_MS = 30_000;
export const DEFAULT_MAX_RETRIES = 5;

/** Small key-value persistence the client id lives in; the auth-side
 * platform store fits. Infallible by contract. */
export interface AnalyticsKeyValueStorage {
  get(key: string): string | null;
  set(key: string, value: string): void;
}

/** One batch, retried by the queue on failure. The promise may be
 * engine-thread-bound (the app's fetch); it is never blocked on
 * synchronously. */
export interface Transport {
  send(batch: Batch): Promise<void>;
}

/** The config an app resolves to; a missing transport is refused at
 * construction rather than dropping events later. */
export interface ResolvedAnalyticsConfig {
  transport: Transport;
  forcePure: boolean;
  flushIntervalMs: number;
  maxBatchSize: number;
  maxQueueSize: number;
  retryBaseMs: number;
  retryMaxMs: number;
  maxRetries: number;
}
