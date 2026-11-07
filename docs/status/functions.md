# functions area status

Cloud Functions (https.callable) for the pure tier, in Rust and
TypeScript: the callable wire protocol verbatim — POST
`{functionsEmulator}/{projectId}/{region}/{name}` with
`Content-Type: application/json` and body `{data}` (null when absent),
2xx answers read as `data` then the legacy `result` key, and errors
mapped from the HTTP status plus the body's
`{error: {status, message, details}}` to the SDK's `functions/*` codes —
plus native routing through `firebase.functions.call`. The wire shapes
were verified by reading the firebase-js-sdk functions layer and the
server-side firebase-functions source; the sources are quoted in
`docs/evidence/functions/wire-protocol.md`. This area was built under
the battery constraint: no command in this file was run here.

Cross-area touch (the one allowed outside this area's files): the
**Functions** row of the tier table in `docs/research.md` (the row's
pure-tier cell now states the shipped wire protocol and what is not
implemented).

## Done

- `crates/nsh-firebase-functions`: `wire.rs` (the URL shapes — emulator
  `{origin}/{projectId}/{region}/{name}`, production
  `https://{region}-{projectId}.cloudfunctions.net/{name}`, default
  region us-central1; the always-present `data` field; the canonical
  `error.status` map and the HTTP-status fallback table, including
  unknown canonical names failing as `functions/internal` with the
  SDK's message; `details` attached whenever the key exists, JSON null
  included; `FunctionsError` carrying code/message/nativeCode/details),
  `functions.rs` (`Functions::new(&app)` on the sandbox default
  emulator 127.0.0.1:15001, `with_config`/`with_auth`/
  `connect_emulator`, `https_callable(name).call(data)`, native routing
  through `with_host` per the contract below), inline unit tests in
  both (URL shapes, body shape, both answer keys, error mapping, status
  fallbacks, error round-trips; native routing via a RecordingHost:
  args shape, `{result}` unwrap, a handler without a result is
  `core/host-error`, a refusal keeps `ERR_ACCESS_DENIED`).
- Native handler contract (coordinator-fixed):
  - firebase.functions.call({name, data?}) -> {result} (region/host
    from config; the emulator serves 127.0.0.1:15001)
- `packages/nsh-firebase/src/functions`: `wire.ts`, `functions.ts`,
  `index.ts` — the same behavior name for name (TS shape:
  `new Functions(app, config?)`, `functions.httpsCallable(name)(data?)`
  or `functions.call(name, data?)`, `withAuth`,
  `connectFunctionsEmulator(host?, port)`, `FunctionsError extends
  FirebaseError` with `details`; the tier decides routing: native hosts
  answer `firebase.functions.call`, everything else POSTs).
- `tests/functions-app.tsx`: the exercise UI both TS test files drive;
  ids and lines carry the `fn` prefix so it can share an entry with
  other areas' UIs.
- Unit tests, headless on a manual clock with stubbed routes, no real
  time: Rust `crates/nsh-firebase-functions/tests/functions_ui.rs`
  (7 tests: the exact request — POST, URL, Content-Type, `{"data":{"a":1,"b":2}}`;
  absent data as `{"data":null}`; the HttpsError body mapping to
  `functions/invalid-argument` with its message and ` [400]` suffix;
  an unknown canonical `error.status` on a 200 mapping to
  `functions/internal` with the SDK's "Unknown backend error status"
  message; the emulator's 404 plain-text body as
  `functions/not-found`; `error.details` reaching the error; a
  `{data}`-shaped answer mapping) plus 14 inline `#[cfg(test)]` tests
  in `src/wire.rs` and 7 in `src/functions.rs` (the native-routing
  ones). TypeScript `tests/functions.test.tsx` (9 tests, the same
  coverage through the exercise UI, plus native routing: the handler
  takes `{name, data}`, answers `{result}`, absent data is null, no
  network request is made, and a throwing handler surfaces
  `core/host-error (ERR_NSH_HOST_ERROR)`).
- Labelled sandbox tests (runner-only), both listed in
  `scripts/test-emulator.sh`'s run loop already: Rust
  `crates/nsh-firebase-functions/tests/emulator_functions.rs`
  (`#[ignore = "sandbox: needs emulator suite"]`, run by
  `scripts/test-emulator.sh` with `--ignored`, live network through
  `UreqNetwork` to 127.0.0.1:15001 only, manual clock) and TS
  `packages/nsh-firebase/tests/emulator/functions.sandbox.test.tsx`
  (`network: "live"`, granted by `tests/nsh.json`'s 127.0.0.1 hosts):
  call `ping` with data and assert the echo `{pong: true, sent}`,
  call `add` and assert `{sum: 5}`, call `missing` and assert
  `functions/not-found` — each summary asserted as one exact line, so
  a wrong wire shape cannot pass. The suite serves `ping`/`add` from
  `emulator/functions/index.js` (firebase-functions v1,
  `firebase.json` functions port 15001).
- Tier table (honest): pure tier = full https.callable wire over fetch,
  emulator-supported; unauthenticated production callables work; the
  ID-token context header is supported via `withAuth`; NOT implemented,
  by design, and said nowhere else: the App Check and Firebase
  Instance-ID context headers (no app-check/messaging provider is
  wired), streaming callables (the SDK's `stream`, SSE), the 70 s
  client deadline (`functions/deadline-exceeded` — timeouts belong to
  the engine's transport), and the SDK's `@type` Int64Value
  encode/decode (callers pass JSON-safe data). Native tier = the
  handler contract above; error fidelity beyond `{result}` is the
  host's.

## Verified how

- Nothing yet on this machine: the area was written under the battery
  constraint (no compile, no build, no test, no emulator start). The
  wire protocol itself is verified by reading, with the exact lines
  quoted in `docs/evidence/functions/wire-protocol.md`:
  - `@firebase/functions` 0.14.0 (the bun cache mirror of the installed
    package), `_url`, `postJSON` (`Content-Type: application/json`),
    `callAtURL` (`{data}` body, `data`-then-`result` reads,
    "Response is missing data field."), `_errorForResponse`,
    `errorCodeMap`, `codeForHTTPStatus`, `FunctionsError`
    (`functions/<code>` prefix).
  - `firebase-functions` v1 `src/common/providers/https.ts` (GitHub):
    `res.status(200).send(responseBody)` with `responseBody = {result}`,
    errors as `{error: httpErr.toJSON()}` with `toJSON()` emitting
    `{status: <canonical name>, message, details?}`.
  - `firebase-tools` `lib/emulator/functionsEmulator.js` (the installed
    dev dependency): a missing function answers 404 with a plain-text
    `Function <triggerId> does not exist, valid functions are: ...`.
  - The emulator config: `emulator/firebase.json` functions port 15001,
    `.firebaserc` project `demo-nsh`, `emulator/functions/index.js`
    defining `ping` (echo) and `add` (sum, HttpsError invalid-argument).
- CI runs the gates first (battery mode): `cargo test -p
  nsh-firebase-functions` (14 wire + 7 service inline tests + 7 UI
  tests, the sandbox test ignored) and `nsh test
  tests/functions.test.tsx` are written to pass on the first run; the
  labelled sandbox tests then run via `bash scripts/test-emulator.sh`.
  CI run: <pending>

## Not verified

- Every runtime claim: no `cargo`/`nsh` command was run for this area
  on this machine (battery mode). The headless and sandbox tests above
  are unexecuted here; their first execution belongs to CI and the
  emulator runner. The repo's fail-once rule (see a new test fail by
  breaking the code on purpose) is therefore unmet here too — CI's
  first run is the earliest it can happen.
- The sandbox tests assume the emulator suite serves the v1 callables
  at `http://127.0.0.1:15001/demo-nsh/us-central1/{name}` as assembled
  from the SDK's `_url` and the suite's config; no live probe was made.
- `functions/ok` on a success body, `functions/unauthenticated`, and
  the remaining documented codes are unit-tested through the mapping
  table only, not against a live emulator error.
- Production callables: never contacted (no account, by design); the
  production URL shape is unit-tested URL building only.
