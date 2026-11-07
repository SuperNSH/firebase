// The analytics clients, mirroring crates/nsh-firebase-analytics/src/
// client.rs. QueuedAnalytics owns the pure tier's batching queue (a
// manual-clock scheduler: timers only fire when the test moves the
// clock, and poll() drives sends directly); NativeAnalytics posts the
// same calls to the firebase.analytics.* handlers the native areas
// register.

import { getApp, type FirebaseApp } from "../core/app";
import { CODES, FirebaseError, toFirebaseError } from "../core/error";
import type { FirebaseTier } from "../core/tier";
import {
  DEFAULT_FLUSH_INTERVAL_MS,
  DEFAULT_MAX_BATCH_SIZE,
  DEFAULT_MAX_QUEUE_SIZE,
  DEFAULT_MAX_RETRIES,
  DEFAULT_RETRY_BASE_MS,
  DEFAULT_RETRY_MAX_MS,
  type AnalyticsConfig,
  type AnalyticsEvent,
  type AnalyticsKeyValueStorage,
  type Batch,
  type Consent,
  type ConsentStatus,
  type ResolvedAnalyticsConfig,
  type Transport,
} from "./types";
import { CollectorTransport, MeasurementProtocolTransport } from "./transport";
import { validateEventName, validateParams, validateUserProperty } from "./validate";

/** The firebase.analytics.* handler names the native areas register. */
export const NATIVE_LOG_EVENT = "firebase.analytics.logEvent";
export const NATIVE_SET_USER_PROPERTY = "firebase.analytics.setUserProperty";
export const NATIVE_SET_USER_ID = "firebase.analytics.setUserId";
export const NATIVE_LOG_SCREEN_VIEW = "firebase.analytics.logScreenView";
export const NATIVE_SET_CONSENT = "firebase.analytics.setConsent";
export const NATIVE_SET_DEBUG_VIEW = "firebase.analytics.setDebugViewEnabled";

/** The storage key the client id lives under, shared with Rust. */
export const CLIENT_ID_KEY = "nsh.firebase.analytics.client_id";

/** The process-lifetime store a client uses when the app passes none. */
export class InMemoryKeyValueStorage implements AnalyticsKeyValueStorage {
  private map = new Map<string, string>();

  get(key: string): string | null {
    return this.map.get(key) ?? null;
  }

  set(key: string, value: string): void {
    this.map.set(key, value);
  }
}

function resolveConfig(config: AnalyticsConfig): ResolvedAnalyticsConfig {
  const transport =
    config.transport ??
    (config.mp
      ? new MeasurementProtocolTransport(
          config.mp.apiSecret,
          config.mp.measurementId,
          config.mp.baseUrl,
        )
      : config.collector
        ? new CollectorTransport(config.collector.endpoint)
        : undefined);
  if (!transport) {
    throw new FirebaseError(
      CODES.invalidArgument,
      "no analytics transport configured; pass a transport, mp or collector",
    );
  }
  const resolved: ResolvedAnalyticsConfig = {
    transport,
    forcePure: config.forcePure ?? false,
    flushIntervalMs: config.flushIntervalMs ?? DEFAULT_FLUSH_INTERVAL_MS,
    maxBatchSize: config.maxBatchSize ?? DEFAULT_MAX_BATCH_SIZE,
    maxQueueSize: config.maxQueueSize ?? DEFAULT_MAX_QUEUE_SIZE,
    retryBaseMs: config.retryBaseMs ?? DEFAULT_RETRY_BASE_MS,
    retryMaxMs: config.retryMaxMs ?? DEFAULT_RETRY_MAX_MS,
    maxRetries: config.maxRetries ?? DEFAULT_MAX_RETRIES,
  };
  if (resolved.maxBatchSize < 1 || resolved.maxBatchSize > 25) {
    throw new FirebaseError(
      CODES.invalidArgument,
      "maxBatchSize must be between 1 and 25 (the Measurement Protocol's per-request limit)",
    );
  }
  if (resolved.maxQueueSize < resolved.maxBatchSize) {
    throw new FirebaseError(
      CODES.invalidArgument,
      "maxQueueSize must hold at least one batch",
    );
  }
  return resolved;
}

/** A UUID-v4-shaped identifier from Math.random: not cryptographic, a
 * per-install tag, enough to tell installs apart. */
function randomClientId(): string {
  const hex = Array.from({ length: 16 }, () =>
    Math.floor(Math.random() * 256).toString(16).padStart(2, "0"),
  ).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-4${hex.slice(13, 16)}-8${hex.slice(17, 20)}-${hex.slice(20, 32)}`;
}

/** The stored client id, or a freshly generated one written back. Two
 * clients over the same storage share one id, which is what makes the
 * GA4 stream see one install. */
export function ensureClientId(storage: AnalyticsKeyValueStorage): string {
  const existing = storage.get(CLIENT_ID_KEY);
  if (existing) return existing;
  const id = randomClientId();
  storage.set(CLIENT_ID_KEY, id);
  return id;
}

/** The user-facing analytics surface both tiers expose. */
export interface Analytics {
  logEvent(name: string, params?: Record<string, string | number | boolean>): void;
  setUserProperty(key: string, value: string | number | boolean): void;
  setUserId(userId: string | null): void;
  logScreenView(screenName: string, screenClass?: string): void;
  setConsent(consent: Consent): void;
  setDebugViewEnabled(enabled: boolean): void;
}

/** The native tier: validation first, then a post to the host. A
 * refused or failed post throws its mapped FirebaseError. */
export class NativeAnalytics implements Analytics {
  constructor(private readonly tier: FirebaseTier) {}

  logEvent(name: string, params?: Record<string, string | number | boolean>): void {
    validateEventName(name);
    validateParams(params);
    this.tier.post(NATIVE_LOG_EVENT, { name, params: params ?? {} });
  }

  setUserProperty(key: string, value: string | number | boolean): void {
    validateUserProperty(key, value);
    this.tier.post(NATIVE_SET_USER_PROPERTY, { key, value });
  }

  setUserId(userId: string | null): void {
    this.tier.post(NATIVE_SET_USER_ID, { userId });
  }

  logScreenView(screenName: string, screenClass?: string): void {
    this.tier.post(NATIVE_LOG_SCREEN_VIEW, { screenName, screenClass: screenClass ?? null });
  }

  setConsent(consent: Consent): void {
    // All four keys always present, null for unset optionals: the shape
    // Rust's NativeAnalytics::set_consent posts, so one handler can
    // match both tiers.
    this.tier.post(NATIVE_SET_CONSENT, {
      analyticsStorage: consent.analyticsStorage,
      adStorage: consent.adStorage ?? null,
      adUserData: consent.adUserData ?? null,
      adPersonalization: consent.adPersonalization ?? null,
    });
  }

  setDebugViewEnabled(enabled: boolean): void {
    this.tier.post(NATIVE_SET_DEBUG_VIEW, { enabled });
  }
}

type Post = { name: string; data: unknown };

/** A recording stand-in a demo screen hands to getAnalytics: every
 * batch lands in `posts`, where the UI (and a test) reads it, and
 * `onSend` fires after each recorded send. */
export class RecordingTransport implements Transport {
  readonly posts: Post[] = [];

  /** Arms one failure for the next send, the retry test's flaky
   * transport. */
  failNext = false;

  /** The demo screen's hook: runs after a batch was recorded. */
  onSend: (() => void) | null = null;

  send(batch: Batch): Promise<void> {
    if (this.failNext) {
      this.failNext = false;
      return Promise.reject(new FirebaseError(CODES.network, "transport down"));
    }
    for (const event of batch.events) {
      this.posts.push({
        name: event.name,
        data: {
          clientId: batch.clientId,
          userId: batch.userId,
          userProperties: batch.userProperties,
          params: event.params,
          timestampMs: event.timestampMs,
        },
      });
    }
    this.onSend?.();
    return Promise.resolve();
  }
}

/** The pure tier's client: validation, the consent gate, the bounded
 * batching queue with retrying sends, and the pluggable transport.
 * Sends happen in poll()/flush(); after each state change a timer is
 * armed for the next due time, which a manual test clock fires only
 * when runFor moves it. */
export class QueuedAnalytics implements Analytics {
  private queue: AnalyticsEvent[] = [];
  private inFlight: Batch | null = null;
  private attempt = 0;
  private nextSendAtMs: number;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private sending = false;
  private consentStatus: ConsentStatus = "granted";
  private adPersonalization: ConsentStatus | null = null;
  private userId: string | null = null;
  private userProperties: Record<string, { value: string | number | boolean }> = {};
  private droppedEvents = 0;
  readonly clientId: string;

  constructor(
    /** The app the client was built for (its options and event bus). */
    readonly app: FirebaseApp,
    private readonly config: ResolvedAnalyticsConfig,
    storage?: AnalyticsKeyValueStorage,
  ) {
    this.clientId = ensureClientId(storage ?? new InMemoryKeyValueStorage());
    this.nextSendAtMs = this.now() + this.config.flushIntervalMs;
  }

  private now(): number {
    // Follows the app's clock: real time in production, the manual
    // test clock under `nsh test`.
    return Date.now();
  }

  /** The UI's hook: runs after each poll attempt settled (sent or
   * failed), when pending/dropped are up to date. */
  onSend: (() => void) | null = null;

  logEvent(name: string, params?: Record<string, string | number | boolean>): void {
    validateEventName(name);
    validateParams(params);
    if (this.consentStatus === "denied") {
      // Collection is off: the event is dropped and counted, as the
      // SDK's own analytics_storage-denied behavior.
      this.droppedEvents += 1;
      return;
    }
    if (this.queue.length >= this.config.maxQueueSize) {
      this.queue.shift();
      this.droppedEvents += 1;
    }
    this.queue.push({ name, params: params ?? {}, timestampMs: this.now() });
    if (this.queue.length >= this.config.maxBatchSize) {
      this.nextSendAtMs = this.now();
    }
    this.armTimer();
  }

  logScreenView(screenName: string, screenClass?: string): void {
    const params: Record<string, string> = { screen_name: screenName };
    if (screenClass !== undefined) params.screen_class = screenClass;
    this.logEvent("screen_view", params);
  }

  setUserProperty(key: string, value: string | number | boolean): void {
    validateUserProperty(key, value);
    this.userProperties[key] = { value };
  }

  setUserId(userId: string | null): void {
    this.userId = userId;
  }

  /** The gate: while analyticsStorage is denied, nothing queued sends
   * and nothing new queues. */
  setConsent(consent: Consent): void {
    this.consentStatus = consent.analyticsStorage;
    this.adPersonalization = consent.adPersonalization ?? this.adPersonalization;
    this.armTimer();
  }

  /** Pure tier: a documented no-op. There is no debug view to switch on
   * without the SDK; the native tier routes it through the facade. */
  setDebugViewEnabled(_enabled: boolean): void {}

  /** Events held (queued or in flight). */
  get pending(): number {
    return this.queue.length + (this.inFlight ? this.inFlight.events.length : 0);
  }

  get dropped(): number {
    return this.droppedEvents;
  }

  get consent(): ConsentStatus {
    return this.consentStatus;
  }

  /** The scheduler tick: sends one batch if one is due (interval
   * elapsed, queue full, or a retry's backoff elapsed), gated on
   * consent. Concurrent calls are safe: only one send runs at a time,
   * and a batch whose poll was interrupted stays in flight for the
   * next poll's retry. */
  async poll(nowMs: number = this.now()): Promise<void> {
    return this.pollInner(nowMs, false);
  }

  /** Sends now, consent permitting, even if the interval has not
   * elapsed and the batch is partial. A batch already in flight keeps
   * its retry schedule. */
  async flush(): Promise<void> {
    return this.pollInner(this.now(), true);
  }

  private async pollInner(nowMs: number, force: boolean): Promise<void> {
    if (this.sending) return;
    this.sending = true;
    try {
      const batch = this.takeAction(nowMs, force);
      if (!batch) return;
      try {
        await this.config.transport.send(batch);
        this.inFlight = null;
        this.attempt = 0;
        this.nextSendAtMs = nowMs + this.config.flushIntervalMs;
      } catch (error) {
        this.recordFailure(nowMs);
        throw error instanceof FirebaseError
          ? error
          : toFirebaseError(error);
      }
    } finally {
      this.sending = false;
      try {
        this.onSend?.();
      } finally {
        // The schedule re-arms even if a consumer's hook throws, or a
        // queued batch / in-flight retry would stall with no tick.
        this.armTimer();
      }
    }
  }

  /** One due batch, moved out of the queue (or retried from
   * in-flight), or null. Consent gates everything. */
  private takeAction(nowMs: number, force: boolean): Batch | null {
    if (this.consentStatus === "denied") return null;
    if (this.inFlight) {
      // A retry waits for its backoff; a flush does not cut it short,
      // because the earlier batch may already have landed.
      return nowMs >= this.nextSendAtMs ? this.inFlight : null;
    }
    const full = this.queue.length >= this.config.maxBatchSize;
    if (this.queue.length === 0 || !(force || full || nowMs >= this.nextSendAtMs)) {
      return null;
    }
    const batch: Batch = {
      clientId: this.clientId,
      userId: this.userId,
      userProperties: { ...this.userProperties },
      nonPersonalizedAds: this.adPersonalization === "denied",
      events: this.queue.splice(0, this.config.maxBatchSize),
    };
    this.inFlight = batch;
    return batch;
  }

  /** Backoff after a failure; past the retry cap the batch is dropped
   * and the schedule resumes. Duplicates are possible either way: a
   * send that failed after landing retries by design, which native
   * analytics tolerates too. */
  private recordFailure(nowMs: number): void {
    if (!this.inFlight) return;
    this.attempt += 1;
    if (this.attempt > this.config.maxRetries) {
      this.droppedEvents += this.inFlight.events.length;
      this.inFlight = null;
      this.attempt = 0;
      this.nextSendAtMs = nowMs + this.config.flushIntervalMs;
      return;
    }
    const backoff = Math.min(
      this.config.retryBaseMs * 2 ** (this.attempt - 1),
      this.config.retryMaxMs,
    );
    this.nextSendAtMs = nowMs + backoff;
  }

  private armTimer(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    if (this.consentStatus === "denied") return;
    // Nothing pending, nothing to retry: no timer, or an empty overdue
    // schedule would re-arm itself at delay 0 forever. logEvent arms
    // the schedule again.
    if (this.queue.length === 0 && !this.inFlight) return;
    // A full batch is due now whatever the interval says: leftovers
    // drain back-to-back, as Rust's take_action does.
    const delay =
      this.queue.length >= this.config.maxBatchSize
        ? 0
        : Math.max(0, this.nextSendAtMs - this.now());
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.poll().catch(() => {});
    }, delay);
  }
}

/** The analytics for `app`: native routing when the host answers and no
 * explicit transport was configured, the queued pure path otherwise
 * (mirrors Analytics::new / Analytics::native in Rust). */
export function getAnalytics(
  app: FirebaseApp = getApp(),
  config: AnalyticsConfig = {},
): Analytics {
  const wantsPure =
    config.transport !== undefined ||
    config.mp !== undefined ||
    config.collector !== undefined ||
    config.forcePure === true;
  if (app.tier.kind === "native" && !wantsPure) {
    return new NativeAnalytics(app.tier);
  }
  return new QueuedAnalytics(app, resolveConfig(config), config.storage);
}
