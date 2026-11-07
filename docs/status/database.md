# database area status

Realtime Database over the documented REST surface, with live listeners
on Server-Sent Events, in Rust (`crates/nsh-firebase-database`) and
TypeScript (`packages/nsh-firebase/src/database`), same behavior name
for name (`r#ref`/`ref`, `on_value`/`onValue`).

## Done

- Refs and reads/writes: `Database::from_app` / `getDatabase` (the
  emulator switch wins over `databaseUrl`, mirroring core's
  `database_base_url`), `ref(path)`, `child`, `get`, `set`,
  multi-path `update` (PATCH, relative-path keys), `remove`, and
  `push` with client-generated ids.
- Push ids implement the official client's `nextPushId`
  (`@firebase/database`'s `packages/database/src/core/util/NextPushId.ts`,
  the keys the REST `POST` of the save-data docs describes): 8 characters
  of the millisecond timestamp in the web-safe-ordered 64-character
  alphabet plus 12 characters of randomness, lexicographically ordered,
  strictly increasing within one millisecond. The clock is the app's
  (`unix_ms()` / `Date.now()`), so a manual test clock makes same-ms
  pushes repeatable.
- Transactions: GET with `X-Firebase-ETag: true`, PUT with `if-match`,
  rebuild on the server's `412` (the next round's tagged read picks up
  what the server holds), at most 25 rounds (the SDK's cap); `apply`
  returning none/undefined aborts with `committed: false`.
- Queries: `orderByChild/Key/Value`, `startAt`, `endAt`, `equalTo`,
  `limitToFirst`, `limitToLast` encoded as REST parameters in a fixed
  order (`orderBy, startAt, endAt, equalTo, limitToFirst, limitToLast`,
  then `auth`), validation per the docs (one orderBy per request, every
  filter needs one, one limit), and a client-side sort on `get`
  (null < false < true < numbers < strings < objects; integer keys
  first, numerically), since the REST answers unsorted.
- Live listeners: one SSE stream per listen (`Accept:
  text/event-stream`), frames parsed incrementally (chunk and UTF-8
  boundaries safe, LF and CRLF delimiters both), `put`/`patch` applied
  to a per-listener cache that is copied before it is touched (so every
  applied event yields a fresh reference and a signal set reports the
  change), `keep-alive` ignored, `cancel`/`auth_revoked` or a failure
  status revoking the listener — and every stop cleans up like `off`:
  the self-stop paths cancel the open stream, and `connect` re-checks
  after storing a task, so a head delivered synchronously inside `open`
  (or an `off` in that window) leaves no live stream behind — with the
  value plus connection status flowing into signals (`on_value` in
  Rust, `onValue` in TSX, `cleanup`-off).
- An invalid query fails at subscribe time in both languages (Rust's
  `listen`/`on_value` return the `invalid-argument` error; TSX throws
  synchronously), never a panic or a silent reconnect loop.
- `getDatabase` hands back one database per app (a module-level map, as
  the modular SDK caches), so `setAuthToken` reaches every later
  `getDatabase(app)`; on the Rust side `Database::from_app` documents
  the same contract as hold-one-Arc — an auth token set on one Arc
  reaches only its own refs.
- Request paths are percent-encoded per segment (`a b` to `a%20b`, `a?b`
  to `a%3Fb`; the separating slashes stay), legal RTDB keys that would
  otherwise break the URL, with the same `encode_component` /
  `encodeComponent` both sides use for query values.
- A non-JSON 200 body maps to `core/network` with the message
  "the database sent invalid JSON: …" in both languages.
- Network loss, exactly: the stream ends or fails (or a non-2xx head —
  that revokes instead), the listener reports `reconnecting`, waits
  1 s after the first loss, doubling, capped at 30 s
  (`ReconnectPolicy` / `reconnectDelayMs`, no jitter, so a manual test
  clock is deterministic), and opens a new stream. The server answers
  a fresh stream with a `put` at `/` carrying the ref's whole current
  value, so the listener resyncs from the server's state rather than
  replaying what it missed; writes issued while offline are not queued
  (they reject). Documented in both crates' docs (`lib.rs` and
  `index.ts`) and here.
- Error mapping: transport failures and `{"error": ...}` bodies to the
  stable codes — `permission_denied` (401/403) to `core/access-denied`,
  400/404 to `core/invalid-argument`, everything else to
  `core/network`; a `412` is a retry signal, not an error.
- `?auth=<idToken>` on every request (the docs' query-parameter form);
  `set_auth_token` / `setAuthToken` is where the auth area's refreshed
  tokens land. No token is set anywhere in this repo (no credentials).

## The 0.4.0 constraint that shaped the Rust transport

nsh 0.4.0 gives a Rust app no incremental fetch: the engine's `fetch`
resolves with the whole body (`element::services::HttpResponse`), and a
live SSE stream never completes, so `prelude::fetch` cannot carry one.
The TSX side reads `response.body.getReader()` directly and needs no
injection. The Rust crate therefore takes the stream from the app:
`listener::StreamTransport` (an `open` + `StreamSink` + `StreamTask`
trio), injected at `Database::from_app`, mirroring core's `HostChannel`
precedent. The engine's own network bridges cross-thread delivery (the
engine marshals `FetchEvent`s onto the engine thread — that is what its
`Message::Fetch` channel does), which the labelled sandbox test uses.

## Verified how

- `bash scripts/gate-crate.sh nsh-firebase-database` — pass: fmt,
  clippy `-D warnings`, 40 unit tests (push format/ordering, query
  encoding/validation/sort, SSE parsing incl. split chunks and
  multi-byte splits, cache application, backoff series, listener state
  machine against a scripted transport with a held reconnect clock, a
  revoked stream being cancelled without the caller going off, URL
  building with params and `auth`, status mapping, transaction shapes)
  + 10 headless UI tests in `tests/database_ui.rs` on a manual clock
  with a stubbed network and a scripted stream transport: set/get round
  trip, multi-path update merge, remove, push ordering within and
  across milliseconds, transaction 412 rebuild (headers
  `x-firebase-etag` and `if-match` asserted on the recorded requests),
  query encoding (the stub route matches the URL whole) with
  client-side sort, a route miss mapping to `core/network`, a listener
  on an invalid query failing at subscribe time with
  `core/invalid-argument`, `getDatabase` handing back one database per
  app (the token set on the first instance reaches the second, read
  through a route matched whole with `?auth=tok-1`), a path with a
  space or a question mark percent-encoded (the route matches whole),
  listener receives-then-reconnects (opens
  asserted, resync from the reconnect's `put`), and off stopping the
  reconnect during the backoff.
- `bash scripts/gate-tsx.sh tests/database-app.tsx tests/database.test.tsx`
  — pass: `nsh check`, warning-gated `nsh build`, 17/17 headless tests
  in `tests/database.test.tsx` mirroring the Rust list plus: a patch in
  a later chunk re-rendering the listener text, a cancel stopping the
  listener without reconnecting, subscribe-time failure on an invalid
  query, a non-JSON body mapping to `core/network` with the Rust
  message, and the parser reading LF and CRLF frames alike (asserted
  directly against `FrameParser`) with a deep put and a patch yielding
  a fresh cache reference (the signal set compares references). The
  reconnect test uses the stub's clock-driven `delay` for the second
  stream.
- Labelled sandbox tests against the running emulator suite
  (`bash scripts/test-emulator.sh`), both green:
  `crates/nsh-firebase-database/tests/emulator_database.rs`
  (`#[ignore = "sandbox: needs emulator suite"]`; writes `sandbox-db/rust`,
  reads it back, subscribes `sandbox-db/watch`, and a second client —
  a blocking request over `nsh_engine::net::network()` — writes the
  watched path, arriving live over the emulator's real SSE stream;
  `docs/evidence/database/rust-sandbox.log`) and
  `packages/nsh-firebase/tests/emulator/database.sandbox.test.tsx`
  (`network: "live"`; the second client is a second app instance whose
  `write-watch` button writes the watched path — the test file itself
  has no network stack; `docs/evidence/database/tsx-sandbox.log`).
  Gate logs: `docs/evidence/database/{rust,tsx}-gate.log`.
- Every new test was seen to fail once by breaking the code on
  purpose; each failure named the right thing, then passed restored:
  - push: the same-millisecond increment disabled — unit
    `ids_within_one_millisecond_keep_increasing` and the UI push test
    failed on ordering.
  - query: the `orderBy` parameter renamed — the fixed-order encode
    test failed with the exact wrong string; parameter order reversed —
    the UI query test failed (stub route miss); client-side sort
    skipped — the UI query test failed showing the stub's order
    (`b d a`), in both languages.
  - SSE: `frame_end` returning none — five parser tests failed; the
    TSX parser likewise — the listener tests failed; the TSX `\r` strip
    dropped — the CRLF parser test failed (LF still parsed); the Rust
    `connect` post-store re-check disabled — the revoked-inside-open
    test failed (the task was never cancelled).
  - cache: patch replacing instead of merging — the SSE patch test and
    the listener patch test failed with merge diffs; the TSX cache
    copied-in-place fix reverted (mutation) — the fresh-reference test
    failed (`afterPut` was the same object).
  - transaction: `412` treated as an error — both UI transaction tests
    failed showing `core/network: 6 (412)` / `core/network (-)`.
  - listener: `lost` returning early — the reconnect tests failed
    (status never left `live`/`reconnecting` without a new stream);
    `off` made a no-op — the off tests failed (a reconnect opened);
    the `Accept: text/event-stream` header dropped — the listener URL
    assertion failed; the Rust revoke skipping the task cancel — the
    revoked-stream-cancellation unit test failed; the TSX revoke not
    stopping — the cancel test failed (a second connection opened);
    subscribe-time validation skipped — the invalid-query tests failed
    in both languages (Rust opened the listener on a bad URL, TSX
    stayed silent on the line); the TSX per-app cache dropped — the
    shared-database test failed (a tokenless second instance missed the
    `?auth` route); the TSX path encoding dropped — the encoded-path
    test failed (an unencoded `ui a?b` missed its route); the Rust path
    encoding dropped — the URL unit test failed showing `a b.json`.
  - mapping: `permission_denied` mapped to `core/network` — the status
    mapping test failed; the TSX invalid-JSON mapping dropped (raw
    SyntaxError) — the bad-json test failed on the message.
  - sandbox: the Rust test failed with refs ignoring their path
    (watching the root showed another path's value); the TSX sandbox
    failed with the `put` break above.

## Not verified

- Pixels: nothing here draws; the exercise UIs are driven headless and
  the screens are plain token-styled test buttons. No `nsh run` look —
  the demo area owns visible work.
- Rust live listeners through a real embedding host: the production
  `StreamTransport` for a Rust app must come from the host or the app's
  own wiring (see the constraint above); the sandbox test exercises the
  engine's network stack directly, which is what a native host would
  hand over.
- The reconnect against a real server's backoff timing (the emulator
  tests cover one reconnect each; the doubling series is pinned by the
  unit test on `ReconnectPolicy`/`reconnectDelayMs`, not by a long
  live run).
- Server-side `onDisconnect`, presence, and offline write queueing:
  intentionally absent (no documented REST surface); the docs say so.
- `auth_revoked` from a real server (no auth is configured in the
  sandbox; the frame is pinned by the parser and listener unit tests).
- The exact abort of an in-flight TSX stream after a revoke: the stub
  harness cannot keep a stream open, so the TSX cancel test pins the
  revoked state and the absence of a reconnect, while the abort call
  itself is pinned by reading `revoke()`'s cleanup, not by an
  observation; the Rust side observes the cancellation directly.
