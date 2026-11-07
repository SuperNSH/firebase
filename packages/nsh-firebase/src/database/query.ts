// Query params and the client-side sort: the REST filters of
// https://firebase.google.com/docs/database/rest/retrieve-data
// (orderBy, startAt, endAt, equalTo, limitToFirst, limitToLast), encoded
// as URL query parameters. The server answers unsorted; sortChildren
// puts the children in the documented order. Mirrors
// crates/nsh-firebase-database/src/query.rs.

import { CODES, FirebaseError } from "../core/error";

/** JSON as the database holds it. */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

/** `$key`: order children by their keys. */
export const ORDER_KEY = "$key";
/** `$value`: order children by their values. */
export const ORDER_VALUE = "$value";

/** What an `orderBy` orders by. */
export type Order =
  | { kind: "child"; key: string }
  | { kind: "key" }
  | { kind: "value" };

/** The `orderBy` string of it. */
export function orderParam(order: Order): string {
  if (order.kind === "key") return ORDER_KEY;
  if (order.kind === "value") return ORDER_VALUE;
  return order.key;
}

/** Parses an `orderBy` value (`"$key"`, `"$value"`, or a child path). */
export function parseOrder(orderBy: string): Order {
  if (orderBy === ORDER_KEY) return { kind: "key" };
  if (orderBy === ORDER_VALUE) return { kind: "value" };
  return { kind: "child", key: orderBy };
}

/** The query a DatabaseReference carries, encoded on `get` and on
 * listeners as REST query parameters. */
export interface QueryParams {
  orderBy?: string;
  /** Inclusive lower bound (a JSON value). */
  startAt?: Json;
  /** Inclusive upper bound. */
  endAt?: Json;
  /** Exactly this value. */
  equalTo?: Json;
  limitToFirst?: number;
  limitToLast?: number;
}

/** Whether a query is set at all. */
export function isEmptyQuery(query: QueryParams): boolean {
  return (
    query.orderBy === undefined &&
    query.startAt === undefined &&
    query.endAt === undefined &&
    query.equalTo === undefined &&
    query.limitToFirst === undefined &&
    query.limitToLast === undefined
  );
}

/** The filters the docs document: one `orderBy` per request, and every
 * filter needs one; a request has one limit at most. */
export function validateQuery(query: QueryParams): void {
  const filtered =
    query.startAt !== undefined ||
    query.endAt !== undefined ||
    query.equalTo !== undefined ||
    query.limitToFirst !== undefined ||
    query.limitToLast !== undefined;
  if (filtered && query.orderBy === undefined) {
    throw new FirebaseError(
      CODES.invalidArgument,
      "a query filter needs orderBy (orderByChild, orderByKey or orderByValue)",
    );
  }
  if (query.limitToFirst !== undefined && query.limitToLast !== undefined) {
    throw new FirebaseError(CODES.invalidArgument, "limitToFirst and limitToLast cannot combine");
  }
}

/** The query as ordered `[name, encoded value]` pairs (the order is
 * fixed, so a test's stub route can match the URL whole). `auth` is
 * appended by the caller that knows the token. */
export function encodeQuery(query: QueryParams): [string, string][] {
  validateQuery(query);
  const params: [string, string][] = [];
  if (query.orderBy !== undefined) {
    params.push(["orderBy", encodeComponent(JSON.stringify(query.orderBy))]);
  }
  for (const [name, value] of [
    ["startAt", query.startAt],
    ["endAt", query.endAt],
    ["equalTo", query.equalTo],
  ] as const) {
    if (value !== undefined) params.push([name, encodeComponent(JSON.stringify(value))]);
  }
  if (query.limitToFirst !== undefined) params.push(["limitToFirst", `${query.limitToFirst}`]);
  if (query.limitToLast !== undefined) params.push(["limitToLast", `${query.limitToLast}`]);
  return params;
}

/** The children of an object value, in the order `order` puts them. A
 * non-object keeps as it is. The sort is stable, and object values
 * compare as equal (the docs put objects last without an inner order). */
export function sortChildren(root: Json, order: Order): Json {
  if (root === null || typeof root !== "object" || Array.isArray(root)) return root;
  const entries = Object.entries(root);
  const byKey = order.kind === "key";
  entries.sort((a, b) => {
    if (byKey) return compareKeys(a[0], b[0]);
    return compareValues(childValue(a[1], order), childValue(b[1], order));
  });
  return Object.fromEntries(entries);
}

/** The value an order reads: the child at its key (`child`), or the
 * value itself (`value`). A missing child is null, which the documented
 * order puts first. */
function childValue(value: Json, order: Order): Json {
  if (order.kind !== "child") return value;
  let at = value;
  for (const part of order.key.split("/")) {
    // Own properties only: an inherited key ("constructor") is absent.
    at =
      at !== null && typeof at === "object" && !Array.isArray(at) && Object.hasOwn(at, part)
        ? at[part]
        : null;
  }
  return at;
}

/** The documented total order: null < false < true < numbers < strings
 * < objects; within numbers and strings, their natural order. */
export function compareValues(a: Json, b: Json): number {
  const rank = (value: Json): number => {
    if (value === null) return 0;
    if (typeof value === "boolean") return value ? 2 : 1;
    if (typeof value === "number") return 3;
    if (typeof value === "string") return 4;
    return 5;
  };
  const byRank = rank(a) - rank(b);
  if (byRank !== 0) return byRank;
  if (typeof a === "number" && typeof b === "number") return a - b;
  if (typeof a === "string" && typeof b === "string") return a < b ? -1 : a > b ? 1 : 0;
  return 0;
}

/** Keys parseable as 32-bit integers come first, in numeric order; the
 * rest follow lexicographically. */
function compareKeys(a: string, b: string): number {
  const aInt = asInt32(a);
  const bInt = asInt32(b);
  if (aInt !== null && bInt !== null) return aInt - bInt;
  if (aInt !== null) return -1;
  if (bInt !== null) return 1;
  return a < b ? -1 : a > b ? 1 : 0;
}

function asInt32(key: string): number | null {
  const parsed = Number(key);
  return Number.isInteger(parsed) && parsed >= -2147483648 && parsed <= 2147483647 ? parsed : null;
}

/** Percent-encodes a query parameter's value, matching
 * `encode_component` in the Rust crate on the characters the query
 * values use. */
export function encodeComponent(value: string): string {
  let out = "";
  for (const byte of new TextEncoder().encode(value)) {
    if (
      (byte >= 0x41 && byte <= 0x5a) ||
      (byte >= 0x61 && byte <= 0x7a) ||
      (byte >= 0x30 && byte <= 0x39) ||
      byte === 0x2d ||
      byte === 0x5f ||
      byte === 0x2e ||
      byte === 0x7e
    ) {
      out += String.fromCharCode(byte);
    } else {
      out += `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
    }
  }
  return out;
}
