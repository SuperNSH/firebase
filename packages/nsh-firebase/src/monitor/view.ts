// The sorted, filterable rows the monitoring screen shows. Mirrors
// crates/nsh-firebase-perf/src/view.rs exactly, ties included: duration
// descending (slowest on top), ties by startedAtMs then id.

import type { NetworkEvent } from "./event";

// What the screen filters by: host exact, path a substring,
// minDurationMs at-or-above, status exact. A filter left undefined keeps
// everything on that axis (including events with no status).
export interface MonitorFilter {
  host?: string;
  path?: string;
  minDurationMs?: number;
  status?: number;
}

function matches(event: NetworkEvent, filter: MonitorFilter): boolean {
  if (filter.host !== undefined && event.host !== filter.host) return false;
  if (filter.path !== undefined && !event.path.includes(filter.path)) return false;
  if (filter.minDurationMs !== undefined && event.durationMs < filter.minDurationMs) return false;
  if (filter.status !== undefined && event.status !== filter.status) return false;
  return true;
}

// The rows the screen shows: `events` under `filter`, slowest first.
export function monitorRows(events: NetworkEvent[], filter: MonitorFilter = {}): NetworkEvent[] {
  return events
    .filter((event) => matches(event, filter))
    .sort(
      (a, b) =>
        b.durationMs - a.durationMs ||
        a.startedAtMs - b.startedAtMs ||
        (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
    );
}
