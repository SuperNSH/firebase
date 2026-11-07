//! Headless tests of the App Check facade as a consumer sees it: the
//! native tier played through core's `NativeHost` with a scripted
//! channel (as a real embedding app would), and the pure tier's debug
//! exchange over the recording transport — no real network, no real
//! time.

use std::sync::{Arc, Mutex};
use std::{future::Future, pin::Pin};

use nsh_firebase_app_check::{
    AppCheck, AppCheckOptions, AppCheckToken, Clock, Exchange, ManualClock, MemoryStore,
    NATIVE_GET_TOKEN, NativeAppCheck, RecordingExchange,
};
use nsh_firebase_core::{
    FirebaseApp, FirebaseError, FirebaseHost, HostChannel, NativeHost, initialize_app,
};
use serde_json::{Value as JsonValue, json};

/// The channel an embedding app would inject: records the handler calls,
/// answers with one scripted reply.
struct ScriptedChannel {
    calls: Mutex<Vec<(String, JsonValue)>>,
    reply: Mutex<Result<JsonValue, FirebaseError>>,
}

impl ScriptedChannel {
    fn answers_with(reply: Result<JsonValue, FirebaseError>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            reply: Mutex::new(reply),
        }
    }

    fn calls(&self) -> Vec<(String, JsonValue)> {
        self.calls.lock().expect("scripted channel lock").clone()
    }
}

impl HostChannel for ScriptedChannel {
    fn call(
        &self,
        name: String,
        args: JsonValue,
    ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + Send>> {
        self.calls
            .lock()
            .expect("scripted channel lock")
            .push((name, args));
        let reply = self.reply.lock().expect("scripted channel lock").clone();
        Box::pin(async move { reply })
    }

    fn post(&self, _name: String, _args: JsonValue) -> Result<(), FirebaseError> {
        Ok(())
    }
}

fn app(name: &str) -> Arc<FirebaseApp> {
    initialize_app(nsh_firebase_core::FirebaseOptions::demo(), name).unwrap()
}

#[tokio::test]
async fn the_native_tier_routes_through_the_host_channel() {
    let channel = Arc::new(ScriptedChannel::answers_with(Ok(json!({
        "token": "host-tok-1",
    }))));
    let host: Arc<dyn FirebaseHost> = Arc::new(NativeHost::new(Arc::clone(&channel)));
    let native = NativeAppCheck::new(host);
    let token = native.get_token(true).await.unwrap();
    assert_eq!(
        token,
        AppCheckToken {
            token: "host-tok-1".to_owned(),
            expires_at_ms: None,
        }
    );
    assert_eq!(
        channel.calls(),
        vec![(NATIVE_GET_TOKEN.to_owned(), json!({ "debug": true }))]
    );
}

#[tokio::test]
async fn a_host_failure_surfaces_with_its_native_code() {
    let channel = Arc::new(ScriptedChannel::answers_with(Err(
        FirebaseError::from_host_code(
            "ERR_NSH_HOST_ERROR",
            "App Check debug exchange failed: the fake project refuses",
        ),
    )));
    let host: Arc<dyn FirebaseHost> = Arc::new(NativeHost::new(Arc::clone(&channel)));
    let error = NativeAppCheck::new(host).get_token(true).await.unwrap_err();
    assert_eq!(error.code, nsh_firebase_core::error::HOST_ERROR);
    assert_eq!(error.native_code.as_deref(), Some("ERR_NSH_HOST_ERROR"));
    assert!(error.message.contains("the fake project refuses"));
}

#[tokio::test]
async fn a_host_reply_without_a_token_is_a_clear_error() {
    let channel = Arc::new(ScriptedChannel::answers_with(Ok(json!({ "token": "" }))));
    let host: Arc<dyn FirebaseHost> = Arc::new(NativeHost::new(Arc::clone(&channel)));
    let error = NativeAppCheck::new(host).get_token(true).await.unwrap_err();
    assert_eq!(error.code, nsh_firebase_app_check::INVALID_RESPONSE);
    assert!(error.message.contains("answered without a token"));
}

#[tokio::test]
async fn the_pure_exchange_runs_caches_and_refreshes_on_the_manual_clock() {
    let exchange = Arc::new(RecordingExchange::new());
    let clock = Arc::new(ManualClock::new(50_000));
    let store = Arc::new(MemoryStore::new());
    let client = AppCheck::with_parts(
        &app("app-check-it-pure"),
        Arc::clone(&exchange) as Arc<dyn Exchange>,
        Arc::clone(&clock) as Arc<dyn Clock>,
        Arc::clone(&store) as Arc<dyn nsh_firebase_app_check::KeyValueStore>,
        AppCheckOptions::debug("it-secret-1"),
    );
    exchange.answer(200, r#"{"token":"it-tok-1","ttl":"900s"}"#);
    let first = client.get_token(true).await.unwrap();
    assert_eq!(
        first,
        AppCheckToken {
            token: "it-tok-1".to_owned(),
            expires_at_ms: Some(950_000),
        }
    );
    // Still inside the ttl: no second request.
    client.get_token(true).await.unwrap();
    assert_eq!(exchange.requests().len(), 1);
    // Past it: one more exchange, and the store holds the latest record.
    clock.advance(900_000);
    exchange.answer(200, r#"{"token":"it-tok-2","ttl":"900s"}"#);
    let second = client.get_token(true).await.unwrap();
    assert_eq!(second.token, "it-tok-2");
    assert_eq!(exchange.requests().len(), 2);
    let stored = store
        .get("nsh.firebase.app-check.app-check-it-pure")
        .expect("the refresh must persist the new record");
    assert!(stored.contains("\"token\":\"it-tok-2\""));
    assert!(stored.contains("\"expiresAtMs\":1850000"));
}

#[tokio::test]
async fn the_pure_tier_refuses_without_debug_and_without_a_token() {
    let exchange = Arc::new(RecordingExchange::new());
    let client = AppCheck::with_parts(
        &app("app-check-it-refuse"),
        Arc::clone(&exchange) as Arc<dyn Exchange>,
        Arc::new(ManualClock::new(0)),
        Arc::new(MemoryStore::new()),
        AppCheckOptions::debug("it-secret-1"),
    );
    let error = client.get_token(false).await.unwrap_err();
    assert_eq!(error.code, nsh_firebase_app_check::NEEDS_ATTESTATION);
    assert!(exchange.requests().is_empty());

    let client = AppCheck::with_parts(
        &app("app-check-it-empty"),
        Arc::clone(&exchange) as Arc<dyn Exchange>,
        Arc::new(ManualClock::new(0)),
        Arc::new(MemoryStore::new()),
        AppCheckOptions::debug(""),
    );
    let error = client.get_token(true).await.unwrap_err();
    assert_eq!(error.code, nsh_firebase_app_check::INVALID_ARGUMENT);
    assert!(exchange.requests().is_empty());
}
