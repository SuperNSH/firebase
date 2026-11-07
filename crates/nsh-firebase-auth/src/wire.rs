//! The Identity Toolkit and Secure Token wire shapes (docs/research.md
//! §3): URLs, request bodies, reply parsing, and the base64url read of an
//! idToken's claims that flows whose reply has no `localId` need for the
//! uid.

use nsh_core::platform::HttpRequest;
use nsh_firebase_core::{FirebaseError, join_url};
use serde_json::Value as JsonValue;

use crate::error::INVALID_RESPONSE;

/// The service paths sit under the base URL as full segments, the shape
/// the emulator serves (`http://127.0.0.1:19099/identitytoolkit.googleapis
/// .com/v1/...`) and the real hosts (`https://identitytoolkit.googleapis
/// .com/...` with the same paths) share.
const IDENTITY_TOOLKIT: &str = "identitytoolkit.googleapis.com/v1";
const SECURE_TOKEN_TOKEN: &str = "securetoken.googleapis.com/v1/token";

/// One sign-in reply reduced to what a session needs. `expires_in_secs`
/// comes as a JSON string on both services (`"3600"`).
#[derive(Debug)]
pub(crate) struct Session {
    pub id_token: String,
    pub refresh_token: String,
    pub expires_in_secs: u64,
    pub local_id: Option<String>,
    pub email: Option<String>,
}

/// `POST {base}/{IDENTITY_TOOLKIT}/{operation}?key={api_key}`.
pub(crate) fn identity_request(
    base: &str,
    api_key: &str,
    operation: &str,
    body: &JsonValue,
) -> Result<HttpRequest, FirebaseError> {
    let url = format!(
        "{}?key={api_key}",
        join_url(base, &format!("{IDENTITY_TOOLKIT}/{operation}"))
    );
    Ok(json_request(&url, body))
}

/// `POST {base}/{SECURE_TOKEN_TOKEN}?key={api_key}` with the form body the
/// Secure Token service takes (the Identity Toolkit calls take JSON).
pub(crate) fn refresh_request(base: &str, api_key: &str, refresh_token: &str) -> HttpRequest {
    let url = format!("{}?key={api_key}", join_url(base, SECURE_TOKEN_TOKEN));
    HttpRequest::post(
        url,
        format!(
            "grant_type=refresh_token&refresh_token={}",
            form_encode(refresh_token)
        )
        .into_bytes(),
    )
    .header("Content-Type", "application/x-www-form-urlencoded")
}

fn json_request(url: &str, body: &JsonValue) -> HttpRequest {
    HttpRequest::post(url, body.to_string().into_bytes()).header("Content-Type", "application/json")
}

/// Percent-encodes every byte but the unreserved set, so any refresh
/// token lands in the form body intact.
fn form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Reads a success reply. Identity Toolkit answers camelCase
/// (`idToken`/`refreshToken`/`expiresIn`), Secure Token snake_case
/// (`id_token`/`refresh_token`/`expires_in`); the emulator may hand
/// Secure Token's `access_token` instead of an `idToken` on refresh.
pub(crate) fn parse_session_reply(
    body: &str,
    accept_access_token: bool,
) -> Result<Session, FirebaseError> {
    let reply: JsonValue = serde_json::from_str(body)
        .map_err(|_| FirebaseError::new(INVALID_RESPONSE, "the reply is not JSON"))?;
    let string = |names: &[&str]| -> Option<&str> {
        names
            .iter()
            .find_map(|name| reply.get(*name))
            .and_then(JsonValue::as_str)
    };
    let id_token = string(&["idToken", "id_token"])
        .or_else(|| {
            accept_access_token
                .then(|| string(&["access_token"]))
                .flatten()
        })
        .ok_or_else(|| missing("idToken"))?
        .to_owned();
    let expires_in_secs = string(&["expiresIn", "expires_in"])
        .and_then(|text| text.parse::<u64>().ok())
        .ok_or_else(|| missing("expiresIn (a number-as-string)"))?;
    Ok(Session {
        id_token,
        refresh_token: string(&["refreshToken", "refresh_token"])
            .unwrap_or_default()
            .to_owned(),
        expires_in_secs,
        local_id: string(&["localId", "local_id"]).map(str::to_owned),
        email: string(&["email"]).map(str::to_owned),
    })
}

fn missing(field: &str) -> FirebaseError {
    FirebaseError::new(INVALID_RESPONSE, format!("the reply has no usable {field}"))
}

/// The `sub` claim of a JWT's base64url payload: the uid of a flow whose
/// reply has no `localId` (custom token, foreign id token, refresh).
pub(crate) fn uid_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let claims: JsonValue = serde_json::from_slice(&base64url_decode(payload)?).ok()?;
    claims
        .get("sub")
        .or_else(|| claims.get("user_id"))
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
}

/// Base64url (RFC 4648 §5), padding optional, no invalid characters.
fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut bits = 0_u32;
    let mut pending = 0_u32;
    for char in input.chars() {
        let value = match char {
            'A'..='Z' => char as u32 - 'A' as u32,
            'a'..='z' => char as u32 - 'a' as u32 + 26,
            '0'..='9' => char as u32 - '0' as u32 + 52,
            '-' => 62,
            '_' => 63,
            '=' => break,
            _ => return None,
        };
        bits = (bits << 6) | value;
        pending += 6;
        if pending >= 8 {
            pending -= 8;
            out.push((bits >> pending) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BASE: &str = "http://127.0.0.1:19099";

    #[test]
    fn identity_requests_sit_under_the_base_with_the_api_key() {
        let request = identity_request(BASE, "demo-key", "accounts:signUp", &json!({})).unwrap();
        assert_eq!(
            request.url,
            "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/accounts:signUp?key=demo-key"
        );
        assert_eq!(request.method, "POST");
        assert!(
            request
                .headers
                .iter()
                .any(|(name, value)| name == "Content-Type" && value == "application/json")
        );
    }

    #[test]
    fn refresh_requests_are_form_encoded() {
        let request = refresh_request(BASE, "demo-key", "abc123");
        assert_eq!(
            request.url,
            "http://127.0.0.1:19099/securetoken.googleapis.com/v1/token?key=demo-key"
        );
        assert_eq!(
            request.body,
            nsh_core::platform::HttpBody::Bytes(
                b"grant_type=refresh_token&refresh_token=abc123".to_vec()
            )
        );
        assert!(
            request
                .headers
                .iter()
                .any(|(name, value)| name == "Content-Type"
                    && value == "application/x-www-form-urlencoded")
        );
    }

    #[test]
    fn form_encode_leaves_reserved_bytes_no_room() {
        assert_eq!(form_encode("aZ09-_"), "aZ09-_");
        assert_eq!(form_encode("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn a_session_reply_parses_string_expires_in() {
        let session = parse_session_reply(
            r#"{"idToken":"t.token","refreshToken":"r","expiresIn":"3600","localId":"u1","email":"e@x.test"}"#,
            false,
        )
        .unwrap();
        assert_eq!(session.id_token, "t.token");
        assert_eq!(session.refresh_token, "r");
        assert_eq!(session.expires_in_secs, 3600);
        assert_eq!(session.local_id.as_deref(), Some("u1"));
        assert_eq!(session.email.as_deref(), Some("e@x.test"));
    }

    #[test]
    fn a_refresh_reply_may_carry_access_token_and_no_local_id() {
        let session = parse_session_reply(
            r#"{"access_token":"at","expires_in":"3600","refresh_token":"r2","user_id":"u2"}"#,
            true,
        )
        .unwrap();
        assert_eq!(session.id_token, "at");
        assert_eq!(session.refresh_token, "r2");
        assert_eq!(session.expires_in_secs, 3600);
        assert_eq!(session.local_id, None);
    }

    #[test]
    fn a_reply_without_a_token_or_expiry_is_an_invalid_response() {
        let no_token = parse_session_reply(r#"{"expiresIn":"3600"}"#, false).unwrap_err();
        assert_eq!(no_token.code, INVALID_RESPONSE);
        let bad_expiry =
            parse_session_reply(r#"{"idToken":"t","expiresIn":3600}"#, false).unwrap_err();
        assert_eq!(bad_expiry.code, INVALID_RESPONSE);
        let not_json = parse_session_reply("<html>", false).unwrap_err();
        assert_eq!(not_json.code, INVALID_RESPONSE);
    }

    #[test]
    fn uid_reads_sub_from_a_base64url_payload() {
        // {"sub":"u42","email":"e@x.test"} without padding.
        let token = format!(
            "header.{}.signature",
            base64url_no_pad(br#"{"sub":"u42","email":"e@x.test"}"#)
        );
        assert_eq!(uid_from_id_token(&token).as_deref(), Some("u42"));

        // user_id is the fallback claim name Secure Token replies use.
        let token = format!(
            "header.{}.signature",
            base64url_no_pad(br#"{"user_id":"u7"}"#)
        );
        assert_eq!(uid_from_id_token(&token).as_deref(), Some("u7"));

        assert_eq!(uid_from_id_token("not-a-jwt"), None);
    }

    fn base64url_no_pad(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            if chunk.len() > 1 {
                out.push(TABLE[(n >> 6) as usize & 63] as char);
            }
            if chunk.len() > 2 {
                out.push(TABLE[n as usize & 63] as char);
            }
        }
        out
    }
}
