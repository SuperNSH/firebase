// Firebase Analytics for Native Shell TSX apps: events, user
// properties, screen views, consent, the clock-driven batching queue,
// and the pluggable transports (GA4 Measurement Protocol with its
// limits, and the collector). Mirrors crates/nsh-firebase-analytics.
export {
  getAnalytics,
  NativeAnalytics,
  QueuedAnalytics,
  RecordingTransport,
  InMemoryKeyValueStorage,
  ensureClientId,
  CLIENT_ID_KEY,
  NATIVE_LOG_EVENT,
  NATIVE_SET_USER_PROPERTY,
  NATIVE_SET_USER_ID,
  NATIVE_LOG_SCREEN_VIEW,
  NATIVE_SET_CONSENT,
  NATIVE_SET_DEBUG_VIEW,
} from "./client";
export type { Analytics } from "./client";
export {
  CollectorTransport,
  InMemoryTransport,
  MeasurementProtocolTransport,
  mpCollectRequest,
  mpCollectUrl,
  toCollectorLines,
  COLLECTOR_DEFAULT_ENDPOINT,
  MP_BASE_URL,
} from "./transport";
export { validateEventName, validateParams, validateUserProperty } from "./validate";
export {
  DEFAULT_FLUSH_INTERVAL_MS,
  DEFAULT_MAX_BATCH_SIZE,
  DEFAULT_MAX_QUEUE_SIZE,
  DEFAULT_MAX_RETRIES,
  DEFAULT_RETRY_BASE_MS,
  DEFAULT_RETRY_MAX_MS,
} from "./types";
export type {
  AnalyticsConfig,
  AnalyticsEvent,
  AnalyticsKeyValueStorage,
  Batch,
  Consent,
  ConsentStatus,
  Transport,
} from "./types";
