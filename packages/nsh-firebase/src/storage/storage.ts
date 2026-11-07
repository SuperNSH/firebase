// Storage references and their reads: download, download URL, metadata,
// list and delete over the pure tier's fetch. The resumable upload lives
// in upload.ts. Mirrors crates/nsh-firebase-storage/src/storage.rs.

import { FirebaseError, toFirebaseError } from "../core/error";
import type { FirebaseApp } from "../core/app";
import { UploadJob, UploadTask } from "./upload";
import {
  mapStatusError,
  parseListResult,
  parseObjectMetadata,
  settableResource,
  downloadUrlFromMetadata,
  objectsBase,
  objectUrl,
  origin,
  startUrl,
  NO_DEFAULT_BUCKET,
  NO_DOWNLOAD_TOKEN,
  type ListResult,
  type ObjectMetadata,
  type SettableMetadata,
} from "./wire";

/** Read again before every request, so a refreshed token is honored. */
export type TokenProvider = () => string | null;

/** What a StorageRef needs of its storage: the resolved URLs and the
 * auth header of the moment. */
export interface StorageWire {
  /** `{origin}/v0/b/<bucket>/o`: uploads start here, object reads and
   * lists hang off it. */
  objectsBase: string;
  /** The origin alone, which download URLs are built from. */
  originUrl: string;
  authHeader(): Record<string, string>;
}

/** The Cloud Storage API of one app. */
export class FirebaseStorage implements StorageWire {
  readonly objectsBase: string;
  readonly originUrl: string;
  private auth: TokenProvider | null = null;

  constructor(app: FirebaseApp) {
    this.originUrl = origin(app.options);
    const bucket = app.options.storageBucket;
    if (!bucket) {
      throw new FirebaseError(NO_DEFAULT_BUCKET, "FirebaseOptions.storageBucket is empty");
    }
    this.objectsBase = objectsBase(this.originUrl, bucket);
  }

  /** Supplies `Authorization: Firebase <idToken>` for private objects. */
  withAuth(auth: TokenProvider): FirebaseStorage {
    this.auth = auth;
    return this;
  }

  authHeader(): Record<string, string> {
    const token = this.auth?.();
    return token ? { Authorization: `Firebase ${token}` } : {};
  }

  /** A reference of path under this bucket ("" is the root, for list);
   * a leading slash is dropped, as the SDK tolerates. */
  ref(path: string): StorageRef {
    const wire: StorageWire = this;
    return new StorageRef(wire, path.replace(/^\/+|\/+$/g, ""));
  }
}

/** One object path in the bucket. */
export class StorageRef {
  private readonly wire: StorageWire;
  /** The path, without a leading or trailing slash. */
  readonly path: string;

  constructor(wire: StorageWire, path: string) {
    this.wire = wire;
    this.path = path;
  }

  /** Starts a resumable upload of bytes (metadata is optional): returns
   * the task at once and runs its loop from here. */
  upload(bytes: Uint8Array, metadata?: SettableMetadata): UploadTask {
    return this.start(bytes, null, metadata);
  }

  /** Continues a previous session by its saved id (the session URI the
   * first run got): queries the server's offset, uploads only the rest. */
  resumeUpload(bytes: Uint8Array, sessionUrl: string): UploadTask {
    return this.start(bytes, sessionUrl, undefined);
  }

  private start(
    bytes: Uint8Array,
    sessionUrl: string | null,
    metadata?: SettableMetadata,
  ): UploadTask {
    // The start body: the metadata's writable fields plus the name
    // (metadataForUpload_); the content type defaults to the header.
    const resource = metadata ? settableResource(metadata) : {};
    const contentType = metadata?.contentType ?? "application/octet-stream";
    const body = { ...resource, name: this.path, contentType };
    const job: UploadJob = {
      startUrl: startUrl(this.wire.objectsBase, this.path),
      path: this.path,
      startBody: JSON.stringify(body),
      startContentType: contentType,
      sessionUrl,
      bytes,
      auth: () => {
        const header = this.wire.authHeader()["Authorization"];
        return header ? header.slice("Firebase ".length) : null;
      },
    };
    return new UploadTask(job, bytes.length);
  }

  /** The object's bytes (alt=media). */
  async download(): Promise<Uint8Array> {
    const response = await this.request(
      `${objectUrl(this.wire.objectsBase, this.path)}?alt=media`,
      { method: "GET" },
    );
    await this.ensureOk(response);
    return new Uint8Array(await response.arrayBuffer());
  }

  /** The first token's download URL from the object's metadata. */
  async downloadUrl(): Promise<string> {
    const metadata = await this.getMetadata();
    const url = downloadUrlFromMetadata(metadata, this.wire.originUrl);
    if (!url) {
      throw new FirebaseError(NO_DOWNLOAD_TOKEN, `${this.path} has no download token`);
    }
    return url;
  }

  /** The object's metadata as the server holds it. */
  async getMetadata(): Promise<ObjectMetadata> {
    const response = await this.request(objectUrl(this.wire.objectsBase, this.path), {
      method: "GET",
    });
    await this.ensureOk(response);
    return parseObjectMetadata(await response.json());
  }

  /** Patches the settable metadata and returns what the server holds. */
  async updateMetadata(change: SettableMetadata): Promise<ObjectMetadata> {
    const response = await this.request(objectUrl(this.wire.objectsBase, this.path), {
      method: "PATCH",
      headers: { "Content-Type": "application/json; charset=utf-8" },
      body: JSON.stringify(settableResource(change)),
    });
    await this.ensureOk(response);
    return parseObjectMetadata(await response.json());
  }

  /** One page of the objects under this prefix: full paths, the
   * prefixes the delimiter folded, and the next page's token. */
  async list(maxResults?: number, pageToken?: string): Promise<ListResult> {
    const prefix = this.path ? `${this.path}/` : "";
    let url = `${this.wire.objectsBase}?prefix=${encodeURIComponent(prefix)}&delimiter=/`;
    if (pageToken !== undefined) {
      url += `&pageToken=${encodeURIComponent(pageToken)}`;
    }
    if (maxResults !== undefined) {
      url += `&maxResults=${maxResults}`;
    }
    const response = await this.request(url, { method: "GET" });
    await this.ensureOk(response);
    return parseListResult(await response.json());
  }

  /** Deletes the object; deleting a missing one is object-not-found. */
  async delete(): Promise<void> {
    const response = await this.request(objectUrl(this.wire.objectsBase, this.path), {
      method: "DELETE",
    });
    await this.ensureOk(response);
  }

  /** Sends the request with the auth header wired on this storage. */
  private async request(url: string, init: RequestInit): Promise<Response> {
    try {
      return await fetch(url, {
        ...init,
        headers: { ...this.wire.authHeader(), ...(init.headers ?? {}) },
      });
    } catch (thrown) {
      throw toFirebaseError(thrown);
    }
  }

  /** An HTTP failure becomes a storage error naming the object. */
  private async ensureOk(response: Response): Promise<void> {
    if (response.ok) return;
    let body = "";
    try {
      body = await response.text();
    } catch {
      // The body may already be gone; the status is enough.
    }
    throw mapStatusError(response.status, this.path, body);
  }
}
