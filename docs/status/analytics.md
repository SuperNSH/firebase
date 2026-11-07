# Analytics area status

## Done

Same behavior in Rust and TypeScript, over core's tier facade:

- `analytics(app)`: `logEvent(name, params?)`, `setUserProperty(key, value)`,
  `setUserId`, `logScreenView({screenName, screenClass?})`,
  `setConsent({analyticsStorage, adStorage?, adUserData?, adPersonalization?})`,
  `setDebugViewEnabled(bool)`. Client-side validation mirrors RNFB's
  `packages/analytics/lib/index.ts` (reserved names, `firebase_`/`google_`/`ga_`
  prefixes, `^[a-zA-Z][a-zA-Z0-9_]{0,39}$`) per docs/research.md §1.6; GA4
  reserves `google_` for user property keys, so that prefix is refused there.
- Batching queue: `flushIntervalMs` (10 000 default), `maxBatchSize` (20, under
  the Measurement Protocol's 25-event cap), bounded queue (500; oldest dropped,
  counted), size trigger sends as soon as a batch is full — on both tiers:
  Rust's `take_action` treats a full queue as due regardless of the schedule,
  and TS's `armTimer` arms a 0-delay tick for a leftover full batch, so
  back-to-back batches drain without waiting the interval. The scheduler is
  clock-driven: Rust's `Analytics::poll(now_ms)` / `flush()` take the clock
  reading from the caller (`ManualClock` in tests, `SystemClock` in an app);
  TS arms a `setTimeout` for the next due time, which the manual test clock
  fires only when `runFor` moves it. Retries with exponential backoff (1 s
  base, 30 s cap, 5 attempts, then the batch is dropped and counted).
- Consent: while `analyticsStorage` is `"denied"` nothing sends and new events
  are dropped and counted; an `adPersonalization` denial rides the batch as
  MP's `non_personalized_ads: true`. Re-granting resumes with the held batch.
- Transports: `Transport { send(batch) }`; in-memory recorder; GA4
  Measurement Protocol (POST `<base>/mp/collect?api_secret=…&measurement_id=…`
  with both query values percent-encoded, so a credential containing `&` or
  `=` cannot corrupt the URL; `client_id` from a stable UUID stored via the
  storage trait, `user_id`, MP-shaped `user_properties`, per-event
  `timestamp_micros`); collector transport posting NDJSON (one JSON object
  per event) to `http://127.0.0.1:18742/collect`; native routing through the
  tier facade's `firebase.analytics.*` handler names (`logEvent`,
  `setUserProperty`, `setUserId`, `logScreenView`, `setConsent`,
  `setDebugViewEnabled`) when the host answers and no explicit transport was
  configured — an explicit `mp`, `collector` or `transport` config key forces
  the pure queue even on a native tier. `NativeAnalytics.setConsent` posts
  all four consent keys with explicit nulls for unset optionals, the same
  shape as Rust. The TS `QueuedAnalytics.onSend` hook runs before the timer
  re-arms, and a throwing hook cannot skip the re-arm (nested finally).
- `setDebugViewEnabled` is a documented no-op on the pure tier (no SDK debug
  view to switch on); the native tier posts it through the facade.
- Client id: `ensure_client_id(storage)` / `ensureClientId(storage)`, key
  `nsh.firebase.analytics.client_id`, stable across clients over one storage.

Cancellation safety: the send-in-progress flag is a drop-guard (Rust
`SendingGuard`), the mutex is never held across the transport await, and a
poll interrupted mid-send leaves its batch in flight for the next poll's
retry (at-least-once; duplicates are possible by design and tolerated, as the
native SDKs tolerate them).

## Verified how

- Rust gate: `bash scripts/gate-crate.sh nsh-firebase-analytics` — fmt,
  clippy `-D warnings`, and 45 tests green (34 crate unit + 9 integration + 2
  engine UI). Integration tests in
  `crates/nsh-firebase-analytics/tests/analytics.rs` cover the six mandated
  scenarios on a `ManualClock` with the in-memory transport; `analytics_ui.rs`
  drives the collector transport through the engine's stubbed network
  (200 round trip and a refused send with events held).
- TSX gate: `bash scripts/gate-tsx.sh tests/analytics-app.tsx
  tests/analytics.test.tsx` — check, warning-free build, 14 tests green,
  re-run repeatedly for stability. Tests cover batching at maxBatchSize,
  interval flush (9 999 ms sends nothing, 10 000 does), leftover full batches
  draining back-to-back on the next tick, the consent gate, retry then
  success across the backoff, a throwing `onSend` hook not stalling the
  retry schedule, screen-view params, user property merge and `user_id`,
  reserved-name refusal, the collector transport's NDJSON body over the
  stubbed network, the MP transport's GA4 body (with percent-encoded
  credentials) over the stubbed network, an explicit mp config overriding
  native routing, native-tier routing with the normalized setConsent payload
  (`app.host.posts()`), and the no-transport construction error.
- Fail-first: Rust — with `QueueState::take_action` broken to never send, 8 of
  9 integration tests failed, then passed again restored; the percent-encoding
  test failed against the raw-interpolation URL
  (`api_secret=ab&c=x&measurement_id=m=1`) before the fix; the reclocked
  collector UI test was seen failing with its stub route broken. TSX — five
  targeted breaks each failed its test: transport stopped recording
  (batching), validation disabled (reserved names), collector URL wrong
  (collector), native handler name wrong (native routing), config silently
  defaulted a transport (no-transport error). Audit round 2: the consent
  merge test failed against the replace-whole-struct `set_consent` (the
  standing ad-personalization denial was lost on an analytics-storage-only
  update) before the merge fix; the NativeAnalytics tests failed with
  deliberately broken posts (raw consent payload, `screenClass` defaulting
  instead of nulling, validation skipped) — three of four failed at their
  assertions, then passed restored. For the round-1 audit fixes: with all
  five fixes reverted, the new/changed tests failed exactly as expected (MP
  encoding, leftover-batch drain "expected 3 to be 6", mp-overrides-native,
  throwing-hook stall, setConsent normalization) while the 9 pre-existing
  tests stayed green; the drain test asserts directly after `runFor` because
  `waitFor`'s frame-advancing would fire the full interval timer and mask
  the difference.
- Sandbox skip path: `tests/emulator/analytics.sandbox.test.tsx` runs with the
  collector port closed and stops honestly ("collector: unreachable" on
  screen; early return) — log at
  `docs/evidence/analytics/sandbox-skip.log`. `scripts/test-emulator.sh`
  skips the file outright while the port is closed.

## Not verified

- The collector round trip against the real `tools/collector`: perfmon has not
  built or started it yet (port 18742 refused at 2026-11-07T05:20Z), so the
  sandbox test never went past its honest skip and
  `docs/evidence/analytics/collector-assert.log` does not exist yet. The wire
  contract the test will assert (`/health`, NDJSON to `/collect`,
  `GET /assert?contains=<substr>&count=<n>` -> `{"ok":bool,"count":n,"matches":k}`)
  is the one this area was given; if the running collector differs, the test
  will show it.
- GA4 Measurement Protocol against Google: never in this sandbox by design;
  only the request shape is proven (unit tests on the request builders in
  Rust, a stubbed-network test in TSX pointing at a local stand-in base).
- The native tier's `firebase.analytics.*` routing against real iOS/Android
  hosts: proven in `nsh test` with the test playing the host; the native
  areas own the handlers.
- No Rust sandbox test against the live collector: the fetch-backed transport
  must run on the engine thread, and the TSX sandbox test covers the same
  contract.

## MP honest limits (docs/research.md §3)

The Measurement Protocol path delivers *events* only: no sessions, no
automatic IDFA/GAID or geo/device metadata (that joins only through an
`app_instance_id` the native SDK generated), no rule-based event creation or
renaming, no audiences that rely on automatic collection, 25 events and ~130 KB
per request, `timestamp_micros` no older than 72 hours. The queue batches 20
to stay under the caps.

## Notes for neighboring areas

- `packages/nsh-firebase/tests/nsh.json` and `tests/App.tsx` exist because
  `nsh test` resolves an app by walking up from the test file, and `open()`
  builds an entry with the `nsh.json` next to it: test apps opened from
  `tests/` run under those capabilities (network `127.0.0.1`, host
  `firebase.*`). The package-root `App.tsx` (core's entry) is untouched and
  `tests/core.test.tsx` still passes.
- The engine's `fetch` future is single-threaded, so `Transport::send`
  deliberately returns a non-`Send` future: await it on the engine thread
  (a component task's `spawn`), like `nsh_firebase_core::send`.
