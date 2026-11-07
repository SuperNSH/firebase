//! Firebase Cloud Firestore for Native Shell Rust apps.
//!
//! The first stretch product, in its pure tier: documents CRUD and
//! shallow structured queries over the v1 REST surface
//! (`{endpoint}/v1/projects/{project}/databases/(default)/documents`),
//! against the suite's Firestore emulator (127.0.0.1:18080) by default.
//! Values cross as the REST typed-value encoding
//! (`stringValue`/`integerValue`/`doubleValue`/`booleanValue`/
//! `nullValue`/`mapValue`/`arrayValue`) — the encoding the firebase-js-sdk's
//! JsonProtoSerializer writes, verified in its compiled source.
//!
//! ## Tiers
//!
//! One facade over the two tiers:
//!
//! - **Pure (every target)**: REST over the engine's `fetch` — `get_doc`
//!   (a 404 reads as `exists: false`), `set_doc` (a patch; with `merge`
//!   the leaf field paths go over as `updateMask.fieldPaths`),
//!   `add_doc` (a createDocument POST; the id comes from the reply's
//!   resource name), `delete_doc` (a 404 is done: deletes are
//!   idempotent), `run_query` (`structuredQuery` with one where filter,
//!   sorts and a limit). No realtime listeners: the REST surface has
//!   none to speak of, and none is faked.
//! - **Native (iOS/Android hosts)**: the same five calls through the
//!   `firebase.firestore.*` handlers the embedding host registers, per
//!   the contract below. The hosts answer JSON maps, so replies pass
//!   through without the typed-value decode. A realtime-snapshot row
//!   could join this tier later; it is not in this contract, and no
//!   watch handler is invented here.
//!
//! Native handler contract (coordinator-fixed; the native-stack agent
//! implements exactly these on both hosts):
//! - firebase.firestore.getDoc({path}) -> {exists: bool, data: object|null}
//! - firebase.firestore.setDoc({path, data, merge?}) -> {}
//! - firebase.firestore.addDoc({collectionPath, data}) -> {id}
//! - firebase.firestore.deleteDoc({path}) -> {}
//! - firebase.firestore.runQuery({collectionPath, where?, orderBy?, limit?})
//!   -> {docs: [{id, data}]}
//!
//! Server replies map through the REST error body's canonical `status`
//! to the `firestore/*` codes; transport failures keep the core's stable
//! codes (`core/network`, `core/access-denied`). Value types the JSON
//! contract cannot carry (a timestamp written by another client, a
//! non-finite double) are `firestore/invalid-data`, never a silent
//! loss. The TypeScript package `packages/nsh-firebase/src/firestore`
//! mirrors this behavior name for name.

pub mod firestore;
pub mod wire;

pub use crate::firestore::Firestore;
pub use crate::wire::{
    Direction, DocSnapshot, EMULATOR_PORT, FirestoreQuery, OrderBy, PRODUCTION_BASE, QueryDoc,
    QueryPlan, WhereFilter, WhereOp,
};
