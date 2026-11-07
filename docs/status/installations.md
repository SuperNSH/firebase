# installations status

Area: crates/nsh-firebase-installations (Rust) +
packages/nsh-firebase/src/installations (TypeScript) + their headless
tests. Pure tier over the Installations REST, plus the native tier's
routing; the native handlers themselves belong to native-ios /
native-android, and this facade is the contract they implement.

## The contract (verbatim, coordinator-fixed; also in the crate docs)

- `firebase.installations.getId()` -> `{id}` (rejects clearly under the
  fake project)
- `firebase.installations.getToken()` -> `{token}` (rejects clearly
  under the fake project)

Both facades and both hosts are held to this: the native tier answers
`{id}` / `{token}` with no args (the calls carry `null`), and a failure
under a fake project keeps the host's own error (stable
`core/host-error`, native code, message) — the same passthrough the
messaging area proved.

## Wire shapes, as implemented and tested

Verified against firebase-js-sdk `packages/installations` (read from
`src/functions/{create,generate-auth-token,delete}-installation-request.ts`,
`src/functions/common.ts`, `src/util/constants.ts`,
`src/helpers/generate-fid.ts`, `src/interfaces/api-response.ts`) and the
RNFB/FlutterFire installations packages (their facades delegate to the
SDKs; the web SDK is the wire source of truth):

- register: `POST https://firebaseinstallations.googleapis.com/v1/
  projects/{projectId}/installations`, headers `Content-Type:
  application/json`, `Accept: application/json`,
  `x-goog-api-key: {apiKey}`; body `{fid, authVersion: "FIS_v2",
  appId, sdkVersion: "w:0.1.0"}`; reply `{name?, fid?, refreshToken,
  authToken: {token, expiresIn: "<secs>s"}}` — a reply without `fid`
  keeps the generated one (the js-SDK's `responseValue.fid || fid`).
- token: `POST .../projects/{projectId}/installations/{fid}/
  authTokens:generate`, the same headers plus
  `Authorization: FIS_v2 {refreshToken}`; body
  `{installation: {sdkVersion, appId}}`; reply
  `{token, expiresIn: "<secs>s"}`.
- FID: 17 random bytes, `bytes[0] = 0b01110000 + (bytes[0] % 16)`,
  base64url cut to 22 characters — matches `/^[cdef][\w-]{21}$/`.
- errors: a 4xx body `{error: {code, message, status}}` maps to
  `installations/request-failed` with the js-SDK's message shape
  (`Create Installation request failed with error "403
  PERMISSION_DENIED: ..."`); a 5xx maps to
  `installations/internal-error`; transport failures keep the core
  codes (`core/network`, `core/access-denied`); a success reply missing
  a field maps to `installations/invalid-response`; a concurrent
  register rejects with `installations/register-in-progress`.

Two corrections to the coordinator's lead, checked against the SDK:
the FID is 22 characters (not 11), and `authTokens:generate` is a POST
authenticated by the `Authorization: FIS_v2 {refreshToken}` header (not
a GET with an authVersion header).

## Done

- `crates/nsh-firebase-installations` — the crate replacing the stub:
  `fid.rs` (the generation above, OS-seeded hashing standing in for a
  random-number crate, as the database push-id and analytics client-id
  do); `wire.rs` (the endpoints, request builders and reply parsers
  above, plus the `firebase.installations.getId`/`.getToken` handler
  name constants); `error.rs` (the codes and `reply_error` mapping);
  `store.rs` (`KeyValueStore` trait + `MemoryStore` + `EngineStore` over
  `nsh/storage`, the camelCase `{v, fid, refreshToken}` record both
  tiers read — a local trait of auth's shape, so installations does not
  depend on auth); `installations.rs` (`Installations::new(app, store,
  host)` restoring on init like auth, `get_id()`, `get_token(force)`,
  `id()`, `refresh_token()`, `token_due()`, `token_expires_at_ms()`,
  the in-memory auth-token cache with the js-SDK's one-hour
  `TOKEN_EXPIRATION_BUFFER`, and a single-flight register guard). The
  native tier (host `available()`) routes the two contract calls and
  persists nothing (the SDK owns its FIS ids); the pure tier drives the
  REST. No delete: the contract carries no delete handler, and the
  references' REST `DELETE .../installations/{fid}` is declared out of
  scope in the crate docs.
- `packages/nsh-firebase/src/installations` — the same behavior name
  for name in TypeScript: `index.ts` (`Installations`,
  `getInstallations()` memoized per app, awaiting the async restore in
  every flow; `restored()` replaces the Rust `restore_ready()` because
  the store here is async), `fid.ts` (over the app's
  `crypto.getRandomValues`), `wire.ts`, `error.ts`, `store.ts`
  (`StorageStore` over nsh Storage by default), `app.tsx` + `nsh.json`
  — the tests' entry, whose nsh.json grants the
  `firebaseinstallations.googleapis.com` network host and
  `firebase.*` (nsh reads the nsh.json next to the entry; a tests/
  entry would have neither grant — the same mechanism the messaging
  area measured).
- `crates/nsh-firebase-installations/tests/installations.rs` — 5
  headless tests: the native routing through a stub channel playing the
  contract handlers (id/token read, no args recorded, nothing
  persisted on the native path), the missing-field host errors, the
  fake-project host failure passthrough, and the refused-name denial.
- `crates/nsh-firebase-installations/tests/installations_ui.rs` — 6
  headless UI tests on a manual clock with a stubbed engine network:
  register request shape (URL, the three headers, body fields, FID
  format) + persistence + restore + cached-token reuse + clock-driven
  due-ness + `authTokens:generate` shape (`Authorization:
  FIS_v2 refresh-token-1`); force-refresh on an unregistered
  installation (registers first, then mints; then cache serves); a
  seeded camelCase record restoring with zero requests; the demo-
  project 403 mapping to the clear `installations/request-failed`
  message with nothing persisted; 500 → `installations/internal-error`;
  transport failure → `core/network`.
- `packages/nsh-firebase/tests/installations-app.tsx` +
  `tests/installations.test.tsx` — the exercise UI and 8 headless tests
  mirroring the Rust set, plus `app.host.calls()` pinning the native
  wire (names and `null` args), `app.requests()` pinning the REST wire,
  `app.storage()` pinning the persisted JSON, and the Rust-record
  restore test.

## CI run: <pending>

Battery mode (user instruction): no compile, build, test or gate
command ran locally for this area — not even once. CI executes these
tests first; they were written to pass on the first run, with every API
name read out of source before use (see Verified how). The gates CI
runs for this area:

- Rust: `bash scripts/gate-crate.sh nsh-firebase-installations` (fmt,
  clippy `-D warnings`, test) — also covered by ci.yml `rust-gate`'s
  `cargo test` over the workspace.
- TSX: `bash scripts/test-packages.sh`'s existing pair
  `tests/installations-app.tsx tests/installations.test.tsx` (via
  `scripts/gate-tsx.sh`: `nsh check`, warning-gated `nsh build`,
  `nsh test`). No runner script needed adding — the pair was already
  listed as a stub.

Evidence to save once CI runs: the Rust and TSX gate outputs as
`docs/evidence/installations/ci-rust.log` and
`docs/evidence/installations/ci-tsx.log` (the directory does not exist
yet — nothing was fabricated for it).

## Verified how (by reading, not by running)

- nsh 0.4.0 APIs from installed source: `FirebaseHost`/`NativeHost`/
  `PureHost` and `FirebaseError` from
  crates/nsh-firebase-core/src/{host,error}.rs; `HttpRequest`/
  `HttpBody`/`HttpResponse::{ok,status,text}` and `fetch` from
  nsh-core-0.4.0 src/platform.rs and src/element/{services,ambient}.rs;
  `unix_ms` from src/time.rs; `Storage::{get_item,set_item,remove_item}`
  from src/storage.rs; the test harness (`open`, `clock`, `network`
  stubs with `*` globs, `host.handlers`, `app.requests()`,
  `app.storage()`, `app.host.calls()`, regex `text:/.../` selectors)
  from nsh-testing-0.4.0 src/{headless/stub,module,selector}.rs and
  .nsh/types/nsh/test.d.ts; `Headless`/`StubNetwork`/`StubRoute`/
  `StubResponse` from nsh-testing; the capability grant path from
  nsh-engine-0.4.0 src/lib.rs (`request()` checks `allows_http` before
  the network stack, which is why the entry's nsh.json must grant the
  REST host even for stubbed routes) and nsh-core-0.4.0 src/access.rs
  (Rust test engines run unrestricted).
- REST shapes from firebase-js-sdk `main` (quoted in the wire section),
  cross-checked with RNFB
  (`~/fo/.refs/react-native-firebase/packages/installations/lib/index.ts`:
  `getId`, `getToken(forceRefresh)`, `delete`, `onIdChange`) and
  FlutterFire (`firebase_app_installations_web` delegates to the same
  js SDK).
- Patterns mirrored from this repo's landed areas: auth (store +
  restore-on-init + wire error mapping + manual-clock UI tests),
  messaging (host routing + stub channel + the exercise-app entry
  layout), analytics/database (OS-seeded randomness without a random
  crate).

## Cross-area touches (to record)

- docs/research.md: exactly one edit — the tier table's Installations
  row, rewritten from "REST ... works against a real project only" to
  the implemented state above. No other line of that file was touched.
- Drift noticed but NOT edited (not my row): research.md §"What cannot
  be done" still says "the GA4 MP and Installations transports exist
  but the sandbox asserts them against `tools/collector` only" — for
  Installations that is now stale (asserted against stubbed network,
  not the collector; the collector has nothing to assert about FIS).
  Owner of that section should split GA4 from Installations there.

## Not verified

- Everything that needs an execution: the gates above, all tests, the
  demo screen wiring (the demo area owns it), any device run. In
  particular the "rejects clearly under the fake project" half of the
  native contract is proven here only against a played host; on a real
  host it is the native areas' device proof, as with messaging tokens.
- The real endpoint was never contacted (no account, battery mode);
  every wire claim traces to the js-SDK source quoted above, not to a
  live response.
- `sdkVersion: "w:0.1.0"` is this repo's own value (the js-SDK sends
  `w:<its version>`); the server treats it as a client label, and both
  tiers' tests pin the exact string, so drift fails a test.
