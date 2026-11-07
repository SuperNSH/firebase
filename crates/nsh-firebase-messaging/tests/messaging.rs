//! Headless tests of the messaging facade: the stub channel plays the
//! native host with the same handler names and reply shapes the native
//! areas register, and `PureHost` plays the pure tier. No real time: the
//! stub's futures complete at once.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use nsh_firebase_core::error::{
    ACCESS_DENIED, FirebaseError, HOST_ERROR, INVALID_ARGUMENT, UNAVAILABLE,
};
use nsh_firebase_core::host::{FirebaseHost, HostChannel, NativeHost, PureHost};
use nsh_firebase_messaging::{
    Event, MessageNotification, Messaging, NEEDS_NATIVE_TIER, Permission,
};
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

fn stub(answers: &[(String, Answer)]) -> (Messaging, Arc<StubChannel>) {
    let channel = Arc::new(StubChannel {
        calls: Mutex::new(Vec::new()),
        answers: answers.iter().cloned().collect(),
    });
    let host: Arc<dyn FirebaseHost> = Arc::new(NativeHost::new(channel.clone()));
    (Messaging::new(host), channel)
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

fn show_args(silent: bool) -> JsonValue {
    json!({
        "id": "chat:42",
        "title": "Ada",
        "body": "Are you coming?",
        "silent": silent,
    })
}

#[tokio::test]
async fn token_resolves_with_the_host_token() {
    let (messaging, channel) = stub(&[answered(
        "firebase.messaging.token",
        json!({ "token": "tok-stub-123" }),
    )]);
    assert_eq!(messaging.token().await.unwrap(), "tok-stub-123");
    assert_eq!(
        calls(&channel),
        vec![("firebase.messaging.token".to_owned(), JsonValue::Null)]
    );
}

#[tokio::test]
async fn a_failed_token_request_keeps_the_host_error() {
    let (messaging, _channel) = stub(&[rejected(
        "firebase.messaging.token",
        "ERR_NSH_HOST_ERROR",
        "fcm token unavailable: fake project rejected",
    )]);
    let error = messaging.token().await.unwrap_err();
    assert_eq!(error.code, HOST_ERROR);
    assert_eq!(error.native_code.as_deref(), Some("ERR_NSH_HOST_ERROR"));
    assert!(error.message.contains("fcm token unavailable"), "{error}");
}

#[tokio::test]
async fn a_token_reply_without_a_token_is_a_host_error() {
    let (messaging, channel) = stub(&[answered("firebase.messaging.token", json!({ "nope": 1 }))]);
    let error = messaging.token().await.unwrap_err();
    assert_eq!(error.code, HOST_ERROR);
    assert_eq!(calls(&channel).len(), 1);
    let (_name, args) = &calls(&channel)[0];
    assert_eq!(args, &JsonValue::Null);
}

#[tokio::test]
async fn permission_reads_the_host_answer() {
    let (messaging, channel) = stub(&[
        answered(
            "firebase.messaging.permission",
            json!({ "permission": "granted" }),
        ),
        answered(
            "firebase.messaging.requestPermission",
            json!({ "permission": "default" }),
        ),
    ]);
    assert_eq!(messaging.permission().await.unwrap(), Permission::Granted);
    assert_eq!(
        messaging.request_permission().await.unwrap(),
        Permission::Default
    );
    assert_eq!(
        calls(&channel),
        vec![
            ("firebase.messaging.permission".to_owned(), JsonValue::Null),
            (
                "firebase.messaging.requestPermission".to_owned(),
                JsonValue::Null
            ),
        ]
    );
}

#[tokio::test]
async fn an_unknown_permission_name_is_a_host_error() {
    let (messaging, _channel) = stub(&[answered(
        "firebase.messaging.permission",
        json!({ "permission": "maybe" }),
    )]);
    let error = messaging.permission().await.unwrap_err();
    assert_eq!(error.code, HOST_ERROR);
    assert!(
        error.message.contains("which is not a permission name"),
        "{error}"
    );
}

#[tokio::test]
async fn show_passes_the_content_and_reads_the_shown_reply() {
    let (messaging, channel) = stub(&[answered(
        "firebase.messaging.show",
        json!({ "shown": true }),
    )]);
    let notification = MessageNotification::new("chat:42", "Ada", "Are you coming?");
    assert!(messaging.show(&notification).await.unwrap());
    assert_eq!(
        calls(&channel),
        vec![("firebase.messaging.show".to_owned(), show_args(false))]
    );
}

#[tokio::test]
async fn show_reads_the_silent_flag_and_a_false_reply() {
    let (messaging, channel) = stub(&[answered(
        "firebase.messaging.show",
        json!({ "shown": false }),
    )]);
    let notification = MessageNotification::new("sync", "Synced", "12 new photos").silent(true);
    assert!(!messaging.show(&notification).await.unwrap());
    let (_name, args) = &calls(&channel)[0];
    assert_eq!(args["silent"], json!(true));
    assert_eq!(args["id"], json!("sync"));
}

#[tokio::test]
async fn show_reads_a_bare_boolean_reply() {
    // Android's facade answers the bare boolean where iOS answers
    // {shown}: both are acceptance.
    let (messaging, _channel) = stub(&[answered("firebase.messaging.show", json!(true))]);
    let notification = MessageNotification::new("chat:42", "Ada", "Are you coming?");
    assert!(messaging.show(&notification).await.unwrap());
}

#[tokio::test]
async fn show_needs_its_content() {
    let (messaging, channel) = stub(&[]);
    let notification = MessageNotification::new("", "Ada", "Are you coming?");
    let error = messaging.show(&notification).await.unwrap_err();
    assert_eq!(error.code, INVALID_ARGUMENT);
    assert!(error.message.contains("non-empty id and title"), "{error}");
    assert!(calls(&channel).is_empty(), "the host never saw the call");
}

#[tokio::test]
async fn show_needs_a_title() {
    // The Notifications module this mirrors rejects an empty title the
    // same way it rejects an empty id; only body is ""-for-none.
    let (messaging, channel) = stub(&[]);
    let notification = MessageNotification::new("quiet", "", "Are you coming?");
    let error = messaging.show(&notification).await.unwrap_err();
    assert_eq!(error.code, INVALID_ARGUMENT);
    assert!(error.message.contains("non-empty id and title"), "{error}");
    assert!(calls(&channel).is_empty(), "the host never saw the call");
}

#[tokio::test]
async fn show_allows_an_empty_body() {
    let (messaging, channel) = stub(&[answered(
        "firebase.messaging.show",
        json!({ "shown": true }),
    )]);
    let notification = MessageNotification::new("quiet", "Hi", "");
    assert!(messaging.show(&notification).await.unwrap());
    assert_eq!(
        calls(&channel),
        vec![(
            "firebase.messaging.show".to_owned(),
            json!({ "id": "quiet", "title": "Hi", "body": "", "silent": false })
        )]
    );
}

#[tokio::test]
async fn close_and_the_badge_pass_through() {
    let (messaging, channel) = stub(&[]);
    messaging.close("chat:42").await.unwrap();
    messaging.set_badge(3).await.unwrap();
    assert_eq!(
        calls(&channel),
        vec![
            (
                "firebase.messaging.close".to_owned(),
                json!({ "id": "chat:42" })
            ),
            (
                "firebase.messaging.setBadge".to_owned(),
                json!({ "count": 3 })
            ),
        ]
    );
}

#[tokio::test]
async fn topics_pass_through_and_an_empty_topic_is_refused() {
    let (messaging, channel) = stub(&[]);
    messaging.subscribe("news").await.unwrap();
    messaging.unsubscribe("news").await.unwrap();
    let error = messaging.subscribe("").await.unwrap_err();
    assert_eq!(error.code, INVALID_ARGUMENT);
    assert!(error.message.contains("non-empty topic"), "{error}");
    assert_eq!(
        calls(&channel),
        vec![
            (
                "firebase.messaging.subscribe".to_owned(),
                json!({ "topic": "news" })
            ),
            (
                "firebase.messaging.unsubscribe".to_owned(),
                json!({ "topic": "news" })
            ),
        ]
    );
}

#[tokio::test]
async fn presses_messages_and_token_refreshes_reach_their_listeners() {
    let (messaging, _channel) = stub(&[]);
    let presses: Arc<Mutex<Vec<JsonValue>>> = Arc::new(Mutex::new(Vec::new()));
    let second_presses: Arc<Mutex<Vec<JsonValue>>> = Arc::new(Mutex::new(Vec::new()));
    let messages: Arc<Mutex<Vec<JsonValue>>> = Arc::new(Mutex::new(Vec::new()));
    let tokens: Arc<Mutex<Vec<JsonValue>>> = Arc::new(Mutex::new(Vec::new()));

    let sink = Arc::clone(&presses);
    messaging
        .on(Event::Press, move |payload| {
            sink.lock().expect("presses").push(payload.clone());
        })
        .expect("the native tier takes listeners");
    let sink = Arc::clone(&messages);
    messaging
        .on(Event::Message, move |payload| {
            sink.lock().expect("messages").push(payload.clone());
        })
        .expect("the native tier takes listeners");
    let sink = Arc::clone(&tokens);
    messaging
        .on(Event::Token, move |payload| {
            sink.lock().expect("tokens").push(payload.clone());
        })
        .expect("the native tier takes listeners");
    let sink = Arc::clone(&second_presses);
    let second_press = messaging
        .on(Event::Press, move |payload| {
            sink.lock().expect("second presses").push(payload.clone());
        })
        .expect("the native tier takes listeners");

    assert!(messaging.dispatch("firebase.messaging.press", &json!({ "id": "chat:42" })));
    assert!(messaging.dispatch(
        "firebase.messaging.message",
        &json!({ "messageId": "m1", "data": {}, "notification": { "title": "Hi", "body": "Yo" } })
    ));
    assert!(messaging.dispatch(
        "firebase.messaging.tokenRefreshed",
        &json!({ "token": "tok-9" })
    ));
    // Not one of the facade's events: the host ignores it, so must the seam.
    assert!(!messaging.dispatch("firebase.analytics.logEvent", &JsonValue::Null));

    assert_eq!(
        *presses.lock().expect("presses"),
        vec![json!({ "id": "chat:42" })]
    );
    assert_eq!(
        *second_presses.lock().expect("second presses"),
        vec![json!({ "id": "chat:42" })]
    );
    assert_eq!(
        *tokens.lock().expect("tokens"),
        vec![json!({ "token": "tok-9" })]
    );
    assert_eq!(messages.lock().expect("messages").len(), 1);

    // Off stops one listener and leaves the others.
    messaging.off(second_press);
    assert!(messaging.dispatch("firebase.messaging.press", &json!({ "id": "second" })));
    assert_eq!(
        *presses.lock().expect("presses"),
        vec![json!({ "id": "chat:42" }), json!({ "id": "second" })]
    );
    assert_eq!(second_presses.lock().expect("second presses").len(), 1);
    assert_eq!(tokens.lock().expect("tokens").len(), 1);
}

#[tokio::test]
async fn the_pure_tier_names_what_is_missing() {
    let messaging = Messaging::new(Arc::new(PureHost));
    let notification = MessageNotification::new("chat:42", "Ada", "Are you coming?");

    let error = messaging.token().await.unwrap_err();
    assert_eq!(error.code, UNAVAILABLE);
    assert_eq!(
        error.native_code.as_deref(),
        Some("ERR_FEATURE_UNAVAILABLE_ON_PLATFORM")
    );
    assert_eq!(error.message, NEEDS_NATIVE_TIER);

    for error in [
        messaging.permission().await.unwrap_err(),
        messaging.request_permission().await.unwrap_err(),
        messaging.show(&notification).await.unwrap_err(),
        messaging.close("chat:42").await.unwrap_err(),
        messaging.set_badge(1).await.unwrap_err(),
        messaging.subscribe("news").await.unwrap_err(),
        messaging.unsubscribe("news").await.unwrap_err(),
    ] {
        assert_eq!(error.code, UNAVAILABLE, "{error}");
        assert_eq!(error.message, NEEDS_NATIVE_TIER, "{error}");
    }

    // A listener that never fires is the silent no-op the contract
    // forbids: on() refuses like every other call.
    let listened: Arc<Mutex<Vec<JsonValue>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&listened);
    let error = messaging
        .on(Event::Press, move |payload| {
            sink.lock().expect("listened").push(payload.clone());
        })
        .unwrap_err();
    assert_eq!(error.code, UNAVAILABLE);
    assert_eq!(error.message, NEEDS_NATIVE_TIER);
    assert!(listened.lock().expect("listened").is_empty());
}

#[tokio::test]
async fn a_refused_host_name_stays_a_denial() {
    // A grant the app does not declare is a refusal, not "unavailable":
    // the facade must not rewrite it into its own message.
    let (messaging, _channel) = stub(&[rejected(
        "firebase.messaging.token",
        "ERR_ACCESS_DENIED",
        "`firebase.messaging.token` is not granted",
    )]);
    let error = messaging.token().await.unwrap_err();
    assert_eq!(error.code, ACCESS_DENIED);
    assert_eq!(error.native_code.as_deref(), Some("ERR_ACCESS_DENIED"));
}

#[test]
fn permission_names_are_the_notifications_modules() {
    assert_eq!(Permission::default(), Permission::Default);
    assert_eq!(Permission::Granted.as_str(), "granted");
    assert_eq!(Permission::from_name("denied"), Some(Permission::Denied));
    assert_eq!(
        Permission::from_name("unavailable"),
        Some(Permission::Unavailable)
    );
    assert_eq!(Permission::from_name("maybe"), None);
}
