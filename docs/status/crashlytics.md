# crashlytics status

Area: crates/nsh-firebase-crashlytics (Rust facade) +
packages/nsh-firebase/src/crashlytics (TypeScript facade) + their
headless tests. The native handlers themselves belong to native-ios /
native-android; this facade is the contract they implement.

Written under the battery constraint: **no compile, build, test or gate
command ran in this session.** CI executes the tests first — they were
written to pass on that first run (see "Verified how"). Everything
below is therefore "written and read-checked", not "proven".

CI run: <pending>

## The contract (both facades and both hosts are held to this verbatim)

- `firebase.crashlytics.log({message}) -> {}`
- `firebase.crashlytics.setUserId({id}) -> {}`
- `firebase.crashlytics.setCustomKey({key, value}) -> {}`
- `firebase.crashlytics.recordError({code, message, stack?}) -> {}`
- `firebase.crashlytics.sendUnsentReports() -> {}`
- `firebase.crashlytics.didCrashDuringPreviousExecution() -> {crashed: bool}`

Wire shapes, as implemented and tested: `log` carries `{message}` (the
breadcrumb is coerced to a string, as RNFB's log does); `setUserId`
carries `{id}` (an empty id clears, as the SDK's own call does);
`setCustomKey` carries `{key, value}` with value a string, number or
boolean — anything else refuses with `core/invalid-argument` before the
host (it would arrive across the wire as another type);
`recordError` carries `{code, message}` with `stack` added only when
the caller has one; `sendUnsentReports` and
`didCrashDuringPreviousExecution` carry no args. Replies: the five
act-only calls read no reply value, so either host's `{}`/`null`/`true`
resolves; `didCrashDuringPreviousExecution` reads `{crashed}` — a reply
without the flag is `core/host-error`, never a silent `false`. The same
table lives in crates/nsh-firebase-crashlytics/src/lib.rs (crate docs).

## The honest tier table (why the pure tier is a failing facade)

Crashlytics has no REST (research.md §4 row — updated this session,
the one cross-area touch). A pure tier that logged breadcrumbs locally
and called itself a buffer would be a fake crash reporter: the
product's meaning is reports reaching Firebase, and nothing pure can do
that. So:

- Pure tier: every call rejects with the product-scoped code
  `crashlytics/unavailable` (native code
  `ERR_FEATURE_UNAVAILABLE_ON_PLATFORM`) and `NEEDS_NATIVE_TIER`
  ("Firebase Crashlytics needs the native tier: iOS/Android host with
  firebase.crashlytics.* handlers (Crashlytics has no REST — no honest
  pure tier exists)"). No queue, no local persistence, no silent
  no-ops. Local validation still runs first (the bad custom-value bug
  is the caller's, whatever tier), matching messaging's order.
- Native tier: routing through the six `firebase.crashlytics.*`
  handlers above. The native SDK already captures crashes (fatals,
  signals, ANRs) automatically; the wrapper adds only the explicit
  calls. No host→app events exist: nothing flows back in this product.
- RNFB's `crash()`, `checkForUnsentReports()`, `deleteUnsentReports()`
  and `setCrashlyticsCollectionEnabled()` have no counterpart on
  purpose: that state machine belongs to the native SDK, and a facade
  method that kills the process is test tooling, not a library API.
  Migration delta vs RNFB: `recordError` takes the contract's
  `{code, message, stack?}` pieces instead of an `Error` instance.
- No sandbox test: the emulator suite has no Crashlytics service (tier
  table); the native proof is the SDK's debug logging, which needs a
  device — that row stays unverified-honest, capture protocol in
  docs/evidence/crashlytics/README.md.

## Done

- `crates/nsh-firebase-crashlytics` — the stub replaced wholly:
  `Crashlytics::new(Arc<dyn FirebaseHost>)`, `log()`, `set_user_id()`,
  `set_attribute()` (validates the value), `set_attributes()` (one call
  per key in order), `record_error()` over `RecordError { code, message,
  stack }`, `send_unsent_reports()`, `did_crash_on_previous_execution()`.
  A native-tier `core/unavailable` refusal (a host that never
  registered the handlers) is rewritten into
  `crashlytics/unavailable` + `NEEDS_NATIVE_TIER`; any other native
  code (a real `ERR_ACCESS_DENIED` refusal, a handler's failure) passes
  through unmapped. `wire` pins the six handler names; `UNAVAILABLE` is
  the product-scoped stable code (not `core/unavailable`: every
  degradable product reports the shared code, Crashlytics refuses with
  its own).
- `packages/nsh-firebase/src/crashlytics/index.ts` — the same facade in
  TypeScript over the app's tier (`getCrashlytics()` wraps
  `getApp().tier`), same names, same shapes, same pure-tier code and
  message, same validation rule, same missing-flag host error.
- `packages/nsh-firebase/src/crashlytics/app.tsx` + `nsh.json` — the
  tests' entry; this directory's nsh.json grants
  `capabilities.host: ["firebase.*"]` (same 0.4.0 mechanics as
  messaging: nsh reads the nsh.json next to the entry and grants host
  functions never without a declaration; the facade's tier decision is
  independent of the declaration — a run without host options exercises
  the pure tier genuinely).
- `packages/nsh-firebase/tests/crashlytics-app.tsx` — the exercise UI:
  every facade call on a button, errors shown with stable code, native
  code and message.
- `packages/nsh-firebase/tests/crashlytics.test.tsx` — 6 headless tests
  on a manual clock: native routing of all six calls with the wire
  names and args asserted via `app.host.calls()` (including
  `setAttributes` expanding to per-key calls in order, the stack riding
  only when given, no-arg calls carrying `null`);
  `didCrashDuringPreviousExecution` answering `{crashed: true}` and
  `{crashed: false}` through played handlers; a missing crashed flag
  surfacing as `core/host-error`; a failing handler surfacing its
  message as `core/host-error`; the bad custom value refused before the
  host (`calls()` empty); the pure tier rejecting every call with the
  naming error.
- `crates/nsh-firebase-crashlytics/tests/crashlytics.rs` — 13 tests:
  the wire-name table, log/user-id args, no-arg send, custom-key value
  types, per-key call order, non-scalar refusal before the host,
  recordError with and without stack, the crashed flag read both ways,
  the missing-flag host error, a refusal staying `core/access-denied`,
  a handler-less host rewritten to `crashlytics/unavailable`, the pure
  tier's rejection of every call, and pure-tier validation order. The
  stub channel plays the host through core's `NativeHost`, as a real
  embedding app would.
- `docs/research.md` §4 tier-table Crashlytics row — the single
  cross-area touch: the pure column now says the pure tier is a
  clearly-failing facade (`crashlytics/unavailable`, no fake recorder),
  the native columns name the six explicit calls and the debug-logging
  proof.
- `docs/evidence/crashlytics/README.md` — the CI gate commands and the
  native capture protocol; nothing captured yet (battery mode).

## Verified how

- Nothing was run: battery mode, no cargo/nsh/bun. CI runs first; the
  commands are `bash scripts/gate-crate.sh nsh-firebase-crashlytics`
  and `bash scripts/gate-tsx.sh src/crashlytics/app.tsx
  tests/crashlytics.test.tsx` (from packages/nsh-firebase).

  CI run: <pending>
- Read-verified API names, every one against current source, not docs:
  `FirebaseError` code/message/nativeCode and `CODES.invalidArgument`/
  `hostError`/`unavailable` plus `toFirebaseError`'s `ERR_*` mapping
  (packages/nsh-firebase/src/core/error.ts); `FirebaseTier.kind/call`
  and `PureTier`'s rejection (src/core/tier.ts); `getApp().tier` /
  `initializeApp` / `demoOptions` (src/core/app.ts, options.ts, and
  their use in tests/messaging-app.tsx); the Rust
  `FirebaseHost::available/call`, `NativeHost`, `PureHost`,
  `FirebaseError::from_host_code` and the pub error constants
  (crates/nsh-firebase-core/src/host.rs, error.rs — committed core);
  the test host's `open` options `clock`/`host.handlers` and
  `app.host.calls()` (proven in tests/messaging.test.tsx and
  tests/analytics.test.tsx, whose patterns this suite follows line for
  line); the RNFB surface (log, setUserId, setAttribute(s), recordError,
  sendUnsentReports, didCrashOnPreviousExecution, crash,
  checkForUnsentReports, setCrashlyticsCollectionEnabled,
  deleteUnsentReports) in
  ~/fo/.refs/react-native-firebase/packages/crashlytics/lib/index.ts
  and lib/types/crashlytics.ts.
- Written to pass on the first CI run: every TS assertion is against
  behavior the messaging suite already proved on this nsh version (the
  error formats, `app.host.calls()`'s `{name, data}` shape, the
  no-arg-call `data: null`, the entry-in-src mechanics, the pure tier
  from a host-less open); the Rust suite is the messaging suite's stub
  pattern with product types swapped. No assertion depends on behavior
  no area of this repo has demonstrated.
- Formatting caveat, stated honestly: the Rust files were hand-formatted
  against the already-gated messaging/analytics files (chain and call
  widths matched to their canonical shapes) because `cargo fmt` could
  not run. If `fmt --check` still flags a line, `cargo fmt -p
  nsh-firebase-crashlytics` is the normalize command — style only, no
  test or behavior affected.
- Fail-first is NOT claimed: the tests were written against the
  finished facades in one pass; there are no failing-first runs to
  cite, because nothing could run.

## Drift noticed in the native areas (they implement this same table)

Read-only check against the in-flight native work (not this area's
files): both hosts register exactly the six contract handler names;
`log`/`setUserId`/`setCustomKey`/`recordError` read the request keys
`{message}`, `{id}`, `{key, value}`, `{code, message, stack?}`; the
act-only calls resolve `true` (Android) / `{}` (iOS) — the facade reads
no reply value, so both resolve; `didCrashDuringPreviousExecution`
resolves `{crashed}` on both. One divergence the facade already
absorbs: Android's `setCustomKey` stringifies any non-boolean/non-number
value (`null` becomes `""`), silently swallowing what the caller sent,
while iOS passes scalars as-is — the facade's local scalar validation
(`core/invalid-argument` before the host) is what keeps that host
difference unreachable, the same reason messaging validates `show`
locally.

## Not verified

- Everything mechanical: fmt/clippy/tests, `nsh check`, the
  warning-gated `nsh build`, `nsh test` — all pending CI (above).
- Nothing on a device: the facade is proven (once CI runs) headless
  only. The native row is unverified-honest: the proof of reports
  reaching Firebase is the SDK's debug logging (logcat
  `-s FirebaseCrashlytics`, `FIRDebugEnabled`), which needs a device or
  simulator; protocol in docs/evidence/crashlytics/README.md.
  `didCrashDuringPreviousExecution → true` additionally needs a real
  previous crash, which the facade deliberately cannot force.
- The hosts' actual handler registrations: native-ios/native-android
  must register exactly the six `firebase.crashlytics.*` names above;
  the facade already absorbs their reply freedom (`{}`/`null`/`true`
  for the act-only calls) the way messaging absorbs `{shown}` vs the
  bare boolean.
- docs/STATUS.md (the root tracker) is not this area's file to edit;
  the coordinator records the area's row there.
