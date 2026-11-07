//! The pluggable fetch seam. The pure tier ships no transport of its
//! own — the REST API is authorized with an OAuth 2.0 service-account
//! token (scope `https://www.googleapis.com/auth/firebase.remoteconfig`),
//! not the web API key, so nothing here can honestly fetch. A holder of
//! such a token (a backend proxy, a future host handler) plugs in
//! through this trait; the native tier routes around it entirely.

use std::future::Future;
use std::pin::Pin;

use serde_json::Value as JsonValue;

use nsh_firebase_core::FirebaseError;

pub trait Transport: Send + Sync {
    /// Returns the template document (the REST `RemoteConfig` JSON
    /// shape) from wherever the implementor can authorize; the client
    /// parses and validates it. The future is deliberately not `Send`
    /// to match `nsh_firebase_core::send`'s engine-thread contract.
    fn fetch_template<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + 'a>>;
}
