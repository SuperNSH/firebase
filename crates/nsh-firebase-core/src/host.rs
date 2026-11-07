//! The native tier's facade: one trait the products call, an impl over
//! the channel the embedding host injects, and the pure tier's stub.
//! `packages/nsh-firebase/src/core/tier.ts` wraps the same channel from
//! TypeScript with `host.call` / `host.on`; this side cannot reach
//! `nsh-core`'s host bridge on 0.4.0 (its `host_call` is `pub(crate)`),
//! so the embedding app injects its own [`HostChannel`].

use std::{future::Future, pin::Pin, sync::Arc};

use serde_json::Value as JsonValue;

use crate::error::FirebaseError;

/// What a native handler call returns.
pub type HostCallFuture = Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + Send>>;

/// The tier facade the products call. Handler names follow the
/// `firebase.*` scheme the app declares under `capabilities.host`
/// (`firebase.auth.getUser`, `firebase.messaging.onToken`).
pub trait FirebaseHost: Send + Sync {
    /// Calls the native handler `name` with `args`.
    fn call(&self, name: &str, args: JsonValue) -> HostCallFuture;
    /// Fire-and-forget post of `args` to `name` (an analytics push). The
    /// injected channel is what refuses: a name `capabilities.host` does
    /// not grant comes back from [`HostChannel::post`] as the mapped
    /// `ERR_ACCESS_DENIED` error.
    fn post(&self, name: &str, args: JsonValue) -> Result<(), FirebaseError>;
}

/// The channel the embedding host (native/ios, native/android, a Rust
/// host) supplies. The failure keeps the platform's native code so
/// [`FirebaseError::from_host_code`] can map it.
pub trait HostChannel: Send + Sync {
    fn call(
        &self,
        name: String,
        args: JsonValue,
    ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + Send>>;
    fn post(&self, name: String, args: JsonValue) -> Result<(), FirebaseError>;
}

/// The native tier over an injected channel.
pub struct NativeHost {
    channel: Arc<dyn HostChannel>,
}

impl NativeHost {
    pub fn new(channel: Arc<dyn HostChannel>) -> Self {
        Self { channel }
    }
}

impl FirebaseHost for NativeHost {
    fn call(&self, name: &str, args: JsonValue) -> HostCallFuture {
        self.channel.call(name.to_owned(), args)
    }

    fn post(&self, name: &str, args: JsonValue) -> Result<(), FirebaseError> {
        self.channel.post(name.to_owned(), args)
    }
}

/// The tier a Rust app runs in when no host is granted: every native
/// handler call fails clearly instead of silently degrading.
pub struct PureHost;

impl FirebaseHost for PureHost {
    fn call(&self, name: &str, _args: JsonValue) -> HostCallFuture {
        let name = name.to_owned();
        Box::pin(async move {
            Err(FirebaseError::from_host_code(
                "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
                format!(
                    "no host answers `{name}` in the pure tier; use the product's fetch transport"
                ),
            ))
        })
    }

    fn post(&self, name: &str, _args: JsonValue) -> Result<(), FirebaseError> {
        Err(FirebaseError::from_host_code(
            "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
            format!("no host answers `{name}` in the pure tier; use the product's fetch transport"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingChannel;

    impl HostChannel for FailingChannel {
        fn call(
            &self,
            name: String,
            _args: JsonValue,
        ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + Send>> {
            Box::pin(async move {
                Err(FirebaseError::from_host_code(
                    "ERR_ACCESS_DENIED",
                    format!("`{name}` is not granted"),
                ))
            })
        }

        fn post(&self, name: String, _args: JsonValue) -> Result<(), FirebaseError> {
            Err(FirebaseError::from_host_code(
                "ERR_ACCESS_DENIED",
                format!("`{name}` is not granted"),
            ))
        }
    }

    async fn call_code(host: &dyn FirebaseHost, name: &str) -> FirebaseError {
        host.call(name, JsonValue::Null).await.err().unwrap()
    }

    #[tokio::test]
    async fn a_refused_channel_maps_to_access_denied() {
        let host = NativeHost::new(Arc::new(FailingChannel));
        let error = call_code(&host, "firebase.auth.getUser").await;
        assert_eq!(error.code, crate::error::ACCESS_DENIED);
        assert_eq!(error.native_code.as_deref(), Some("ERR_ACCESS_DENIED"));
    }

    #[tokio::test]
    async fn the_pure_tier_fails_every_native_call() {
        let error = call_code(&PureHost, "firebase.core.ping").await;
        assert_eq!(error.code, crate::error::UNAVAILABLE);
    }

    #[test]
    fn a_refused_post_maps_to_access_denied() {
        let host = NativeHost::new(Arc::new(FailingChannel));
        let error = host
            .post("firebase.analytics.logEvent", JsonValue::Null)
            .unwrap_err();
        assert_eq!(error.code, crate::error::ACCESS_DENIED);
        assert_eq!(error.native_code.as_deref(), Some("ERR_ACCESS_DENIED"));
    }

    #[test]
    fn the_pure_tier_fails_every_native_post() {
        let error = PureHost
            .post("firebase.analytics.logEvent", JsonValue::Null)
            .unwrap_err();
        assert_eq!(error.code, crate::error::UNAVAILABLE);
    }
}
