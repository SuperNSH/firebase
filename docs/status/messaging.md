# messaging status

Area: crates/nsh-firebase-messaging (Rust facade) +
packages/nsh-firebase/src/messaging (TypeScript facade) + their headless
tests. The native handlers themselves belong to native-ios /
native-android; this facade is the contract they implement.

## The contract (both facades and both hosts are held to this verbatim)

- facade API `messaging.permission()` / `messaging.requestPermission()` →
  `"default"|"granted"|"denied"|"unavailable"` (same strings as nsh
  Notifications); `messaging.show({id,title,body,silent?})` →
  `Promise<boolean>`; `messaging.close(id)`; `messaging.setBadge(n)`;
  `messaging.on("press", cb)` with `{id}`; `messaging.on("message", cb)`
  with the FCM message payload; `messaging.on("token", cb)` with
  `{token}`; `messaging.token()` → `Promise<string>` (rejects clearly when
  unavailable or the token request fails); `messaging.subscribe(topic)` /
  `messaging.unsubscribe(topic)`.
- host handlers the native tier registers (declare
  `"firebase.messaging.*"` in `capabilities.host`):
  `firebase.messaging.token`, `.permission`, `.requestPermission`,
  `.show`, `.close`, `.setBadge`, `.subscribe`, `.unsubscribe` (calls);
  host→app events: `firebase.messaging.message`,
  `firebase.messaging.tokenRefreshed`, `firebase.messaging.press`.

Wire shapes, as implemented and tested: `token` / `permission` /
`requestPermission` carry no args; `show` carries
`{id,title,body,silent}`; `close` carries `{id}`; `setBadge` carries
`{count}`; the topic calls carry `{topic}`. Replies read: `{permission}`,
`{token}`, `{shown}` — `show` also accepts the Android host's bare
boolean, and the act-only calls (`close`, `setBadge`, topics) read no
reply value, so either host's `null`/`true` resolves. Event payloads pass
through untouched. The same table lives in
crates/nsh-firebase-messaging/src/lib.rs (crate docs).

## Done

- `crates/nsh-firebase-messaging` — the facade replacing the stub:
  `Messaging::new(Arc<dyn FirebaseHost>)`, `token()`, `permission()`,
  `request_permission()`, `show()`, `close()`, `set_badge()`,
  `subscribe()`, `unsubscribe()`, `on/off` over a core `EventBus`, and
  `dispatch(event, payload)` — the seam a Rust embedding host forwards
  host→app events through (0.4.0 has no public `host.on` for Rust; TSX
  reaches `host.on` directly, so it needs no seam). `Permission` uses the
  unreleased Notifications module's strings; `MessageNotification` mirrors
  its `Notification::new/.silent`. A pure-tier refusal
  (`ERR_FEATURE_UNAVAILABLE_ON_PLATFORM`) is rewritten into
  `NEEDS_NATIVE_TIER` ("Firebase Messaging needs the native tier:
  iOS/Android host with firebase.messaging.* handlers", stable code
  `core/unavailable`); any other native code (a real `ERR_ACCESS_DENIED`
  refusal, a handler's failure) passes through unmapped. `on` rejects in
  the pure tier — Rust returns `Err(unavailable_error())` gated on
  `FirebaseHost::available`, TS throws the same error — because a
  listener that never fires is the silent no-op the contract forbids.
  Local validation (a non-empty `show` id and title, non-empty `close` id
  / topics) rejects with `core/invalid-argument` before any host call;
  `show`'s body may be empty, as the Notifications module it mirrors shows
  `body: ""` as none — that same module rejects an empty title exactly
  like an empty id (RangeError, nsh-main
  runtime/notifications.rs:151-154), so the facade does too.
- `packages/nsh-firebase/src/messaging/index.ts` — the same facade in
  TypeScript over the app's tier (`getMessaging()` wraps `getApp().tier`),
  same names, same shapes, same pure-tier error, same validations (the
  show rule above is enforced identically on both sides).
- `packages/nsh-firebase/src/messaging/app.tsx` + `nsh.json` — the tests'
  entry: mounts the exercise UI from tests/, and this directory's
  nsh.json grants `capabilities.host: ["firebase.*"]` (see "Why the entry
  lives in src/messaging" below).
- `packages/nsh-firebase/tests/messaging-app.tsx` — the exercise UI:
  every facade call on a button, the three host→app events behind a
  listen press, errors shown with code, native code and message.
- `packages/nsh-firebase/tests/messaging.test.tsx` — 5 headless tests on
  a manual clock: happy path through the test host with the wire names
  and args asserted via `app.host.calls()` (including a `show` with an
  empty body reaching the host, and the empty-title press refused with
  nothing recorded); token failure surfacing as
  `core/host-error` with the native message; press/message/token event
  delivery via `app.host.send`; the empty-topic refusal before the host;
  the pure tier failing every call plus `on` with the naming error. The
  show handler answers `{shown:true}`, `{shown:false}` and a bare `true`
  (both hosts' shapes).
- `crates/nsh-firebase-messaging/tests/messaging.rs` — 17 tests: token
  resolve/failure/missing-token, permission read/unknown-name, show
  shapes, validation (empty id or title refused before any host call,
  empty body allowed) and
  close/badge/topic pass-through (args asserted),
  event delivery with off and unknown-event `false`, the pure tier's
  message on every call plus the pure-tier `on` rejection, a refused host
  name staying `core/access-denied`,
  and the permission names. The stub channel plays the host through core's
  `NativeHost`, as a real embedding app would.

## Verified how

- Rust gate: `bash scripts/gate-crate.sh nsh-firebase-messaging` → fmt
  clean, `cargo clippy -p ... --all-targets -- -D warnings` exit 0,
  `cargo test -p nsh-firebase-messaging` 17/17 (after the auditor fixes;
  15/15 originally).
- TSX gate: `bash scripts/gate-tsx.sh tests/messaging-app.tsx
  tests/messaging.test.tsx` → exit 0 (`nsh check` clean, `nsh build`
  warning-free, `nsh test` 5/5). Earlier: 5/5 twice — before and after
  tests/ became an app dir (a concurrent area added tests/nsh.json +
  tests/App.tsx mid-flight).
- Fail-first, original suite: the TS tests were written first and run
  against the empty stub — "MISSING_EXPORT: getMessaging is not
  exported", 5/5 failed; the Rust tests were written first — `E0432:
  unresolved imports Messaging, Permission, Event, MessageNotification,
  NEEDS_NATIVE_TIER`. Both implementations were written after those
  failing runs.
- Fail-first, auditor fixes: (1) the pure-tier `on` rejection test was
  added first and failed to compile against the old API — "no method
  `expect`/`unwrap_err` found for struct `ListenerId`" (5 errors) — then
  the trait method and the Result signature landed; (2) round 1's id-only
  show rule was superseded by the reference rule (empty id or title
  rejected, empty body allowed): Rust's `show_needs_a_title` failed first
  against the id-only rule (`unwrap_err` on `Ok(false)`) and
  `show_needs_its_content` on the old message; TS's empty-title press
  passed through as `true` before the refusal landed; the empty-body
  cases were then seen failing under a deliberately re-added
  empty-body rejection on both sides, before it was removed again.
- Every API name verified in 0.4.0 source before use: `host.call/post/on`
  and the `ERR_*` codes from
  packages/nsh-firebase/.nsh/types/nsh/index.d.ts (generated); the test
  host (`open`'s `host.handlers`, `app.host.calls/.send`) from
  .nsh/types/nsh/test.d.ts; `FirebaseHost`/`HostChannel`/`NativeHost`/
  `PureHost` from crates/nsh-firebase-core/src/host.rs (this repo, core
  area, committed).

## Core file touched (for core's review)

The pure-tier `on` gate needed the tier to be observable, and 0.4.0's
`FirebaseHost` had no way to ask. One additive change in
crates/nsh-firebase-core/src/host.rs (everything else in that file is
untouched):

- `FirebaseHost::available(&self) -> bool`, a defaulted method returning
  `true`, documented as "the pure tier's is false, and a product that
  cannot degrade (messaging) gates on it instead of leaving its listeners
  silent";
- `impl FirebaseHost for PureHost` overrides it to `false`.

`NativeHost` keeps the default (`true`). Every other implementor in the
repo (analytics, perf, the core tests) compiles unchanged because the
method has a default. Core's own gate (`bash scripts/gate-crate.sh
nsh-firebase-core`) still passes with the change; the messaging crate's
gate compiles core as its dependency.

Audit round 3 touched one more line of that file, also doc-only: the
trait doc's example handler `firebase.messaging.onToken` — a name no
host registers and the contract table does not contain — became
`firebase.messaging.token` (token refresh arrives only as the
`firebase.messaging.tokenRefreshed` event). Both gates re-run green
after it.

## Why the entry lives in src/messaging (0.4.0 mechanics, measured)

- nsh reads the `nsh.json` **next to the entry** only
  (nsh-bundler/src/bundle.rs: "The `"capabilities"` of the `nsh.json`
  next to the entry"), and `Capabilities::all()` grants no host functions
  ("The host's functions are never granted without a declaration",
  nsh-core access.rs). A tests/ entry therefore has every `host.call`
  refused with `ERR_ACCESS_DENIED` — the first test run proved it.
- `nsh test <file>` resolves the app dir by walking up from the test file
  to the nearest directory with an nsh.json and requires an `App.tsx`
  there (nsh-cli test.rs `app_entry`) — so a bare `tests/nsh.json`
  without `tests/App.tsx` breaks the test discovery of *every* test file
  in tests/. That happened mid-flight when another area added one; their
  `tests/App.tsx` (10:03) resolved it.
- The facade's tier decision is independent of the declaration: the test
  host's `host.available()` is the engine channel's `configured` flag,
  true only when `open` was given `host` options — so a run without
  `host` options exercises the pure tier genuinely.

## Drift noticed in the native areas (they implement this same table)

Both hosts' handler names, request keys and the three event names match
the contract. The differences the facade already absorbs:

- `show`'s reply: iOS resolves `{shown: bool}`; Android resolves the bare
  boolean. Facade accepts both. The contract pins nothing here, but a
  native-side change to either shape stays safe only while it is one of
  these two.
- Act-only calls: iOS resolves `null` for `close`/`setBadge`, Android
  `TRUE`. The facade reads no reply value for them; the topic calls
  differ the same way (`{topic}` vs `TRUE`).
- `firebase.messaging.message` payloads differ slightly: iOS's
  serializer adds `messageType`/`collapseKey`/`from`/`to`; Android sends
  `notification: null` explicitly and omits the message-level extras.
  Both ride through the message handler unchanged; the TS `RemoteMessage`
  interface documents the union.
- Android's `show` does not reject on a missing or empty id/title (it
  silently no-ops them); iOS rejects. The facade validates locally — a
  non-empty id and title, per the reference rule — so no host ever sees
  what Android would swallow — this is why the facade's
  `core/invalid-argument` check exists.

## Not verified

- Nothing on a device: the facade is proven headless. The device phase
  (real APNs/FCM token requests, notification taps) belongs to the
  native areas' verifiers; token *success* with a fake project is
  expected to fail there and surface through the same
  `core/host-error` path the token-failure test proves.
- The whole-package gate is shared: `gate-tsx.sh`'s `nsh check` covers
  every area's files, and other areas were committing mid-run. My scoped
  evidence is a clean `nsh check` in a quiet window plus a green
  `nsh test` and `nsh build`; a single atomic `gate-tsx.sh` pass with all
  areas landed is the smoke gate's job, not re-proven here.
- `dispatch` (the Rust host→app seam) has no real Rust host calling it
  yet — examples/demo-rs is still a stub; the tests drive it directly.
