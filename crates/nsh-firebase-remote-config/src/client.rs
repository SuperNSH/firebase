//! The remote-config clients. [`RemoteConfig`] is the pure tier:
//! defaults, typed reads, template activation through a caller-supplied
//! [`Transport`], and an honest refusal to fetch without one.
//! [`NativeRemoteConfig`] calls the same work through the
//! `firebase.remote-config.*` handlers an embedding host registers. The
//! TypeScript package `packages/nsh-firebase/src/remote-config` mirrors
//! this behavior name for name.

use std::sync::{Arc, Mutex};

use serde_json::Value as JsonValue;

use nsh_firebase_core::{FirebaseError, FirebaseHost, error::INVALID_ARGUMENT};

use crate::{
    clock::{Clock, SystemClock},
    codes::UNAVAILABLE,
    template::parse_template,
    transport::Transport,
};

/// The `firebase.remote-config.*` handler names the native areas
/// register; [`NativeRemoteConfig`] calls exactly these.
pub const NATIVE_SET_DEFAULTS: &str = "firebase.remote-config.setDefaults";
pub const NATIVE_GET_DEFAULTS: &str = "firebase.remote-config.getDefaults";
pub const NATIVE_FETCH_AND_ACTIVATE: &str = "firebase.remote-config.fetchAndActivate";
pub const NATIVE_GET: &str = "firebase.remote-config.get";

/// The fetch statuses, RNFB's `LastFetchStatus` spellings.
pub const NO_FETCH_YET: &str = "no_fetch_yet";
pub const FETCH_SUCCESS: &str = "success";
pub const FETCH_FAILURE: &str = "failure";

/// The truthy spellings the web SDK's `asBoolean` accepts, lowercased.
const SDK_BOOL_TRUE: [&str; 6] = ["1", "true", "t", "yes", "y", "on"];

/// What `fetch_and_activate` resolves to; `last_fetch_status` mirrors
/// the client's own status after the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchAndActivateResult {
    pub activated: bool,
    pub last_fetch_status: String,
}

/// The pure tier's client. Reads resolve the activated template values
/// first, then the app defaults; anything else is unset. State is
/// in-memory only — nothing here persists across a process restart.
pub struct RemoteConfig {
    transport: Option<Arc<dyn Transport>>,
    clock: Arc<dyn Clock>,
    state: Mutex<ConfigState>,
}

#[derive(Default)]
struct ConfigState {
    defaults: std::collections::BTreeMap<String, JsonValue>,
    active: std::collections::BTreeMap<String, String>,
    last_fetch_status: String,
    last_fetch_time_ms: Option<u64>,
}

impl ConfigState {
    fn new() -> Self {
        Self {
            last_fetch_status: NO_FETCH_YET.to_owned(),
            ..Default::default()
        }
    }

    fn record_failure(&mut self, now_ms: u64) {
        self.last_fetch_status = FETCH_FAILURE.to_owned();
        self.last_fetch_time_ms = Some(now_ms);
    }
}

/// Rejects anything but a flat map of string, number or boolean — RNFB's
/// `ConfigDefaults` shape — before a value is stored or sent.
fn validate_defaults(defaults: &JsonValue) -> Result<(), FirebaseError> {
    let object = defaults
        .as_object()
        .ok_or_else(|| FirebaseError::new(INVALID_ARGUMENT, "defaults is not an object"))?;
    for (key, value) in object {
        if !value.is_string() && !value.is_number() && !value.is_boolean() {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                format!("defaults.{key} is not a string, number or boolean"),
            ));
        }
    }
    Ok(())
}

/// The string form of a stored default (`get` resolves to strings, as
/// the template's values are strings).
fn value_string(value: &JsonValue) -> String {
    match value {
        JsonValue::String(text) => text.clone(),
        JsonValue::Bool(flag) => flag.to_string(),
        JsonValue::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

impl RemoteConfig {
    /// A client that reads defaults only: `fetch_and_activate` refuses.
    pub fn new() -> Self {
        Self::with_parts(None, Arc::new(SystemClock))
    }

    /// A client whose fetches go through `transport`.
    pub fn with_transport(transport: Arc<dyn Transport>) -> Self {
        Self::with_parts(Some(transport), Arc::new(SystemClock))
    }

    /// The client tests drive: any transport and clock.
    pub fn with_parts(transport: Option<Arc<dyn Transport>>, clock: Arc<dyn Clock>) -> Self {
        Self {
            transport,
            clock,
            state: Mutex::new(ConfigState::new()),
        }
    }

    /// Replaces the whole defaults map (the JS SDK's `setDefaults`
    /// semantics: later calls do not merge).
    pub fn set_defaults(&self, defaults: &JsonValue) -> Result<(), FirebaseError> {
        validate_defaults(defaults)?;
        let object = defaults
            .as_object()
            .expect("validated defaults are an object");
        self.lock().defaults = object
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        Ok(())
    }

    /// The defaults as set (string, number and boolean values kept).
    pub fn get_defaults(&self) -> JsonValue {
        JsonValue::Object(self.lock().defaults.clone().into_iter().collect())
    }

    /// The activated template value, else the default's string form,
    /// else nothing.
    pub fn get(&self, key: &str) -> Option<String> {
        let state = self.lock();
        if let Some(active) = state.active.get(key) {
            return Some(active.clone());
        }
        state.defaults.get(key).map(value_string)
    }

    pub fn get_string(&self, key: &str) -> String {
        self.get(key).unwrap_or_default()
    }

    pub fn get_boolean(&self, key: &str) -> bool {
        match self.get(key) {
            Some(value) => SDK_BOOL_TRUE.contains(&value.to_lowercase().as_str()),
            None => false,
        }
    }

    pub fn get_number(&self, key: &str) -> f64 {
        match self.get(key) {
            Some(value) => value
                .parse::<f64>()
                .ok()
                .filter(|parsed| parsed.is_finite())
                .unwrap_or(0.0),
            None => 0.0,
        }
    }

    pub fn last_fetch_status(&self) -> String {
        self.lock().last_fetch_status.clone()
    }

    /// The clock's ms at the last fetch attempt, success or failure.
    pub fn last_fetch_time_ms(&self) -> Option<u64> {
        self.lock().last_fetch_time_ms
    }

    /// Fetches through the [`Transport`], validates the document and
    /// activates its resolved values. Without a transport this refuses
    /// with `remote-config/unavailable`: no fake fetch is attempted,
    /// because the REST API is OAuth-token-only and the emulator suite
    /// has no Remote Config service.
    pub async fn fetch_and_activate(&self) -> Result<FetchAndActivateResult, FirebaseError> {
        let Some(transport) = self.transport.as_ref() else {
            return Err(FirebaseError::new(
                UNAVAILABLE,
                "Remote Config fetch needs an OAuth 2.0 access token from a service account \
                 (scope https://www.googleapis.com/auth/firebase.remoteconfig); the web API key \
                 cannot authorize firebaseremoteconfig.googleapis.com and the emulator suite has \
                 no Remote Config service, so nothing was fetched — supply a Transport",
            ));
        };
        let now_ms = self.clock.now_ms();
        let document = match transport.fetch_template().await {
            Ok(document) => document,
            Err(error) => {
                self.lock().record_failure(now_ms);
                return Err(error);
            }
        };
        let template = match parse_template(&document) {
            Ok(template) => template,
            Err(error) => {
                self.lock().record_failure(now_ms);
                return Err(error);
            }
        };
        let values = template.resolved_values();
        {
            let mut state = self.lock();
            state.active = values;
            state.last_fetch_status = FETCH_SUCCESS.to_owned();
            state.last_fetch_time_ms = Some(now_ms);
        }
        Ok(FetchAndActivateResult {
            activated: true,
            last_fetch_status: FETCH_SUCCESS.to_owned(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ConfigState> {
        self.state.lock().expect("remote-config state lock")
    }
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self::new()
    }
}

fn host_error(handler: &str, problem: impl Into<String>) -> FirebaseError {
    FirebaseError::new(
        nsh_firebase_core::error::HOST_ERROR,
        format!("the {handler} handler returned {problem}"),
    )
}

/// The native tier's client: validated arguments, then a host call per
/// the fixed contract. The typed reads coerce the wire value locally,
/// as the native SDKs do; the fetch status is the local echo of the
/// last result the host returned.
pub struct NativeRemoteConfig {
    host: Arc<dyn FirebaseHost>,
    last_fetch_status: Mutex<String>,
}

impl NativeRemoteConfig {
    pub fn new(host: Arc<dyn FirebaseHost>) -> Self {
        Self {
            host,
            last_fetch_status: Mutex::new(NO_FETCH_YET.to_owned()),
        }
    }

    pub async fn set_defaults(&self, defaults: &JsonValue) -> Result<(), FirebaseError> {
        validate_defaults(defaults)?;
        let args = serde_json::json!({ "map": defaults });
        self.host.call(NATIVE_SET_DEFAULTS, args).await?;
        Ok(())
    }

    pub async fn get_defaults(&self) -> Result<JsonValue, FirebaseError> {
        let result = self.host.call(NATIVE_GET_DEFAULTS, JsonValue::Null).await?;
        match result.get("map") {
            Some(map) if map.is_object() => Ok(map.clone()),
            _ => Err(host_error(NATIVE_GET_DEFAULTS, "no `map` object")),
        }
    }

    pub async fn fetch_and_activate(&self) -> Result<FetchAndActivateResult, FirebaseError> {
        let result = self
            .host
            .call(NATIVE_FETCH_AND_ACTIVATE, JsonValue::Null)
            .await?;
        let activated = result
            .get("activated")
            .and_then(JsonValue::as_bool)
            .ok_or_else(|| host_error(NATIVE_FETCH_AND_ACTIVATE, "no `activated` boolean"))?;
        let last_fetch_status = result
            .get("lastFetchStatus")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| host_error(NATIVE_FETCH_AND_ACTIVATE, "no `lastFetchStatus` string"))?
            .to_owned();
        let echo = last_fetch_status.clone();
        *self.last_fetch_status.lock().expect("status lock") = echo;
        Ok(FetchAndActivateResult {
            activated,
            last_fetch_status,
        })
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>, FirebaseError> {
        if key.is_empty() {
            return Err(FirebaseError::new(INVALID_ARGUMENT, "the key is empty"));
        }
        let args = serde_json::json!({ "key": key });
        let result = self.host.call(NATIVE_GET, args).await?;
        match result.get("value") {
            Some(JsonValue::String(value)) => Ok(Some(value.clone())),
            Some(JsonValue::Null) | None => Ok(None),
            Some(_) => Err(host_error(NATIVE_GET, "a non-string `value`")),
        }
    }

    pub async fn get_string(&self, key: &str) -> Result<String, FirebaseError> {
        Ok(self.get(key).await?.unwrap_or_default())
    }

    pub async fn get_boolean(&self, key: &str) -> Result<bool, FirebaseError> {
        Ok(match self.get(key).await? {
            Some(value) => SDK_BOOL_TRUE.contains(&value.to_lowercase().as_str()),
            None => false,
        })
    }

    pub async fn get_number(&self, key: &str) -> Result<f64, FirebaseError> {
        Ok(match self.get(key).await? {
            Some(value) => value
                .parse::<f64>()
                .ok()
                .filter(|parsed| parsed.is_finite())
                .unwrap_or(0.0),
            None => 0.0,
        })
    }

    /// The local echo of the last `fetch_and_activate` result.
    pub fn last_fetch_status(&self) -> String {
        self.last_fetch_status.lock().expect("status lock").clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use nsh_firebase_core::error::HOST_ERROR;
    use serde_json::json;
    use std::{collections::BTreeMap, future::Future, pin::Pin};

    fn pure() -> RemoteConfig {
        RemoteConfig::with_parts(None, Arc::new(ManualClock::new(1_000)))
    }

    #[test]
    fn defaults_round_trip_and_replace() {
        let config = pure();
        config
            .set_defaults(&json!({"welcome_message": "hello", "tip_amount": 10, "dark": false}))
            .unwrap();
        assert_eq!(
            config.get_defaults(),
            json!({"welcome_message": "hello", "tip_amount": 10, "dark": false})
        );
        config.set_defaults(&json!({"only": "this"})).unwrap();
        assert_eq!(config.get_defaults(), json!({"only": "this"}));
    }

    #[test]
    fn invalid_defaults_are_refused_before_any_state_changes() {
        let config = pure();
        config.set_defaults(&json!({"ok": "yes"})).unwrap();
        for bad in [
            json!("nope"),
            json!([1]),
            json!({"nested": {}}),
            json!({"none": null}),
        ] {
            let error = config.set_defaults(&bad).unwrap_err();
            assert_eq!(error.code, INVALID_ARGUMENT, "for {bad}");
        }
        assert_eq!(config.get_defaults(), json!({"ok": "yes"}));
    }

    #[test]
    fn typed_reads_coerce_the_web_sdk_way() {
        let config = pure();
        config
            .set_defaults(&json!({
                "truthy": "TRUE",
                "truthy_on": "on",
                "falsy": "0",
                "numeric": "3.5",
                "not_a_number": "abc",
                "nan_like": "NaN",
            }))
            .unwrap();
        assert_eq!(config.get_string("truthy"), "TRUE");
        assert!(config.get_boolean("truthy"));
        assert!(config.get_boolean("truthy_on"));
        assert!(!config.get_boolean("falsy"));
        assert_eq!(config.get_number("numeric"), 3.5);
        assert_eq!(config.get_number("not_a_number"), 0.0);
        assert_eq!(config.get_number("nan_like"), 0.0);
        assert_eq!(config.get_string("missing"), "");
        assert!(!config.get_boolean("missing"));
        assert_eq!(config.get_number("missing"), 0.0);
        assert_eq!(config.get("missing"), None);
    }

    fn template_transport(document: JsonValue) -> Arc<dyn Transport> {
        Arc::new(StaticTransport { document })
    }

    struct StaticTransport {
        document: JsonValue,
    }

    impl Transport for StaticTransport {
        fn fetch_template<'a>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + 'a>> {
            Box::pin(async move { Ok(self.document.clone()) })
        }
    }

    fn realistic_document() -> JsonValue {
        serde_json::json!({
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
        })
    }

    #[test]
    fn a_realistic_template_parses_and_resolves_defaults_only() {
        let template = parse_template(&realistic_document()).unwrap();
        assert_eq!(template.conditions.len(), 2);
        assert_eq!(template.conditions[0].name, "ios_users");
        assert_eq!(template.conditions[0].tag_color.as_deref(), Some("BLUE"));
        assert_eq!(template.parameters.len(), 6);
        assert_eq!(
            template.parameters["welcome_message"].conditional_values["ios_users"],
            "Welcome, iOS user"
        );
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
            ]),
            "conditions are not evaluated, useInAppDefault and dynamic values stay unset"
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
    async fn activation_overrides_defaults_and_conditions_stay_unevaluated() {
        let clock = Arc::new(ManualClock::new(1_000));
        let config = RemoteConfig::with_parts(
            Some(template_transport(realistic_document())),
            Arc::clone(&clock) as Arc<dyn Clock>,
        );
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
        assert_eq!(config.last_fetch_time_ms(), Some(1_000));
    }

    #[tokio::test]
    async fn fetch_without_a_transport_rejects_with_the_oauth_code() {
        let config = pure();
        let error = config.fetch_and_activate().await.unwrap_err();
        assert_eq!(error.code, UNAVAILABLE);
        assert!(error.message.contains("OAuth 2.0"), "names the requirement");
        assert!(
            error
                .message
                .contains("firebaseremoteconfig.googleapis.com")
        );
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
                    nsh_firebase_core::error::NETWORK,
                    "HTTP 401: the backend refused the fake project",
                ))
            })
        }
    }

    #[tokio::test]
    async fn a_refusing_backend_records_failure_and_rejects() {
        let clock = Arc::new(ManualClock::new(500));
        let config = RemoteConfig::with_parts(
            Some(Arc::new(RefusingTransport)),
            Arc::clone(&clock) as Arc<dyn Clock>,
        );
        let error = config.fetch_and_activate().await.unwrap_err();
        assert_eq!(error.code, nsh_firebase_core::error::NETWORK);
        assert_eq!(config.last_fetch_status(), FETCH_FAILURE);
        assert_eq!(config.last_fetch_time_ms(), Some(500));
    }

    #[tokio::test]
    async fn an_invalid_template_records_failure_and_names_itself() {
        let clock = Arc::new(ManualClock::new(500));
        let document = json!({"conditions": [{"name": "dup", "expression": "x"}, {"name": "dup", "expression": "y"}]});
        let config = RemoteConfig::with_parts(
            Some(template_transport(document)),
            Arc::clone(&clock) as Arc<dyn Clock>,
        );
        let error = config.fetch_and_activate().await.unwrap_err();
        assert_eq!(error.code, crate::codes::INVALID_TEMPLATE);
        assert!(error.message.contains("duplicate"));
        assert_eq!(config.last_fetch_status(), FETCH_FAILURE);
        assert!(config.get("anything").is_none());
    }

    #[test]
    fn the_manual_clock_never_moves_by_itself() {
        let clock = ManualClock::new(1_000);
        clock.advance(4_000);
        assert_eq!(clock.now_ms(), 5_000);
        let config = RemoteConfig::with_parts(None, Arc::new(clock));
        assert_eq!(config.last_fetch_time_ms(), None);
    }

    fn host_with_answers(
        answers: BTreeMap<String, JsonValue>,
    ) -> (NativeRemoteConfig, Arc<RecordingHost>) {
        let host = Arc::new(RecordingHost {
            answers,
            ..RecordingHost::default()
        });
        let native = NativeRemoteConfig::new(Arc::clone(&host) as Arc<dyn FirebaseHost>);
        (native, host)
    }

    /// Records every call with its args, answering from `answers`.
    #[derive(Default)]
    struct RecordingHost {
        answers: BTreeMap<String, JsonValue>,
        calls: std::sync::Mutex<Vec<(String, JsonValue)>>,
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

    #[tokio::test]
    async fn native_calls_carry_the_contract_wire_shapes() {
        let (native, host) = host_with_answers(BTreeMap::from([
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
            ]
        );
    }

    #[tokio::test]
    async fn native_typed_reads_coerce_the_wire_value() {
        let (native, _) = host_with_answers(BTreeMap::from([(
            NATIVE_GET.to_owned(),
            json!({"value": "TRUE"}),
        )]));
        assert!(native.get_boolean("anything").await.unwrap());
        assert_eq!(native.get_string("anything").await.unwrap(), "TRUE");
        assert_eq!(native.get_number("anything").await.unwrap(), 0.0);
    }

    #[tokio::test]
    async fn native_validation_refuses_before_any_call() {
        let (native, host) = host_with_answers(BTreeMap::new());
        let error = native.set_defaults(&json!("nope")).await.unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
        let error = native.get("").await.unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
        assert!(host.calls.lock().expect("recording host lock").is_empty());
    }

    #[tokio::test]
    async fn a_malformed_native_answer_is_a_host_error() {
        let (native, _) = host_with_answers(BTreeMap::from([
            (NATIVE_GET_DEFAULTS.to_owned(), json!({"noMap": true})),
            (NATIVE_GET.to_owned(), json!({"value": 7})),
        ]));
        let error = native.get_defaults().await.unwrap_err();
        assert_eq!(error.code, HOST_ERROR);
        let error = native.get("key").await.unwrap_err();
        assert_eq!(error.code, HOST_ERROR);
    }
}
