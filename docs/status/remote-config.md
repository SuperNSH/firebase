# remote-config area status

Firebase Remote Config for the pure tier, in Rust and TypeScript, with
an honest tier split: defaults, typed reads, template
parse/validation, and a fetch that refuses instead of pretending. The
REST API (`firebaseremoteconfig.googleapis.com`) is authorized with an
OAuth 2.0 access token from a service account (scope
`https://www.googleapis.com/auth/firebase.remoteconfig`), not the web
API key (per https://firebase.google.com/docs/remote-config/automate-rc,
quoted and linked in the tier table), so a pure-tier NSH app cannot
fetch a real template at all, and the emulator suite has no Remote
Config service — nothing here fakes a fetch, and there is no sandbox
test for that reason. This area was built under the battery constraint:
no command in this file was run here; the template and wire shapes were
verified by reading (sources below).

Cross-area touch (the one allowed outside this area's files): the
**Remote Config** row of the tier table in `docs/research.md` (the row's
pure-tier cell now states what shipped, the OAuth requirement, and the
pluggable transport seam).

## Native handler contract (coordinator-fixed, verbatim)

- firebase.remote-config.setDefaults({map}) -> {}
- firebase.remote-config.getDefaults() -> {map}
- firebase.remote-config.fetchAndActivate() -> {activated: bool, lastFetchStatus: string} (rejects clearly when the backend refuses a fake project)
- firebase.remote-config.get({key}) -> {value: string|null}

On the wire: `setDefaults` carries `{"map": <defaults>}`, `get` carries
`{"key": <key>}`, `getDefaults` and `fetchAndActivate` carry no
arguments; a malformed answer is a `core/host-error`. Both the crate
docs (`crates/nsh-firebase-remote-config/src/lib.rs`) and the TS module
(`packages/nsh-firebase/src/remote-config/client.ts`) state the same
shapes, and the tests assert them exactly.

## Done

Same behavior in Rust and TypeScript, mirroring name for name:

- `crates/nsh-firebase-remote-config` (`src/lib.rs`, `codes.rs`,
  `clock.rs`, `template.rs`, `transport.rs`, `client.rs`):
  - `RemoteConfig` (pure tier): `set_defaults(&JsonValue)` replacing the
    whole map (the JS SDK's semantics) and refusing anything but a flat
    map of string/number/boolean with `core/invalid-argument`;
    `get_defaults`; `get(key) -> Option<String>` resolving the activated
    template values first, then the defaults' string form, else none;
    `get_string`/`get_boolean`/`get_number` with the SDKs' coercions
    (RNFB `RemoteConfigValue.ts` / FlutterFire `remote_config_value.dart`:
    truthy spellings `1/true/t/yes/y/on` case-insensitive, false/0/"" for
    unset, NaN-to-0 for unparseable — `Number.isFinite` semantics on the
    TS side); `last_fetch_status()` (`no_fetch_yet`/`success`/`failure`,
    RNFB's `LastFetchStatus` spellings) and `last_fetch_time_ms()` over a
    `Clock` (`SystemClock` in an app, `ManualClock` in tests).
  - `fetch_and_activate`: without a transport it rejects with
    `remote-config/unavailable` and a message naming the OAuth 2.0
    service-account requirement, the REST host, and the missing emulator
    service — status stays `no_fetch_yet`, no time is stamped, no value
    appears. With a transport it fetches the template document, parses
    and validates it, activates the resolved values, and answers
    `{activated: true, last_fetch_status: "success"}`. A refusing
    transport or a corrupt template records `failure` (with the manual
    clock's stamp) and rejects, surfacing the transport's error or
    `remote-config/invalid-template` respectively.
  - `Transport { fetch_template() }` in `transport.rs`: the one seam a
    future OAuth holder plugs in (a backend proxy, a host handler); the
    trait returns the raw REST document and the client does all parsing,
    so the implementor only authorizes. No built-in HTTP transport
    ships — that would be the fake fetch this row forbids. The future is
    deliberately not `Send`, matching `nsh_firebase_core::send`'s
    engine-thread contract.
  - `parse_template` (`template.rs`): the REST `RemoteConfig` resource
    shape from the v1 discovery document of
    `firebaseremoteconfig.googleapis.com` (fetched 2026-11-07):
    `conditions` (non-empty unique names, non-empty `expression`,
    optional `tagColor`, descending priority), `parameters`
    (`defaultValue`, `conditionalValues` keyed by condition name,
    `description`, `valueType`), `parameterGroups` (their `parameters`
    merge into the same namespace; a key appearing twice is
    `remote-config/invalid-template`, per "a parameter only appears once
    per RemoteConfig"), `version` kept as an object. A parameter value is
    the oneof `{value}` / `{useInAppDefault}` /
    `{personalizationValue|experimentValue|rolloutValue}`: only `{value}`
    resolves to a string; `useInAppDefault` and the dynamic values leave
    the parameter unset, so reads fall back to the app defaults.
    Conditions are recorded but never evaluated — the expression language
    needs the backend's instance context — so `resolved_values()` is
    exactly the defaults, which the resource's own resolution rule states
    as the no-true-condition case. Unknown fields are ignored (the REST
    surface is additive).
  - `NativeRemoteConfig`: validated arguments, then the four contract
    calls through the injected `FirebaseHost`; typed reads coerce the
    wire value locally (as the native SDKs do); `last_fetch_status` is
    the local echo of the last `fetch_and_activate` result; malformed
    answers reject with `core/host-error`. Nothing is posted — the
    contract is calls.
  - Codes (`codes.rs`): `remote-config/unavailable`,
    `remote-config/invalid-template`; argument validation keeps the
    package-wide `core/invalid-argument`.
- `packages/nsh-firebase/src/remote-config` (`index.ts`, `types.ts`,
  `codes.ts`, `template.ts`, `client.ts`): the same rules. `RemoteConfig`
  (pure, async API over the same surface), `NativeRemoteConfig`
  (`tier.call` per the contract), `getRemoteConfig(app?, config?)`
  routing native when `app.tier.kind === "native"` and no
  `config.transport` and not `config.forcePure` — a configured transport
  forces the pure path even on a native tier, mirroring analytics.
  `OAUTH_MESSAGE` is exported so tests pin the refusal text; every call
  that can cross the host channel is async, so the shared
  `RemoteConfigClient` interface needs no per-tier branches. The subpath
  `nsh-firebase/remote-config` was already in `package.json`.
- `tests/remote-config-app.tsx` + `tests/remote-config.test.tsx`: the
  exercise UI (init pure / transport / native, set and bad defaults,
  reads, fetch, fail-next, break-template) and 11 headless tests on a
  manual clock: defaults round trip and typed reads; unset-key fallbacks;
  the no-transport refusal (`remote-config/unavailable`, status stays
  `no_fetch_yet`); activation over the defaults (`useInAppDefault` and
  the dynamic value fall back to the app defaults, the conditional value
  does not apply, the group parameter lands); a refusing backend
  (`failure`); a corrupt template (`remote-config/invalid-template` with
  the duplicate-condition message); bad defaults refused with
  `core/invalid-argument`; native routing asserting `app.host.calls()`
  in order with the exact contract shapes (map rides `setDefaults`, key
  rides `get`, the other two carry no arguments); a malformed native
  answer as `core/host-error`; a throwing host handler surfacing
  `core/host-error` (the backend refusing a fake project, played by the
  host); and transport-config-forces-pure (`app.host.calls()` empty).
  Tests open under `tests/nsh.json` (host `firebase.*`, network
  `127.0.0.1`); no network route is needed — the transport is a stub
  object, so nothing touches the network at all.
- Rust tests: `crates/nsh-firebase-remote-config/tests/remote_config.rs`
  (10 integration tests, `ManualClock`, a `StubbedTransport` serving the
  realistic template as a JSON string standing in for a network body, a
  `RefusingTransport` for the fake-project refusal, and a `RecordingHost`
  asserting the four contract wire calls and the refusal-before-call
  rule) plus inline `#[cfg(test)]` tests in `client.rs` (11),
  `template.rs` (4), `codes.rs` (1), `clock.rs` (2). No sandbox test:
  the tier table's row states plainly that the emulator suite has no
  Remote Config service and the REST API is OAuth-token-only, so there
  is nothing honest to point a sandbox test at.

## Verified how

- Nothing yet on this machine: the area was written under the battery
  constraint (no compile, no build, no test). Verified by reading:
  - The OAuth requirement: https://firebase.google.com/docs/remote-config/automate-rc
    ("Use your Firebase credentials together with the Google Auth
    Library ... to retrieve a short-lived OAuth 2.0 access token",
    scope `https://www.googleapis.com/auth/firebase.remoteconfig`; the
    curl examples send `Authorization: Bearer token`; no API-key path is
    mentioned).
  - The template shape: the v1 discovery document
    (`https://firebaseremoteconfig.googleapis.com/$discovery/rest?version=v1`)
    — `RemoteConfigParameterValue` properties `value` (string) and
    `useInAppDefault` (boolean) plus the `personalizationValue` /
    `experimentValue` / `rolloutValue` refs; `RemoteConfigParameter`
    (`defaultValue`, `conditionalValues`, `description`, `valueType`);
    `RemoteConfigCondition` (required non-empty unique `name`, required
    `expression`, optional `tagColor`); `RemoteConfigParameterGroup`
    (`parameters`, `description`); `RemoteConfig` top level
    `parameters`/`version`/`conditions`/`parameterGroups`; the
    `RemoteConfig` description's resolution rule (earliest true
    condition, else the default value, else omitted). Note the current
    v1 schema uses `value`/`useInAppDefault` — not the older
    `stringValue`/`boolValue`/`useInDefault` spellings some older docs
    show; the parser reads the current shape only.
  - The coercion rules: RNFB `packages/remote-config/lib/RemoteConfigValue.ts`
    (`BOOL_VALUES = ['1','true','t','yes','y','on']`, static source ->
    false/0/""), `statics.ts` (`LastFetchStatus`, `ValueSource`), and
    FlutterFire's `remote_config_value.dart` (same fallback constants).
    RNFB `lib/index.ts` for `setDefaults`/`fetchAndActivate`/
    `lastFetchStatus` naming.
  - The test harness: `nsh-testing` 0.4.0 `module.rs` — `host.handlers`
    value and function forms, `app.host.calls()` recording only the
    listed names, `HostMessage {name, data}`; `selector.rs` — `text:`
    matches exactly (or `text:/regex/`), which the TS tests use for the
    long refusal line.
  - The repo's own established shapes: `crates/nsh-firebase-core/src/host.rs`
    (`FirebaseHost::call`), the analytics crate's clock/transport/native
    patterns, and the gate scripts (`scripts/gate-crate.sh`,
    `scripts/gate-tsx.sh`).
- CI runs the gates first (battery mode): `bash scripts/gate-crate.sh
  nsh-firebase-remote-config` (fmt, clippy `-D warnings`, 32 Rust
  tests) and `bash scripts/gate-tsx.sh tests/remote-config-app.tsx
  tests/remote-config.test.tsx` (check, warning-free build, 11 TS
  tests) are written to pass on the first run. One known blind spot:
  `cargo fmt --check` conformity was written by hand against the repo's
  existing rustfmt output (import version-sort, call/chain widths); if
  CI's fmt pass disagrees, it is a mechanical reformat, not a logic
  change. CI run: <pending>

## Not verified

- Every runtime claim: no `cargo`/`nsh` command was run for this area
  on this machine (battery mode). The headless tests above are
  unexecuted here; their first execution belongs to CI. The repo's
  fail-once rule (see a new test fail by breaking the code on purpose)
  is therefore unmet here too — CI's first run is the earliest it can
  happen.
- The native tier's `firebase.remote-config.*` routing against real
  iOS/Android hosts: proven only in `nsh test` with the test playing the
  host; the native areas own the handlers.
- The `Transport` seam against a real OAuth holder: none exists in this
  sandbox by design; only the stub transports prove the client side.
- `lastFetchTimeMs` on the native tier is a constant `null` (the
  contract's answers carry no time); only the pure tier stamps fetch
  times.
- Nothing was fetched from any Google endpoint, ever — that is the
  area's honesty property, not a gap.
