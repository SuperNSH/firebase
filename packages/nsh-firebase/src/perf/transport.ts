// The pluggable transport and its two shapes: batching (queue, flush at
// the batch size or on demand) and the newline-JSON sink poster for
// tools/collector. Mirrors crates/nsh-firebase-perf/src/perf.rs.

import { CODES, FirebaseError, requestText } from "../core";
import type { FirebaseRequestInit } from "../core";
import type { PerfRecord } from "./records";

// Where records go in the pure tier. Analytics uses the same shape: one
// `send` per batch, resolving when they are accepted.
export interface PerfTransport {
  send(records: PerfRecord[]): Promise<void>;
  // Delivers whatever the transport queued; a non-batching transport has
  // nothing to flush.
  flush(): Promise<void>;
}

// Queues records and delivers them through `inner` once `maxBatch` are
// waiting, or when flush() is called.
export class BatchingTransport implements PerfTransport {
  readonly capacity: number;
  private readonly queue: PerfRecord[] = [];

  constructor(
    private readonly inner: PerfTransport,
    maxBatch: number,
  ) {
    this.capacity = Math.max(1, maxBatch);
  }

  get queued(): number {
    return this.queue.length;
  }

  async send(records: PerfRecord[]): Promise<void> {
    this.queue.push(...records);
    if (this.queue.length >= this.capacity) await this.flush();
  }

  async flush(): Promise<void> {
    if (this.queue.length === 0) return;
    const batch = this.queue.splice(0, this.queue.length);
    await this.inner.send(batch);
  }
}

// Posts each batch as newline-separated JSON records
// (`application/x-ndjson`) to `url` — the shape tools/collector appends
// to its sink. The sandbox's stand-in for Performance's own ingestion,
// which has no public REST to call.
export class HttpLineTransport implements PerfTransport {
  constructor(private readonly url: string) {}

  async send(records: PerfRecord[]): Promise<void> {
    if (records.length === 0) return;
    const body = records.map((record) => JSON.stringify(record)).join("\n");
    const init: FirebaseRequestInit = {
      method: "POST",
      headers: { "content-type": "application/x-ndjson" },
      body,
    };
    const response = await requestText(this.url, init);
    if (!response.ok) {
      throw new FirebaseError(CODES.network, `the sink answered ${response.status}`);
    }
  }

  // Delivers as it goes: nothing is ever queued.
  async flush(): Promise<void> {}
}
