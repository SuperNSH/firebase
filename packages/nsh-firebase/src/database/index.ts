// Firebase Realtime Database over the documented REST surface with live
// listeners on Server-Sent Events. The Rust crate
// `nsh-firebase-database` mirrors this behavior name for name (`ref` is
// `r#ref`, `onValue` is `on_value`).
//
// On network loss a listener reports "reconnecting", waits
// reconnectDelayMs (one second after the first loss, doubling, capped at
// thirty) and opens a new stream, which the server answers with the
// ref's whole current value — a resync from the server's state, not a
// replay of what was missed. Writes do not queue offline. `cancel` or
// `auth_revoked` stops the listener. There is no server-side
// `onDisconnect`: that operation has no documented REST surface, and is
// not faked.

export { pushId } from "./push";
export {
  ORDER_KEY,
  ORDER_VALUE,
  compareValues,
  encodeComponent,
  encodeQuery,
  isEmptyQuery,
  orderParam,
  parseOrder,
  sortChildren,
  validateQuery,
} from "./query";
export type { Json, Order, QueryParams } from "./query";
export { FrameParser, applyEvent } from "./sse";
export type { SseEvent } from "./sse";
export { DEFAULT_POLICY, reconnectDelayMs, startListen } from "./listener";
export type {
  ListenHandle,
  ListenOptions,
  ListenStatus,
  Listener,
  ReconnectPolicy,
} from "./listener";
export { Database, DatabaseReference, getDatabase, ref } from "./database";
export type { TransactionResult } from "./database";
