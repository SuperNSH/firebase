//! The App Check exchange wire shapes: the debug-token exchange URL,
//! request, and the `{token, ttl}` reply parse, per the REST reference
//! (firebaseappcheck.googleapis.com/v1, `AppCheckToken` = `{token, ttl}`)
//! and the client SDKs' exchange (`debug_token` in the body, `ttl` a
//! Protobuf Duration like `"1800s"`). The exchange carries only
//! `Content-Type`: `X-Firebase-AppCheck` is the header CALLS to
//! protected backends attach afterwards, not this exchange.

use nsh_core::platform::HttpRequest;
use nsh_firebase_core::{FirebaseError, join_url};
use serde_json::{Value as JsonValue, json};

use crate::error::{INVALID_RESPONSE, invalid_response};

/// The REST reference's service endpoint (the client SDKs use the
/// `content-` frontend variant; both serve the same paths).
pub const APP_CHECK_BASE: &str = "https://firebaseappcheck.googleapis.com/v1";

const EXCHANGE_DEBUG_TOKEN: &str = "exchangeDebugToken";

/// One success reply reduced to what the cache needs.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TokenReply {
    pub token: String,
    pub ttl_ms: u64,
}

/// The exchange URL:
/// `{base}/projects/{project}/apps/{app_id}:exchangeDebugToken?key={api_key}`
/// — the API key in the query as the client SDKs send it, the ids raw
/// (`:` is legal in a path segment).
pub(crate) fn exchange_debug_token_url(
    base: &str,
    project_id: &str,
    app_id: &str,
    api_key: &str,
) -> String {
    format!(
        "{}?key={api_key}",
        join_url(
            base,
            &format!("projects/{project_id}/apps/{app_id}:{EXCHANGE_DEBUG_TOKEN}")
        )
    )
}

/// The exchange request: POST with `{"debug_token": ...}` (the snake_case
/// spelling every client SDK sends; the API reference's camelCase
/// `debugToken` is the same proto field, and no other header rides it).
pub(crate) fn exchange_debug_token_request(
    base: &str,
    api_key: &str,
    project_id: &str,
    app_id: &str,
    debug_token: &str,
) -> HttpRequest {
    let url = exchange_debug_token_url(base, project_id, app_id, api_key);
    HttpRequest::post(
        url,
        json!({ "debug_token": debug_token })
            .to_string()
            .into_bytes(),
    )
    .header("Content-Type", "application/json")
}

/// Reads a success reply.
pub(crate) fn parse_token_reply(body: &str) -> Result<TokenReply, FirebaseError> {
    let reply: JsonValue =
        serde_json::from_str(body).map_err(|_| invalid_response("the reply is not JSON"))?;
    let token = reply
        .get("token")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| invalid_response("the reply has no token"))?
        .to_owned();
    let ttl = reply
        .get("ttl")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            invalid_response(r#"the reply has no ttl (a Protobuf Duration like "1800s")"#)
        })?;
    let ttl_ms = parse_ttl_ms(ttl).ok_or_else(|| {
        invalid_response(r#"the reply has no usable ttl (a Protobuf Duration like "1800s")"#)
    })?;
    Ok(TokenReply { token, ttl_ms })
}

/// Parses a Protobuf Duration (`"1800s"`, `"0s"`, fractional allowed) to
/// milliseconds, the way the client SDKs do (`^([\d.]+)(s)$`).
pub(crate) fn parse_ttl_ms(ttl: &str) -> Option<u64> {
    let secs = ttl.strip_suffix('s')?;
    if secs.is_empty()
        || !secs
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    let value = secs.parse::<f64>().ok()?;
    (value.is_finite() && value >= 0.0).then_some((value * 1000.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_core::platform::HttpBody;

    const BASE: &str = "https://firebaseappcheck.googleapis.com/v1";

    #[test]
    fn the_exchange_url_sits_under_the_base_with_the_api_key() {
        assert_eq!(
            exchange_debug_token_url(BASE, "demo-nsh", "1:000000000000:web:demo-nsh", "demo-key"),
            "https://firebaseappcheck.googleapis.com/v1/projects/demo-nsh/apps/1:000000000000:web:demo-nsh:exchangeDebugToken?key=demo-key"
        );
        // A trailing slash on the base must not double.
        assert_eq!(
            exchange_debug_token_url("http://127.0.0.1:19191/v1/", "p", "a", "k"),
            "http://127.0.0.1:19191/v1/projects/p/apps/a:exchangeDebugToken?key=k"
        );
    }

    #[test]
    fn the_exchange_request_is_json_with_the_debug_token_and_no_attest_header() {
        let request =
            exchange_debug_token_request(BASE, "demo-key", "demo-nsh", "app-1", "secret-1");
        assert_eq!(request.method, "POST");
        assert_eq!(
            request.url,
            "https://firebaseappcheck.googleapis.com/v1/projects/demo-nsh/apps/app-1:exchangeDebugToken?key=demo-key"
        );
        assert_eq!(
            request.headers,
            vec![("Content-Type".to_owned(), "application/json".to_owned())]
        );
        let body = String::from_utf8(match request.body {
            HttpBody::Bytes(bytes) => bytes,
            _ => panic!("the exchange carries bytes"),
        })
        .unwrap();
        assert_eq!(body, r#"{"debug_token":"secret-1"}"#);
    }

    #[test]
    fn a_token_reply_parses_with_its_ttl() {
        let reply = parse_token_reply(r#"{"token":"tok-1","ttl":"1800s"}"#).unwrap();
        assert_eq!(reply.token, "tok-1");
        assert_eq!(reply.ttl_ms, 1_800_000);
        let zero = parse_token_reply(r#"{"token":"tok","ttl":"0s"}"#).unwrap();
        assert_eq!(zero.ttl_ms, 0);
        let fractional = parse_token_reply(r#"{"token":"tok","ttl":"1.5s"}"#).unwrap();
        assert_eq!(fractional.ttl_ms, 1_500);
    }

    #[test]
    fn a_reply_without_a_token_or_a_usable_ttl_is_an_invalid_response() {
        let no_token = parse_token_reply(r#"{"ttl":"1800s"}"#).unwrap_err();
        assert_eq!(no_token.code, INVALID_RESPONSE);
        let no_ttl = parse_token_reply(r#"{"token":"tok"}"#).unwrap_err();
        assert_eq!(no_ttl.code, INVALID_RESPONSE);
        let not_json = parse_token_reply("<html>").unwrap_err();
        assert_eq!(not_json.code, INVALID_RESPONSE);
        let number_ttl = parse_token_reply(r#"{"token":"tok","ttl":1800}"#).unwrap_err();
        assert_eq!(number_ttl.code, INVALID_RESPONSE);
    }

    #[test]
    fn ttl_parsing_takes_only_the_duration_format() {
        assert_eq!(parse_ttl_ms("1800s"), Some(1_800_000));
        assert_eq!(parse_ttl_ms("0s"), Some(0));
        assert_eq!(parse_ttl_ms("1800"), None);
        assert_eq!(parse_ttl_ms("s"), None);
        assert_eq!(parse_ttl_ms("soon"), None);
        assert_eq!(parse_ttl_ms(""), None);
        assert_eq!(parse_ttl_ms("-5s"), None);
    }
}
