// The records a perf transport delivers: a stopped trace, or an HTTP
// metric built from a monitor event. Mirrors
// crates/nsh-firebase-perf/src/perf.rs. The JSON of one record is one
// collector line; an http record's NetworkEvent fields sit flat beside
// `type` so lines stay greppable by host and path.

import type { NetworkEvent } from "../monitor";

export interface TraceRecord {
  type: "trace";
  name: string;
  startedAtMs: number;
  durationMs: number;
  metrics: Record<string, number>;
  attributes: Record<string, string>;
}

export interface HttpMetricRecord {
  type: "http";
  // The event's own fields, flattened into the record line.
  id: string;
  method: string;
  url: string;
  host: string;
  path: string;
  status: number | null;
  startedAtMs: number;
  durationMs: number;
  ttfbMs: number | null;
  requestBytes: number | null;
  responseBytes: number | null;
  error: string | null;
  initiator: string | null;
}

export type PerfRecord = TraceRecord | HttpMetricRecord;

// Builds an HTTP metric record from a monitor event.
export function httpMetric(event: NetworkEvent): HttpMetricRecord {
  return { type: "http", ...event };
}
