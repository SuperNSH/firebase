# storage area status

Cloud Storage for the pure tier, in Rust and TypeScript: resumable upload
with progress/pause/resume/cancel and resume-by-session-id, download,
download URL, metadata, list, delete — on the Firebase `X-Goog-Upload-*`
wire protocol exactly as `docs/research.md` section 3 records it (verified
against the firebase-js-sdk storage layer and the live emulator).

## Done

- `crates/nsh-firebase-storage`: `wire.rs` (encodeURIComponent-exact
  percent-encoding, the objects/object/start URLs, the metadata mapping
  with the SDK's writable fields, list-page parsing, `storage/*` error
  codes, the 256 KiB chunk constant), `upload.rs` (the `UploadTask`
  state machine: start -> query the server's offset -> 256 KiB slices ->
  `upload, finalize`; pause at slice boundaries with a query on resume;
  cancel ends in `storage/canceled`; progress as a signal of
  bytesTransferred/totalBytes/state paused|running|done|error;
  `session_url()` for persisting the session across process death;
  `Authorization: Firebase <idToken>` from a wired provider on every
  request; HTTP statuses map to the SDK's storage codes before any
  upload-status header is read, and a missing or empty
  X-Goog-Upload-Size-Received is a lost session
  (storage/retry-limit-exceeded), never an offset of 0), `storage.rs` (`FirebaseStorage::new(&app)`
  over the emulator switch or the production REST host, `r#ref(path)`,
  `upload`/`resume_upload` (a saved session URI), `download`
  (`alt=media`), `download_url` (the first `downloadTokens` entry),
  `get_metadata`/`update_metadata` (PATCH of the writable mappings only),
  `list` (prefix + `delimiter=/` + pageToken/maxResults), `delete`).
- `packages/nsh-firebase/src/storage`: `wire.ts`, `storage.ts`,
  `upload.ts`, `index.ts` — the same behavior name for name (TS
  shape: `new FirebaseStorage(app)`, `storage.ref(path)`,
  `ref.upload(bytes, metadata?)` -> `UploadTask` with `progress` signal,
  `result` promise, `sessionUrl` getter, `pause`/`resume`/`cancel`).
- Unit tests, headless on a manual clock with stubbed routes, no real
  time: Rust `crates/nsh-firebase-storage/tests/storage_ui.rs` (10 tests:
  the exact wire traffic of start/query/slices, an
  interrupted-then-resumed run that saves the task's session URI with
  `session_url()` and resumes by it, pause holding under a moved clock,
  cancel ending in `storage/canceled`, the Firebase auth header on
  downloads, download URL from the metadata tokens, metadata GET + PATCH
  round trip, list paging by nextPageToken, delete and the missing
  object's `storage/object-not-found`, and a denied finalize being an
  error despite the emulator's `x-goog-upload-status: final` header on
  the 403) plus 10 in-crate unit tests (URL shapes, encoding, metadata
  parse/build, list parse, error mapping, state strings, auth header).
  TypeScript `tests/storage.test.tsx` (11 tests, the same coverage
  through the exercise UI, save-then-resume included, and a query
  without its size header ending the task in
  storage/retry-limit-exceeded instead of restarting from 0).
- `tests/storage-app.tsx`: the exercise UI both TS test files drive; ids
  and lines carry the `st` prefix so it can share an entry with other
  areas' UIs. The tests open it directly under `tests/nsh.json`'s
  capabilities.
- Labelled sandbox tests (runner-only), both green against the restarted
  emulator: Rust `crates/nsh-firebase-storage/tests/emulator_storage.rs`
  (`#[ignore = "sandbox: needs emulator suite"]`, run by
  `scripts/test-emulator.sh` with `--ignored`, live network through
  `UreqNetwork`, manual clock) and TS
  `packages/nsh-firebase/tests/emulator/storage.sandbox.test.tsx`
  (`network: "live"`): upload a 36-byte fixture with progress events,
  read its download URL, download and compare the bytes, list it under
  its parent prefix, delete it, and see the follow-up download fail as
  object-not-found.

## Verified how

- `bash scripts/gate-crate.sh nsh-firebase-storage` — pass (fmt, clippy
  -D warnings, 10 unit + 10 UI tests + 1 ignored sandbox test); log in
  `docs/evidence/storage/gate-crate.log`.
- `bash scripts/gate-tsx.sh tests/storage-app.tsx tests/storage.test.tsx`
  — pass (`nsh check`, warning-gated `nsh build`, 11/11 tests); log in
  `docs/evidence/storage/gate-tsx.log`. The core tests still pass with
  the storage UI in the tree (6/6, run separately).
- The labelled sandbox tests pass through the documented runner
  commands: `cargo test -p nsh-firebase-storage --test emulator_storage
  -- --ignored` (1 passed, log `docs/evidence/storage/rust-sandbox-green.log`)
  and `nsh test tests/emulator/storage.sandbox.test.tsx` (1 passed, log
  `docs/evidence/storage/ts-sandbox-green.log`), against the restarted
  emulator suite (core fixed the rules-runtime glob bug).
- Wire-protocol cross-checks while building: the emulator's
  start/query/chunk responses match the documented header names byte for
  byte; probes in `docs/evidence/storage/emulator-write-denial.log`
  (that log also records the now-fixed denial window, kept for history).
- Every new or changed test was seen to fail once by breaking the code
  on purpose, then green when fixed. Original round: slice offsets
  shifted by one (the 4 upload tests each side); the auth header dropped
  (download test and the in-crate auth test); the list page token
  dropped (list test); the PATCH body emptied (metadata test); the
  download URL's token dropped (url test and the in-crate metadata
  test); 404 mapped to `storage/unauthorized` (delete/errors test and
  the in-crate error test); `r#ref` keeping slashes (refs test); the
  paused state string changed (states test); the denied-finalize test
  failed before the HTTP-status guard was added; the missing-size-header
  test failed before the query guard treated a missing header as 0. Auditor round: with
  `session_url()`/`sessionUrl` stubbed to return nothing, the
  interrupted-then-resumed tests fail (the resume starts a new session
  instead of the saved one) and pass with the real accessors; with list
  parsing dropping `items`, the Rust sandbox fails at `listed=true`;
  with the sandbox payload one byte short, the TS sandbox fails at
  `36/36 done` — which also caught the sandbox flow not routing its
  upload through the task signal the progress log tracks, fixed in the
  app.

## Not verified

- The `Authorization: Firebase <idToken>` path is verified against a
  stubbed provider only; wiring it to the auth area's `currentUser`
  token is a one-line provider at the call site and is not proven here
  (the auth area's crate is still a stub).
- No production-host request is ever made in tests; the non-emulator
  URL shape (`https://firebasestorage.googleapis.com/v0/b/<bucket>/o`)
  is covered by unit tests of `wire::origin`/URL building only.
