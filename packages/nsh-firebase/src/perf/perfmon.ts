// The Performance Monitoring facade: startTrace and recordHttpMetric,
// with a local ring of records and delivery through the native tier when
// the firebase.perf.* handlers exist, else through the pluggable
// transport. Mirrors crates/nsh-firebase-perf/src/perf.rs and the fixed
// coordinator shapes (docs/status/perfmon.md):
//   firebase.perf.startTrace({name}) -> {id: string}
//   firebase.perf.stopTrace({id})
//   firebase.perf.recordHttpMetric(<a NetworkEvent, as the args object>)

import { CODES, FirebaseError, type FirebaseTier } from "../core";
import type { NetworkEvent } from "../monitor";
import { httpMetric, type PerfRecord, type TraceRecord } from "./records";
import type { PerfTransport } from "./transport";

// The local ring keeps the last 500 records.
const RECORDS_CAPACITY = 500;

// The name of the throwaway trace the first record probes with: it tells
// "handlers exist" apart from "a host exists but grants nothing", which
// posts alone cannot (an unhandled post is dropped silently).
export const PROBE_TRACE_NAME = "__nsh_probe__";

// The fixed handler names.
export const START_TRACE = "firebase.perf.startTrace";
export const STOP_TRACE = "firebase.perf.stopTrace";
export const RECORD_HTTP_METRIC = "firebase.perf.recordHttpMetric";

export interface PerfmonOptions {
  tier: FirebaseTier;
  transport?: PerfTransport;
}

export interface TraceHandle {
  putMetric(name: string, value: number): void;
  putAttribute(name: string, value: string): void;
  stop(): Promise<TraceRecord>;
}

interface TraceState {
  name: string;
  startedAtMs: number;
  start: number;
  metrics: Record<string, number>;
  attributes: Record<string, string>;
  nativeId: string | null;
}

export class Perfmon {
  private native: Promise<boolean> | null = null;
  private readonly records_: PerfRecord[] = [];

  constructor(private readonly options: PerfmonOptions) {}

  // Starts a trace named `name` on the clock. On the native tier the
  // handler gives the trace's id and stop() stops it there; a pure-tier
  // trace goes out through the transport at stop().
  async startTrace(name: string): Promise<TraceHandle> {
    const native = await this.nativeAvailable();
    let nativeId: string | null = null;
    if (native) {
      const answer = (await this.options.tier.call(START_TRACE, { name })) as { id?: unknown };
      if (typeof answer?.id !== "string") {
        // The same shape the Rust trace_id returns for this case.
        throw new FirebaseError(
          CODES.hostError,
          "firebase.perf.startTrace answered without a string id",
        );
      }
      nativeId = answer.id;
    }
    const state: TraceState = {
      name,
      startedAtMs: Date.now(),
      start: performance.now(),
      metrics: {},
      attributes: {},
      nativeId,
    };
    const perfmon = this;
    return {
      putMetric(metricName, value) {
        state.metrics[metricName] = value;
      },
      putAttribute(attributeName, value) {
        state.attributes[attributeName] = value;
      },
      async stop(): Promise<TraceRecord> {
        const durationMs = performance.now() - state.start;
        const record: TraceRecord = {
          type: "trace",
          name: state.name,
          startedAtMs: state.startedAtMs,
          durationMs,
          metrics: { ...state.metrics },
          attributes: { ...state.attributes },
        };
        state.metrics = {};
        state.attributes = {};
        perfmon.push(record);
        if (state.nativeId !== null) {
          await perfmon.options.tier.call(STOP_TRACE, { id: state.nativeId });
          return record;
        }
        if (perfmon.options.transport) {
          await perfmon.options.transport.send([record]);
        }
        return record;
      },
    };
  }

  // Builds an HTTP metric from a monitor event and records it. The event
  // joins the local ring either way; the native tier gets it as the
  // recordHttpMetric post, the pure tier through the transport.
  async recordHttpMetric(event: NetworkEvent): Promise<void> {
    const native = await this.nativeAvailable();
    this.push(httpMetric(event));
    if (native) {
      this.options.tier.post(RECORD_HTTP_METRIC, event);
      return;
    }
    if (this.options.transport) {
      await this.options.transport.send([httpMetric(event)]);
    }
  }

  // The last records, oldest first (traces and HTTP metrics mixed).
  records(): PerfRecord[] {
    return [...this.records_];
  }

  // Flushes the transport's queue (a no-op without a batching one).
  async flush(): Promise<void> {
    await this.options.transport?.flush();
  }

  // Whether firebase.perf.* handlers answer, probed once with a
  // throwaway startTrace (stopped again when it works). A host that
  // grants nothing must not swallow records into unhandled posts, which
  // the runtime drops silently.
  private nativeAvailable(): Promise<boolean> {
    if (!this.native) {
      this.native = this.probe();
    }
    return this.native;
  }

  private async probe(): Promise<boolean> {
    try {
      // A refusal (the name not granted) or a missing host rejects; both
      // mean the transport takes the records, so nothing is lost.
      const answer = (await this.options.tier.call(START_TRACE, { name: PROBE_TRACE_NAME })) as {
        id?: unknown;
      };
      if (typeof answer?.id === "string") {
        await this.options.tier.call(STOP_TRACE, { id: answer.id });
        return true;
      }
      return false;
    } catch {
      return false;
    }
  }

  private push(record: PerfRecord): void {
    while (this.records_.length >= RECORDS_CAPACITY) this.records_.shift();
    this.records_.push(record);
  }
}

// The one-line builder the demo and tests use: a facade over the app's
// tier with an optional pure-tier transport.
export function createPerfmon(options: PerfmonOptions): Perfmon {
  return new Perfmon(options);
}
