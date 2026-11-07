# Upstream issues: nsh-query and nsh-form

Read-only review of `~/fo/nsh-query` and `~/fo/nsh-form` (READMEs, package
sources, Rust crate sources) for bugs or gaps nsh-firebase must work around.
Nothing here blocks this repo; each item lists the workaround where one is
needed.

## nsh-query

- **Call-shape difference on keyed re-subscription, not a parity gap**: the
  README's "A key that changes" row is `use_query_by(move ||
  key(id.get()), ..)` on Rust and `key: () => ["users", id()]` on
  TypeScript. Both re-subscribe when the key changes; on Rust that means the
  separate `use_query_by` hook with a `move` closure, on TypeScript the
  reactive key closure goes inside the one `createQuery` options object. Not
  a bug for us — our demo uses the TypeScript shape — a Rust demo screen
  with a changing key must use `use_query_by`, not `use_query` with an id
  inside the fetcher. Workaround: none needed, just use the right call per
  language.
- **Foreground refetch rides NSH's `lifecycle` signal**
  (`crates/nsh-query/src/query.rs:458`, `packages/nsh-query/src/query.ts:272`
  — `Lifecycle::Active`/`"active"` transitions trigger `refetchOnForeground`,
  default `"if-stale"`). Not a bug; noted because our live SSE listeners must
  not fight it: a foreground refetch of the same data a listener already
  pushed is harmless but wasteful, so our screens either prefer a listener or
  a query, not both on one key.
- **Retry/backoff and GC timers use real timers** (`setTimeout` /
  `setInterval` at `packages/nsh-query/src/query.ts:113,151,266`). Under
  `nsh test`'s manual clock these advance with the fake clock (the library's
  own tests pass that way per its README), so our tests on a manual clock
  are fine. No workaround.
- No bug found that affects nsh-firebase. Cache invalidation
  (`invalidates: ["todos"]`) and `setData`/`rollback` give us everything the
  demo needs.

## nsh-form

- **Parity gap: `form.array(...)` exists only in TypeScript.** The README
  says "TypeScript also has typed dotted paths and `form.array("items")`"
  (`packages/nsh-form/src/form.ts`); there is no array builder in
  `crates/nsh-form/src/` (checked `form.rs`, `field.rs`). Any Rust demo
  screen that needs a dynamic list of rows must hand-roll it with signals.
  Workaround for us: the demo's forms are sign-in/flat (email, password),
  so this does not bite; noted so a later screen does not assume parity.
- **Async-rule debouncing uses `setTimeout`** (`form.ts:138,162`). Same
  manual-clock situation as nsh-query; no workaround.
- No bug found that affects nsh-firebase. Server errors landing on a field
  by dotted path (`{ fields: { email: "Unknown account" } }` from `submit`)
  is exactly the shape our auth demo needs to surface Identity Toolkit
  error codes as field messages (`EMAIL_NOT_FOUND` / `INVALID_PASSWORD` -> a
  message on the offending field).
