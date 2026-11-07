//! Firebase Cloud Messaging for Native Shell Rust apps.
//!
//! The messaging facade over the tier facade of `nsh-firebase-core`:
//! calls ride a [`nsh_firebase_core::host::FirebaseHost`], and the pure
//! tier fails every call naming what is missing. The TypeScript package
//! `packages/nsh-firebase/src/messaging` mirrors this behavior name for
//! name.
//!
//! # The contract
//!
//! The native areas (native/ios/NshFirebase, native/android) implement
//! this table. Both facades and both hosts are held to it verbatim:
//!
//! - facade API `messaging.permission()` / `messaging.requestPermission()`
//!   → `"default"|"granted"|"denied"|"unavailable"` (same strings as nsh
//!   Notifications); `messaging.show({id,title,body,silent?})` →
//!   `Promise<boolean>`; `messaging.close(id)`; `messaging.setBadge(n)`;
//!   `messaging.on("press", cb)` with `{id}`; `messaging.on("message", cb)`
//!   with the FCM message payload; `messaging.on("token", cb)` with
//!   `{token}`; `messaging.token()` → `Promise<string>` (rejects clearly
//!   when unavailable or the token request fails);
//!   `messaging.subscribe(topic)` / `messaging.unsubscribe(topic)`.
//! - host handlers the native tier registers (declare
//!   `"firebase.messaging.*"` in `capabilities.host`):
//!   `firebase.messaging.token`, `.permission`, `.requestPermission`,
//!   `.show`, `.close`, `.setBadge`, `.subscribe`, `.unsubscribe` (calls);
//!   host→app events: `firebase.messaging.message`,
//!   `firebase.messaging.tokenRefreshed`, `firebase.messaging.press`.
//!
//! # Wire shapes
//!
//! Requests: `token`, `permission`, `requestPermission` carry no args;
//! `show` carries `{id, title, body, silent}`; `close` carries `{id}`;
//! `setBadge` carries `{count}`; the topic calls carry `{topic}`. Replies
//! the facade reads: `{permission}` for the permissions, `{token}` for the
//! token, `{shown}` for `show` (the Android host's bare boolean is also
//! accepted); the calls that only act (`close`, `setBadge`, the topic
//! calls) read no reply value, so either host's `null`/`true` resolves.
//! Event payloads pass through untouched: `{id}` on `press`, the
//! serializer's message shape (`messageId`, `data`, `notification`,
//! `sentTime`, ...) on `message`, `{token}` on `tokenRefreshed`.

pub mod messaging;
pub mod types;

pub use crate::{
    messaging::Messaging,
    types::{Event, MessageNotification, NEEDS_NATIVE_TIER, Permission, wire},
};
