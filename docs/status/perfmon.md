# perfmon area status (performance + network monitoring)

Owns `crates/nsh-firebase-perf`, `packages/nsh-firebase/src/{perf,monitor}`,
`tools/collector`, the perf pair of test files, the monitor sandbox test, and —
since the demo shell landed — the demo's monitoring screen
(`examples/demo/src/screens/monitor.tsx`) and its screenshot test
(`examples/demo/tests/perfmon-shots.test.tsx`).

## Done

- **The demo's monitoring screen** (`examples/demo/src/screens/monitor.tsx`):
  the `monitor` route the shell wires. It measures four live calls against
  the collector on 127.0.0.1:18742 on mount (the deliberately slow
  `/slow?ms=1200`, fast `/health` and `/assert`, and a `POST /collect`),
  through `instrumentFetch` over the package's shared monitor instance
  (`sharedMonitor`, exported from `nsh-firebase/monitor`), and shows the
  ring as a virtualized `List` sorted by duration descending — the slow
  route visibly on top — with per-call method, path, duration, ttfb, status
  and initiator, a calls/errors summary, visible reversible filters
  (All / ≥ 1 s / Errors) and a Reload control. Theme tokens only; the
  honest empty state explains the next action; failed measures land in the
  ring as error events and their rejections are swallowed.
- **The screenshot test** (`examples/demo/tests/perfmon-shots.test.tsx`):
  drives the route on a real platform with the collector's responses as
  stubs (same statuses, headers, slow-route delay — no real internet),
  asserts the slow row is first and the filters reverse, and saves
  `docs/evidence/perfmon/monitor-{light,dark}.png` via `app.screenshot`.
  Run: `nsh test examples/demo/tests/perfmon-shots.test.tsx --target macos`.

## Done (first pass)

- **The shared `NetworkEvent` type** (PROMPT.md's exact interface, identical
  JSON on both sides): `crates/nsh-firebase-perf/src/event.rs`
  (`serde`-serializable, camelCase keys, nulls kept as explicit `null`s) and
  `packages/nsh-firebase/src/monitor/event.ts`. `nsh-http` (the parallel
  project) exchanges the same shape; nothing here depends on it.
- **Monitor**: ring buffer, default 500 events, configurable
  (`Monitor::with_capacity` / `new Monitor({ capacity })`), subscribe with
  snapshot-order fan-out (`subscribe` returns an unsubscribe in TS, a
  `MonitorId` + `unsubscribe` in Rust).
- **Aggregates**: per-host and per-path `{count, errorRate, p50, p95}`,
  deterministic — nearest-rank percentiles over durations sorted ascending,
  errors by the shared rule (`error != null` or status outside 2xx), groups in
  ascending key order. `aggregate.rs` / `monitor.ts`.
- **View model**: `rows(events, filter)` / `monitorRows(events, filter)` —
  sorted by duration descending (slowest on top, criterion 7), ties broken by
  `startedAtMs` ascending then `id` ascending; filters host (exact), path
  (substring), minDuration (>=), status (exact; unset keeps null-status
  events).
- **instrumentFetch / instrumented_send**: a drop-in fetch wrapper emitting
  events with `initiator: "firebase-fetch"`. TS signature is the global
  `fetch`'s (`input, init` with `init.signal` passed through), so a
  `nsh-query` `fetch` option can take it directly; Rust is an async wrapper
  over `nsh_firebase_core::send` with the same `(request) -> Result<...>`
  shape a query fetcher calls. Timing reads the engine clock
  (`performance.now()` in TS, `nsh_core::time::now()` in Rust), `startedAtMs`
  the wall clock.
- **perf**: `startTrace(name) -> {putMetric, putAttribute, stop}` recording
  durations on the clock; `recordHttpMetric(event)` building HTTP metrics from
  monitor events; records kept in a local ring (500) whatever the tier;
  delivery through a pluggable transport (`PerfTransport` — same shape as
  analytics': one `send(records)` per batch plus `flush`) with
  `BatchingTransport` (auto-flush at the batch size, `flush()` on demand) and
  `HttpLineTransport` (newline-delimited JSON POST, the collector's format).
- **Native tier, fixed coordinator shapes** (used verbatim in both hosts and
  this facade):
  - `firebase.perf.startTrace({name})` → `{id: string}`
  - `firebase.perf.stopTrace({id})`
  - `firebase.perf.recordHttpMetric(<a NetworkEvent, as the args object>)`
  Detection is one probe: the first record calls `startTrace` with the
  throwaway name `__nsh_probe__` (and stops it when it works). Success routes
  every record through the handlers; a refusal (the name not granted) or no
  host routes records to the transport instead — nothing is lost on an
  unhandled post, which the runtime drops silently.
- **tools/collector**: `bun tools/collector/server.ts` — POST `/collect`
  appends each JSON body as a line to
  `docs/evidence/perfmon/collector.jsonl` (`--out` overrides), GET
  `/assert?contains=<substr>&count=<n>` → `{"ok":bool,"count":n,"matches":k}`
  (count is echoed back and asserted as a minimum total; contains needs at
  least one matching line; both asks must hold for `ok`), GET|POST
  `/slow?ms=1500` replies after ms (the demo's deliberately slow route), GET
  `/health` → 200. Port 18742 is this area's (`--port` overrides). The sink
  logic is unit-tested without the server (`bun test tools/collector`).
- **Tests**, all fail-first (every one seen failing under a deliberate
  mutation, then green), manual clock, stubbed routes: event fields, delayed
  response timing, error events, ring eviction, p50/p95 math, aggregates per
  host/path, trace metric/attribute/stop duration, httpMetric from events,
  batched transport flush, view-model sorting/filters, native-tier fallback
  (TS) and full native routing (Rust), collector sink logic (bun).

## Verified how (second pass, demo screen)

- `cd examples/demo && nsh check App.tsx` — clean; `nsh build` clean, no
  warnings (no unknown classes, no destructured props).
- `nsh test examples/demo/tests/perfmon-shots.test.tsx --target macos` —
  pass (`docs/evidence/perfmon/demo-shots.log`): slow row first (~1200 ms
  of real time against the stubbed slow route), fast routes at
  single-digit milliseconds, Errors filter empties to the honest empty
  state and All restores, both PNGs saved and looked at
  (`docs/evidence/perfmon/vision.md` records what they show). Seen to fail
  once by breaking the screen's sort to ascending: the "slowest on top"
  assertion failed, green after restore.
- The demo's own shell tests (`examples/demo/App.test.tsx`) still pass 3/3
  with the real screen in place of the placeholder (its unrouted mount
  fetches stay quiet: error events, swallowed rejections).
- The granted native path is covered headless in TS too (added in the
  audit fix round, once `tests/nsh.json` existed): the test plays the
  three fixed handlers, and `app.host.calls()` shows the probe's
  start/stop pair then the trace's own start and stop
  (`{name: "screen-load"}` → stop with the handler's id), while
  `app.host.posts()` shows the `recordHttpMetric` post with the event as
  its args object. Seen to fail once by breaking the facade (probe forced
  to false, so no call and no post crossed), green after restore.
- The reported strict-clippy failures (`cargo clippy -p demo-rs
  --all-targets -- -D warnings` on two `clone_on_copy` in
  `crates/nsh-firebase-perf/src/monitor.rs`) do not reproduce on the
  current tree: both `-p nsh-firebase-perf` and `-p demo-rs` strict clippy
  are clean after a forced recheck (the earlier warnings matched an
  intermediate file state already fixed in the first pass, where
  `native_available`'s `Option<bool>.clone()` became a deref-copy).
- Scoped gates re-run after the screen landed: `bash scripts/gate-crate.sh
  nsh-firebase-perf` and `bash scripts/gate-tsx.sh tests/perf-app.tsx
  tests/perf.test.tsx` — both pass (fresh logs in
  `docs/evidence/perfmon/`).

## Verified how (first pass)

- `bash scripts/gate-crate.sh nsh-firebase-perf` — pass: `cargo fmt -p
  nsh-firebase-perf -- --check` clean, `cargo clippy -p nsh-firebase-perf
  --all-targets -- -D warnings` clean
  (clippy caught a guard held across an await in `Trace::stop`; the guard is
  now scoped out before any await), then 21 unit tests + 5 headless UI tests
  + 0 doc-tests, all green on the manual clock
  (`docs/evidence/perfmon/gate-crate.log`). The UI tests drive the
  instrumented transport over `StubNetwork` with a 120 ms stub delay on the
  manual clock (duration exactly 120.0), the error event of an unrouted
  request, ring eviction, a trace stopped on the clock and flushed as one
  ndjson line to the sink stub, and the native tier's fixed handlers over an
  injected channel (probe → start/stopTrace calls in order, the event as the
  `recordHttpMetric` post).
- `bash scripts/gate-tsx.sh tests/perf-app.tsx tests/perf.test.tsx` — pass:
  `nsh check`, warning-gated `nsh build`, the headless suite green
  (`docs/evidence/perfmon/gate-tsx.log`, the refreshed 10-test run: the
  original 9 plus the granted-native-path test added in the audit fix
  round). Note: one earlier run of this gate
  failed on *other areas'* in-flight files (their import errors fail the
  package-wide check); it passes once those landed.
- `bun test tools/collector` — 6 pass (`docs/evidence/perfmon/collector-bun-test.log`).
- The collector sandbox pair `tests/emulator/monitor.sandbox.test.tsx` against
  the real sink on 127.0.0.1:18742 — 2/2 pass, live network and real time
  (`docs/evidence/perfmon/sandbox-monitor.log`, sink lines in
  `docs/evidence/perfmon/collector.jsonl`): a queued HTTP metric posts to the
  collector and `/assert` answers `{"ok":true,"count":1,"matches":1}`, and the
  collector's `/slow?ms=900` route records a real three-digit duration through
  `instrumentFetch`. The collector ran as
  `bun tools/collector/server.ts`, this area's process.
- Every new test seen to fail once, by breaking the code on purpose:
  - collector: `appendBody` made to not push (3 appendBody tests failed), to
    keep blank lines (blank-line and newline tests failed), `assertLines` made
    to always answer `{ok:true,count:null,matches:0}` (all 3 assert tests
    failed); each test failed under at least one mutation and passed restored.
  - TS, in four batches, each restored to green after: (A) `splitHostPath`
    dropping the port, ring eviction removed, view sort ascending → event
    fields, error event, ring and view-model tests failed; (B) nearest-rank
    percentile made floor, trace duration zeroed → aggregates and trace tests
    failed; (C) batching transport made to forward without queueing, probe
    forced true → the ndjson-flush and native-fallback tests failed; (D)
    error-event recording skipped and success timing zeroed → the error-event
    and delayed-timing tests failed. All 9 TS tests failed under at least one
    mutation and all 9 passed after restore.
  - Sandbox pair: the queued metric's id changed to one the assert never
    matches, against a fresh sink file → the collector-assert test failed;
    restored, green. (Against the canonical sink the first attempt passed
    because the earlier run's line was already there — the assert is
    at-least-over-the-file by design, so the break needs a fresh sink.)
  - Rust, in batches, each restored to green after: (R1) `split_host_path`
    dropping the port, serde keys made snake_case → 4 event/perf-record
    tests failed; (R2) ring eviction removed, unsubscribe a no-op, view sort
    ascending, host filter dropped → 5 monitor/view tests failed; (R3) floor
    percentile and zeroed error rate → 2 aggregate tests failed; (R4b)
    batch-size auto-flush removed, the `type` tag renamed and TraceRecord
    keys made snake_case → 3 perf tests failed; (R5) the empty-group early
    return removed (NaN error rate) → `an_empty_group_is_all_zeros` failed;
    (R6) `instrumented_send` recording nothing, the probe forced false,
    trace duration zeroed → 4 UI tests failed; (R7) `with_capacity`
    ignoring the capacity → the ring UI test failed; (R8) `is_error`
    ignoring status, constant ids, by-path grouping by host, the status
    filter passing everything, the probe forced true, an empty flush
    reaching the inner transport → the last 6 tests failed. All 26 Rust
    tests then present failed under at least one mutation and passed after
    restore. (Audit round 2 added one more, seen failing first: a probe
    whose startTrace answers Ok without a string id — the facade then
    mistook the broken handler for native and posted, pinned by
    `a_probe_without_a_string_id_falls_back_to_the_transport`, green after
    `native_available` derived native from the parsed id, matching the
    TypeScript probe.) One real bug also came out of this pass:
    `Monitor::record` called listeners while holding the ring lock,
    deadlocking a listener that read the monitor — found because
    `a_listener_sees_the_event_already_in_the_ring` hung, and fixed by
    calling listeners outside the lock (the TypeScript side has no locks
    and never had the hazard). Clippy's `await_holding_lock` also pushed
    `Trace::stop` to scope its guard before the awaits, which is the
    honest shape anyway.

## The collector, one command

```sh
bun tools/collector/server.ts
# or with an explicit sink file:
bun tools/collector/server.ts --out docs/evidence/perfmon/collector.jsonl
```

It writes its sink relative to the repository root and listens on
`127.0.0.1:18742` (this area's port; if another process holds it, that is a
blocker to report, never a process to kill). `scripts/test-emulator.sh` runs
the monitor sandbox pair only when `/health` answers.

## What the native automatic capture adds, and what stays invisible

The pure tier (every target, including `nsh package` apps) sees exactly what
its own transport sees: the app's instrumented fetches and the records the
facade is handed. It cannot see anything the SDKs capture, because Performance
has no public ingestion REST — the pure tier's records go to a pluggable
transport (`tools/collector` here), never to the Firebase console.

On the native hosts (iOS/Android, through the embedding SDKs), the handlers
above give the app the SDK's real traces (name + duration, with whatever
attributes the native side records about the process) and the SDK's automatic
HTTP capture for calls the app makes outside the instrumented wrapper —
native libraries, the SDK's own network calls. What stays invisible from the
app:

- **Trace metrics and attributes do not cross the channel.** The fixed
  handler list has no putMetric/putAttribute call, so on the native tier a
  stopped trace carries only its name and duration; the metrics and
  attributes remain in the local record ring and in the pure transport when
  one is attached. This is a coordinator-fixed shape, documented here rather
  than widened unilaterally.
- **The monitor's event list does not include the SDK's automatic captures.**
  Native-side requests (NSURLSession / OkHttp instrumentation) appear in the
  Firebase console, not in this monitor's ring buffer; bridging them back
  would need an event the handler list does not define.
- **No console/dashboard verification exists in the sandbox** — no real
  project, and Performance has no emulator. Traces are proven by reaching
  `tools/collector` (pure tier) and by the handler traffic the tests assert
  (native tier).
- **TTFB differs by tier.** TS `fetch` resolves at response headers, so TS
  events carry `ttfbMs` and their `durationMs` covers start-to-headers (the
  monitor never reads the caller's body; `responseBytes` needs a
  `content-length`). The Rust transport resolves with the whole response
  (`nsh-core` delivers at `End`), so Rust events carry the full duration and
  the body length, with `ttfbMs` null. Same fields, honest per-transport
  values.

## Not verified

- Pixels on a phone target: the screenshots are the macOS target only
  (`--target macos`). The criterion-7 wording also names a phone target; an
  `--target ios`/`android` run of the same shot test needs a booted
  simulator/emulator of this repo's own and is still to do.
- The Rust `record_http_metric` post over a *real* embedding host
  (`examples/host-ios`, `examples/host-android`) — those hosts belong to the
  native areas; here the channel is a test double that answers exactly the
  fixed shapes.

## Capabilities note (corrected)

The headless TS tests run under `tests/nsh.json`, the manifest next to their
entry: it grants network hosts `127.0.0.1` (which is what lets every stubbed
loopback fetch through) and host `firebase.*` (which is why the played
`firebase.perf.*` handlers are reachable at all). `nsh-bundler`'s
`capabilities()` reads the manifest next to the entry file. Both native
tiers are covered headless in TS now: the granted path (probe succeeds;
`startTrace`/`stopTrace` calls and the `recordHttpMetric` post cross, read
back via `app.host.calls()`/`app.host.posts()`) and the refusal path (the
played `startTrace` handler throws, the probe treats that as "no native
capture", and the transport takes the record). The refusal path is
behavioral, not a capability refusal: the name is granted and the handler
runs.
