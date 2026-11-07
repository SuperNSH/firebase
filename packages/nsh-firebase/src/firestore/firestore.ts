// The Firestore of one app: documents CRUD and structured queries over
// the v1 REST surface (the emulator by default), or native routing
// through the coordinator-fixed handlers when the host answers. Mirrors
// crates/nsh-firebase-firestore/src/firestore.rs name for name.
//
// The REST tier is the pure tier: document reads, writes, creates,
// deletes and shallow structured queries, with values in the REST
// typed-value encoding. There are no realtime listeners here: the REST
// surface has none to speak of, and the contract below adds no watch
// handler on purpose. The native tier's hosts return JSON maps, so its
// replies pass through without the typed-value decode.
//
// Native handler contract (coordinator-fixed; the native-stack agent
// implements exactly these on both hosts):
// - firebase.firestore.getDoc({path}) -> {exists: bool, data: object|null}
// - firebase.firestore.setDoc({path, data, merge?}) -> {}
// - firebase.firestore.addDoc({collectionPath, data}) -> {id}
// - firebase.firestore.deleteDoc({path}) -> {}
// - firebase.firestore.runQuery({collectionPath, where?, orderBy?, limit?})
//   -> {docs: [{id, data}]}

import type { FirebaseApp } from "../core/app";
import { requestText, type FirebaseRequestInit, type FirebaseResponse } from "../core/http";
import {
  ADD_DOC,
  DELETE_DOC,
  EMULATOR_BASE,
  GET_DOC,
  RUN_QUERY,
  SET_DOC,
  collectionUrl,
  documentUrl,
  docsFromRunQuery,
  encodeFields,
  ensureOk,
  invalidResponse,
  leafPaths,
  mergeMaskQuery,
  parseBody,
  queryDocFrom,
  runQueryPlan,
  snapshotFrom,
  splitCollectionPath,
  splitDocPath,
  documentsBase,
  type DocSnapshot,
  type FirestoreQuery,
  type Json,
  type QueryDoc,
} from "./wire";

/** The Firestore of one app. */
export class Firestore {
  /** The endpoint root the pure tier talks to (the emulator by
   * default). The native tier ignores it. */
  readonly restBase: string;
  private authToken: string | null = null;

  constructor(readonly app: FirebaseApp, restBase: string = EMULATOR_BASE) {
    this.restBase = restBase;
  }

  /** The id token `Authorization: Bearer` carries on the REST calls (the
   * documented REST auth; the rules then see the user). None: the rules
   * decide. The auth area's refreshed tokens land here. The native tier
   * does its own auth. */
  setAuthToken(token: string | null): void {
    this.authToken = token;
  }

  /** Reads one document. A 404 is the REST read of an absent document,
   * not a failure: exists:false, data null. */
  async getDoc(path: string): Promise<DocSnapshot> {
    splitDocPath(path);
    if (this.native) {
      const reply = await this.call(GET_DOC, { path });
      const exists = reply?.exists;
      if (typeof exists !== "boolean") {
        throw invalidResponse("`firebase.firestore.getDoc` answered without `exists`");
      }
      if (!exists) return { exists: false, data: null };
      const data = reply.data;
      if (data === undefined || data === null || typeof data !== "object" || Array.isArray(data)) {
        throw invalidResponse("`firebase.firestore.getDoc` answered without `data`");
      }
      return { exists: true, data: data as Record<string, Json> };
    }
    const response = await this.send(this.documentsUrl(path), { method: "GET" });
    if (response.status === 404) return { exists: false, data: null };
    return snapshotFrom(parseBody(ensureOk(response).text));
  }

  /** Writes `data` (a JSON object of fields) at `path`, creating the
   * document if absent. With `merge` only the leaf field paths of `data`
   * are written (the REST patch's updateMask); without it the document
   * is replaced whole. */
  async setDoc(path: string, data: Record<string, Json>, merge?: boolean): Promise<void> {
    const fields = encodeFields(data);
    // The mask is computed before routing, so a merge the REST patch
    // cannot express is refused in both tiers.
    const mask = merge ? leafPaths(data) : null;
    if (this.native) {
      const args: Record<string, unknown> = { path, data };
      if (merge) args.merge = true;
      await this.call(SET_DOC, args);
      return;
    }
    let url = this.documentsUrl(path);
    if (mask) url += `?${mergeMaskQuery(mask)}`;
    ensureOk(
      await this.send(url, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ fields }),
      }),
    );
  }

  /** Creates a document in `collectionPath` with a server-assigned id
   * (the REST createDocument POST) and answers it. */
  async addDoc(collectionPath: string, data: Record<string, Json>): Promise<string> {
    splitCollectionPath(collectionPath);
    const fields = encodeFields(data);
    if (this.native) {
      const reply = await this.call(ADD_DOC, { collectionPath, data });
      const id = reply?.id;
      if (typeof id !== "string" || id === "") {
        throw invalidResponse("`firebase.firestore.addDoc` answered without an `id`");
      }
      return id;
    }
    const response = await this.send(collectionUrl(this.root(), collectionPath), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ fields }),
    });
    return queryDocFrom(parseBody(ensureOk(response).text)).id;
  }

  /** Deletes the document at `path`. A 404 (already absent) is done, as
   * a Firestore delete is idempotent. */
  async deleteDoc(path: string): Promise<void> {
    splitDocPath(path);
    if (this.native) {
      await this.call(DELETE_DOC, { path });
      return;
    }
    const response = await this.send(this.documentsUrl(path), { method: "DELETE" });
    if (response.status === 404) return;
    ensureOk(response);
  }

  /** Runs the structured query: the collection's documents that match
   * the where filter, sorted, at most `limit` of them. */
  async runQuery(query: FirestoreQuery): Promise<QueryDoc[]> {
    const plan = runQueryPlan(this.restBase, this.app.options.projectId, query);
    if (this.native) {
      const reply = await this.call(RUN_QUERY, plan.nativeArgs);
      const docs = reply?.docs;
      if (!Array.isArray(docs)) {
        throw invalidResponse("`firebase.firestore.runQuery` answered without `docs`");
      }
      return docs.map((doc) => {
        const entry = (typeof doc === "object" && doc !== null ? doc : {}) as Record<
          string,
          unknown
        >;
        const id = entry.id;
        if (typeof id !== "string" || id === "") {
          throw invalidResponse("a `docs` entry answered without an `id`");
        }
        return { id, data: (entry.data ?? null) as Record<string, Json> };
      });
    }
    const response = await this.send(plan.url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(plan.body),
    });
    return docsFromRunQuery(ensureOk(response).text);
  }

  /** Whether the host answers, so calls route through the native
   * handlers instead of fetch. */
  private get native(): boolean {
    return this.app.tier.kind === "native";
  }

  /** The documents root of the configured endpoint. */
  private root(): string {
    return documentsBase(this.restBase, this.app.options.projectId);
  }

  /** The URL of one document under the configured endpoint. */
  private documentsUrl(path: string): string {
    return documentUrl(this.root(), path);
  }

  /** One native handler call; the tier facade maps a refusal or a
   * handler failure to the stable codes already. */
  private async call(name: string, args: unknown): Promise<Record<string, unknown> | null> {
    const reply = await this.app.tier.call(name, args);
    return typeof reply === "object" && reply !== null ? (reply as Record<string, unknown>) : null;
  }

  /** Sends one REST call with the auth header of the moment. */
  private send(url: string, init: FirebaseRequestInit): Promise<FirebaseResponse> {
    const headers: Record<string, string> = { ...init.headers };
    const token = this.authToken;
    if (token) headers.Authorization = `Bearer ${token}`;
    return requestText(url, { ...init, headers });
  }
}

/** Settings `firestore` takes; every field optional. */
export interface FirestoreSettings {
  /** The REST endpoint root, without `/v1`: the emulator's
   * `http://127.0.0.1:18080` (the default), or production. The native
   * tier ignores it. */
  restBase?: string;
}

/** The Firestore of an app (React Native Firebase's `firestore()`). */
export function firestore(app: FirebaseApp, settings?: FirestoreSettings): Firestore {
  return new Firestore(app, settings?.restBase ?? EMULATOR_BASE);
}
