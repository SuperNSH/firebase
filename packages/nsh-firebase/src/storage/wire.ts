// The Cloud Storage REST wire format: percent-encoding, URLs and the
// object-metadata mapping, as the firebase-js-sdk storage layer speaks
// it. Mirrors crates/nsh-firebase-storage/src/wire.rs name for name.

import { FirebaseError } from "../core/error";
import type { FirebaseOptions } from "../core/options";

/** A slice must be a multiple of this, except the last. */
export const CHUNK_SIZE = 256 * 1024;

// The storage error codes, `storage/`-prefixed like the SDK's.
export const CANCELED = "storage/canceled";
export const UNKNOWN = "storage/unknown";
export const OBJECT_NOT_FOUND = "storage/object-not-found";
export const QUOTA_EXCEEDED = "storage/quota-exceeded";
export const UNAUTHENTICATED = "storage/unauthenticated";
export const UNAUTHORIZED = "storage/unauthorized";
export const RETRY_LIMIT_EXCEEDED = "storage/retry-limit-exceeded";
export const NO_DOWNLOAD_TOKEN = "storage/no-download-token";
export const NO_DEFAULT_BUCKET = "storage/no-default-bucket";
export const SERVER_FILE_WRONG_SIZE = "storage/server-wrong-file-size";

/** Percent-encodes a URL component exactly as `encodeURIComponent` does. */
export function encodeComponent(text: string): string {
  // encodeURIComponent is a global in the runtime; the runtime's is the
  // definition of the shape, so use it rather than re-deriving it.
  return encodeURIComponent(text);
}

/** The emulator switch answers a bare origin; the real config names the
 * full objects URL. Either way storage talks at `{origin}/v0/b/<bucket>/o`. */
export function origin(options: FirebaseOptions): string {
  if (options.emulator?.storage) {
    const endpoint = options.emulator.storage;
    return `http://${endpoint.host ?? "127.0.0.1"}:${endpoint.port}`;
  }
  if (!options.storageBucket) {
    throw new FirebaseError(NO_DEFAULT_BUCKET, "FirebaseOptions.storageBucket is empty");
  }
  return "https://firebasestorage.googleapis.com";
}

/** The objects URL of a bucket: uploads start here, lists and object
 * reads hang off it. */
export function objectsBase(originUrl: string, bucket: string): string {
  return `${originUrl}/v0/b/${encodeComponent(bucket)}/o`;
}

/** The object URL: the whole path encoded, slashes included. */
export function objectUrl(objectsBase: string, path: string): string {
  return `${objectsBase}/${encodeComponent(path)}`;
}

/** The URL a resumable upload starts at. */
export function startUrl(objectsBase: string, path: string): string {
  return `${objectsBase}?name=${encodeComponent(path)}`;
}

/** The object metadata a response describes; fullPath is the server's
 * `name`, name its last component, like the SDK's mappings. */
export interface ObjectMetadata {
  bucket?: string;
  generation?: string;
  metageneration?: string;
  fullPath: string;
  name: string;
  size?: number;
  timeCreated?: string;
  updated?: string;
  md5Hash?: string;
  cacheControl?: string;
  contentDisposition?: string;
  contentEncoding?: string;
  contentLanguage?: string;
  contentType?: string;
  customMetadata?: Record<string, string>;
  downloadTokens?: string;
}

/** The metadata a caller may set: upload's optional argument and what
 * updateMetadata patches — the writable mappings only. */
export interface SettableMetadata {
  cacheControl?: string;
  contentDisposition?: string;
  contentEncoding?: string;
  contentLanguage?: string;
  contentType?: string;
  md5Hash?: string;
  customMetadata?: Record<string, string>;
}

/** The first token's download URL, as the SDK builds it. */
export function downloadUrlFromMetadata(
  metadata: ObjectMetadata,
  originUrl: string,
): string | null {
  const token = metadata.downloadTokens?.split(",")[0];
  if (!token) return null;
  return (
    `${originUrl}/v0/b/${encodeComponent(metadata.bucket ?? "")}/o/` +
    `${encodeComponent(metadata.fullPath)}?alt=media&token=${encodeComponent(token)}`
  );
}

/** Parses a server metadata document (fromResourceString plus the
 * downloadTokens list the download URL is built from). */
export function parseObjectMetadata(json: Record<string, unknown>): ObjectMetadata {
  const string = (key: string): string | undefined =>
    typeof json[key] === "string" ? (json[key] as string) : undefined;
  const fullPath = string("name") ?? "";
  const rawSize = json["size"];
  const size =
    typeof rawSize === "number"
      ? rawSize
      : typeof rawSize === "string" && rawSize !== ""
        ? Number(rawSize)
        : undefined;
  const custom = json["metadata"];
  const customMetadata =
    custom && typeof custom === "object" && !Array.isArray(custom)
      ? (Object.fromEntries(
          Object.entries(custom as Record<string, unknown>).filter(
            ([, value]) => typeof value === "string",
          ),
        ) as Record<string, string>)
      : undefined;
  return {
    bucket: string("bucket"),
    generation: string("generation"),
    metageneration: string("metageneration"),
    fullPath,
    name: fullPath.slice(fullPath.lastIndexOf("/") + 1),
    size: Number.isFinite(size) ? size : undefined,
    timeCreated: string("timeCreated"),
    updated: string("updated"),
    md5Hash: string("md5Hash"),
    cacheControl: string("cacheControl"),
    contentDisposition: string("contentDisposition"),
    contentEncoding: string("contentEncoding"),
    contentLanguage: string("contentLanguage"),
    contentType: string("contentType"),
    customMetadata,
    downloadTokens: string("downloadTokens"),
  };
}

/** The fields set, as the server resource JSON: only writable mappings,
 * only when present. */
export function settableResource(metadata: SettableMetadata): Record<string, unknown> {
  const resource: Record<string, unknown> = {};
  for (const key of [
    "cacheControl",
    "contentDisposition",
    "contentEncoding",
    "contentLanguage",
    "contentType",
    "md5Hash",
  ] as const) {
    const value = metadata[key];
    if (value !== undefined) resource[key] = value;
  }
  if (metadata.customMetadata !== undefined) {
    resource["metadata"] = { ...metadata.customMetadata };
  }
  return resource;
}

/** A list page: the object paths under the prefix, the prefixes the
 * delimiter folded, and the next page's token. */
export interface ListResult {
  prefixes: string[];
  items: string[];
  nextPageToken: string | null;
}

/** Parses a list response; a missing items is no objects. */
export function parseListResult(json: Record<string, unknown>): ListResult {
  const items = Array.isArray(json["items"]) ? (json["items"] as unknown[]) : [];
  const prefixes = Array.isArray(json["prefixes"]) ? (json["prefixes"] as unknown[]) : [];
  const token = json["nextPageToken"];
  return {
    prefixes: prefixes.filter((value): value is string => typeof value === "string"),
    items: items
      .map((item) => (item as Record<string, unknown>)["name"])
      .filter((name): name is string => typeof name === "string"),
    nextPageToken: typeof token === "string" ? token : null,
  };
}

/** An HTTP failure becomes a storage error naming the object. */
export function mapStatusError(status: number, path: string, body: string): FirebaseError {
  const code =
    status === 401
      ? UNAUTHENTICATED
      : status === 402
        ? QUOTA_EXCEEDED
        : status === 403
          ? UNAUTHORIZED
          : status === 404
            ? OBJECT_NOT_FOUND
            : UNKNOWN;
  const detail = body.trim();
  return new FirebaseError(
    code,
    detail ? `${status} on ${path}: ${detail}` : `${status} on ${path}`,
  );
}
