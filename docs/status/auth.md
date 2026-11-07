# auth area status

Firebase Auth for NSH apps: Identity Toolkit sign-up/sign-in, Secure
Token refresh on a clock-based schedule, the `currentUser` signal, and
persistence behind a key-value store. Same behavior in Rust and
TypeScript, down to the persisted JSON (either tier restores the other's
record).

## Done

- `crates/nsh-firebase-auth`: `wire.rs` (the Identity Toolkit and Secure
  Token request shapes from docs/research.md §3 — URLs under the base
  with `?key=`, JSON bodies, the form-encoded refresh, reply parsing
  across both services' camelCase/snake_case, the base64url `sub`-claim
  read that flows without a `localId` need for the uid), `error.rs` (the
  `auth/*` stable codes and the wire-code table, including the
  `"CODE : human"` message form), `store.rs` (the `KeyValueStore` trait,
  `MemoryStore`, `EngineStore` over `nsh/storage`, the versioned
  camelCase session record), `auth.rs` (`Auth`: `createUserWithEmailAndPassword`,
  `signInWithEmailAndPassword`, `signInAnonymously` (accounts:signUp
  with no credentials — there is no `accounts:signInAnonymously`),
  `signInWithCustomToken`, `signInWithCredential` with `Credential::
  IdToken` sent to `accounts:verifyCustomToken`, `refresh_due`/
  `next_refresh_at_ms`/`refresh_now`, `restore`/`state_ready`,
  `signOut`; the `user()` signal; refresh is single-flight behind a
  drop-guard flag, and a session the server calls lost
  (`auth/user-token-expired`, `auth/invalid-refresh-token`,
  `auth/user-not-found` — on a refresh that is the Secure Token's
  deleted-account reply) signs out).
- `packages/nsh-firebase/src/auth`: `error.ts`, `wire.ts`, `store.ts`,
  `user.ts`, `index.ts` — the same names, codes, messages and record
  JSON; `Auth.currentUser` is the signal, `authStateReady()` the
  readiness promise (restore is async over `nsh/storage`; the Rust
  restore is sync, where `state_ready()` answers it). The default store
  is `StorageStore`; `MemoryStore` is exported for tests and cache-only
  apps.
- Refresh schedule: no timers anywhere. `expiresAtMs` and
  `nextRefreshAtMs` are absolute wall-clock ms computed from the app's
  clock (`nsh_core::time::unix_ms()` in Rust — the engine's manual clock
  in tests — `Date.now()` in TSX, which a manual test clock moves); the
  margin is 5 minutes (`REFRESH_MARGIN_MS`); `refreshDue()` is the
  schedule-check and `refreshNow()` works regardless of it.
- Tests, all fail-first (both tiers: broke the refresh margin and the
  EMAIL_NOT_FOUND mapping on purpose, saw exactly the schedule and
  mapping tests fail, then reverted):
  - Rust `crates/nsh-firebase-auth/tests/auth_ui.rs` (13): headless
    through the UI on a manual clock (`Clock::manual`), stubbed network,
    asserting request URLs and bodies, the persisted record, restore of
    a TSX-written record, sign-out, the lost-session sign-out, the
    network-refused code. Plus 17 in-crate unit tests for the pure
    layers.
  - TSX `tests/auth-app.tsx` + `tests/auth.test.tsx` (19): the same
    flows on `clock: { time: T0 }` with stubbed routes, asserting
    request bodies, `app.storage()` contents, and restore of a
    Rust-written record seeded through `storage`.
  - Sandbox, run only by `scripts/test-emulator.sh` against the suite on
    127.0.0.1 (`demo-nsh`): Rust `tests/emulator_auth.rs`
    (`#[ignore = "sandbox: needs emulator suite"]`) and TSX
    `tests/emulator/auth.sandbox.test.tsx` — sign up (or fall through to
    sign-in on an existing account), sign in, refresh with an expiry
    about an hour out, an unminted custom token refusing with a stable
    code live, and restore over the real store (Rust round 3: a reauth
    step builds a fresh Auth over `EngineStore` and reads the session
    the sign-in persisted; until then only the TSX run restored); the
    TSX run also asserts every live request stayed on 127.0.0.1:19099.

## Verified how

- `bash scripts/gate-crate.sh nsh-firebase-auth` — exit 0 (fmt, clippy
  `-D warnings`, tests: 17 unit + 13 UI + 1 ignored sandbox).
- `bash scripts/gate-tsx.sh tests/auth-app.tsx tests/auth.test.tsx` —
  exit 0 (`nsh check`, warning-gated `nsh build`, 19 tests).
- `bash scripts/test-emulator.sh` — exit 0; the auth sandbox tests pass
  against the live suite (auth 19099). Logs:
  `docs/evidence/auth/gate-crate.log`, `docs/evidence/auth/gate-tsx.log`,
  `docs/evidence/auth/test-emulator.log`.
- Wire shapes double-checked against the running emulator with curl and
  against `node_modules/firebase-tools/lib/emulator/auth/operations.js`
  (the emulator's own source for its custom-token audience and its
  JSON/JWT fake-token acceptance).

## Audit fixes (second pass)

- Secure Token `USER_NOT_FOUND` (the account was deleted) is now in both
  wire tables (`auth/user-not-found`, "no such account on the server")
  and in `is_session_lost`/`isSessionLost`: a refresh that meets it
  signs the user out on both tiers. The first pass had missed the plain
  wire code, so a deleted account stayed signed in. Seen failing first
  (Rust UI test + unit test, TSX test) and passing after; the Rust UI
  test is `a_deleted_user_signs_out_on_refresh`, the TSX one the same
  name, and the unit test `only_the_lost_session_codes_sign_the_user_out`
  asserts the new code.
- `parseSessionReply` (TSX) no longer lets a non-JSON 2xx body throw a
  raw SyntaxError: it throws `auth/invalid-response: the reply is not
  JSON`, matching Rust. Its `expiresIn` check mirrors the Rust u64
  `FromStr` exactly (round 3): an optional `+`, digits only, and a
  string-bound comparison against the u64 ceiling, so values past
  `Number.MAX_SAFE_INTEGER` parse like they do in Rust (to a float);
  `"99999999999999999999"` and one-past-`u64::MAX` are invalid-response.
  The margin subtraction clamps at 0 like the Rust `saturating_sub`.
  Seen failing first via "a non-JSON success reply is an
  auth/invalid-response", "an expiresIn of zero is accepted and one
  past u64 is not", and (round 3) "expiresIn parses like the Rust u64:
  plus sign and u64 ceiling".
- `StorageStore.get`/`remove` (TSX) swallow platform failures like `set`
  and the Rust `EngineStore` do: an unreadable store reads as signed out
  instead of rejecting `restore()` unawaited. Code-only change — the
  headless harness has no seam to make `nsh/storage` fail, so this one
  is verified by inspection against the Rust counterpart, not by a test.
- Round 2: `decodeSession` (TSX) treats a valid-JSON non-object record
  (`"null"`) as corrupt, like the Rust `as_object()` read — pinned by
  "a seeded null record restores as signed out, not a crash" (seen
  failing: the restore threw and the reauth line never landed);
  `parseSessionReply` (TSX) maps a JSON-null 2xx body to
  `auth/invalid-response` — pinned by "a JSON null success reply is an
  auth/invalid-response too" (seen failing: a raw TypeError leaked).
  Cross-area touch, made with the workflow's leave: one additive bullet
  for `accounts:verifyCustomToken` in docs/research.md §3 (research
  area's file); this file records it.
- Round 3: the Rust sandbox test performs the restore it names (a
  reauth button builds a fresh Auth over `EngineStore`; seen failing
  first as "no pressable view matches `id:reauth`", then passing live).
  `decodeSession` (TSX) now requires integer timestamps `>= 0`, like
  the Rust `as_u64` read — pinned by "a record with a negative
  timestamp restores as signed out" (seen failing: the corrupt record
  restored as signed-in). The lost-session comments in
  `Auth::refresh_now` (Rust) and `Auth.refreshNow` (TSX) name
  `auth/user-not-found` alongside the other two codes.

## Not verified / notes

- `signInWithCredential(Credential::IdToken)` is proven against stubs
  only. Its endpoint `accounts:verifyCustomToken` is in the documented
  production surface but the emulator suite does not serve it (404), and
  production `signInWithCustomToken` explicitly rejects ID tokens — so
  there is no live sandbox path for the foreign-ID-token exchange; a
  real check needs a project with Identity Platform.
- The custom-token sign-in flow is wire-verified live (a curl/python
  probe of `accounts:signInWithCustomToken` with the IdentityToolkit
  audience succeeds; recorded here, not scripted, because minting needs
  a per-run JWT), and the refusal path is verified in-app live; the full
  in-app custom-token success flow is stub-verified only.
- The unit tests open `tests/auth-app.tsx` directly, so `nsh` bundles
  them with no `nsh.json` next to the entry: capabilities are unset
  (everything declared) for those runs. The stub route list still gates
  every request, and the app's real declared posture is what the
  bundler enforces for package builds; a stricter arrangement would need
  an app-root entry file, which this area does not own.
- The network-refusal message differs slightly per tier: the TSX engine
  prefixes fetch failures with `Network request failed: `; the Rust stub
  delivers the failure text raw. The stable code (`core/network`) is the
  same, and that code — not the message — is the facade contract.
- Anonymous sign-up creates a real account in the running emulator suite
  each time the probe/test runs it; the suite is local and disposable,
  and the probe accounts this area created were deleted (one left by the
  sandbox run itself, which signs in rather than re-signing-up).
