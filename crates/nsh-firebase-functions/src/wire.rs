//! The https.callable wire format: URLs, the request body, and the
//! response/error mapping, exactly as the firebase-js-sdk functions layer
//! speaks it (`@firebase/functions` 0.14.0 `dist/esm/index.esm.js`:
//! `_url`, `postJSON`, `callAtURL`, `_errorForResponse`, `errorCodeMap`)
//! and as the server side writes it (`firebase-functions` v1
//! `src/common/providers/https.ts`: `res.status(200).send({ result })`,
//! `res.status(status).send({ error: httpErr.toJSON() })`).
//! `packages/nsh-firebase/src/functions/wire.ts` mirrors this name for name.

use serde_json::Value as JsonValue;

use nsh_firebase_core::{EmulatorEndpoint, FirebaseError};

/// The region a callable answers under when none is configured (the
/// SDK's `DEFAULT_REGION`, also firebase-functions v1's default).
pub const DEFAULT_REGION: &str = "us-central1";

/// The sandbox suite's Functions emulator (emulator/firebase.json).
pub const EMULATOR_PORT: u16 = 15_001;

/// The native handler this crate routes through when a host answers.
pub const NATIVE_CALL: &str = "firebase.functions.call";

/// The documented callable error codes (`FunctionsErrorCode`), each
/// `functions/`-prefixed like the SDK's `FunctionsError` throws them.
pub const OK: &str = "functions/ok";
pub const CANCELLED: &str = "functions/cancelled";
pub const UNKNOWN: &str = "functions/unknown";
pub const INVALID_ARGUMENT: &str = "functions/invalid-argument";
pub const DEADLINE_EXCEEDED: &str = "functions/deadline-exceeded";
pub const NOT_FOUND: &str = "functions/not-found";
pub const ALREADY_EXISTS: &str = "functions/already-exists";
pub const PERMISSION_DENIED: &str = "functions/permission-denied";
pub const UNAUTHENTICATED: &str = "functions/unauthenticated";
pub const RESOURCE_EXHAUSTED: &str = "functions/resource-exhausted";
pub const FAILED_PRECONDITION: &str = "functions/failed-precondition";
pub const ABORTED: &str = "functions/aborted";
pub const OUT_OF_RANGE: &str = "functions/out-of-range";
pub const UNIMPLEMENTED: &str = "functions/unimplemented";
pub const INTERNAL: &str = "functions/internal";
pub const UNAVAILABLE: &str = "functions/unavailable";
pub const DATA_LOSS: &str = "functions/data-loss";

/// Where a call goes: the region, and the emulator switch that wins over
/// production (`{region}-{project}.cloudfunctions.net`).
pub struct FunctionsConfig {
    pub region: String,
    pub emulator: Option<EmulatorEndpoint>,
}

impl Default for FunctionsConfig {
    /// The sandbox default: the suite's Functions emulator on
    /// 127.0.0.1:15001 (a host override keeps the http scheme).
    fn default() -> Self {
        Self {
            region: DEFAULT_REGION.to_owned(),
            emulator: Some(EmulatorEndpoint::new(EMULATOR_PORT)),
        }
    }
}

/// The URL of one callable: the emulator answers at
/// `{origin}/{projectId}/{region}/{name}`, production at
/// `https://{region}-{projectId}.cloudfunctions.net/{name}` (the SDK's
/// `FunctionsService::_url`; custom domains are not configured here).
pub fn callable_url(config: &FunctionsConfig, project_id: &str, name: &str) -> String {
    match &config.emulator {
        Some(endpoint) => format!(
            "{}/{project_id}/{}/{name}",
            endpoint.base_url(),
            config.region
        ),
        None => format!(
            "https://{}-{project_id}.cloudfunctions.net/{name}",
            config.region
        ),
    }
}

/// The POST body: the caller's data under `data`, `null` when absent
/// (the SDK's `encode`: `data == null` encodes to `null`, so the field
/// is always present on the wire).
pub fn request_body(data: &JsonValue) -> String {
    JsonValue::Object(
        [("data".to_owned(), data.clone())]
            .into_iter()
            .collect::<serde_json::Map<String, JsonValue>>(),
    )
    .to_string()
}

/// A failed call. `code` is the SDK's `functions/<code>` (or a `core/*`
/// code when the failure came from the native channel), `message` carries
/// the backend's ` [<httpStatus>]` suffix like `_errorForResponse`
/// builds it, and `details` is the backend's optional `error.details`.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionsError {
    pub code: String,
    pub message: String,
    pub native_code: Option<String>,
    pub details: Option<JsonValue>,
}

impl FunctionsError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            native_code: None,
            details: None,
        }
    }

    pub fn to_firebase(&self) -> FirebaseError {
        FirebaseError {
            code: self.code.clone(),
            message: self.message.clone(),
            native_code: self.native_code.clone(),
        }
    }
}

impl From<FirebaseError> for FunctionsError {
    fn from(error: FirebaseError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            native_code: error.native_code,
            details: None,
        }
    }
}

impl From<FunctionsError> for FirebaseError {
    fn from(error: FunctionsError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            native_code: error.native_code,
        }
    }
}

impl std::fmt::Display for FunctionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.native_code {
            Some(native) => write!(f, "{} ({}): {}", self.code, native, self.message),
            None => write!(f, "{}: {}", self.code, self.message),
        }
    }
}

impl std::error::Error for FunctionsError {}

/// The canonical names the server writes into `error.status`
/// (`CanonicalErrorCodeName`), mapped to the SDK's client codes.
fn canonical_client_code(canonical: &str) -> Option<&'static str> {
    Some(match canonical {
        "OK" => "ok",
        "CANCELLED" => "cancelled",
        "UNKNOWN" => "unknown",
        "INVALID_ARGUMENT" => "invalid-argument",
        "DEADLINE_EXCEEDED" => "deadline-exceeded",
        "NOT_FOUND" => "not-found",
        "ALREADY_EXISTS" => "already-exists",
        "PERMISSION_DENIED" => "permission-denied",
        "UNAUTHENTICATED" => "unauthenticated",
        "RESOURCE_EXHAUSTED" => "resource-exhausted",
        "FAILED_PRECONDITION" => "failed-precondition",
        "ABORTED" => "aborted",
        "OUT_OF_RANGE" => "out-of-range",
        "UNIMPLEMENTED" => "unimplemented",
        "INTERNAL" => "internal",
        "UNAVAILABLE" => "unavailable",
        "DATA_LOSS" => "data-loss",
        _ => return None,
    })
}

/// The HTTP status fallback (`codeForHTTPStatus`, google/rpc/code.proto).
fn client_code_for_status(status: u16) -> &'static str {
    match status {
        200..=299 => "ok",
        0 | 500 => "internal",
        400 => "invalid-argument",
        401 => "unauthenticated",
        403 => "permission-denied",
        404 => "not-found",
        409 => "aborted",
        429 => "resource-exhausted",
        499 => "cancelled",
        501 => "unimplemented",
        503 => "unavailable",
        504 => "deadline-exceeded",
        _ => "unknown",
    }
}

/// The error a response carries, if any (the SDK's `_errorForResponse`):
/// the HTTP status gives the defaults, then a JSON body
/// `{error: {status, message, details}}` overrides them — an unknown
/// canonical `status` is itself an internal error. `ok` means success.
pub fn error_for_response(status: u16, body: Option<&JsonValue>) -> Option<FunctionsError> {
    let mut code = client_code_for_status(status);
    let mut description = code.to_owned();
    let mut details = None;
    if let Some(error) = body.and_then(|body| body.get("error")) {
        if let Some(canonical) = error.get("status").and_then(|status| status.as_str()) {
            // An unknown canonical name in the body must not fall through
            // to the status-code default: the SDK fails it as internal.
            let Some(client_code) = canonical_client_code(canonical) else {
                return Some(FunctionsError::new(
                    INTERNAL,
                    format!("Unknown backend error status: {canonical} [{status}]"),
                ));
            };
            code = client_code;
            description = format!("Backend error status: {canonical}");
        }
        if let Some(message) = error.get("message").and_then(|message| message.as_str()) {
            description = message.to_owned();
        }
        // Attached whenever the key exists, JSON null included: the SDK
        // checks `details !== undefined`, not truthiness.
        if let Some(found) = error.get("details") {
            details = Some(found.clone());
        }
    }
    if code == "ok" {
        return None;
    }
    let prefixed = format!("functions/{code}");
    Some(FunctionsError {
        code: prefixed,
        message: format!("{description} [{status}]"),
        native_code: None,
        details,
    })
}

/// The result of one callable response: 2xx reads `data`, then the
/// legacy `result` key (the SDK's `callAtURL`); a body that is not JSON
/// at all — or a falsy JSON literal, which the SDK's `!response.json`
/// treats the same — is an internal error, as is a body without either
/// key.
pub fn result_from_response(status: u16, text: &str) -> Result<JsonValue, FunctionsError> {
    let parsed: Option<JsonValue> = serde_json::from_str(text).ok();
    if let Some(error) = error_for_response(status, parsed.as_ref()) {
        return Err(error);
    }
    // JS truthiness of the parsed body: null, false, 0 and "" are falsy,
    // everything else (arrays and objects included) is truthy.
    let Some(body) = parsed.filter(|body| match body {
        JsonValue::Null => false,
        JsonValue::Bool(false) => false,
        JsonValue::Number(number) => number.as_f64() == Some(0.0),
        JsonValue::String(text) => text.is_empty(),
        _ => true,
    }) else {
        return Err(FunctionsError::new(
            INTERNAL,
            "Response is not valid JSON object.",
        ));
    };
    match body.get("data") {
        Some(data) => Ok(data.clone()),
        None => match body.get("result") {
            Some(result) => Ok(result.clone()),
            None => Err(FunctionsError::new(
                INTERNAL,
                "Response is missing data field.",
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(emulator: bool) -> FunctionsConfig {
        FunctionsConfig {
            region: DEFAULT_REGION.to_owned(),
            emulator: emulator.then(|| EmulatorEndpoint::new(EMULATOR_PORT)),
        }
    }

    #[test]
    fn the_emulator_url_is_origin_project_region_name() {
        assert_eq!(
            callable_url(&config(true), "demo-nsh", "ping"),
            "http://127.0.0.1:15001/demo-nsh/us-central1/ping"
        );
    }

    #[test]
    fn a_host_override_keeps_the_project_region_name_path() {
        let with_host = FunctionsConfig {
            emulator: Some(EmulatorEndpoint {
                host: Some("10.0.0.2".to_owned()),
                port: 5001,
            }),
            ..config(true)
        };
        assert_eq!(
            callable_url(&with_host, "demo-nsh", "add"),
            "http://10.0.0.2:5001/demo-nsh/us-central1/add"
        );
    }

    #[test]
    fn production_builds_the_region_prefixed_host() {
        assert_eq!(
            callable_url(&config(false), "my-proj", "add"),
            "https://us-central1-my-proj.cloudfunctions.net/add"
        );
    }

    #[test]
    fn the_body_always_carries_data_null_when_absent() {
        assert_eq!(request_body(&JsonValue::Null), r#"{"data":null}"#);
        assert_eq!(
            request_body(&json!({"a": 1, "b": 2})),
            r#"{"data":{"a":1,"b":2}}"#
        );
    }

    #[test]
    fn a_result_answer_maps_and_the_legacy_data_key_wins_first() {
        let result = result_from_response(200, r#"{"result":{"sum":3}}"#).unwrap();
        assert_eq!(result, json!({"sum": 3}));
        let data = result_from_response(200, r#"{"data":{"sum":7}}"#).unwrap();
        assert_eq!(data, json!({"sum": 7}));
        // data read first, like the SDK's callAtURL.
        let first = result_from_response(200, r#"{"data":1,"result":2}"#).unwrap();
        assert_eq!(first, json!(1));
    }

    #[test]
    fn a_2xx_answer_without_data_or_result_is_internal() {
        let error = result_from_response(200, r#"{"other":1}"#).unwrap_err();
        assert_eq!(error.code, INTERNAL);
        assert_eq!(error.message, "Response is missing data field.");
    }

    #[test]
    fn a_2xx_body_that_is_not_json_is_internal() {
        let error = result_from_response(200, "<html>hi</html>").unwrap_err();
        assert_eq!(error.code, INTERNAL);
        assert_eq!(error.message, "Response is not valid JSON object.");
    }

    #[test]
    fn a_backend_error_maps_to_its_functions_code() {
        let body =
            json!({"error": {"status": "INVALID_ARGUMENT", "message": "a and b must be numbers"}});
        let error = error_for_response(400, Some(&body)).unwrap();
        assert_eq!(error.code, INVALID_ARGUMENT);
        assert_eq!(error.message, "a and b must be numbers [400]");
        assert_eq!(error.details, None);
    }

    #[test]
    fn details_reach_the_error_whenever_the_key_exists() {
        let body = json!({"error": {"status": "FAILED_PRECONDITION", "message": "no", "details": {"retry": true}}});
        let error = error_for_response(400, Some(&body)).unwrap();
        assert_eq!(error.code, FAILED_PRECONDITION);
        assert_eq!(error.details, Some(json!({"retry": true})));

        // A present null is attached, like the SDK's `!== undefined` check.
        let null_details =
            json!({"error": {"status": "INTERNAL", "message": "x", "details": null}});
        assert_eq!(
            error_for_response(500, Some(&null_details))
                .unwrap()
                .details,
            Some(JsonValue::Null)
        );
    }

    #[test]
    fn an_unknown_canonical_status_is_internal_not_the_status_default() {
        let body = json!({"error": {"status": "WEIRD", "message": "m"}});
        let error = error_for_response(400, Some(&body)).unwrap();
        assert_eq!(error.code, INTERNAL);
        assert_eq!(error.message, "Unknown backend error status: WEIRD [400]");
    }

    #[test]
    fn a_status_without_a_body_still_names_the_code() {
        let error = error_for_response(404, None).unwrap();
        assert_eq!(error.code, NOT_FOUND);
        assert_eq!(error.message, "not-found [404]");
        let internal = error_for_response(500, None).unwrap();
        assert_eq!(internal.code, INTERNAL);
    }

    #[test]
    fn a_success_status_is_no_error_even_with_a_body() {
        assert_eq!(error_for_response(200, None), None);
        let body = json!({"error": {"status": "OK", "message": "fine"}});
        assert_eq!(error_for_response(200, Some(&body)), None);
    }

    #[test]
    fn status_fallbacks_match_the_sdk_table() {
        for (status, code) in [
            (0u16, "internal"),
            (400, "invalid-argument"),
            (401, "unauthenticated"),
            (403, "permission-denied"),
            (404, "not-found"),
            (409, "aborted"),
            (429, "resource-exhausted"),
            (499, "cancelled"),
            (500, "internal"),
            (501, "unimplemented"),
            (503, "unavailable"),
            (504, "deadline-exceeded"),
            (418, "unknown"),
        ] {
            let error = error_for_response(status, None).unwrap();
            assert_eq!(error.code, format!("functions/{code}"), "status {status}");
        }
    }

    #[test]
    fn firebase_errors_round_trip_through_functions_errors() {
        let refused = FunctionsError::from(FirebaseError::from_host_code(
            "ERR_ACCESS_DENIED",
            "not granted",
        ));
        assert_eq!(refused.code, "core/access-denied");
        assert_eq!(refused.native_code.as_deref(), Some("ERR_ACCESS_DENIED"));
        let back: FirebaseError = refused.clone().into();
        assert_eq!(back.code, "core/access-denied");
        assert_eq!(
            refused.to_string(),
            "core/access-denied (ERR_ACCESS_DENIED): not granted"
        );
    }
}
