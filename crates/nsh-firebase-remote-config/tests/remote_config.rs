//! Headless integration tests of the public remote-config surface: the
//! defaults map, typed reads, a realistic template fetched through a
//! stubbed transport (a JSON string standing in for a network body), the
//! honest pure-tier refusal, and the native tier's wire shapes. No real
//! network and no real time: the transport is a stub and the clock is
//! manual.

use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use serde_json::{Value as JsonValue, json};

use nsh_firebase_core::error::{HOST_ERROR, INVALID_ARGUMENT, NETWORK};
use nsh_firebase_core::{FirebaseError, FirebaseHost};
use nsh_firebase_remote_config::{
    FETCH_FAILURE, FETCH_SUCCESS, FetchAndActivateResult, ManualClock, NATIVE_FETCH_AND_ACTIVATE,
    NATIVE_GET, NATIVE_GET_DEFAULTS, NATIVE_SET_DEFAULTS, NO_FETCH_YET, NativeRemoteConfig,
    RemoteConfig, Transport, UNAVAILABLE, parse_template,
};

/// A realistic template document, the REST `RemoteConfig` shape: two
/// conditions, five parameters (default, conditional, `useInAppDefault`
/// and a dynamic value), one parameter group, and a version block.
const REALISTIC_TEMPLATE: &str = r#"{
  "conditions": [
    {"name": "ios_users", "expression": "device.os == 'ios'", "tagColor": "BLUE"},
    {"name": "beta_cohort", "expression": "user.property('cohort') == 'beta'"}
  ],
  "parameters": {
    "welcome_message": {
      "defaultValue": {"value": "Welcome to nsh-firebase"},
      "conditionalValues": {"ios_users": {"value": "Welcome, iOS user"}}
    },
    "feature_flag_dark_mode": {"defaultValue": {"value": "true"}},
    "tip_amount": {"defaultValue": {"value": "25"}, "valueType": "NUMBER"},
    "rollout_copy": {
      "defaultValue": {"useInAppDefault": true},
      "conditionalValues": {"beta_cohort": {"value": "beta copy"}}
    },
    "ml_prompt": {"defaultValue": {"personalizationValue": {"personalizationId": "53"}}}
  },
  "parameterGroups": {
    "checkout": {
      "description": "Checkout tuning",
      "parameters": {"checkout_button_label": {"defaultValue": {"value": "Pay now"}}}
    }
  },
  "version": {
    "versionNumber": "12",
    "updateOrigin": "CONSOLE",
    "updateTime": "2026-11-01T10:00:00.000000Z"
  }
}"#;

/// Serves the template body a "network" delivered, like a fetch would.
struct StubbedTransport {
    body: String,
}

impl Transport for StubbedTransport {
    fn fetch_template<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + 'a>> {
        let body = self.body.clone();
        Box::pin(async move {
            let parsed: Result<JsonValue, _> = serde_json::from_str(&body);
            parsed.map_err(|error| FirebaseError::new(NETWORK, error.to_string()))
        })
    }
}

fn manual_config() -> RemoteConfig {
    RemoteConfig::with_parts(None, Arc::new(ManualClock::new(10_000)))
}

fn config_with_template(body: &str) -> RemoteConfig {
    let transport = Arc::new(StubbedTransport {
        body: body.to_owned(),
    });
    RemoteConfig::with_parts(Some(transport), Arc::new(ManualClock::new(10_000)))
}

#[test]
fn defaults_set_get_and_get_defaults_round_trip() {
    let config = manual_config();
    assert_eq!(config.get_defaults(), json!({}));
    config
        .set_defaults(&json!({
            "welcome_message": "hello from defaults",
            "feature_flag_dark_mode": false,
            "tip_amount": 10,
        }))
        .unwrap();
    assert_eq!(
        config.get_defaults(),
        json!({
            "welcome_message": "hello from defaults",
            "feature_flag_dark_mode": false,
            "tip_amount": 10,
        })
    );
    assert_eq!(
        config.get("welcome_message").as_deref(),
        Some("hello from defaults")
    );
}

#[test]
fn set_defaults_replaces_and_refuses_invalid_maps() {
    let config = manual_config();
    config.set_defaults(&json!({"first": 1})).unwrap();
    config.set_defaults(&json!({"second": 2})).unwrap();
    assert_eq!(
        config.get_defaults(),
        json!({"second": 2}),
        "later calls replace, not merge"
    );
    for bad in [
        json!("scalar"),
        json!([]),
        json!({"object": {}}),
        json!({"null": null}),
    ] {
        let error = config.set_defaults(&bad).unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT, "for {bad}");
    }
    assert_eq!(
        config.get_defaults(),
        json!({"second": 2}),
        "a refused call changes nothing"
    );
}

#[test]
fn typed_gets_coerce_over_the_defaults() {
    let config = manual_config();
    config
        .set_defaults(&json!({
            "feature_flag_dark_mode": true,
            "tip_amount": 10,
            "welcome_message": "hello",
            "truthy_text": "Yes",
            "garbage_number": "abc",
        }))
        .unwrap();
    assert_eq!(config.get_string("welcome_message"), "hello");
    assert!(config.get_boolean("feature_flag_dark_mode"));
    assert!(
        config.get_boolean("truthy_text"),
        "the web SDK's truthy spellings"
    );
    assert_eq!(config.get_number("tip_amount"), 10.0);
    // Missing and unparseable values fall back like the SDKs do.
    assert_eq!(config.get_string("missing"), "");
    assert!(!config.get_boolean("missing"));
    assert_eq!(config.get_number("missing"), 0.0);
    assert_eq!(config.get_number("garbage_number"), 0.0);
    assert_eq!(config.get("missing"), None);
}

#[test]
fn a_realistic_template_document_parses() {
    let document: JsonValue = serde_json::from_str(REALISTIC_TEMPLATE).unwrap();
    let template = parse_template(&document).unwrap();
    assert_eq!(template.conditions.len(), 2);
    assert_eq!(template.conditions[1].name, "beta_cohort");
    assert_eq!(
        template.parameters.len(),
        6,
        "five parameters plus the group's"
    );
    assert_eq!(
        template.parameters["welcome_message"].conditional_values["ios_users"],
        "Welcome, iOS user"
    );
    // The client evaluates no conditions, so defaults resolve; the
    // `useInAppDefault` and dynamic defaults stay unset.
    assert_eq!(
        template.resolved_values(),
        BTreeMap::from([
            ("checkout_button_label".to_owned(), "Pay now".to_owned()),
            ("feature_flag_dark_mode".to_owned(), "true".to_owned()),
            ("tip_amount".to_owned(), "25".to_owned()),
            (
                "welcome_message".to_owned(),
                "Welcome to nsh-firebase".to_owned()
            ),
        ])
    );
    assert_eq!(
        template
            .version
            .as_ref()
            .and_then(|version| version["versionNumber"].as_str()),
        Some("12")
    );
}

#[tokio::test]
async fn fetch_and_activate_applies_the_template_over_the_defaults() {
    let config = config_with_template(REALISTIC_TEMPLATE);
    config
        .set_defaults(&json!({
            "welcome_message": "default hello",
            "rollout_copy": "app default copy",
            "ml_prompt": "default prompt",
            "only_default": "kept",
        }))
        .unwrap();
    let result = config.fetch_and_activate().await.unwrap();
    assert_eq!(
        result,
        FetchAndActivateResult {
            activated: true,
            last_fetch_status: FETCH_SUCCESS.to_owned(),
        }
    );
    assert_eq!(
        config.get_string("welcome_message"),
        "Welcome to nsh-firebase"
    );
    assert!(config.get_boolean("feature_flag_dark_mode"));
    assert_eq!(config.get_number("tip_amount"), 25.0);
    assert_eq!(config.get_string("checkout_button_label"), "Pay now");
    assert_eq!(
        config.get_string("rollout_copy"),
        "app default copy",
        "useInAppDefault falls back to the app default"
    );
    assert_eq!(
        config.get_string("ml_prompt"),
        "default prompt",
        "a dynamic value falls back to the app default"
    );
    assert_eq!(config.get("only_default"), Some("kept".to_owned()));
    assert_eq!(config.last_fetch_status(), FETCH_SUCCESS);
    assert_eq!(config.last_fetch_time_ms(), Some(10_000));
}

#[tokio::test]
async fn fetch_and_activate_without_a_transport_rejects_with_the_documented_code() {
    let config = manual_config();
    let error = config.fetch_and_activate().await.unwrap_err();
    assert_eq!(error.code, UNAVAILABLE);
    assert!(error.message.contains("OAuth 2.0"));
    assert!(
        error
            .message
            .contains("firebaseremoteconfig.googleapis.com")
    );
    // Nothing was fetched and nothing is pretended: no template, no
    // fetch time, the RNFB initial status.
    assert_eq!(config.last_fetch_status(), NO_FETCH_YET);
    assert_eq!(config.last_fetch_time_ms(), None);
}

struct RefusingTransport;

impl Transport for RefusingTransport {
    fn fetch_template<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + 'a>> {
        Box::pin(async move {
            Err(FirebaseError::new(
                NETWORK,
                "HTTP 401: Request had invalid authentication credentials",
            ))
        })
    }
}

#[tokio::test]
async fn a_refusing_backend_is_surfaced_and_recorded_as_a_failure() {
    let config = RemoteConfig::with_parts(
        Some(Arc::new(RefusingTransport)),
        Arc::new(ManualClock::new(777)),
    );
    let error = config.fetch_and_activate().await.unwrap_err();
    assert_eq!(error.code, NETWORK);
    assert!(error.message.contains("401"));
    assert_eq!(config.last_fetch_status(), FETCH_FAILURE);
    assert_eq!(config.last_fetch_time_ms(), Some(777));
    assert!(config.get("welcome_message").is_none());
}

#[tokio::test]
async fn a_corrupt_template_body_rejects_with_the_template_code() {
    let body = r#"{"parameters": {"p": {"defaultValue": {"value": 3}}}}"#;
    let config = config_with_template(body);
    let error = config.fetch_and_activate().await.unwrap_err();
    assert_eq!(error.code, nsh_firebase_remote_config::INVALID_TEMPLATE);
    assert_eq!(config.last_fetch_status(), FETCH_FAILURE);
}

/// Records every `call` with its args, answering per handler name.
#[derive(Default)]
struct RecordingHost {
    answers: BTreeMap<String, JsonValue>,
    calls: Mutex<Vec<(String, JsonValue)>>,
}

impl FirebaseHost for RecordingHost {
    fn call(&self, name: &str, args: JsonValue) -> nsh_firebase_core::host::HostCallFuture {
        self.calls
            .lock()
            .expect("recording host lock")
            .push((name.to_owned(), args.clone()));
        let answer = self.answers.get(name).cloned().unwrap_or(JsonValue::Null);
        Box::pin(async move { Ok(answer) })
    }

    fn post(&self, name: &str, _args: JsonValue) -> Result<(), FirebaseError> {
        Err(FirebaseError::from_host_code(
            "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
            format!("`{name}` is never posted"),
        ))
    }
}

fn native_with_answers(
    answers: BTreeMap<String, JsonValue>,
) -> (NativeRemoteConfig, Arc<RecordingHost>) {
    let host = Arc::new(RecordingHost {
        answers,
        ..RecordingHost::default()
    });
    let native = NativeRemoteConfig::new(Arc::clone(&host) as Arc<dyn FirebaseHost>);
    (native, host)
}

#[tokio::test]
async fn native_routing_asserts_the_contract_wire_calls() {
    let (native, host) = native_with_answers(BTreeMap::from([
        (NATIVE_SET_DEFAULTS.to_owned(), json!({})),
        (
            NATIVE_GET_DEFAULTS.to_owned(),
            json!({"map": {"k": "host"}}),
        ),
        (
            NATIVE_FETCH_AND_ACTIVATE.to_owned(),
            json!({"activated": true, "lastFetchStatus": "success"}),
        ),
        (NATIVE_GET.to_owned(), json!({"value": "host value"})),
    ]));

    native
        .set_defaults(&json!({"welcome_message": "host defaults", "tip_amount": 5}))
        .await
        .unwrap();
    assert_eq!(native.get_defaults().await.unwrap(), json!({"k": "host"}));
    assert_eq!(
        native.fetch_and_activate().await.unwrap(),
        FetchAndActivateResult {
            activated: true,
            last_fetch_status: "success".to_owned(),
        }
    );
    assert_eq!(
        native.get("welcome_message").await.unwrap().as_deref(),
        Some("host value")
    );
    // The stub answers every key with the same value; the typed reads
    // coerce whatever the wire carried.
    assert_eq!(
        native.get_string("missing_key").await.unwrap(),
        "host value"
    );
    assert!(!native.get_boolean("missing_key").await.unwrap());
    assert_eq!(native.get_number("missing_key").await.unwrap(), 0.0);
    assert_eq!(native.last_fetch_status(), "success");

    assert_eq!(
        host.calls.lock().expect("recording host lock").clone(),
        vec![
            (
                NATIVE_SET_DEFAULTS.to_owned(),
                json!({"map": {"welcome_message": "host defaults", "tip_amount": 5}}),
            ),
            (NATIVE_GET_DEFAULTS.to_owned(), JsonValue::Null),
            (NATIVE_FETCH_AND_ACTIVATE.to_owned(), JsonValue::Null),
            (NATIVE_GET.to_owned(), json!({"key": "welcome_message"})),
            (NATIVE_GET.to_owned(), json!({"key": "missing_key"})),
            (NATIVE_GET.to_owned(), json!({"key": "missing_key"})),
            (NATIVE_GET.to_owned(), json!({"key": "missing_key"})),
        ]
    );
}

#[tokio::test]
async fn native_validation_refuses_before_any_wire_call() {
    let (native, host) = native_with_answers(BTreeMap::new());
    let error = native.set_defaults(&json!({"bad": []})).await.unwrap_err();
    assert_eq!(error.code, INVALID_ARGUMENT);
    let error = native.get("").await.unwrap_err();
    assert_eq!(error.code, INVALID_ARGUMENT);
    assert!(host.calls.lock().expect("recording host lock").is_empty());
}

#[tokio::test]
async fn a_malformed_native_answer_is_a_clear_host_error() {
    let (native, _) = native_with_answers(BTreeMap::from([
        (
            NATIVE_FETCH_AND_ACTIVATE.to_owned(),
            json!({"activated": "yes", "lastFetchStatus": "success"}),
        ),
        (NATIVE_GET.to_owned(), json!({"value": {"nested": true}})),
    ]));
    let error = native.fetch_and_activate().await.unwrap_err();
    assert_eq!(error.code, HOST_ERROR);
    let error = native.get("key").await.unwrap_err();
    assert_eq!(error.code, HOST_ERROR);
}

#[test]
fn the_manual_clock_never_moves_by_itself() {
    let clock = ManualClock::new(10_000);
    clock.advance(500);
    assert_eq!(clock.now_ms(), 10_500);
    let config = RemoteConfig::with_parts(None, Arc::new(clock));
    assert_eq!(config.last_fetch_time_ms(), None);
}
