//! Firebase App Check for Native Shell Rust apps.
//!
//! The honest tier table (docs/research.md §4): DeviceCheck/App Attest
//! (iOS), Play Integrity (Android) and even the debug provider's exchange
//! against firebaseappcheck.googleapis.com all need the real backend —
//! the emulator suite has no App Check service and `demo-` projects
//! refuse. The pure tier implements the debug-token exchange REQUEST
//! faithfully (the one provider that needs no device attestor, only a
//! token registered in a real Firebase console) and maps its failure to a
//! clear `FirebaseError`; it never fakes a token. The native tier
//! routes through the host handler instead, whose reply carries no ttl
//! and whose SDK owns attestation and its own cache.
//!
//! The native handler contract, verbatim:
//! - `firebase.appCheck.getToken({debug?}) -> {token}` (debug provider
//!   when debug is true; rejects clearly when the exchange fails — which
//!   it does under the fake project)
//!
//! The TypeScript package `packages/nsh-firebase/src/app-check` mirrors
//! this behavior name for name, down to the persisted cache record.
//!
//! The flows run on the engine thread (call them in `spawn` from a
//! component's handler), where the engine's `fetch` and clock live.
//! Transport failures surface with the core's stable codes
//! (`core/network`, `core/access-denied`); a server reply maps to an
//! `app-check/*` code ([`error`]).

pub mod app_check;
pub mod clock;
pub mod error;
pub mod store;
pub mod transport;
pub mod wire;

pub use crate::{
    app_check::{AppCheck, AppCheckOptions, AppCheckToken, NativeAppCheck, NATIVE_GET_TOKEN},
    clock::{Clock, ManualClock, SystemClock},
    error::{
        EXCHANGE_FAILED, INVALID_ARGUMENT, INVALID_DEBUG_TOKEN, INVALID_RESPONSE,
        NEEDS_ATTESTATION,
    },
    store::{EngineStore, KeyValueStore, MemoryStore},
    transport::{Exchange, FetchExchange, RecordingExchange},
    wire::APP_CHECK_BASE,
};
