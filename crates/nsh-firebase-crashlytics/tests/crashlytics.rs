//! Headless tests of the crashlytics facade: the stub channel plays the
//! native host with the same handler names and reply shapes the native
//! areas register, and `PureHost` plays the pure tier. No real time: the
//! stub's futures complete at once.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use nsh_firebase_core::error::{ACCESS_DENIED, FirebaseError, HOST_ERROR, INVALID_ARGUMENT};
use nsh_firebase_core::host::{FirebaseHost, HostChannel, NativeHost, PureHost};
use nsh_firebase_crashlytics::{Crashlytics, NEEDS_NATIVE_TIER, RecordError, UNAVAILABLE, wire};
use serde_json::{Value as JsonValue, json};

/// The test's host: it records every call and answers from a fixed table,
/// the way the native areas' handlers do. A name with no answer answers
/// `null`, which the act-only calls must accept.
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

fn stub(answers: &[(String, Answer)]) -> (Crashlytics, Arc<StubChannel>) {
    let channel = Arc::new(StubChannel {
        calls: Mutex::new(Vec::new()),
        answers: answers.iter().cloned().collect(),
    });
    let host: Arc<dyn FirebaseHost> = Arc::new(NativeHost::new(channel.clone()));
    (Crashlytics::new(host), channel)
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

fn pure() -> Crashlytics {
    Crashlytics::new(Arc::new(PureHost))
}

#[test]
fn the_wire_names_are_the_contract() {
    assert_eq!(wire::LOG, "firebase.crashlytics.log");
    assert_eq!(wire::SET_USER_ID, "firebase.crashlytics.setUserId");
    assert_eq!(wire::SET_CUSTOM_KEY, "firebase.crashlytics.setCustomKey");
    assert_eq!(wire::RECORD_ERROR, "firebase.crashlytics.recordError");
    assert_eq!(
        wire::SEND_UNSENT_REPORTS,
        "firebase.crashlytics.sendUnsentReports"
    );
    assert_eq!(
        wire::DID_CRASH,
        "firebase.crashlytics.didCrashDuringPreviousExecution"
    );
}

#[tokio::test]
async fn log_and_the_user_id_carry_their_args() {
    let (crashlytics, channel) = stub(&[]);
    crashlytics.log("payment screen shown").await.unwrap();
    crashlytics.set_user_id("user-41").await.unwrap();
    // The act-only calls accept the default `null` reply.
    assert_eq!(
        calls(&channel),
        vec![
            (
                wire::LOG.to_owned(),
                json!({ "message": "payment screen shown" })
            ),
            (wire::SET_USER_ID.to_owned(), json!({ "id": "user-41" })),
        ]
    );
}

#[tokio::test]
async fn send_unsent_reports_carries_no_args() {
    let (crashlytics, channel) = stub(&[]);
    crashlytics.send_unsent_reports().await.unwrap();
    assert_eq!(
        calls(&channel),
        vec![(wire::SEND_UNSENT_REPORTS.to_owned(), JsonValue::Null)]
    );
}

#[tokio::test]
async fn a_custom_key_passes_strings_numbers_and_booleans() {
    let (crashlytics, channel) = stub(&[]);
    crashlytics
        .set_attribute("difficulty", json!("hard"))
        .await
        .unwrap();
    crashlytics.set_attribute("level", json!(7)).await.unwrap();
    crashlytics
        .set_attribute("paid", json!(true))
        .await
        .unwrap();
    let logged = calls(&channel);
    assert_eq!(logged.len(), 3);
    for (name, args) in &logged {
        assert_eq!(name, wire::SET_CUSTOM_KEY);
        assert!(args.get("key").and_then(JsonValue::as_str).is_some());
        let value = args.get("value").expect("value");
        assert!(
            value.is_string() || value.is_number() || value.is_boolean(),
            "{value}"
        );
    }
    assert_eq!(logged[0].1["key"], json!("difficulty"));
    assert_eq!(logged[1].1["value"], json!(7));
    assert_eq!(logged[2].1["value"], json!(true));
}

#[tokio::test]
async fn attributes_send_one_call_per_key_in_order() {
    let (crashlytics, channel) = stub(&[]);
    crashlytics
        .set_attributes(&[
            ("level".to_owned(), json!(7)),
            ("paid".to_owned(), json!(true)),
        ])
        .await
        .unwrap();
    assert_eq!(
        calls(&channel),
        vec![
            (
                wire::SET_CUSTOM_KEY.to_owned(),
                json!({ "key": "level", "value": 7 })
            ),
            (
                wire::SET_CUSTOM_KEY.to_owned(),
                json!({ "key": "paid", "value": true })
            ),
        ]
    );
}

#[tokio::test]
async fn a_non_scalar_custom_value_is_refused_before_the_host() {
    for bad in [JsonValue::Null, json!({ "deep": 1 }), json!(["x"])] {
        let (crashlytics, channel) = stub(&[]);
        let error = crashlytics.set_attribute("bad", bad).await.unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT, "{error}");
        assert!(error.message.contains("custom keys"), "{error}");
        assert!(calls(&channel).is_empty(), "the host never saw the call");
    }
}

#[tokio::test]
async fn record_error_carries_the_stack_only_when_given() {
    let (crashlytics, channel) = stub(&[]);
    crashlytics
        .record_error(&RecordError::new(500, "boom").stack("at run (app.rs:1)"))
        .await
        .unwrap();
    crashlytics
        .record_error(&RecordError::new(401, "payment declined"))
        .await
        .unwrap();
    assert_eq!(
        calls(&channel),
        vec![
            (
                wire::RECORD_ERROR.to_owned(),
                json!({ "code": 500, "message": "boom", "stack": "at run (app.rs:1)" })
            ),
            (
                wire::RECORD_ERROR.to_owned(),
                json!({ "code": 401, "message": "payment declined" })
            ),
        ]
    );
    // The no-stack call must not carry a null stack either.
    let (_, args) = &calls(&channel)[1];
    assert!(args.get("stack").is_none());
}

#[tokio::test]
async fn did_crash_reads_the_flag_both_ways() {
    let (crashlytics, channel) = stub(&[answered(wire::DID_CRASH, json!({ "crashed": true }))]);
    assert!(crashlytics.did_crash_on_previous_execution().await.unwrap());
    let (crashlytics, _channel) = stub(&[answered(wire::DID_CRASH, json!({ "crashed": false }))]);
    assert!(!crashlytics.did_crash_on_previous_execution().await.unwrap());
    // The no-arg call really carried no args.
    assert_eq!(calls(&channel)[0].1, JsonValue::Null);
}

#[tokio::test]
async fn a_reply_without_the_flag_is_a_host_error_not_a_false() {
    let (crashlytics, channel) = stub(&[answered(wire::DID_CRASH, json!({}))]);
    let error = crashlytics
        .did_crash_on_previous_execution()
        .await
        .unwrap_err();
    assert_eq!(error.code, HOST_ERROR, "{error}");
    assert!(error.message.contains("crashed flag"), "{error}");
    assert_eq!(calls(&channel).len(), 1);
}

#[tokio::test]
async fn a_refused_host_name_stays_a_denial() {
    // A grant the app does not declare is a refusal, not "unavailable":
    // the facade must not rewrite it into its own message.
    let (crashlytics, _channel) = stub(&[rejected(
        wire::LOG,
        "ERR_ACCESS_DENIED",
        "`firebase.crashlytics.log` is not granted",
    )]);
    let error = crashlytics.log("x").await.unwrap_err();
    assert_eq!(error.code, ACCESS_DENIED);
    assert_eq!(error.native_code.as_deref(), Some("ERR_ACCESS_DENIED"));
}

#[tokio::test]
async fn a_host_without_the_handlers_is_rewritten_to_unavailable() {
    // A native host that does not register firebase.crashlytics.* (an
    // older host) answers the runtime's feature refusal; the facade
    // names what is missing instead of passing the generic message.
    let (crashlytics, _channel) = stub(&[rejected(
        wire::SET_USER_ID,
        "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
        "no handler for firebase.crashlytics.setUserId",
    )]);
    let error = crashlytics.set_user_id("user-41").await.unwrap_err();
    assert_eq!(error.code, UNAVAILABLE);
    assert_eq!(error.message, NEEDS_NATIVE_TIER);
    assert_eq!(
        error.native_code.as_deref(),
        Some("ERR_FEATURE_UNAVAILABLE_ON_PLATFORM")
    );
}

#[tokio::test]
async fn the_pure_tier_rejects_every_call_naming_what_is_missing() {
    let crashlytics = pure();
    let attributes = vec![("difficulty".to_owned(), json!("hard"))];

    let error = crashlytics.log("x").await.unwrap_err();
    assert_eq!(error.code, UNAVAILABLE);
    assert_eq!(error.message, NEEDS_NATIVE_TIER);
    assert_eq!(
        error.native_code.as_deref(),
        Some("ERR_FEATURE_UNAVAILABLE_ON_PLATFORM")
    );

    for error in [
        crashlytics.set_user_id("user-41").await.unwrap_err(),
        crashlytics
            .set_attribute("difficulty", json!("hard"))
            .await
            .unwrap_err(),
        crashlytics.set_attributes(&attributes).await.unwrap_err(),
        crashlytics
            .record_error(&RecordError::new(500, "boom"))
            .await
            .unwrap_err(),
        crashlytics.send_unsent_reports().await.unwrap_err(),
        crashlytics
            .did_crash_on_previous_execution()
            .await
            .unwrap_err(),
    ] {
        assert_eq!(error.code, UNAVAILABLE, "{error}");
        assert_eq!(error.message, NEEDS_NATIVE_TIER, "{error}");
    }
}

#[tokio::test]
async fn the_pure_tier_still_validates_its_arguments_first() {
    // Local validation runs before the tier refusal (messaging's order):
    // a bad custom value is the caller's bug, whatever tier it runs in.
    // PureHost records nothing, so "the host never saw it" is structural.
    let crashlytics = pure();
    let error = crashlytics
        .set_attribute("bad", JsonValue::Null)
        .await
        .unwrap_err();
    assert_eq!(error.code, INVALID_ARGUMENT, "{error}");
}
