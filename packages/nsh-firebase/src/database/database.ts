// The database and its references: the REST verbs of
// https://firebase.google.com/docs/database/rest/save-data and the
// reads of https://firebase.google.com/docs/database/rest/retrieve-data
// — get, set, multi-path update, remove, client-generated push,
// conditional-write transactions (a GET with `X-Firebase-ETag: true`, a
// PUT with `if-match`; a `412` carries the current value and a fresh
// ETag, and the client rebuilds — the docs do not retry for you) — plus
// the live listeners of ./listener. Mirrors
// crates/nsh-firebase-database/src/database.rs name for name.

import { cleanup, signal } from "nsh";
import {
  CODES,
  FirebaseError,
  databaseBaseUrl,
  getApp,
  joinUrl,
  toFirebaseError,
  type FirebaseApp,
} from "../core";
import { pushId } from "./push";
import {
  encodeComponent,
  encodeQuery,
  isEmptyQuery,
  parseOrder,
  sortChildren,
  type Json,
  type QueryParams,
} from "./query";
import { startListen, type ListenHandle, type ListenStatus, type Listener } from "./listener";

/** A conditional-write transaction's outcome. */
export interface TransactionResult {
  /** Whether the write happened. False when `apply` aborted (returning
   * undefined) or the attempts ran out. */
  committed: boolean;
  /** The value at the ref when it ended. */
  snapshot: Json | null;
}

/** Conditional writes give up after this many `412` rebuilds (the SDK's
 * own cap); the last snapshot stands, committed false. */
const MAX_TRANSACTION_ATTEMPTS = 25;

/** One app's Realtime Database: the base URL (the emulator switch wins
 * over databaseURL) and the auth token `?auth=` carries. */
export class Database {
  readonly baseUrl: string;
  private authToken: string | null = null;

  constructor(readonly app: FirebaseApp) {
    const base = databaseBaseUrl(app.options);
    if (!base) {
      throw new FirebaseError(
        CODES.invalidArgument,
        "the app has no databaseURL and no database emulator",
      );
    }
    this.baseUrl = base;
  }

  /** The id token `?auth=` carries on every request (the documented
   * query-parameter form, REST page "Auth"); none: the rules decide.
   * The auth area's refreshed tokens land here. */
  setAuthToken(token: string | null): void {
    this.authToken = token;
  }

  /** Internal: the token a request builder appends. */
  auth(): string | null {
    return this.authToken;
  }

  /** A reference at `path` (relative, `/`-separated; `""` is the root). */
  ref(path = ""): DatabaseReference {
    return new DatabaseReference(this, path.replace(/^\/+|\/+$/g, ""), {});
  }
}

/** One database per app, as the modular SDK's `getDatabase` keeps: the
 * auth token set on the instance every later `getDatabase(app)` hands
 * back is the same one. */
const databases = new Map<FirebaseApp, Database>();

/** The database of an app (`"[DEFAULT]"` without one). */
export function getDatabase(app: FirebaseApp = getApp()): Database {
  let db = databases.get(app);
  if (!db) {
    db = new Database(app);
    databases.set(app, db);
  }
  return db;
}

/** The request a call makes, and what a failed one carries. */
interface DatabaseResponse {
  status: number;
  ok: boolean;
  text: string;
  header(name: string): string | null;
}

/** Sends one REST call; a refusal or a failed request surfaces as a
 * FirebaseError. An HTTP error status does not throw (the caller's
 * mapStatus does). */
async function request(url: string, init?: RequestInit): Promise<DatabaseResponse> {
  let response: Response;
  try {
    response = await fetch(url, init);
  } catch (error) {
    throw toFirebaseError(error);
  }
  const headers = response.headers;
  return {
    status: response.status,
    ok: response.ok,
    text: await response.text(),
    header(name: string): string | null {
      return headers.get(name);
    },
  };
}

/** Maps a failed REST response to the stable codes: a `permission_denied`
 * body is an access denial, a wrong request an invalid argument, and
 * everything else a network failure. */
function mapStatus(response: DatabaseResponse): DatabaseResponse {
  const status = response.status;
  if (status >= 200 && status < 300) return response;
  let message = response.text;
  try {
    const body = JSON.parse(message) as Record<string, unknown>;
    if (body && typeof body.error === "string") message = body.error;
  } catch {
    // The body was not the error document: the text stands.
  }
  const code =
    status === 401 || status === 403
      ? CODES.accessDenied
      : status === 400 || status === 404
        ? CODES.invalidArgument
        : CODES.network;
  throw new FirebaseError(code, `${message} (${status})`);
}

/** A body of `null` (an absent location) is none; anything else must
 * parse, as the Rust side's parseOptional demands — a non-JSON body is
 * a network failure, not a stray SyntaxError. */
function parseOptional(text: string): Json | null {
  if (text === "" || text === "null") return null;
  try {
    return JSON.parse(text) as Json;
  } catch (error) {
    throw new FirebaseError(CODES.network, `the database sent invalid JSON: ${error}`);
  }
}

/** A location in the database, with the query its reads carry. */
export class DatabaseReference {
  /** A reference under this one (`"a/b"` descends two levels). */
  child(path: string): DatabaseReference {
    const trimmed = path.replace(/^\/+|\/+$/g, "");
    const full = this.path === "" ? trimmed : trimmed === "" ? this.path : `${this.path}/${trimmed}`;
    return new DatabaseReference(this.db, full, this.query);
  }

  /** Reads the value, none when the location holds null. A queried
   * read sorts its children (the REST answers unsorted). */
  async get(): Promise<Json | null> {
    const params = isEmptyQuery(this.query) ? [] : encodeQuery(this.query);
    const response = mapStatus(await request(this.urlOf(params)));
    let value = parseOptional(response.text);
    if (value !== null && !isEmptyQuery(this.query)) {
      value = sortChildren(value, parseOrder(this.query.orderBy as string));
    }
    return value;
  }

  /** Writes `value` (a null is a remove). */
  async set(value: Json): Promise<void> {
    mapStatus(
      await request(this.plainUrl(), { method: "PUT", body: JSON.stringify(value) }),
    );
  }

  /** Merges `value`'s keys (relative paths from this ref, `"a/b"`
   * descending) into what is there. */
  async update(value: Record<string, Json>): Promise<void> {
    mapStatus(await request(this.plainUrl(), { method: "PATCH", body: JSON.stringify(value) }));
  }

  /** Deletes the location. */
  async remove(): Promise<void> {
    mapStatus(await request(this.plainUrl(), { method: "DELETE" }));
  }

  /** Generates a push id (the timestamp of the app's clock, plus random
   * bits; two within one millisecond keep their order) and writes
   * `value` there. The id is known before the write lands, so lists can
   * hold it at once. */
  async push(value: Json): Promise<string> {
    const id = pushId(Date.now());
    await this.child(id).set(value);
    return id;
  }

  /** Runs a transaction: reads with an ETag, applies `apply` to the
   * current value (returning undefined aborts, committed false), and
   * puts with `if-match`; on the server's `412` it rebuilds from the
   * carried value, at most MAX_TRANSACTION_ATTEMPTS times. */
  async transaction(
    apply: (current: Json | null) => Json | undefined,
  ): Promise<TransactionResult> {
    const url = this.plainUrl();
    for (let round = 0; round < MAX_TRANSACTION_ATTEMPTS; round++) {
      const read = mapStatus(await request(url, { headers: { "X-Firebase-ETag": "true" } }));
      const etag = read.header("etag");
      if (!etag) {
        throw new FirebaseError(CODES.network, "the database sent no ETag for the transaction read");
      }
      const current = parseOptional(read.text);
      const next = apply(current);
      if (next === undefined) {
        return { committed: false, snapshot: current };
      }
      const write = await request(url, {
        method: "PUT",
        headers: { "if-match": etag },
        body: JSON.stringify(next),
      });
      if (write.status >= 200 && write.status < 300) {
        return { committed: true, snapshot: parseOptional(write.text) };
      }
      if (write.status === 412) {
        // Changed elsewhere: the next round's tagged read rebuilds from
        // what the server holds now.
        continue;
      }
      mapStatus(write);
    }
    return { committed: false, snapshot: await this.get() };
  }

  // The query builders: each returns a reference with one more filter,
  // as React Native Firebase's chain does.

  orderByChild(key: string): DatabaseReference {
    return this.withQuery({ orderBy: key });
  }

  orderByKey(): DatabaseReference {
    return this.orderByChild("$key");
  }

  orderByValue(): DatabaseReference {
    return this.orderByChild("$value");
  }

  /** An inclusive lower bound (a number, string, boolean or null). */
  startAt(value: Json): DatabaseReference {
    return this.withQuery({ startAt: value });
  }

  /** An inclusive upper bound. */
  endAt(value: Json): DatabaseReference {
    return this.withQuery({ endAt: value });
  }

  /** Exactly this value. */
  equalTo(value: Json): DatabaseReference {
    return this.withQuery({ equalTo: value });
  }

  limitToFirst(count: number): DatabaseReference {
    return this.withQuery({ limitToFirst: count });
  }

  limitToLast(count: number): DatabaseReference {
    return this.withQuery({ limitToLast: count });
  }

  /** Subscribes a listener: the value and the connection state arrive
   * through `listener`. Off with the handle. An invalid query throws
   * here, like `get` on the same ref would. */
  listen(listener: Listener): ListenHandle {
    // Built once now, so a query that cannot encode fails at subscribe
    // time instead of silently retrying forever.
    this.streamUrl();
    return startListen({
      url: () => this.streamUrl(),
      listener,
    });
  }

  /** The value and the status as signals, so UI bindings update by
   * themselves: `data` holds none until the first put, `status` the
   * connection's state. The listener stops in the cleanup of the scope
   * it was made in; `off` stops it before that. */
  onValue(): { data: () => Json | null; status: () => ListenStatus; off(): void } {
    const data = signal<Json | null>(null);
    const status = signal<ListenStatus>("connecting");
    const handle = this.listen((value, state) => {
      data.set(value);
      status.set(state);
    });
    cleanup(() => handle.off());
    return {
      data: () => data(),
      status: () => status(),
      off: () => handle.off(),
    };
  }

  /** Internal: make references through Database.ref and the builders. */
  constructor(
    readonly db: Database,
    readonly path: string,
    readonly query: QueryParams,
  ) {}

  private withQuery(change: Partial<QueryParams>): DatabaseReference {
    return new DatabaseReference(this.db, this.path, { ...this.query, ...change });
  }

  /** The request URL: `<base>/<path>.json`, the query's parameters,
   * then `auth`. Each path segment is percent-encoded (a space or a
   * `?` is a legal RTDB key and must not break the URL); the slashes
   * that separate segments stay. */
  private urlOf(params: [string, string][]): string {
    const path = this.path === "" ? "" : this.path.split("/").map(encodeComponent).join("/");
    let url = joinUrl(this.db.baseUrl, `${path}.json`);
    const query = [...params];
    const auth = this.db.auth();
    if (auth) query.push(["auth", auth]);
    if (query.length > 0) {
      url += `?${query.map(([name, value]) => `${name}=${value}`).join("&")}`;
    }
    return url;
  }

  private plainUrl(): string {
    return this.urlOf([]);
  }

  /** The listen URL: the read URL, which the listener asks for as an
   * event stream (the streaming section's first rule). */
  private streamUrl(): string {
    return this.urlOf(isEmptyQuery(this.query) ? [] : encodeQuery(this.query));
  }
}

/** A reference into `db` (React Native Firebase's `database().ref`). */
export function ref(db: Database, path?: string): DatabaseReference {
  return db.ref(path);
}
