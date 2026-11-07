//! Headless tests of the native-tier routing: the stub channel plays the
//! native host with the same handler names and reply shapes the contract
//! fixes, and the REST flows are never reached (those live in
//! installations_ui.rs over a stubbed engine network). No real time: the
//! stub's futures complete at once.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use nsh_firebase_core::error::{ACCESS_DENIED, FirebaseError, HOST_ERROR};
use nsh_firebase_core::host::{FirebaseHost, HostChannel, NativeHost};
use nsh_firebase_installations::{Installations, KeyValueStore, MemoryStore};
use serde_json::{Value as JsonValue, json};

/// The test's host: it records every call and answers from a fixed table,
/// the way the native areas' handlers do. A name with no answer answers
/// `null`.
struct StubChannel {
    calls: Mutex<Vec<(String, JsonValue)>>,
    answers: HashMap<String, Answer>,
}

#[derive(Clone)]
enum Answer {
    Value(JsonValue),
    /// A rejection as the runtime carries it: the platform's code and the
    /// handler's message.
    Error(String, String),
}

impl HostChannel for StubChannel {
    fn call(
        &self,
        name: String,
        args: JsonValue,
    ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + Send>> {
        self.calls
            .lock()
            .expect("call log")
            .push((name.clone(), args));
        let answer = self.answers.get(&name).cloned();
        Box::pin(async move {
            match answer {
                Some(Answer::Value(value)) => Ok(value),
                Some(Answer::Error(code, message)) => {
                    Err(FirebaseError::from_host_code(&code, message))
                }
                None => Ok(JsonValue::Null),
            }
        })
    }

    fn post(&self, _name: String, _args: JsonValue) -> Result<(), FirebaseError> {
        Ok(())
    }
}

fn stub(answers: &[(String, Answer)]) -> (Installations, Arc<StubChannel>) {
    let channel = Arc::new(StubChannel {
        calls: Mutex::new(Vec::new()),
        answers: answers.iter().cloned().collect(),
    });
    let host: Arc<dyn FirebaseHost> = Arc::new(NativeHost::new(channel.clone()));
    (installations(host), channel)
}

fn installations(host: Arc<dyn FirebaseHost>) -> Installations {
    let app = nsh_firebase_core::initialize_app(
        nsh_firebase_core::FirebaseOptions::demo(),
        "installations-native-test",
    )
    .unwrap();
    let store: Arc<dyn KeyValueStore> = Arc::new(MemoryStore::new());
    Installations::new(app, store, host)
}

fn answered(name: &str, reply: JsonValue) -> (String, Answer) {
    (name.to_owned(), Answer::Value(reply))
}

fn rejected(name: &str, native_code: &str, message: &str) -> (String, Answer) {
    (
        name.to_owned(),
        Answer::Error(native_code.to_owned(), message.to_owned()),
    )
}

fn calls(channel: &StubChannel) -> Vec<(String, JsonValue)> {
    channel.calls.lock().expect("call log").clone()
}

#[tokio::test]
async fn get_id_routes_through_the_host_and_reads_the_id() {
    let (installations, channel) = stub(&[answered(
        "firebase.installations.getId",
        json!({ "id": "native-fid-1" }),
    )]);
    assert_eq!(installations.get_id().await.unwrap(), "native-fid-1");
    // The native tier's SDK id is not persisted here.
    assert_eq!(installations.id(), None);
    assert_eq!(
        calls(&channel),
        vec![("firebase.installations.getId".to_owned(), JsonValue::Null)]
    );
}

#[tokio::test]
async fn get_token_routes_through_the_host_and_reads_the_token() {
    let (installations, channel) = stub(&[answered(
        "firebase.installations.getToken",
        json!({ "token": "native-token-1" }),
    )]);
    // force_refresh has no meaning across the contract: the call carries
    // no args either way.
    assert_eq!(installations.get_token(true).await.unwrap(), "native-token-1");
    assert_eq!(
        calls(&channel),
        vec![(
            "firebase.installations.getToken".to_owned(),
            JsonValue::Null
        )]
    );
}

#[tokio::test]
async fn a_reply_without_the_field_is_a_host_error() {
    let (installations, channel) = stub(&[
        answered("firebase.installations.getId", json!({ "nope": 1 })),
        answered("firebase.installations.getToken", json!({ "token": "" })),
    ]);
    let error = installations.get_id().await.unwrap_err();
    assert_eq!(error.code, HOST_ERROR);
    assert!(
        error
            .message
            .contains("`firebase.installations.getId` answered without an id"),
        "{error}"
    );
    let error = installations.get_token(false).await.unwrap_err();
    assert_eq!(error.code, HOST_ERROR);
    assert!(
        error
            .message
            .contains("`firebase.installations.getToken` answered without a token"),
        "{error}"
    );
    assert_eq!(calls(&channel).len(), 2);
}

#[tokio::test]
async fn a_failed_registration_keeps_the_host_error() {
    let (installations, _channel) = stub(&[rejected(
        "firebase.installations.getId",
        "ERR_NSH_HOST_ERROR",
        "FIS registration failed: fake project rejected",
    )]);
    let error = installations.get_id().await.unwrap_err();
    assert_eq!(error.code, HOST_ERROR);
    assert_eq!(error.native_code.as_deref(), Some("ERR_NSH_HOST_ERROR"));
    assert!(error.message.contains("fake project rejected"), "{error}");
}

#[tokio::test]
async fn a_refused_host_name_stays_a_denial() {
    // A grant the app does not declare is a refusal, not something the
    // facade rewrites.
    let (installations, _channel) = stub(&[rejected(
        "firebase.installations.getToken",
        "ERR_ACCESS_DENIED",
        "`firebase.installations.getToken` is not granted",
    )]);
    let error = installations.get_token(false).await.unwrap_err();
    assert_eq!(error.code, ACCESS_DENIED);
    assert_eq!(error.native_code.as_deref(), Some("ERR_ACCESS_DENIED"));
}
