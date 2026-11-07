# app-check status

Area: crates/nsh-firebase-app-check (Rust client) +
packages/nsh-firebase/src/app-check (TypeScript facade) + their headless
tests. The native handler itself belongs to native-ios / native-android;
this area is the contract they implement.

## The contract (both facades and both hosts are held to this verbatim)

- `firebase.appCheck.getToken({debug?}) -> {token}` (debug provider when
  debug is true; rejects clearly when the exchange fails — which it does
  under the fake project)

Wire shapes, as implemented and tested: the call carries `{debug: bool}`
(`false` when the flag is absent); the reply reads `{token}` (a missing
or empty token is `app-check/invalid-response` on the facade side). The
native reply carries no ttl, so the facade returns
`{token, expiresAtMs: null}` there; the pure tier returns
`{token, expiresAtMs}` from the exchange's ttl. The same table lives in
crates/nsh-firebase-app-check/src/lib.rs (crate docs).

## The honest tier table (docs/research.md §4 row, updated)

DeviceCheck/App Attest (iOS), Play Integrity (Android) and even the
debug provider's exchange against firebaseappcheck.googleapis.com all
need the real backend — the emulator suite has no App Check service and
`demo-` projects refuse. The pure tier implements the debug-token
exchange REQUEST faithfully (POST with `{"debug_token": ...}`, the one
provider that needs no device attestor, only a token registered in a
real Firebase console), caches the reply `{token, ttl}` for its ttl, and
maps the failure to a clear `FirebaseError`; it never fakes a token.
`getToken` without `debug:true` rejects with `app-check/needs-attestation`
in the pure tier and routes to the host in the native tier. The honest
native proof is the SDK-level debug logging on a device — unverified
here, owned by the native areas' device phase.

## Stable codes

- `app-check/invalid-argument` — empty debug token, before any request.
- `app-check/needs-attestation` — pure-tier getToken without `debug:true`.
- `app-check/invalid-debug-token` — the exchange refused (HTTP 400-403);
  Google's `error.message` rides in the message text.
- `app-check/exchange-failed` — any other non-2xx.
- `app-check/invalid-response` — a 2xx reply without a usable
  `{token, ttl}`, or a native reply without a token.
- Transport failures keep the core's codes (`core/network`,
  `core/access-denied`); host failures map through `core/host-error` /
  `core/unavailable` as everywhere else.

## Wire facts (verified by reading, cited)

- Request: `POST https://firebaseappcheck.googleapis.com/v1/projects/{project}/apps/{appId}:exchangeDebugToken?key={apiKey}`,
  header `Content-Type: application/json` only, body
  `{"debug_token": "<secret>"}` (snake_case is what every client SDK
  sends; the REST reference's camelCase `debugToken` is the same proto
  field). Sources: firebase-js-sdk packages/app-check/src/client.ts
  (`getExchangeDebugTokenRequest`, read via the GitHub API) and
  constants.ts (`EXCHANGE_DEBUG_TOKEN_METHOD`); the REST reference page
  (firebase.google.com/docs/reference/appcheck/rest/v1/projects.apps/
  exchangeDebugToken, fetched: path `projects/*/apps/*`, project id may
  replace the project number).
- No `X-Firebase-AppCheck` header on the exchange — that header is what
  calls to protected backends attach afterwards (asserted in the tests).
- Reply: `{"token": "...", "ttl": "1800s"}` — `ttl` is a Protobuf
  Duration string; expiry = now + ttl*1000 (the JS SDK parses it the
  same way, `^([\d.]+)(s)$`).
- The client SDKs' exchange base is the `content-` frontend variant;
  this repo uses the REST reference's plain endpoint, and both tiers
  accept a `baseUrl` override (the sandbox points it at a local
  stand-in, never at Google).

## Done

- `crates/nsh-firebase-app-check` — the crate replacing the stub:
  `AppCheck::new(app, store, options)` / `with_parts` (any exchange,
  clock, store), `get_token(debug)`, cache restore on init and refresh
  past expiry on the clock; `NativeAppCheck::new(host).get_token(debug)`
  routing `firebase.appCheck.getToken`; `Exchange` trait with
  `FetchExchange` (engine fetch, engine-thread only) and
  `RecordingExchange` (tests + an offline shape display); `Clock` with
  `SystemClock`/`ManualClock`; `KeyValueStore` with `MemoryStore`/
  `EngineStore` and the camelCase cache record
  (`nsh.firebase.app-check.{app}`, `{v, token, expiresAtMs}` — either
  tier restores the other's record); wire + error modules with unit
  tests.
- `packages/nsh-firebase/src/app-check` — the same facade in TypeScript
  (`AppCheck`, `getAppCheck(app?, options?)`, same codes, same messages,
  same cache record), tier decided by `app.tier.kind` (native routes the
  handler; pure exchanges), store over `nsh/storage` by default.
- `packages/nsh-firebase/tests/app-check-app.tsx` +
  `tests/app-check.test.tsx` — the exercise UI and 13 headless tests
  (listed below).
- `crates/nsh-firebase-app-check/tests/app_check.rs` — 5 integration
  tests: 3 play the host through core's `NativeHost` with a scripted
  channel (as a real embedding app would), 2 drive the pure tier's
  public API.

## What the tests pin (all headless: manual clock, stubbed network/host)

- Debug exchange request shape: URL
  (`.../v1/projects/demo-nsh/apps/{appId}:exchangeDebugToken?key=...`),
  POST, `Content-Type: application/json`, body
  `{"debug_token":"demo-nsh-fake-debug-token"}`, and no
  `X-Firebase-AppCheck` request header (Rust: unit + integration;
  TSX: `app.requests()`).
- Reply mapping `{token, ttl}` → token + expiry (Rust unit + integration;
  TSX: the `expires:` line equals T0 + ttl).
- Cache hit before expiry, refresh at/after expiry on the manual clock,
  restore of a stored record with zero requests, corrupt/expired records
  exchanged over, nothing cached from a failed exchange.
- Failure mapping: 403 → `app-check/invalid-debug-token` with the
  server's message, 500 → `app-check/exchange-failed`, bad ttl →
  `app-check/invalid-response`, network failure → `core/network`.
- `needs-attestation` and empty-debug-token refusals before any request.
- Native routing: call name/args (`{debug:true}` / `{debug:false}`),
  `{token}` reply read, missing token → clear error, handler failure →
  `core/host-error` with the host's message, no host → `core/unavailable`
  naming what is missing.
- The default exchange URL pins the real endpoint (TSX text assert).

## Verified how

- Nothing yet on this machine — the build is on battery and compile,
  test and gate commands are forbidden. **CI run: <pending>**; the first
  CI gate run is expected to be the first run of every test here (they
  are written to pass on it), and its outputs go to
  docs/evidence/app-check/ (empty now).
- Verified by reading instead: every API name checked in source before
  use — `HttpRequest`/`HttpBody`/`HttpResponse` (status/ok/text) in
  nsh-core 0.4.0 (`platform.rs`, `element/services.rs`),
  `FirebaseHost`/`HostChannel`/`NativeHost`/`PureHost` and
  `send`/`join_url` in crates/nsh-firebase-core (this repo), the test
  harness (`open` options `clock/network/storage/host`,
  `app.requests`/`app.storage`/`app.host.calls`, `StubRequest.headers`,
  `clock.setSystemTime`) in packages/nsh-firebase/tests/.nsh/types/nsh/
  test.d.ts, `Storage`/`mount`/`signal`/`Text`/`View` from the TSX side
  of sibling areas (auth store.ts, analytics-app.tsx), and the exchange
  shapes from the firebase-js-sdk source + REST reference above.

## Not verified

- No compile, no test run, no gate (battery mode): `cargo check/test`,
  `nsh check/build/test` all pending CI. The TSX tests in particular
  carry long exact error-message expectations; if CI shows a wording
  drift, the fix belongs on the facade side, not the test.
- Nothing on a device: no real attestation, no real exchange (no
  account, no network to Google), no emulator service for App Check at
  all. The native proof is the SDK's debug logging on a device and
  belongs to the native areas.
- The Rust-side `EngineStore`/`FetchExchange` paths are unexercised by
  tests by construction (they need the engine's fetch/storage); the
  TSX `StorageStore` path is exercised through the harness's
  nsh/storage. Mirror-code review is the only cross-check until CI.

## Cross-area touch recorded

One line outside this area's files: the App Check row of the tier table
in docs/research.md (§4), rewritten from "pure tier = disabled with a
clear error" to the honest debug-exchange row above. Nothing else
outside crates/nsh-firebase-app-check/**,
packages/nsh-firebase/src/app-check/**,
packages/nsh-firebase/tests/app-check-app.tsx,
packages/nsh-firebase/tests/app-check.test.tsx, docs/status/app-check.md
and docs/evidence/app-check/ was touched.
