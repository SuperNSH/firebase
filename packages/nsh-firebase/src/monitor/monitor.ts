// The ring-buffer monitor with a subscribe API. Mirrors
// crates/nsh-firebase-perf/src/monitor.rs: record joins the ring (the
// oldest falls out past the capacity) and then the listeners hear it, in
// subscription order, over a snapshot taken at record.

import { isNetworkError, type NetworkEvent } from "./event";

// The ring keeps 500 events unless `capacity` says otherwise.
export const DEFAULT_CAPACITY = 500;

export type NetworkListener = (event: NetworkEvent) => void;

export interface MonitorOptions {
  capacity?: number;
}

// One host's or path's aggregate, as the screen and the perf records
// read it. An empty group is all zeros with null percentiles.
export interface Aggregate {
  count: number;
  errorRate: number;
  p50Ms: number | null;
  p95Ms: number | null;
}

// The nearest-rank percentile over the durations sorted ascending.
export function percentile(sorted: number[], q: number): number | null {
  if (sorted.length === 0) return null;
  const rank = Math.ceil(q * sorted.length);
  const index = Math.min(Math.max(rank, 1), sorted.length) - 1;
  return sorted[index];
}

// The aggregate of one group of events, deterministic: nearest-rank
// p50/p95, errors by isNetworkError.
export function aggregate(events: NetworkEvent[]): Aggregate {
  const count = events.length;
  if (count === 0) return { count: 0, errorRate: 0, p50Ms: null, p95Ms: null };
  const durations = events.map((event) => event.durationMs).sort((a, b) => a - b);
  const errors = events.filter(isNetworkError).length;
  return {
    count,
    errorRate: errors / count,
    p50Ms: percentile(durations, 0.5),
    p95Ms: percentile(durations, 0.95),
  };
}

// One aggregate per host (as the URL names it, port kept), hosts
// ascending. `by` keys the grouping so the per-path call shares the code.
export function aggregates(events: NetworkEvent[], by: (event: NetworkEvent) => string): Map<string, Aggregate> {
  const groups = new Map<string, NetworkEvent[]>();
  for (const event of events) {
    const key = by(event);
    const group = groups.get(key);
    if (group) group.push(event);
    else groups.set(key, [event]);
  }
  const out = new Map<string, Aggregate>();
  for (const key of [...groups.keys()].sort()) out.set(key, aggregate(groups.get(key)!));
  return out;
}

export const aggregatesByHost = (events: NetworkEvent[]) => aggregates(events, (event) => event.host);
export const aggregatesByPath = (events: NetworkEvent[]) => aggregates(events, (event) => event.path);

export class Monitor {
  readonly capacity: number;
  private readonly ring: NetworkEvent[] = [];
  private readonly byId = new Map<number, NetworkListener>();
  private nextId = 1;

  constructor(options?: MonitorOptions) {
    this.capacity = Math.max(1, options?.capacity ?? DEFAULT_CAPACITY);
  }

  // Records `event`: it joins the ring, then the listeners hear it. A
  // listener reading the monitor sees the event already in the ring.
  record(event: NetworkEvent): void {
    while (this.ring.length >= this.capacity) this.ring.shift();
    this.ring.push(event);
    for (const listener of [...this.byId.values()]) listener(event);
  }

  // The kept events, oldest first.
  events(): NetworkEvent[] {
    return [...this.ring];
  }

  // How many events are kept now.
  get length(): number {
    return this.ring.length;
  }

  // Subscribes to every recorded event; the returned function
  // unsubscribes (an unknown one changes nothing).
  subscribe(listener: NetworkListener): () => void {
    const id = this.nextId++;
    this.byId.set(id, listener);
    return () => this.byId.delete(id);
  }
}

// The app-wide instance: screens and instrumented fetches share one
// ring, so what the wrapper measures is what the monitor shows.
export const sharedMonitor = new Monitor();
