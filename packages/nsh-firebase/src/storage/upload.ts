// The resumable upload task: one async loop that starts a session,
// queries what the server already holds and pushes 256 KiB slices, with
// the progress signal and the caller's pause/resume/cancel. Mirrors
// crates/nsh-firebase-storage/src/upload.rs.

import { signal, type Signal } from "nsh";
import { FirebaseError, toFirebaseError } from "../core/error";
import {
  CANCELED,
  CHUNK_SIZE,
  RETRY_LIMIT_EXCEEDED,
  SERVER_FILE_WRONG_SIZE,
  mapStatusError as mapStatusErrorOf,
  parseObjectMetadata,
  type ObjectMetadata,
} from "./wire";

/** The loop's sentinel for "the caller paused": runSession throws it and
 * run() turns it into the paused state, never into a rejection. */
const PAUSED = "storage/paused";

export type UploadState = "paused" | "running" | "done" | "error";

export interface UploadProgress {
  bytesTransferred: number;
  totalBytes: number;
  state: UploadState;
}

export interface UploadAuth {
  /** Read before every request, so a refreshed token is honored. */
  (): string | null;
}

export interface UploadJob {
  startUrl: string;
  path: string;
  startBody: string;
  startContentType: string;
  sessionUrl: string | null;
  bytes: Uint8Array;
  auth: UploadAuth | null;
}

export class UploadTask {
  /** The progress signal: bytesTransferred, totalBytes, state. */
  readonly progress: Signal<UploadProgress>;
  /** The final metadata; rejects with the reason on error or cancel. */
  readonly result: Promise<ObjectMetadata>;

  private paused = false;
  private cancelled = false;
  private readonly job: UploadJob;
  private ended = false;
  private resolveResult!: (metadata: ObjectMetadata) => void;
  private rejectResult!: (error: FirebaseError) => void;

  constructor(job: UploadJob, totalBytes: number) {
    this.progress = signal({ bytesTransferred: 0, totalBytes, state: "running" });
    this.job = job;
    this.result = new Promise((resolve, reject) => {
      this.resolveResult = resolve;
      this.rejectResult = reject;
    });
    // Nobody may await `result`: its rejection must not surface as an
    // unhandled rejection on top of the progress state.
    this.result.catch(() => {});
    void this.run();
  }

  /** The session URI the server issued — what a resumed run persists
   * across process death — or null before the start answered. */
  get sessionUrl(): string | null {
    return this.job.sessionUrl;
  }

  /** Stops after the slice in flight; the loop ends in `paused` and the
   * session stays on the server for resume(). */
  pause(): void {
    if (this.progress().state !== "running") return;
    this.paused = true;
  }

  /** Queries the session for the server's offset and runs the rest. */
  resume(): void {
    if (this.progress().state !== "paused") return;
    this.paused = false;
    this.setProgress(this.progress().bytesTransferred, "running");
    void this.run();
  }

  /** Abandons the upload: the task ends in `error` with
   * storage/canceled (an in-flight slice still lands). */
  cancel(): void {
    this.cancelled = true;
    if (this.progress().state === "paused") {
      // No loop is running: end it here.
      this.end(new FirebaseError(CANCELED, "the upload was canceled"));
    }
  }

  private async run(): Promise<void> {
    this.setRunning(this.progress().bytesTransferred);
    try {
      this.end(await this.runSession());
    } catch (thrown) {
      const error = toFirebaseError(thrown);
      if (error.code === PAUSED && this.paused) {
        this.setProgress(this.progress().bytesTransferred, "paused");
        return;
      }
      this.end(error);
    }
  }

  private async runSession(): Promise<ObjectMetadata> {
    const total = this.progress().totalBytes;
    const path = this.job.path;
    // A denial carries `x-goog-upload-status: final` even on a 403:
    // the HTTP status is read before any upload-status header.
    const ensureOk = async (response: Response): Promise<void> => {
      if (response.ok) return;
      let body = "";
      try {
        body = await response.text();
      } catch {
        // The body may already be gone; the status is enough.
      }
      throw mapStatusErrorOf(response.status, path, body);
    };
    let session = this.job.sessionUrl;
    if (session === null) {
      const response = await this.request(this.job.startUrl, {
        method: "POST",
        headers: {
          "X-Goog-Upload-Protocol": "resumable",
          "X-Goog-Upload-Command": "start",
          "X-Goog-Upload-Header-Content-Length": String(total),
          "X-Goog-Upload-Header-Content-Type": this.job.startContentType,
          "Content-Type": "application/json; charset=utf-8",
        },
        body: this.job.startBody,
      });
      await ensureOk(response);
      checkUploadStatus(response, ["active"]);
      session = response.headers.get("X-Goog-Upload-URL");
      if (!session) {
        throw new FirebaseError(
          RETRY_LIMIT_EXCEEDED,
          "the start response has no X-Goog-Upload-URL",
        );
      }
      this.job.sessionUrl = session;
    }

    // The server's offset is the truth: it may hold more than the last
    // confirmation reached, which is what makes resume-from-id safe.
    const query = await this.request(session, {
      method: "POST",
      headers: { "X-Goog-Upload-Command": "query" },
    });
    await ensureOk(query);
    const queryStatus = checkUploadStatus(query, ["active", "final"]);
    const rawSize = query.headers.get("X-Goog-Upload-Size-Received");
    // Missing or empty would read as 0 and restart the upload from
    // scratch, so it counts as a lost session, not an offset.
    const sizeReceived = rawSize === null || rawSize === "" ? NaN : Number(rawSize);
    if (!Number.isFinite(sizeReceived)) {
      throw new FirebaseError(
        RETRY_LIMIT_EXCEEDED,
        "the query response has no X-Goog-Upload-Size-Received",
      );
    }
    if (sizeReceived > total) {
      throw new FirebaseError(
        SERVER_FILE_WRONG_SIZE,
        `the session holds ${sizeReceived} bytes, more than the ${total} to upload`,
      );
    }
    if (queryStatus === "final" && sizeReceived !== total) {
      throw new FirebaseError(
        SERVER_FILE_WRONG_SIZE,
        `the session finalized at ${sizeReceived} bytes, not the ${total} to upload`,
      );
    }
    this.setRunning(sizeReceived);

    let offset = sizeReceived;
    while (offset < total) {
      if (this.cancelled) {
        throw new FirebaseError(CANCELED, "the upload was canceled");
      }
      if (this.paused) {
        // Leave the session live; resume() starts a fresh loop that
        // queries before it slices.
        throw new FirebaseError(PAUSED, "the upload is paused; resume() continues it");
      }
      const end = Math.min(offset + CHUNK_SIZE, total);
      const command = end === total ? "upload, finalize" : "upload";
      const response = await this.request(session, {
        method: "POST",
        headers: {
          "X-Goog-Upload-Command": command,
          "X-Goog-Upload-Offset": String(offset),
          "Content-Type": "application/octet-stream",
        },
        body: this.job.bytes.slice(offset, end),
      });
      await ensureOk(response);
      const status = checkUploadStatus(response, ["active", "final"]);
      offset = end;
      if (status === "final") {
        return parseObjectMetadata(await response.json());
      }
      this.transferred(offset);
    }

    // The server holds every byte but has not finalized (the size landed
    // on the chunk boundary): an empty `finalize` asks for the metadata.
    if (this.cancelled) {
      throw new FirebaseError(CANCELED, "the upload was canceled");
    }
    const finalize = await this.request(session, {
      method: "POST",
      headers: {
        "X-Goog-Upload-Command": "finalize",
        "X-Goog-Upload-Offset": String(offset),
        "Content-Type": "application/octet-stream",
      },
    });
    await ensureOk(finalize);
    checkUploadStatus(finalize, ["final"]);
    return parseObjectMetadata(await finalize.json());
  }

  /** Sends the request with the wired auth token, mapping refusals and
   * failed requests to stable errors; an HTTP error status does not. */
  private async request(url: string, init: RequestInit): Promise<Response> {
    const headers = { ...(init.headers as Record<string, string>) };
    const token = this.job.auth?.();
    if (token) headers["Authorization"] = `Firebase ${token}`;
    try {
      return await fetch(url, { ...init, headers });
    } catch (thrown) {
      throw toFirebaseError(thrown);
    }
  }

  private setProgress(bytesTransferred: number, state: UploadState): void {
    const current = this.progress();
    // A write of the state it already holds notifies nobody (the Rust
    // side's signal equality does the same).
    if (current.bytesTransferred === bytesTransferred && current.state === state) {
      return;
    }
    this.progress.set({
      bytesTransferred,
      totalBytes: current.totalBytes,
      state,
    });
  }

  private setRunning(bytesTransferred: number): void {
    this.setProgress(bytesTransferred, "running");
  }

  /** Advances bytesTransferred while the loop is still running (a late
   * slice response after a pause leaves the state alone). */
  private transferred(bytesTransferred: number): void {
    if (this.progress().state === "running") {
      this.setProgress(bytesTransferred, "running");
    }
  }

  /** Records a final outcome (once) and moves the state to done or
   * error. A done upload reports all bytes: the finalize answered. */
  private end(outcome: ObjectMetadata | FirebaseError): void {
    if (this.ended) return;
    this.ended = true;
    const failed = outcome instanceof FirebaseError;
    const snapshot = this.progress();
    this.progress.set({
      bytesTransferred: failed ? snapshot.bytesTransferred : snapshot.totalBytes,
      totalBytes: snapshot.totalBytes,
      state: failed ? "error" : "done",
    });
    if (outcome instanceof FirebaseError) {
      this.rejectResult(outcome);
    } else {
      this.resolveResult(outcome);
    }
  }
}

/** The upload status header must name one of `allowed`
 * (checkResumeHeader_ in the SDK; a miss is the retry-limit error). */
function checkUploadStatus(response: Response, allowed: string[]): string {
  const status = response.headers.get("X-Goog-Upload-Status");
  if (!status || !allowed.includes(status)) {
    throw new FirebaseError(
      RETRY_LIMIT_EXCEEDED,
      `the upload status is ${status ?? "missing"}, not one of ${allowed.join("/")}`,
    );
  }
  return status;
}
