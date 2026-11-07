# firestore area status

Cloud Firestore, the first stretch product, in Rust
(`crates/nsh-firebase-firestore`) and TypeScript
(`packages/nsh-firebase/src/firestore`), same behavior name for name
(`get_doc`/`getDoc`, `r#where`/`where`). Pure tier only in this contract:
documents CRUD and shallow structured queries over the v1 REST surface;
no realtime listeners anywhere, honestly.

## Battery-mode note

The build machine was on battery for this area's writing pass: no
compile, build, test, or gate command was run locally. Everything below
was verified by reading sources (the repo's core/database/storage/
functions areas, the firebase-js-sdk compiled mirror, the official REST
reference) — the tests are written to pass on their first CI run, and CI
executes them first.

CI run: <pending>

## Native handler contract (coordinator-fixed, verbatim)

The native-stack agent implements exactly these on both hosts:

- firebase.firestore.getDoc({path}) -> {exists: bool, data: object|null}
- firebase.firestore.setDoc({path, data, merge?}) -> {}
- firebase.firestore.addDoc({collectionPath, data}) -> {id}
- firebase.firestore.deleteDoc({path}) -> {}
- firebase.firestore.runQuery({collectionPath, where?, orderBy?, limit?})
  -> {docs: [{id, data}]}

No watch handler exists in the contract, none was invented, and the
realtime row below says so.

## Tier table

| Tier | What it does | Honest gap |
|---|---|---|
| Pure (every target: `nsh package`, desktop, web) | `getDoc` (a 404 reads as `exists: false`), `setDoc` (REST patch; `merge` sends the leaf field paths as `updateMask.fieldPaths`), `addDoc` (createDocument POST, id from the reply's resource name), `deleteDoc` (a 404 is done: deletes are idempotent), `runQuery` (`structuredQuery` with one where filter, sorts, a limit) — over the v1 REST surface (`/v1/projects/{project}/databases/(default)/documents`) against the suite's Firestore emulator (127.0.0.1:18080), values in the REST typed-value encoding | **No realtime listeners over REST** — the REST surface has none to speak of, and none is faked. Composite filters, cursors, offsets, server timestamps/tokens, transactions and named databases are outside the facade. Value types the JSON contract cannot carry (a `timestampValue` written by another client, a non-finite double read back) fail as `firestore/invalid-data`, never silently |
| Native (iOS/Android hosts) | The same five calls through the `firebase.firestore.*` handlers, per the contract above; hosts answer JSON maps, so replies pass through without the typed-value decode; broken reply shapes fail as `firestore/invalid-response` | **Real-time snapshots: native tier could add it later; not in this contract.** The contract fixed above has no watch handler, so this area registers none and invents none; adding snapshots later is a native-stack decision on top of this contract |

Production REST is reachable (`with_rest_base`/`restBase`) but needs an
id token (`set_auth_token`/`setAuthToken` sends `Authorization: Bearer`,
the documented REST auth); nothing in this repo ever talks to production.

## Done

- `crates/nsh-firebase-firestore`: `wire.rs` (the documents/collection/
  runQuery URLs with `encodeURIComponent`-exact segment encoding,
  `(default)` kept parenthesized; the typed-value codec — null to the
  `NULL_VALUE` sentinel, safe integers to their string form, `-0` to
  "-0", past-2^53 integers and non-safe integer strings landing in
  floats; the structured-query builder with the SDK's direction and
  operator maps and the proto3-JSON `limit` (a bare number);
  `leaf_paths` for the merge mask, refusing keys a dot path cannot
  address; the runQuery reply parser (a JSON array, `document` items
  kept, `readTime`/`done` tails skipped); the REST error mapping —
  canonical `status` names to `firestore/*` codes, HTTP status fallback
  per google/rpc/code.proto), `firestore.rs` (the `Firestore` facade:
  `new(&app)` on the emulator default, `with_rest_base`, `with_host`,
  `set_auth_token`; native routing through the contract handlers when a
  host is wired, REST otherwise; `QueryPlan` building the URL, body and
  native args in one pass so both tiers validate identically).
- `packages/nsh-firebase/src/firestore`: `wire.ts`, `firestore.ts`,
  `index.ts` — the same behavior name for name (`firestore(app,
  settings?)` factory, `Firestore` class with `getDoc/setDoc/addDoc/
  deleteDoc/runQuery/setAuthToken`).
- Unit tests, headless with no real network: Rust in-crate (wire: value
  encode/decode round trips and key-order parity, path validation,
  query plans for top-level and nested collections, merge masks, id
  extraction, document/doc-list parsing, error mapping; facade via
  tokio + a recording host: the five contract calls' exact args and
  reply handling, `merge` present only when set, invalid-response on
  broken shapes, validation before routing in both tiers) — 16 wire
  tests + 8 facade tests.
  Rust UI `tests/firestore_ui.rs` (11 tests on a manual clock with
  `StubNetwork`: set/get round trip with the exact recorded PATCH body
  and content-type, merge mask URL + body, missing document as
  exists=false, addDoc POST body + id, delete and 404-done, the
  structured-query body exact, permission-denied mapping, bad path
  refused with zero requests, timestampValue invalid-data, Bearer
  header, non-JSON body invalid-response). TS `tests/firestore.test.tsx`
  (14 tests mirroring the list plus direct wire assertions, and the
  native tier through `open({ host: { handlers } })` with
  `app.host.calls()` asserted against the contract, including
  `merge` present only when set, and the invalid-response shapes).
- Labelled sandbox tests (runner-only), written to run green against
  the emulator suite by `bash scripts/test-emulator.sh` (which already
  lists both files): Rust
  `crates/nsh-firebase-firestore/tests/emulator_firestore.rs`
  (`#[ignore = "sandbox: needs emulator suite"]`; set, read back,
  addDoc, runQuery with `kind == <unique-per-run marker>`, deleteDoc,
  getDoc → exists=false) and TS
  `packages/nsh-firebase/tests/emulator/firestore.sandbox.test.tsx`
  (`network: "live"`, 127.0.0.1 only per tests/nsh.json; the same
  sequence through the `sb-*` buttons of the exercise UI).

## Cross-area touch

- `docs/research.md`: the single Firestore row of the tier table
  (section 4) was rewritten to say what is built and what is not.
  Nothing outside this area's files and that row was touched.

## Verified how

- Read-verification only (battery mode): every API name, wire shape and
  constant was checked against the sources — the v1 REST reference
  (documents get/patch/createDocument/delete/runQuery; parent URL
  templates; updateMask semantics "fields not referenced in the mask
  are left unchanged"; Int32Value `limit`), the firebase-js-sdk
  compiled bundle (`@firebase/firestore` 4.18
  `dist/common-*.esm.js`: `parseScalarValue` → `{nullValue:
  "NULL_VALUE"}`/`{stringValue}`/`{booleanValue}`; `toNumber` →
  `{integerValue: "" + n}` for safe integers else `doubleValue`,
  "-0"/"NaN"/"Infinity" spellings; `toInt32Proto` → bare number;
  `toQueryTarget` → parent split, `from[].collectionId`,
  `structuredQuery.parent` in the body; the `Ut`/`kt` direction and
  operator maps; `toUriEncodedString` → per-segment encodeURIComponent)
  and the repo's own core/database/storage/functions areas for
  conventions. The runQuery REST response shape (a JSON array of
  `{document, readTime}` items with `readTime`/`done` tails) is the
  documented production REST streaming shape; the emulator aims for
  wire compatibility with it.
- Every behavior the unit tests pin was hand-derived from those
  sources in both languages, and the two sides were written against
  each other (same messages, same key orders, same codes) so the
  mirror holds before the first run.

## Not verified (CI runs these first)

- Nothing was executed: `cargo fmt`/`clippy`/`test`,
  `nsh check`/`build`/`test`, and both sandbox tests are pending their
  first CI run (placeholder above). The riskiest unproven assumptions,
  in order: (1) the Firestore emulator serving runQuery's reply as a
  JSON array (the documented REST streaming shape); (2) the emulator
  accepting the structuredQuery `limit` as a bare proto3-JSON number;
  (3) the emulator's field ordering surviving set→get for multi-field
  docs (the sandbox uses one-field docs so its assertions do not
  depend on it); (4) Rust formatting/clippy lints on first compile.
- Pixels: nothing here draws; the exercise UIs are driven headless.
- Realtime listeners, transactions, composite filters, cursors and
  server transforms: intentionally absent (see the tier table), not
  failures.

## The one file each side must keep in step

`src/firestore/wire.ts` mirrors `src/wire.rs` function for function and
message for message (validation texts are asserted in both languages'
tests, so a one-sided edit fails CI on the other side's next run).
