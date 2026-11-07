//! Firebase Cloud Functions (https.callable) for Native Shell Rust apps.
//!
//! The pure tier speaks the callable wire protocol over the engine's
//! `fetch`: a POST of `{"data": ...}` to
//! `{functionsEmulator}/{projectId}/{region}/{name}` (emulator,
//! `http://127.0.0.1:15001` by default; the sandbox's suite serves the v1
//! callables `ping` and `add`) or
//! `https://{region}-{projectId}.cloudfunctions.net/{name}` (production),
//! with `Content-Type: application/json`. A 2xx answer is read as
//! `data`, then the legacy `result` key; anything else maps through the
//! HTTP status and the body's `{error: {status, message, details}}` to
//! the SDK's `functions/*` codes (a missing function is
//! `functions/not-found`). [`Functions`] routes through the native
//! handler instead when a host is wired.
//!
//! Native handler contract (coordinator-fixed):
//! - firebase.functions.call({name, data?}) -> {result} (region/host from
//!   config; the emulator serves 127.0.0.1:15001)
//!
//! `packages/nsh-firebase/src/functions` mirrors this crate name for
//! name; the wire format's sources are recorded in
//! `docs/evidence/functions/wire-protocol.md`.

pub mod functions;
pub mod wire;

pub use crate::{
    functions::{Functions, HttpsCallable, TokenProvider},
    wire::{
        ABORTED, ALREADY_EXISTS, CANCELLED, DATA_LOSS, DEADLINE_EXCEEDED, DEFAULT_REGION,
        EMULATOR_PORT, FAILED_PRECONDITION, FunctionsConfig, FunctionsError, INTERNAL,
        INVALID_ARGUMENT, NATIVE_CALL, NOT_FOUND, OK, OUT_OF_RANGE, PERMISSION_DENIED,
        RESOURCE_EXHAUSTED, UNAUTHENTICATED, UNAVAILABLE, UNIMPLEMENTED, UNKNOWN, callable_url,
        error_for_response, request_body, result_from_response,
    },
};
