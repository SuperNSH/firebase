# demo

The demo area: `examples/demo` (TSX app) and `examples/demo-rs` (Rust app), one screen per
product over the landed nsh-firebase libraries. Pass one built the shell; pass two (this
pass) replaced the placeholders with real screens and tests.

## The app

- `examples/demo`: TSX app (`dev.nsh.demo`). Theme module `theme.json` (semantic tokens only,
  light + dark, `radius`); one wrapper per `nsh/base` control used (`ui/button.tsx`,
  `ui/input.tsx`, `ui/progress.tsx`); shared `components/Header.tsx` (desktop/web header with
  back; iOS/Android get the system header from the page title). Navigation: `createNavStack`
  in `nav.ts` with routes `home, auth, database, storage, messaging, analytics, monitor`.
  Product metadata is `src/products.ts`; the product clients live in `src/firebase.ts` (core's
  `demoOptions`, one instance per product at module level). Libraries linked through
  `node_modules` symlinks (`nsh-firebase`, `nsh-query`, `nsh-form`).
- `examples/demo-rs`: Rust app (`dev.nsh.demo-rs`, `"rust": ".", "root": "App"`), same routes.
  `src/nav.rs` (Route enum, global routes signal), `src/ui.rs` (`app_button`, `index_row`,
  `app_progress`; tokens only), `src/firebase.rs` (demo clients over `FirebaseOptions::demo`;
  the database's `WholeBodyStream` stands in for an embedding host's stream transport, as
  nsh 0.4.0 gives a Rust app no incremental fetch), `src/screens.rs` (one screen per product,
  each taking its product client, so tests pass stubs). Path dependencies on
  `nsh-firebase-{core,auth,database,storage,messaging,analytics,perf}` and on the sibling
  `nsh-query`/`nsh-form` crates.
- `examples/demo/src/screens/monitor.tsx` and `examples/demo/tests/perfmon-shots.test.tsx`
  are the perfmon area's (the real monitor screen; the demo imports it as
  `MonitorScreen`). `dist/nsh` holds the release bundle (gitignored).

## The screens

- auth: nsh-form rules (required, email, min-length) around the auth library's
  sign-in/sign-up, emulator config from core, server replies mapped to their fields
  (EMAIL_NOT_FOUND/INVALID_PASSWORD/EMAIL_EXISTS/WEAK_PASSWORD), currentUser signal as text,
  sign out.
- database: `onValue` listener → `List` (TSX) / `virtual_list` (Rust) of `demo/items`, push
  form, connection status ("Live"/"Reconnecting…" — a stubbed stream that ends reports
  reconnecting honestly).
- storage: resumable upload of 600 kB in 256 KiB slices with the progress bar bound to the
  task's progress signal, then download and delete.
- messaging: permission and token calls; the pure tier's refusal rendered with its stable
  code (`core/unavailable`, ERR_FEATURE_UNAVAILABLE_ON_PLATFORM) and the native-tier note —
  no silent no-op. The native path is covered by a test with stubbed `firebase.messaging.*`
  handlers (TSX).
- analytics (TSX): `QueuedAnalytics` over the library's `RecordingTransport` (batch of 3),
  queue/sent/dropped status, flush button. analytics (Rust): `Analytics::with_collector`
  against 127.0.0.1:18742, pending/dropped, flush.
- monitor (TSX): perfmon's screen, wired through the same router.

## Done / verified

- TSX gate: `bash scripts/test-demo.sh` passes (nsh check clean; nsh build without
  warnings; `tests/demo.test.tsx` 9/9; `cargo test -p demo-rs` 8/8). 2026-11-07.
- `bash scripts/gate-crate.sh demo-rs` passes: fmt --check, clippy --all-targets
  -D warnings (clean once the perf area's `clone_on_copy` landed its fix), test 8/8.
- Headless tests drive the real UIs on a manual clock with stubbed network and host: nav
  (6 routes), auth validation + stubbed sign-in + server error on its field, database live
  put + push request, upload progress (in-flight state observed through a clock-delayed
  slice) + download + delete with slice counts asserted, messaging pure-tier refusal +
  native path (TSX), analytics queue + flush, monitor over a stubbed collector.
- Every new test was seen to fail once by breaking the code on purpose (TSX: row ids,
  signed-in text, wrong-password mapping, list mapping, storage line, messaging catch,
  analytics bump, monitor stub; Rust: the same surfaces), then restored.
- Visual: `docs/evidence/demo/demo2-{auth,database,messaging}-{light,dark}.png` on the
  macos target, looked at; pass one's shell captures remain alongside. Captured with a
  temporary `tests/look.tmp.test.tsx`, deleted after the run.
- Vision evidence (criterion 7 corroboration + 10): `tests/screenshots.test.tsx` walks
  home → auth → database → storage → analytics → monitor on the macos target and saves
  `docs/evidence/demo/macos-{home,auth,database,storage,analytics,monitor}.png`, with the
  runner output in `docs/evidence/demo/macos.log`. Run locally on 2026-11-07 before the
  battery pause (2/2 passed); CI can re-run it with
  `nsh test examples/demo/tests/screenshots.test.tsx --target macos`. Stubbed on macos:
  the database's SSE stream and the storage session (emulator wire shapes). Real: home,
  the typed auth form, the analytics queue over its recording transport, and the monitor's
  live calls against the collector on 127.0.0.1:18742 (tools/collector was already running,
  another area's process — used, not started or stopped): the capture shows `/slow` at
  1230 ms on top of `/collect` 31 ms, `/assert` 29 ms, `/health` 29 ms, all 200. Every PNG
  was looked at.
- Release bundle: `cd examples/demo && nsh build App.tsx --release --out dist/nsh`
  (61.7 KB js, 74.3 KB bytecode); `dist/` is gitignored.

## Not verified

- Phones, web and TUI: verified on macOS only (headless everywhere, pixels on the macos
  target). iOS/Android capture belongs with the host areas.
- The live emulator suite path (`scripts/test-emulator.sh`) is the packages' own evidence;
  the demo's headless tests stub the wire.
- The Rust demo's messaging has no native-tier test (no host channel in a standalone Rust
  app; the TSX app covers the host path).

## Pass three (pending, optional)

Re-run this gate after the whole-tree smoke. The screenshot walk
(`tests/screenshots.test.tsx --target macos`) moves to CI per the battery pause; the file
is ready as committed. The emulator sandbox run (`scripts/test-emulator.sh`) against the
live demo screens is the remaining label-worthy evidence.
