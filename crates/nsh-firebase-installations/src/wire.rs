//! The Installations REST wire shapes (docs/research.md §4, verified
//! against firebase-js-sdk `packages/installations`): endpoints, request
//! bodies, headers, reply parsing, and the `firebase.installations.*`
//! handler names the native tier registers.

use nsh_core::platform::HttpRequest;
use nsh_firebase_core::FirebaseError;
use serde_json::{Value as JsonValue, json};

use crate::error::{INVALID_RESPONSE, MISSING_APP_CONFIG};
use crate::store::StoredInstallation;

/// The API base the js-SDK's `INSTALLATIONS_API_URL` pins; no emulator
/// serves this product, so there is no base-URL override.
pub const INSTALLATIONS_API_URL: &str = "https://firebaseinstallations.googleapis.com/v1";
/// The `authVersion` of the register body and the `Authorization` scheme
/// of the token call (the js-SDK's `INTERNAL_AUTH_VERSION`).
pub const INTERNAL_AUTH_VERSION: &str = "FIS_v2";
/// The `sdkVersion` the register and token bodies carry; keep it equal to
/// the TypeScript side's (packages/nsh-firebase/src/installations/wire.ts),
/// both wire tests pin the exact value.
pub const SDK_VERSION: &str = concat!("w:", env!("CARGO_PKG_VERSION"));

/// The native tier's handlers (the contract; declare
/// `firebase.installations.*` in `capabilities.host`).
pub const GET_ID: &str = "firebase.installations.getId";
pub const GET_TOKEN: &str = "firebase.installations.getToken";

/// One auth token as a reply carried it: the bearer token and its
/// lifetime in milliseconds (the reply's `expiresIn` is seconds with an
/// `s` suffix, `"604800s"`).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AuthToken {
    pub token: String,
    pub expires_in_ms: u64,
}

/// `projects/{projectId}/installations`, the register endpoint and the
/// prefix of the token endpoint.
pub fn endpoint(project_id: &str) -> String {
    format!("{INSTALLATIONS_API_URL}/projects/{project_id}/installations")
}

/// The headers every Installations call carries (the js-SDK's
/// `getHeaders`).
fn base_headers(api_key: &str) -> Vec<(String, String)> {
    vec![
        ("Content-Type".to_owned(), "application/json".to_owned()),
        ("Accept".to_owned(), "application/json".to_owned()),
        ("x-goog-api-key".to_owned(), api_key.to_owned()),
    ]
}

/// `POST projects/{projectId}/installations` registering `fid`
/// (the js-SDK's `createInstallationRequest`). The body's field names are
/// the server's: `fid`, `authVersion`, `appId`, `sdkVersion`.
pub fn register_request(
    project_id: &str,
    api_key: &str,
    app_id: &str,
    fid: &str,
) -> Result<HttpRequest, FirebaseError> {
    require_config(api_key, app_id)?;
    let body = json!({
        "fid": fid,
        "authVersion": INTERNAL_AUTH_VERSION,
        "appId": app_id,
        "sdkVersion": SDK_VERSION,
    });
    Ok(request(endpoint(project_id), api_key, body))
}

/// `POST projects/{projectId}/installations/{fid}/authTokens:generate`
/// minting an auth token for the registered `fid` (the js-SDK's
/// `generateAuthTokenRequest`). The refresh token authenticates the call
/// as `FIS_v2 {refreshToken}`.
pub fn token_request(
    project_id: &str,
    api_key: &str,
    app_id: &str,
    fid: &str,
    refresh_token: &str,
) -> Result<HttpRequest, FirebaseError> {
    require_config(api_key, app_id)?;
    let body = json!({
        "installation": {
            "sdkVersion": SDK_VERSION,
            "appId": app_id,
        }
    });
    let mut request = request(
        format!("{}/{fid}/authTokens:generate", endpoint(project_id)),
        api_key,
        body,
    );
    request.headers.push((
        "Authorization".to_owned(),
        format!("{INTERNAL_AUTH_VERSION} {refresh_token}"),
    ));
    Ok(request)
}

fn request(url: String, api_key: &str, body: JsonValue) -> HttpRequest {
    let mut request = HttpRequest::post(url, body.to_string().into_bytes());
    request.headers = base_headers(api_key);
    request
}

fn require_config(api_key: &str, app_id: &str) -> Result<(), FirebaseError> {
    if api_key.is_empty() || app_id.is_empty() {
        return Err(FirebaseError::new(
            MISSING_APP_CONFIG,
            "the app options need a non-empty apiKey and appId",
        ));
    }
    Ok(())
}

/// Reads a success reply of the register call: `{name?, fid?,
/// refreshToken, authToken: {token, expiresIn}}`. The reply may omit the
/// `fid`; the one the client generated stands in then, as the js-SDK's
/// `responseValue.fid || fid` does.
pub(crate) fn parse_register_reply(
    body: &str,
    requested_fid: &str,
) -> Result<(StoredInstallation, AuthToken), FirebaseError> {
    let reply: JsonValue = serde_json::from_str(body)
        .map_err(|_| FirebaseError::new(INVALID_RESPONSE, "the reply is not JSON"))?;
    let refresh_token = reply
        .get("refreshToken")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| missing("refreshToken"))?;
    let auth_token_value = reply.get("authToken").ok_or_else(|| missing("authToken"))?;
    let auth_token = parse_auth_token(auth_token_value)?;
    let fid = reply
        .get("fid")
        .and_then(JsonValue::as_str)
        .unwrap_or(requested_fid);
    Ok((
        StoredInstallation {
            fid: fid.to_owned(),
            refresh_token: refresh_token.to_owned(),
        },
        auth_token,
    ))
}

/// Reads a success reply of the token call: `{token, expiresIn}`.
pub(crate) fn parse_token_reply(body: &str) -> Result<AuthToken, FirebaseError> {
    let reply: JsonValue = serde_json::from_str(body)
        .map_err(|_| FirebaseError::new(INVALID_RESPONSE, "the reply is not JSON"))?;
    parse_auth_token(&reply)
}

fn parse_auth_token(value: &JsonValue) -> Result<AuthToken, FirebaseError> {
    let token = value
        .get("token")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| missing("authToken.token"))?;
    let text = value
        .get("expiresIn")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| missing("authToken.expiresIn (seconds with an s suffix)"))?;
    let expires_in_ms = text
        .strip_suffix('s')
        .and_then(|secs| secs.parse::<u64>().ok())
        .map(|secs| secs.saturating_mul(1000))
        .ok_or_else(|| missing("authToken.expiresIn (seconds with an s suffix)"))?;
    Ok(AuthToken {
        token: token.to_owned(),
        expires_in_ms,
    })
}

fn missing(field: &str) -> FirebaseError {
    FirebaseError::new(INVALID_RESPONSE, format!("the reply has no usable {field}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = "demo-nsh";
    const API_KEY: &str = "demo-nsh-fake-api-key";
    const APP_ID: &str = "1:000000000000:web:demo-nsh";
    const FID: &str = "c0123456789abcdef-_xyz";

    fn header<'a>(request: &'a HttpRequest, name: &str) -> &'a str {
        request
            .headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
            .unwrap_or("")
    }

    fn body_text(request: &HttpRequest) -> String {
        match &request.body {
            nsh_core::platform::HttpBody::Bytes(bytes) => {
                String::from_utf8_lossy(bytes).into_owned()
            }
            _ => String::new(),
        }
    }

    #[test]
    fn the_register_request_carries_the_body_and_headers_the_service_takes() {
        let request = register_request(PROJECT, API_KEY, APP_ID, FID).unwrap();
        assert_eq!(
            request.url,
            "https://firebaseinstallations.googleapis.com/v1/projects/demo-nsh/installations"
        );
        assert_eq!(request.method, "POST");
        assert_eq!(header(&request, "x-goog-api-key"), API_KEY);
        assert_eq!(header(&request, "Content-Type"), "application/json");
        assert_eq!(header(&request, "Accept"), "application/json");
        let body: JsonValue = serde_json::from_str(&body_text(&request)).unwrap();
        assert_eq!(
            body,
            json!({
                "fid": FID,
                "authVersion": "FIS_v2",
                "appId": APP_ID,
                "sdkVersion": "w:0.1.0",
            })
        );
    }

    #[test]
    fn the_token_request_authenticates_with_the_fis_scheme() {
        let request = token_request(PROJECT, API_KEY, APP_ID, FID, "refresh-1").unwrap();
        assert_eq!(
            request.url,
            "https://firebaseinstallations.googleapis.com/v1/projects/demo-nsh/installations/c0123456789abcdef-_xyz/authTokens:generate"
        );
        assert_eq!(header(&request, "Authorization"), "FIS_v2 refresh-1");
        assert_eq!(header(&request, "x-goog-api-key"), API_KEY);
        let body: JsonValue = serde_json::from_str(&body_text(&request)).unwrap();
        assert_eq!(
            body,
            json!({ "installation": { "sdkVersion": "w:0.1.0", "appId": APP_ID } })
        );
    }

    #[test]
    fn an_empty_api_key_or_app_id_is_refused_before_any_request() {
        let error = register_request(PROJECT, "", APP_ID, FID).unwrap_err();
        assert_eq!(error.code, MISSING_APP_CONFIG);
        let error = token_request(PROJECT, API_KEY, "", FID, "refresh-1").unwrap_err();
        assert_eq!(error.code, MISSING_APP_CONFIG);
    }

    #[test]
    fn a_register_reply_parses_into_the_record_and_token() {
        let (stored, auth) = parse_register_reply(
            r#"{"name":"projects/demo-nsh/installations/c0123456789abcdef-_xyz","fid":"c0123456789abcdef-_xyz","refreshToken":"refresh-1","authToken":{"token":"auth-token-1","expiresIn":"604800s"}}"#,
            FID,
        )
        .unwrap();
        assert_eq!(stored.fid, FID);
        assert_eq!(stored.refresh_token, "refresh-1");
        assert_eq!(
            auth,
            AuthToken {
                token: "auth-token-1".to_owned(),
                expires_in_ms: 604_800_000,
            }
        );
    }

    #[test]
    fn a_register_reply_without_a_fid_keeps_the_generated_one() {
        let (stored, _) = parse_register_reply(
            r#"{"refreshToken":"refresh-1","authToken":{"token":"t","expiresIn":"3600s"}}"#,
            FID,
        )
        .unwrap();
        assert_eq!(stored.fid, FID);
    }

    #[test]
    fn a_token_reply_parses_the_seconds_suffixed_expiry() {
        let reply = r#"{"token":"auth-token-2","expiresIn":"604800s"}"#;
        let auth = parse_token_reply(reply).unwrap();
        assert_eq!(auth.token, "auth-token-2");
        assert_eq!(auth.expires_in_ms, 604_800_000);
    }

    #[test]
    fn a_reply_missing_a_field_is_an_invalid_response() {
        let no_refresh = parse_register_reply(
            r#"{"fid":"f","authToken":{"token":"t","expiresIn":"1s"}}"#,
            FID,
        )
        .unwrap_err();
        assert_eq!(no_refresh.code, INVALID_RESPONSE);
        let no_token = parse_token_reply(r#"{"expiresIn":"604800s"}"#).unwrap_err();
        assert_eq!(no_token.code, INVALID_RESPONSE);
        let bad_reply = r#"{"token":"t","expiresIn":"soon"}"#;
        let bad_expiry = parse_token_reply(bad_reply).unwrap_err();
        assert_eq!(bad_expiry.code, INVALID_RESPONSE);
        assert_eq!(
            parse_token_reply("<html>").unwrap_err().code,
            INVALID_RESPONSE
        );
    }
}
