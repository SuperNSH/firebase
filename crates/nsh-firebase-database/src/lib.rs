//! Firebase Realtime Database for Native Shell Rust apps, over the
//! documented REST surface with live listeners on Server-Sent Events.
//! The TypeScript package `packages/nsh-firebase/src/database` mirrors
//! this behavior name for name (`r#ref` is `ref`, `on_value` is
//! `onValue`).
//!
//! Reads and writes go over `<databaseUrl>/<path>.json` (`?auth=` for
//! the id token, the documented query-parameter form). Push ids are
//! generated on the client with the ordering properties the official
//! client's algorithm gives ([`push_id`]). A transaction is a
//! conditional write: GET with `X-Firebase-ETag: true`, PUT with
//! `if-match`, rebuild on `412`. Queries are the REST parameters
//! (`orderBy`, `startAt`, `endAt`, `equalTo`, `limitToFirst`,
//! `limitToLast`), and a queried read sorts its children client-side,
//! where the REST answers unsorted.
//!
//! ## Live listeners and network loss
//!
//! `DatabaseReference::listen` and `on_value` hold one SSE stream each
//! (`Accept: text/event-stream`), applying the server's `put` and
//! `patch` events to a per-listener cache. On network loss — the stream
//! ends or fails — the listener reports
//! [`listener::ListenStatus::Reconnecting`], waits
//! [`listener::ReconnectPolicy::delay_ms`] (one second after the first
//! loss, doubling, capped at thirty; a stream that opens resets the
//! count) and opens a new stream. The server answers a fresh stream
//! with a `put` at `/` carrying the ref's whole current value, so the
//! listener resyncs from the server's state rather than accumulating
//! events it missed; locally issued writes are not queued while
//! offline — a write that cannot reach the server rejects. `cancel` or
//! `auth_revoked`, or a stream answered with a failure status, stops
//! the listener: the caller re-authenticates and listens again. There
//! is no server-side `onDisconnect`: that operation has no documented
//! REST surface, and is not faked.
//!
//! ## The stream transport
//!
//! nsh 0.4.0 gives a Rust app no incremental fetch — the engine's
//! `fetch` resolves with the whole body, and a live SSE stream never
//! completes — so the app supplies the stream:
//! [`listener::StreamTransport`], injected at
//! [`Database::from_app`] (the TypeScript side reads
//! `response.body.getReader()` and needs none). Everything else —
//! parsing, cache application, backoff, resync — lives in this crate
//! and runs against the transport's contract.

pub mod database;
pub mod listener;
pub mod push;
pub mod query;
pub mod sse;

pub use crate::database::{Database, DatabaseReference, TransactionResult};
pub use crate::listener::{
    EngineReconnect, ListenHandle, ListenSignals, ListenStatus, Listener, ReconnectClock,
    ReconnectPolicy, StreamSink, StreamTask, StreamTransport, ThreadReconnect, start_listen,
};
pub use crate::push::push_id;
pub use crate::query::{
    ORDER_KEY, ORDER_VALUE, Order, QueryParams, encode_component, sort_children,
};
pub use crate::sse::{FrameParser, SseEvent, apply_event};
