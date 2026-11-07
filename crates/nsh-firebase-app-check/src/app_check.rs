//! The App Check clients: the pure tier's debug exchange with ttl
//! caching through the storage seam, and the native tier's
//! `firebase.appCheck.getToken` router. `get_token` is an async fn the
//! app drives on the engine thread (`spawn` in a component's handler):
//! that is where the engine's `fetch` lives, which
//! [`crate::transport::FetchExchange`] must be awaited on.

use std::sync::{Arc, Mutex};

use nsh_firebase_core::{FirebaseApp, FirebaseError, FirebaseHost};
use serde_json::{Value as JsonValue, json};

use crate::clock::{Clock, SystemClock};
use crate::error::{INVALID_ARGUMENT, invalid_response, needs_attestation, reply_error};
use crate::store::{CachedToken, KeyValueStore, store_key};
use crate::transport::{Exchange, FetchExchange};
use crate::wire::{exchange_debug_token_request, parse_token_reply};

/// The `firebase.appCheck.*` handler name the native areas register;
/// [`NativeAppCheck`] calls exactly this.
pub const NATIVE_GET_TOKEN: &str = "firebase.appCheck.getToken";

/// One token. `expires_at_ms` is `None` on the native tier, whose reply
/// carries no ttl and whose SDK owns the refresh schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCheckToken {
    pub token: String,
    pub expires_at_ms: Option<u64>,
}

/// The client's settings: the debug token registered in the project's
/// Firebase console (App Check settings), and the exchange base URL (the
/// sandbox points it at a local stand-in; the default is the REST
/// reference's endpoint).
#[derive(Debug, Clone)]
pub struct AppCheckOptions {
    pub debug_token: String,
    pub base_url: String,
}

impl AppCheckOptions {
    /// The options for the debug provider over the real endpoint.
    pub fn debug(debug_token: impl Into<String>) -> Self {
        Self {
            debug_token: debug_token.into(),
            base_url: crate::wire::APP_CHECK_BASE.to_owned(),
        }
    }
}

/// The pure tier's App Check: the debug exchange with a ttl cache that
/// restores on init and refreshes once the clock passes its expiry. It
/// never fakes a token: a failed exchange is a clear error, and
/// `get_token(false)` refuses, naming the device attestors it lacks.
pub struct AppCheck {
    app: Arc<FirebaseApp>,
    exchange: Arc<dyn Exchange>,
    clock: Arc<dyn Clock>,
    store: Arc<dyn KeyValueStore>,
    options: AppCheckOptions,
    cache: Mutex<Option<CachedToken>>,
}

impl std::fmt::Debug for AppCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppCheck")
            .field("app", &self.app.name)
            .finish_non_exhaustive()
    }
}

impl AppCheck {
    /// A client over the engine's `fetch` and the system clock.
    pub fn new(
        app: &Arc<FirebaseApp>,
        store: Arc<dyn KeyValueStore>,
        options: AppCheckOptions,
    ) -> Self {
        Self::with_parts(
            app,
            Arc::new(FetchExchange),
            Arc::new(SystemClock),
            store,
            options,
        )
    }

    /// The client tests drive: any exchange, clock and store.
    pub fn with_parts(
        app: &Arc<FirebaseApp>,
        exchange: Arc<dyn Exchange>,
        clock: Arc<dyn Clock>,
        store: Arc<dyn KeyValueStore>,
        options: AppCheckOptions,
    ) -> Self {
        let client = Self {
            app: Arc::clone(app),
            exchange,
            clock,
            store,
            options,
            cache: Mutex::new(None),
        };
        client.restore();
        client
    }

    /// Re-reads the store into the cache. A record the other tier (or an
    /// older launch) wrote restores too; a corrupt one is no cache.
    pub fn restore(&self) {
        let cached = self
            .store
            .get(&store_key(&self.app.name))
            .as_deref()
            .and_then(CachedToken::decode);
        *self.lock() = cached;
    }

    /// The token, from the cache while it lives and from the debug
    /// exchange once past its expiry on this client's clock. `debug`
    /// must be true: the pure tier has no device attestor.
    pub async fn get_token(&self, debug: bool) -> Result<AppCheckToken, FirebaseError> {
        if !debug {
            return Err(needs_attestation());
        }
        if self.options.debug_token.trim().is_empty() {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "the debug token is empty: register one in the Firebase console and pass it to \
                 the App Check options",
            ));
        }
        let now = self.clock.now_ms();
        if let Some(cached) = &*self.lock()
            && now < cached.expires_at_ms
        {
            return Ok(AppCheckToken {
                token: cached.token.clone(),
                expires_at_ms: Some(cached.expires_at_ms),
            });
        }
        let request = exchange_debug_token_request(
            &self.options.base_url,
            &self.app.options.api_key,
            &self.app.options.project_id,
            &self.app.options.app_id,
            &self.options.debug_token,
        );
        // A transport failure keeps the core's stable codes; only a
        // server reply maps to an app-check code.
        let response = self.exchange.exchange(request).await?;
        if !response.ok() {
            return Err(reply_error(response.status(), &response.text()));
        }
        let reply = parse_token_reply(&response.text())?;
        let expires_at_ms = now.saturating_add(reply.ttl_ms);
        let cached = CachedToken {
            token: reply.token,
            expires_at_ms,
        };
        self.store.set(&store_key(&self.app.name), cached.encode());
        let token = AppCheckToken {
            token: cached.token.clone(),
            expires_at_ms: Some(expires_at_ms),
        };
        *self.lock() = Some(cached);
        Ok(token)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<CachedToken>> {
        self.cache.lock().expect("app-check cache lock")
    }
}

/// The native tier's App Check: the contract call to the
/// `firebase.appCheck.getToken` handler an embedding host registers. The
/// host's SDK picks the provider (DeviceCheck/App Attest, Play
/// Integrity, or its own debug provider) and caches what it issues.
pub struct NativeAppCheck {
    host: Arc<dyn FirebaseHost>,
}

impl NativeAppCheck {
    pub fn new(host: Arc<dyn FirebaseHost>) -> Self {
        Self { host }
    }

    pub async fn get_token(&self, debug: bool) -> Result<AppCheckToken, FirebaseError> {
        if !self.host.available() {
            return Err(FirebaseError::from_host_code(
                "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
                "Firebase App Check needs the native tier: an iOS/Android host with \
                 firebase.appCheck.* handlers",
            ));
        }
        let reply = self
            .host
            .call(NATIVE_GET_TOKEN, json!({ "debug": debug }))
            .await?;
        let token = reply
            .get("token")
            .and_then(JsonValue::as_str)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                invalid_response(format!("`{NATIVE_GET_TOKEN}` answered without a token"))
            })?
            .to_owned();
        Ok(AppCheckToken {
            token,
            expires_at_ms: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use crate::store::MemoryStore;
    use crate::transport::RecordingExchange;
    use nsh_firebase_core::initialize_app;

    fn app(name: &str) -> Arc<FirebaseApp> {
        initialize_app(nsh_firebase_core::FirebaseOptions::demo(), name).unwrap()
    }

    fn recording(name: &str) -> (AppCheck, Arc<RecordingExchange>, Arc<ManualClock>) {
        let exchange = Arc::new(RecordingExchange::new());
        let clock = Arc::new(ManualClock::new(1_000));
        let client = AppCheck::with_parts(
            &app(name),
            Arc::clone(&exchange) as Arc<dyn Exchange>,
            Arc::clone(&clock) as Arc<dyn Clock>,
            Arc::new(MemoryStore::new()),
            AppCheckOptions::debug("secret-1"),
        );
        (client, exchange, clock)
    }

    /// Records what a native tier would hand the embedding host, and
    /// answers with one scripted reply.
    struct RecordingHost {
        calls: Mutex<Vec<(String, JsonValue)>>,
        reply: Mutex<Result<JsonValue, FirebaseError>>,
    }

    impl RecordingHost {
        fn answers_with(reply: Result<JsonValue, FirebaseError>) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                reply: Mutex::new(reply),
            })
        }

        fn calls(&self) -> Vec<(String, JsonValue)> {
            self.calls.lock().expect("recording host lock").clone()
        }
    }

    impl FirebaseHost for RecordingHost {
        fn call(&self, name: &str, args: JsonValue) -> nsh_firebase_core::host::HostCallFuture {
            self.calls
                .lock()
                .expect("recording host lock")
                .push((name.to_owned(), args));
            let reply = self.reply.lock().expect("recording host lock").clone();
            Box::pin(async move { reply })
        }

        fn post(&self, _name: &str, _args: JsonValue) -> Result<(), FirebaseError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_token_is_issued_cached_and_reissued_after_expiry() {
        let (client, exchange, clock) = recording("app-check-issue-test");
        exchange.answer(200, r#"{"token":"tok-1","ttl":"1800s"}"#);
        let first = client.get_token(true).await.unwrap();
        assert_eq!(
            first,
            AppCheckToken {
                token: "tok-1".to_owned(),
                expires_at_ms: Some(1_801_000),
            }
        );
        // Inside the ttl the cache answers and no second request goes.
        let second = client.get_token(true).await.unwrap();
        assert_eq!(second, first);
        assert_eq!(exchange.requests().len(), 1);
        // Past the expiry (equality counts as past: the cache serves only
        // while now < expiry) the exchange runs again.
        clock.advance(1_800_000);
        exchange.answer(200, r#"{"token":"tok-2","ttl":"1800s"}"#);
        let third = client.get_token(true).await.unwrap();
        assert_eq!(third.token, "tok-2");
        assert_eq!(third.expires_at_ms, Some(3_601_000));
        assert_eq!(exchange.requests().len(), 2);
    }

    #[tokio::test]
    async fn the_exchange_request_matches_the_wire() {
        let (client, exchange, _) = recording("app-check-wire-test");
        exchange.answer(200, r#"{"token":"tok","ttl":"1800s"}"#);
        client.get_token(true).await.unwrap();
        let requests = exchange.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, "POST");
        assert_eq!(
            request.url,
            "https://firebaseappcheck.googleapis.com/v1/projects/demo-nsh/apps/1:000000000000:web:demo-nsh:exchangeDebugToken?key=demo-nsh-fake-api-key"
        );
        assert_eq!(
            request.headers,
            vec![("Content-Type".to_owned(), "application/json".to_owned())]
        );
        assert_eq!(
            String::from_utf8(match &request.body {
                nsh_core::platform::HttpBody::Bytes(bytes) => bytes.clone(),
                _ => panic!("the exchange carries bytes"),
            })
            .unwrap(),
            r#"{"debug_token":"secret-1"}"#
        );
    }

    #[tokio::test]
    async fn a_restored_record_answers_without_a_request() {
        // The record a previous launch (or the other tier) wrote, seeded
        // into the store before the client exists.
        let store = Arc::new(MemoryStore::seeded([(
            store_key("app-check-restore-test"),
            CachedToken {
                token: "stored-tok".to_owned(),
                expires_at_ms: 2_000,
            }
            .encode(),
        )]));
        let exchange = Arc::new(RecordingExchange::new());
        let clock = Arc::new(ManualClock::new(1_000));
        let client = AppCheck::with_parts(
            &app("app-check-restore-test"),
            Arc::clone(&exchange) as Arc<dyn Exchange>,
            Arc::clone(&clock) as Arc<dyn Clock>,
            Arc::clone(&store) as Arc<dyn KeyValueStore>,
            AppCheckOptions::debug("secret-1"),
        );
        let token = client.get_token(true).await.unwrap();
        assert_eq!(
            token,
            AppCheckToken {
                token: "stored-tok".to_owned(),
                expires_at_ms: Some(2_000),
            }
        );
        assert!(exchange.requests().is_empty());
    }

    #[tokio::test]
    async fn a_corrupt_or_expired_record_exchanges_instead() {
        // A corrupt record is no cache: the exchange answers instead.
        let (client, exchange, _clock) = recording("app-check-corrupt-test");
        client
            .store
            .set(&store_key("app-check-corrupt-test"), "not json".to_owned());
        client.restore();
        exchange.answer(200, r#"{"token":"fresh","ttl":"60s"}"#);
        let token = client.get_token(true).await.unwrap();
        assert_eq!(token.token, "fresh");
        assert_eq!(exchange.requests().len(), 1);

        // An expired record is refreshed, not served.
        let (client, exchange, _clock) = recording("app-check-expired-test");
        client.store.set(
            &store_key("app-check-expired-test"),
            CachedToken {
                token: "stale".to_owned(),
                expires_at_ms: 999,
            }
            .encode(),
        );
        client.restore();
        exchange.answer(200, r#"{"token":"renewed","ttl":"60s"}"#);
        let token = client.get_token(true).await.unwrap();
        assert_eq!(token.token, "renewed");
        assert_eq!(exchange.requests().len(), 1);
    }

    #[tokio::test]
    async fn failures_map_to_their_stable_codes() {
        // A refusal of the debug token carries the server's text.
        let (client, exchange, _) = recording("app-check-fail-test");
        exchange.answer(
            403,
            r#"{"error":{"code":403,"message":"App attestation failed.","status":"PERMISSION_DENIED"}}"#,
        );
        let error = client.get_token(true).await.unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_DEBUG_TOKEN);
        assert!(error.message.contains("App attestation failed."));

        // A server failure is its own code.
        exchange.answer(500, "{}");
        let error = client.get_token(true).await.unwrap_err();
        assert_eq!(error.code, crate::error::EXCHANGE_FAILED);

        // A 200 without a usable ttl is an invalid response, and nothing
        // is cached from it.
        exchange.answer(200, r#"{"token":"tok","ttl":"soon"}"#);
        let error = client.get_token(true).await.unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_RESPONSE);
        assert!(
            client
                .store
                .get(&store_key("app-check-fail-test"))
                .is_none()
        );

        // A transport refusal keeps the core's stable code.
        exchange.fail(FirebaseError::from_transport_message(
            "fetch POST https://firebaseappcheck.googleapis.com/ is not granted: add \
             firebaseappcheck.googleapis.com to capabilities.network",
        ));
        let error = client.get_token(true).await.unwrap_err();
        assert_eq!(error.code, nsh_firebase_core::error::ACCESS_DENIED);
    }

    #[tokio::test]
    async fn the_pure_tier_refuses_attestation_and_an_empty_debug_token() {
        let (client, exchange, _) = recording("app-check-refuse-test");
        let error = client.get_token(false).await.unwrap_err();
        assert_eq!(error.code, crate::error::NEEDS_ATTESTATION);
        assert!(error.message.contains("DeviceCheck"));
        assert!(exchange.requests().is_empty());

        let exchange = Arc::new(RecordingExchange::new());
        let client = AppCheck::with_parts(
            &app("app-check-empty-token-test"),
            Arc::clone(&exchange) as Arc<dyn Exchange>,
            Arc::new(ManualClock::new(0)),
            Arc::new(MemoryStore::new()),
            AppCheckOptions::debug("  "),
        );
        let error = client.get_token(true).await.unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
        assert!(exchange.requests().is_empty());
    }

    #[tokio::test]
    async fn the_native_tier_routes_the_contract_call() {
        let host = RecordingHost::answers_with(Ok(json!({ "token": "native-tok" })));
        let native = NativeAppCheck::new(Arc::clone(&host) as Arc<dyn FirebaseHost>);
        let token = native.get_token(true).await.unwrap();
        assert_eq!(token.token, "native-tok");
        // The host reply names no ttl: the host's SDK owns the schedule.
        assert_eq!(token.expires_at_ms, None);
        assert_eq!(
            host.calls(),
            vec![(
                "firebase.appCheck.getToken".to_owned(),
                json!({"debug": true})
            )]
        );
        // debug:false rides the same way: the host picks the provider.
        native.get_token(false).await.unwrap();
        assert_eq!(
            host.calls()[1],
            (
                "firebase.appCheck.getToken".to_owned(),
                json!({"debug": false})
            )
        );
    }

    #[tokio::test]
    async fn a_native_reply_without_a_token_is_a_clear_error() {
        let host = RecordingHost::answers_with(Ok(json!({})));
        let native = NativeAppCheck::new(Arc::clone(&host) as Arc<dyn FirebaseHost>);
        let error = native.get_token(true).await.unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_RESPONSE);
        assert!(error.message.contains("answered without a token"));
    }

    #[tokio::test]
    async fn native_failures_map_their_host_codes() {
        let host = RecordingHost::answers_with(Err(FirebaseError::from_host_code(
            "ERR_NSH_HOST_ERROR",
            "`firebase.appCheck.getToken` unhandled",
        )));
        let native = NativeAppCheck::new(Arc::clone(&host) as Arc<dyn FirebaseHost>);
        let error = native.get_token(true).await.unwrap_err();
        assert_eq!(error.code, nsh_firebase_core::error::HOST_ERROR);
        assert!(error.message.contains("unhandled"));

        // No host at all names what is missing instead of staying silent.
        let native = NativeAppCheck::new(Arc::new(nsh_firebase_core::PureHost));
        let error = native.get_token(true).await.unwrap_err();
        assert_eq!(error.code, nsh_firebase_core::error::UNAVAILABLE);
        assert_eq!(
            error.native_code.as_deref(),
            Some("ERR_FEATURE_UNAVAILABLE_ON_PLATFORM")
        );
        assert!(error.message.contains("firebase.appCheck.*"));
    }
}
