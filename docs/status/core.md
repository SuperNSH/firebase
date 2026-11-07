# core area status

The other areas' crates, packages, screens and hosts start from this
skeleton. Everything beyond the core itself is a compiling stub those areas
replace wholesale.

## Done

- Workspace `Cargo.toml` with all 14 members (12 `nsh-firebase-*` crates,
  `nsh-firebase-core`, `examples/demo-rs` with package name exactly
  `demo-rs`), shared `[workspace.dependencies]` (`nsh-core = "0.4.0"`,
  `nsh-engine`, `nsh-testing`, `serde_json`, path deps between the
  firebase crates) and a shared release profile.
- `crates/nsh-firebase-core`: options and demo defaults (projectId
  `demo-nsh`, emulator switches defaulting to 127.0.0.1:19099/19000/19199),
  `initialize_app`/`get_app`, `FirebaseError` with the stable codes
  (`core/access-denied`, `core/host-error`, `core/unavailable`,
  `core/timeout`, `core/network`, `core/invalid-argument`, `core/no-app`)
  mapped from `ERR_ACCESS_DENIED`, `ERR_NSH_HOST_ERROR`,
  `ERR_FEATURE_UNAVAILABLE_ON_PLATFORM`, `ETIMEDOUT`, the `EventBus`
  (on/off/emit over snapshots, same behavior as the TS side), the tier
  facade (`FirebaseHost` trait, `NativeHost` over an injected `HostChannel`,
  `PureHost`), and the pure-tier transports (`send` over the engine's
  `fetch`, `join_url`, `socket_request` for the platform WebSocket
  transport).
- `packages/nsh-firebase`: package.json with the full exports map, tsconfig,
  `nsh.json` (`id dev.nsh.firebase`, capabilities: network hosts
  `["127.0.0.1"]`, host `["firebase.*"]`), App.tsx mounting the core
  exercise UI, `src/index.ts` re-exporting core only, 13 `src/<area>/index.ts`
  stubs (`export {};`), and `src/core/` (options, error, bus, app, tier,
  http) mirroring the Rust crate name for name.
- `tests/core-app.tsx` + `tests/core.test.tsx`: headless UI tests on a
  manual clock with stubbed network covering init/config, the pure-tier
  fetch, host-call resolution and failure mapping, bus fan-out/off, and the
  criterion-8 test (undeclared network host refused with a clear
  `ERR_ACCESS_DENIED` through the facade before the stub route answers).
- Config loading on both tiers: `from_config(config: &serde_json::Value)`
  in Rust and `optionsFromConfig(config: unknown)` in TS read a config
  document (apiKey, appId, projectId, databaseUrl, authUrl, storageBucket
  plus an optional emulator block), apply the same validation, and reject
  wrong shapes with invalid-argument; covered by a JSON fixture whose
  fallback URLs are distinct from the emulator ports, in unit tests (Rust)
  and through the exercise UI (TS).
- `post` on the tier facade, both tiers: TS `FirebaseTier.post` wraps
  `host.post` (throws on a refusal, mapped to the stable codes; PureTier
  throws unavailable) and Rust `FirebaseHost::post`/`HostChannel::post`
  mirror it; the UI drives post through native and pure paths.
- `scripts/`: gate-crate.sh, gate-tsx.sh, gate-workspace.sh,
  test-packages.sh, test-demo.sh, start-emulator.sh, test-emulator.sh,
  probe-emulator.sh, check-evidence.sh.
- Root `package.json` (bun) with firebase-tools 14.27.0 installed via
  `bun install`; scripts `emulator:start`, `test:emulator`, `test:packages`,
  `test:demo`, `gate:rust`, `gate:ts`.
- `emulator/`: firebase.json (auth 19099, database 19000, storage 19199,
  hub 14400, UI disabled, storage bucket `demo-nsh.firebasestorage.app`),
  .firebaserc, permissive database/storage rules (sandbox-only, see
  emulator/README.md), plus `.gitignore` additions.

## Verified how

- `bash scripts/gate-crate.sh nsh-firebase-core` — pass (fmt, clippy
  -D warnings, 30 tests: 26 unit + 4 headless UI on a manual clock with
  StubNetwork; the unit tests cover from_config — including a JSON float
  port and an explicit null host — and post; the UI drives post through
  native and pure paths).
- `bash scripts/gate-tsx.sh tests/core-app.tsx tests/core.test.tsx` — pass
  (`nsh check`, warning-gated `nsh build`, 6/6 headless tests, including
  the host-side refusals (ping-denied, post-denied) the runtime itself
  returns, the config-document test, and the fetch through `joinUrl` with
  a trailing-slash base).
- `bash scripts/gate-workspace.sh` — pass for all 14 crates (fmt, clippy,
  test).
- `cargo metadata --format-version 1 --no-deps` — 14 members, `demo-rs`
  named exactly.
- `bash scripts/probe-emulator.sh` — pass: the suite is up and detached
  (`emulator/emulator.pid` 38135, log `emulator/emulator.log`): auth
  19099 ready, database 19000 answering, storage 19199 and hub 14400
  both probed (any HTTP answer; storage answers plain `/` with 501).
  Started with `bash scripts/start-emulator.sh`
  (JAVA_HOME=Android Studio JBR, `firebase emulators:start --project
  demo-nsh --config emulator/firebase.json`).
- `bash scripts/test-emulator.sh` — the suite is up; all labelled sandbox
  test files print SKIPPED (their areas have not written them yet); the
  collector-backed pair is skipped with a note because tools/collector is
  not built yet.
- Criterion 8 evidence in `docs/evidence/core/access-denied.log`: headless
  pass (the stub route for `denied.example.test` never answered, mapped
  error `core/access-denied (ERR_ACCESS_DENIED)`; `other.ping` and
  `other.post` refused by the runtime's capabilities.host check), plus a
  run on the real macOS target where the platform's own capability check
  refuses the same request.
- Every new or changed test was seen to fail once by breaking the code on
  purpose, then green when fixed. The breaks performed, and what failed:
  - Mapping break (both sides map `ERR_ACCESS_DENIED` to `core/network`):
    TS test 3 failed at ping-denied (`other: core/network` on screen) and
    the deny test; Rust failed `a_refused_post_maps_to_access_denied` and
    the mapping tests.
  - Config-loader break (both loaders drop the document's databaseUrl):
    Rust `from_config_falls_back_to_the_document_urls_without_an_emulator_block`
    failed and the TS config test failed at the `config-pure` step.
  - Storage-fallback break (none needed as a mutation: the storage fallback
    assertions were added against the bug itself, the bucket-prefixed host
    `https://{bucket}.firebasestorage.googleapis.com/...`): the new Rust
    assertion failed, and the TS config test failed at `config-pure` on the
    same wrong URL; both pass with the unprefixed REST host.
  - `NativeTier.post` silent no-op: "post: ok" still showed, and the new
    `app.host.posts()` assertion failed (`expected [] to equal [...]`).
  - `EventBus::off` made a no-op: Rust
    `off_stops_the_delivery_and_an_unknown_id_is_a_no_op` failed and the UI
    bus test failed (`a=4 b=4` instead of `a=4 b=2`).
  - `join_url` made double-slash on a trailing-slash base: Rust
    `join_url_uses_one_slash_whatever_the_sides_have` failed, the Rust UI
    transport test failed with `19000//todos.json` getting no stub
    response, and the TS pure-tier test failed the same way — the TS
    fetch button now builds its URL through `joinUrl(`${DB}/`,
    "todos.json")` with the trailing-slash base kept on purpose, mirroring
    the Rust TransportApp.
  - `EmulatorEndpoint::base_url` made to ignore a configured host:
    `a_host_override_changes_the_endpoint_but_keeps_the_scheme` failed.
  - Unknown-code break (Rust `from_host_code` default arm drops
    `native_code`): the strengthened
    `an_unknown_host_code_is_a_network_failure` failed.
  - Endpoint shape break (Rust `endpoint()` without the is_object check):
    the new message assertion in `from_config_rejects_wrong_shapes` failed
    ("port is not a number" vs the expected "emulator.database is not an
    object"). The TS message strings are fixed to match but have no TS unit
    harness; their parity is pinned by the Rust assertions.
  - Non-string emulator host break (Rust `endpoint()` left without the
    host shape check): the new `{"host": 19000}` case in
    `from_config_rejects_wrong_shapes` failed because the config loaded
    with the host silently defaulted instead of throwing
    "emulator.database.host is not a string". `an_explicit_null_host_is_absent`
    pins the null-is-absent behavior both tiers now share.
  - Float-port break (Rust `endpoint()` without the integral-float
    fallback): `a_json_float_port_is_taken_as_its_integer` failed
    (`{"port": 19000.0}` rejected as "not a number" where TypeScript's
    JSON.parse accepts the integer), and the `70000.0` case in the rejects
    test failed on the same wrong message instead of "out of range".
- `bash scripts/check-evidence.sh` — fails with the expected MISSING list
  (native-ios, native-android, demo, perfmon, analytics evidence belong to
  their areas and do not exist yet; `docs/evidence/core/access-denied.log`
  is present).

## Not verified

- Pixels: the core exercise UI is driven headless only; nothing here is a
  designed screen (the demo area owns visible work). No `nsh run` look.
- `connectSocket` (TS) and `socket_request` (Rust) have no live WebSocket
  test: the test harness refuses sockets unless `network: "live"`, and no
  product uses them yet. Type-checked only.
- The Rust `NativeHost` over a real embedding host: nsh-core 0.4.0 keeps
  its host bridge (`host_call`) `pub(crate)`, so Rust apps cannot dial
  `host.call` themselves. `NativeHost` takes the `HostChannel` the
  embedding app injects (native/ios, native/android supply it). Verified
  only against a test channel.
- `host.available()` being false (the `ERR_FEATURE_UNAVAILABLE_ON_PLATFORM`
  path from the runtime itself) — headless tests always have a host; the
  mapping is covered by the `map-unavailable` unit path and the Rust
  `PureHost` UI test instead.
