// The Firestore v1 REST wire format: the document URLs, the typed-value
// encoding, the structured-query builder and the response/error mapping,
// exactly as the REST reference
// (https://firebase.google.com/docs/firestore/reference/rest/v1) writes
// it and as the firebase-js-sdk's JsonProtoSerializer encodes values
// (`@firebase/firestore` 4.18, parseScalarValue, toNumber, toInt32Proto,
// toQueryTarget, the direction and operator maps). Mirrors
// crates/nsh-firebase-firestore/src/wire.rs name for name.

import { CODES, FirebaseError } from "../core/error";
import type { FirebaseResponse } from "../core/http";

/** The sandbox suite's Firestore emulator (emulator/firebase.json). */
export const EMULATOR_PORT = 18080;
export const EMULATOR_BASE = `http://127.0.0.1:${EMULATOR_PORT}`;
/** The production REST host; the emulator base wins in this sandbox. */
export const PRODUCTION_BASE = "https://firestore.googleapis.com";
/** The database every call here names; named databases are not configured. */
export const DEFAULT_DATABASE = "(default)";

// The native handlers this module routes through when a host answers
// (coordinator-fixed; see the crate docs for the full shapes).
export const GET_DOC = "firebase.firestore.getDoc";
export const SET_DOC = "firebase.firestore.setDoc";
export const ADD_DOC = "firebase.firestore.addDoc";
export const DELETE_DOC = "firebase.firestore.deleteDoc";
export const RUN_QUERY = "firebase.firestore.runQuery";

/** A reply that breaks its contract (firestore/invalid-response). */
export const INVALID_RESPONSE = "firestore/invalid-response";
/** A value the JSON contract cannot carry (firestore/invalid-data):
 * non-finite doubles, and typed values beyond the seven this client
 * speaks (timestampValue, bytesValue, geoPointValue, referenceValue and
 * friends). */
export const INVALID_DATA = "firestore/invalid-data";

// The canonical error statuses the REST error body carries, each as the
// firestore/-prefixed code the SDK's FirestoreError uses.
export const CANCELLED = "firestore/cancelled";
export const UNKNOWN = "firestore/unknown";
export const INVALID_ARGUMENT_CODE = "firestore/invalid-argument";
export const DEADLINE_EXCEEDED = "firestore/deadline-exceeded";
export const NOT_FOUND = "firestore/not-found";
export const ALREADY_EXISTS = "firestore/already-exists";
export const PERMISSION_DENIED = "firestore/permission-denied";
export const UNAUTHENTICATED = "firestore/unauthenticated";
export const RESOURCE_EXHAUSTED = "firestore/resource-exhausted";
export const FAILED_PRECONDITION = "firestore/failed-precondition";
export const ABORTED = "firestore/aborted";
export const OUT_OF_RANGE = "firestore/out-of-range";
export const UNIMPLEMENTED = "firestore/unimplemented";
export const INTERNAL = "firestore/internal";
export const UNAVAILABLE = "firestore/unavailable";
export const DATA_LOSS = "firestore/data-loss";

/** The JSON one document carries. */
export type Json = string | number | boolean | null | Json[] | { [key: string]: Json };

/** A getDoc outcome: `data` is null exactly when the document is absent
 * (the REST read answers 404, which the facade turns into exists:false). */
export interface DocSnapshot {
  exists: boolean;
  data: Record<string, Json> | null;
}

/** One query result: the id the server assigned and its JSON data. */
export interface QueryDoc {
  id: string;
  data: Record<string, Json>;
}

/** The filter operators the wire speaks, keyed by the SDK's client
 * strings (its `kt` map). */
export type WhereOp =
  | "=="
  | "<"
  | "<="
  | ">"
  | ">="
  | "!="
  | "array-contains"
  | "in"
  | "not-in"
  | "array-contains-any";

const WIRE_OPS: Record<WhereOp, string> = {
  "==": "EQUAL",
  "<": "LESS_THAN",
  "<=": "LESS_THAN_OR_EQUAL",
  ">": "GREATER_THAN",
  ">=": "GREATER_THAN_OR_EQUAL",
  "!=": "NOT_EQUAL",
  "array-contains": "ARRAY_CONTAINS",
  in: "IN",
  "not-in": "NOT_IN",
  "array-contains-any": "ARRAY_CONTAINS_ANY",
};

/** The where filter the facade carries. */
export interface WhereFilter {
  field: string;
  op: WhereOp;
  value: Json;
}

/** The sort directions (the SDK's `Ut` map). */
export type Direction = "asc" | "desc";

const WIRE_DIRECTIONS: Record<Direction, string> = {
  asc: "ASCENDING",
  desc: "DESCENDING",
};

/** One sort; the direction defaults to asc, as the SDK's does. */
export interface OrderBy {
  field: string;
  direction?: Direction;
}

/** The structured query one runQuery runs: a shallow collection query
 * with one optional where filter, sorts, and a limit. Composite filters,
 * cursors and offsets have no facade surface. */
export interface FirestoreQuery {
  collectionPath: string;
  where?: WhereFilter;
  orderBy?: OrderBy[];
  limit?: number;
}

export function invalidArgument(message: string): FirebaseError {
  return new FirebaseError(CODES.invalidArgument, message);
}

export function invalidResponse(message: string): FirebaseError {
  return new FirebaseError(INVALID_RESPONSE, message);
}

export function invalidData(message: string): FirebaseError {
  return new FirebaseError(INVALID_DATA, message);
}

/** The largest integer the JSON contract carries exactly
 * (Number.MAX_SAFE_INTEGER); larger integers go over as doubles, as the
 * SDK's isSafeInteger gate does. */
const MAX_SAFE_INTEGER = 9_007_199_254_740_991;

/** Percent-encodes one URL path segment (the SDK's toUriEncodedString);
 * `(default)` keeps its parentheses. */
export function encodeSegment(text: string): string {
  return encodeURIComponent(text);
}

/** The documents root of the v1 surface:
 * `{base}/v1/projects/{project}/databases/(default)/documents`. */
export function documentsBase(restBase: string, projectId: string): string {
  return `${restBase.replace(/\/+$/, "")}/v1/projects/${encodeSegment(projectId)}/databases/${encodeSegment(DEFAULT_DATABASE)}/documents`;
}

/** Splits `path` into its segments, checking the shape Firestore gives
 * each kind: a document path names collection/document pairs (even
 * count), a collection path an odd count; ids may not be empty, `.` or
 * `..`. */
function splitPath(path: string, kind: "document" | "collection"): string[] {
  const segments = path.replace(/^\/+|\/+$/g, "").split("/");
  for (const segment of segments) {
    if (segment === "") {
      throw invalidArgument(`the path \`${path}\` has an empty segment`);
    }
    if (segment === "." || segment === "..") {
      throw invalidArgument(
        `the path \`${path}\` uses \`.\` or \`..\`; Firestore ids cannot`,
      );
    }
  }
  const even = segments.length % 2 === 0;
  const fits = kind === "document" ? even : !even;
  if (!fits) {
    throw invalidArgument(
      kind === "document"
        ? `not a document path: \`${path}\` must name collection/document pairs (an even segment count)`
        : `not a collection path: \`${path}\` must name collections (an odd segment count)`,
    );
  }
  return segments;
}

export function splitDocPath(path: string): string[] {
  return splitPath(path, "document");
}

export function splitCollectionPath(path: string): string[] {
  return splitPath(path, "collection");
}

function encoded(segments: string[]): string {
  return segments.map(encodeSegment).join("/");
}

/** The URL of one document: `{base}/{segments}`, each segment
 * percent-encoded. */
export function documentUrl(base: string, path: string): string {
  return `${base}/${encoded(splitDocPath(path))}`;
}

/** The URL one document is created under: the collection path itself,
 * whose last segment is the collectionId. */
export function collectionUrl(base: string, path: string): string {
  return `${base}/${encoded(splitCollectionPath(path))}`;
}

/** What one runQuery needs on the wire: the POST URL (the parent path,
 * minus the collectionId, then `:runQuery`), the JSON body (`parent`
 * carried canonically, unencoded) and the native handler's args. One
 * plan so both tiers validate identically. */
export interface QueryPlan {
  url: string;
  body: Record<string, unknown>;
  nativeArgs: Record<string, unknown>;
}

/** Builds the plan for `query` against `restBase`. Every validation the
 * two tiers share lives here, so a refusal is identical whichever tier
 * answers. */
export function runQueryPlan(
  restBase: string,
  projectId: string,
  query: FirestoreQuery,
): QueryPlan {
  const segments = splitCollectionPath(query.collectionPath);
  const collectionId = segments[segments.length - 1];
  const parent = segments.slice(0, -1);
  // Key order is load-bearing: the Rust side builds the same order, so
  // the JSON bodies match character for character.
  const body: Record<string, unknown> = {
    parent: canonicalParent(projectId, parent),
  };
  const structured: Record<string, unknown> = {
    from: [{ collectionId }],
  };
  if (query.where) structured.where = fieldFilter(query.where);
  if (query.orderBy && query.orderBy.length > 0) {
    structured.orderBy = query.orderBy.map(orderEntry);
  }
  if (query.limit !== undefined) {
    if (!Number.isInteger(query.limit) || query.limit < 1) {
      throw invalidArgument("limit must be at least 1");
    }
    structured.limit = query.limit;
  }
  body.structuredQuery = structured;
  const base = documentsBase(restBase, projectId);
  const url =
    parent.length === 0
      ? `${base}:runQuery`
      : `${base}/${encoded(parent)}:runQuery`;
  return { url, body, nativeArgs: nativeQueryArgs(query) };
}

/** The resource name of the query's parent, canonical (unencoded) as the
 * request body's `parent` carries it. */
function canonicalParent(projectId: string, parent: string[]): string {
  const base = `projects/${projectId}/databases/${DEFAULT_DATABASE}/documents`;
  return parent.length === 0 ? base : `${base}/${parent.join("/")}`;
}

function fieldFilter(filter: WhereFilter): Record<string, unknown> {
  if (!filter.field) {
    throw invalidArgument("the where filter needs a field path");
  }
  const wireOp = WIRE_OPS[filter.op];
  if (!wireOp) {
    throw invalidArgument(
      `${filter.op} is not a where op; one of ==, <, <=, >, >=, !=, array-contains, in, not-in, array-contains-any`,
    );
  }
  if (
    (filter.op === "in" || filter.op === "not-in" || filter.op === "array-contains-any") &&
    !Array.isArray(filter.value)
  ) {
    throw invalidArgument(`\`${filter.op}\` needs an array value`);
  }
  return {
    fieldFilter: {
      field: { fieldPath: filter.field },
      op: wireOp,
      value: encodeValue(filter.value),
    },
  };
}

function orderEntry(order: OrderBy): Record<string, unknown> {
  if (!order.field) {
    throw invalidArgument("the orderBy needs a field path");
  }
  const direction = order.direction ?? "asc";
  const wire = WIRE_DIRECTIONS[direction];
  if (!wire) {
    throw invalidArgument(`${direction} is not a direction; asc or desc`);
  }
  return {
    field: { fieldPath: order.field },
    direction: wire,
  };
}

/** The native handler's args: the contract's optional keys appear only
 * when set, in the contract's order. */
function nativeQueryArgs(query: FirestoreQuery): Record<string, unknown> {
  const args: Record<string, unknown> = { collectionPath: query.collectionPath };
  if (query.where) {
    args.where = {
      field: query.where.field,
      op: query.where.op,
      value: query.where.value,
    };
  }
  if (query.orderBy && query.orderBy.length > 0) {
    args.orderBy = query.orderBy.map((order) => ({
      field: order.field,
      direction: order.direction ?? "asc",
    }));
  }
  if (query.limit !== undefined) args.limit = query.limit;
  return args;
}

/** Encodes one JSON value as the REST typed value: null to the
 * NULL_VALUE sentinel, safe integers to their string form, -0 to the
 * SDK's "-0" spelling. A JS number JSON cannot carry (NaN, the
 * infinities) is invalid-data, not a silent null. */
export function encodeValue(value: Json): Record<string, unknown> {
  if (value === null) return { nullValue: "NULL_VALUE" };
  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      throw invalidData(`the number ${value} has no JSON form`);
    }
    // -0 as a JSON number would lose its sign; the SDK spells it "-0".
    if (Object.is(value, -0)) return { doubleValue: "-0" };
    if (Number.isInteger(value) && Math.abs(value) <= MAX_SAFE_INTEGER) {
      return { integerValue: `${value}` };
    }
    return { doubleValue: value };
  }
  if (typeof value === "string") return { stringValue: value };
  if (typeof value === "boolean") return { booleanValue: value };
  if (Array.isArray(value)) {
    return { arrayValue: { values: value.map(encodeValue) } };
  }
  if (typeof value === "object") {
    const fields: Record<string, unknown> = {};
    for (const [key, inner] of Object.entries(value)) {
      fields[key] = encodeValue(inner);
    }
    return { mapValue: { fields } };
  }
  throw invalidData(`a value of type \`${typeof value}\` has no wire form`);
}

/** Decodes one typed value back to JSON. Types beyond the seven this
 * client speaks (a timestamp written by another client, say) are an
 * invalid-data error, not a silent loss. */
export function decodeValue(typed: unknown): Json {
  if (typeof typed !== "object" || typed === null) {
    throw invalidData("the value is not a typed value object");
  }
  const entries = typed as Record<string, unknown>;
  if ("nullValue" in entries) return null;
  if ("booleanValue" in entries) {
    if (typeof entries.booleanValue !== "boolean") {
      throw invalidData("the booleanValue is not a boolean");
    }
    return entries.booleanValue;
  }
  if ("integerValue" in entries) return decodeInteger(entries.integerValue);
  if ("doubleValue" in entries) return decodeDouble(entries.doubleValue);
  if ("stringValue" in entries) {
    if (typeof entries.stringValue !== "string") {
      throw invalidData("the stringValue is not a string");
    }
    return entries.stringValue;
  }
  if ("arrayValue" in entries) {
    const values = (entries.arrayValue as { values?: unknown } | null)?.values;
    if (values === undefined || values === null) return [];
    if (!Array.isArray(values)) {
      throw invalidData("the arrayValue's values are not an array");
    }
    return values.map(decodeValue);
  }
  if ("mapValue" in entries) {
    return decodeFields((entries.mapValue as { fields?: unknown } | null)?.fields);
  }
  const named = Object.keys(entries)[0] ?? "?";
  throw invalidData(
    `the value type \`${named}\` has no JSON form; this client carries string, integer, double, boolean, null, map and array values only`,
  );
}

function decodeInteger(inner: unknown): number {
  if (typeof inner !== "string") {
    throw invalidData(`the integerValue ${JSON.stringify(inner)} is not a string`);
  }
  // The Rust side parses i64 (a sign and digits, nothing else, in i64
  // range) and then lands past-2^53 values in a float; the same two
  // steps here.
  let exact: bigint;
  try {
    if (!/^[+-]?[0-9]+$/.test(inner)) throw new Error("digits only");
    exact = BigInt(inner);
  } catch {
    throw invalidData(`the integerValue \`${inner}\` is not a number`);
  }
  if (exact > 9223372036854775807n || exact < -9223372036854775808n) {
    throw invalidData(`the integerValue \`${inner}\` is not a number`);
  }
  return Number(inner);
}

function decodeDouble(inner: unknown): number {
  if (typeof inner === "number") return inner;
  if (typeof inner === "string") {
    if (inner === "NaN" || inner === "Infinity" || inner === "-Infinity") {
      throw invalidData(
        `the doubleValue \`${inner}\` has no JSON form; JSON cannot carry it`,
      );
    }
    // Only the decimal spellings decode, as the Rust f64 parse does.
    if (!/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(inner)) {
      throw invalidData(`the doubleValue \`${inner}\` is not a number`);
    }
    return Number(inner);
  }
  throw invalidData(`the doubleValue ${JSON.stringify(inner)} is not a number`);
}

/** Encodes the document data (a JSON object of fields) as the wire's
 * `fields` map. */
export function encodeFields(data: Record<string, Json>): Record<string, unknown> {
  if (typeof data !== "object" || data === null || Array.isArray(data)) {
    throw invalidArgument("the document data must be a JSON object of fields");
  }
  const fields: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(data)) {
    fields[key] = encodeValue(value);
  }
  return fields;
}

/** Decodes the wire's `fields` map (absent: an empty object). */
export function decodeFields(fields: unknown): Record<string, Json> {
  if (fields === undefined || fields === null) return {};
  if (typeof fields !== "object" || Array.isArray(fields)) {
    throw invalidResponse("the document's `fields` is not an object");
  }
  const out: Record<string, Json> = {};
  for (const [key, typed] of Object.entries(fields)) {
    out[key] = decodeValue(typed);
  }
  return out;
}

/** The leaf field paths of `data` (dot-joined; maps descend, arrays and
 * empty maps are leaves): what a merge write sends as
 * updateMask.fieldPaths. Keys a dot path cannot address (`.`, backtick,
 * `~`, `*`, `/`, `\`) are refused — the server would read them as other
 * fields. */
export function leafPaths(data: Record<string, Json>): string[] {
  if (typeof data !== "object" || data === null || Array.isArray(data)) {
    throw invalidArgument("the document data must be a JSON object of fields");
  }
  if (Object.keys(data).length === 0) {
    throw invalidArgument(
      "a merge write needs at least one field; the REST patch mask cannot be empty",
    );
  }
  const paths: string[] = [];
  collectLeafPaths(data, "", paths);
  return paths;
}

// The characters a dot-joined mask path cannot address.
const MASK_UNSAFE = new Set([".", "`", "~", "*", "/", "\\"]);

function collectLeafPaths(value: Json, prefix: string, paths: string[]): void {
  if (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.keys(value).length > 0
  ) {
    for (const [key, inner] of Object.entries(value)) {
      const unsafe = [...key].find((char) => MASK_UNSAFE.has(char));
      if (unsafe !== undefined) {
        throw invalidArgument(
          `the field key \`${key}\` cannot be addressed by a merge mask; write it without merge`,
        );
      }
      collectLeafPaths(inner, prefix === "" ? key : `${prefix}.${key}`, paths);
    }
    return;
  }
  // An empty map is itself the leaf value.
  paths.push(prefix);
}

/** The merge patch's query string: one updateMask.fieldPaths= pair per
 * leaf path, in the data's order. */
export function mergeMaskQuery(paths: string[]): string {
  return paths
    .map((path) => `updateMask.fieldPaths=${encodeSegment(path)}`)
    .join("&");
}

/** The document id: the last segment of the resource name. */
export function idFromName(name: string): string {
  const id = name.split("/").pop();
  if (!id) {
    throw invalidResponse(`the document name \`${name}\` has no id`);
  }
  return id;
}

/** Maps a query result document to its id and JSON data. */
export function queryDocFrom(document: unknown): QueryDoc {
  const name = (document as Record<string, unknown> | null)?.name;
  if (typeof name !== "string") {
    throw invalidResponse("the document answered without a name");
  }
  return {
    id: idFromName(name),
    data: decodeFields((document as Record<string, unknown>).fields),
  };
}

/** Maps a present document (a getDoc or createDocument reply) to a
 * snapshot. */
export function snapshotFrom(document: unknown): DocSnapshot {
  return {
    exists: true,
    data: decodeFields((document as Record<string, unknown> | null)?.fields),
  };
}

/** Parses a JSON reply body (a non-JSON 2xx body is a broken server, not
 * a network loss). */
export function parseBody(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    throw invalidResponse("the server sent a body that is not JSON");
  }
}

/** Maps a runQuery reply (the REST streaming shape: a JSON array whose
 * items carry `document` and `readTime`, the trailing ones only
 * readTime/done) to its documents. */
export function docsFromRunQuery(text: string): QueryDoc[] {
  const items = parseBody(text);
  if (!Array.isArray(items)) {
    throw invalidResponse("the runQuery response is not an array");
  }
  const docs: QueryDoc[] = [];
  for (const item of items) {
    // The stream's tail items mark progress, not documents.
    const record = item as Record<string, unknown> | null;
    if (typeof record !== "object" || record === null || !("document" in record)) {
      continue;
    }
    docs.push(queryDocFrom(record.document));
  }
  return docs;
}

/** A 2xx stands; anything else maps through the REST error body. */
export function ensureOk(response: FirebaseResponse): FirebaseResponse {
  if (response.status >= 200 && response.status < 300) return response;
  throw errorForResponse(response.status, response.text);
}

const CANONICAL_CODES: Record<string, string> = {
  CANCELLED,
  UNKNOWN,
  INVALID_ARGUMENT: INVALID_ARGUMENT_CODE,
  DEADLINE_EXCEEDED,
  NOT_FOUND,
  ALREADY_EXISTS,
  PERMISSION_DENIED,
  UNAUTHENTICATED,
  RESOURCE_EXHAUSTED,
  FAILED_PRECONDITION,
  ABORTED,
  OUT_OF_RANGE,
  UNIMPLEMENTED,
  INTERNAL,
  UNAVAILABLE,
  DATA_LOSS,
};

/** The HTTP status fallback (google/rpc/code.proto's mapping, as the
 * SDK's mapCodeFromRpcCode applies it), used when the body names no
 * status. */
const STATUS_CODES: Record<number, string> = {
  400: INVALID_ARGUMENT_CODE,
  401: UNAUTHENTICATED,
  403: PERMISSION_DENIED,
  404: NOT_FOUND,
  409: ABORTED,
  429: RESOURCE_EXHAUSTED,
  499: CANCELLED,
  500: INTERNAL,
  501: UNIMPLEMENTED,
  503: UNAVAILABLE,
  504: DEADLINE_EXCEEDED,
};

/** The error a non-2xx reply carries: the body's canonical `status`
 * names the code (an unknown name is firestore/unknown), its `message`
 * is the message, and the HTTP status stands when the body carries
 * neither. */
export function errorForResponse(status: number, text: string): FirebaseError {
  let error: Record<string, unknown> | undefined;
  try {
    error = (JSON.parse(text) as { error?: Record<string, unknown> } | null)?.error;
  } catch {
    error = undefined;
  }
  const canonical = typeof error?.status === "string" ? error.status : null;
  const code = canonical
    ? (CANONICAL_CODES[canonical] ?? UNKNOWN)
    : (STATUS_CODES[status] ?? UNKNOWN);
  const message = typeof error?.message === "string" ? error.message : text;
  return new FirebaseError(code, `${message} (${status})`);
}
