# perfmon vision record

The demo's monitor route, looked at on the real macOS target
(`nsh test examples/demo/tests/perfmon-shots.test.tsx --target macos`,
log in `demo-shots.log`). Both PNGs were opened and read by a person.

## monitor-light.png

The Performance page with four live measured calls. The collector's slow
route (`GET /slow`, ~1200 ms, semibold) is visibly the first row; below it
the `POST /collect`, `GET /assert` and `GET /health` rows at single- and
low-double-digit milliseconds, so the slowest call is on top and the
ordering is obvious at a glance. The exact numbers move a few
milliseconds per run — real time on a real target — and this record
describes the PNGs in this directory.
Each row carries the method chip, the path, the duration on the right, and
a second line with host, ttfb, status and the initiator
(`firebase-fetch`). The filter chips (All selected, primary; ≥ 1 s;
Errors) and the Reload control sit above the list; the summary reads
"4 calls · 0 errors". Nothing uses a raw color; every surface and text
is a theme token.

## monitor-dark.png

The same screen under the dark appearance: tokens follow the theme
(muted chips, dark background, light text), the selected All chip uses
the dark primary with readable `primary-foreground`, the slow row stays
bold and on top. No clipped text, no invisible part.

## What the run proved

- Real per-call timings against the collector's deliberately slow route
  (the delay is the route's own ~1200 ms; the recorded duration is real
  time, ~1200 ms in the saved PNGs) with the slow route visibly on top.
- Sorting and filtering are visible and reversible: the Errors filter
  empties the list to the honest empty state, All restores it.
- The screen is fed by `instrumentFetch` over the shared monitor, so the
  rows are the same ring a real app's calls fill.
