//! The app-check stable codes and the exchange reply's mapping to them.
//! Transport failures keep the core's stable codes (`core/network`,
//! `core/access-denied`): only a server reply maps to an `app-check/*`
//! code.

use nsh_firebase_core::FirebaseError;
use serde_json::Value as JsonValue;

/// The arguments are wrong before any request (an empty debug token).
pub const INVALID_ARGUMENT: &str = "app-check/invalid-argument";
/// The pure tier cannot attest: DeviceCheck/App Attest and Play
/// Integrity need a device. `getToken` without `debug: true` says so.
pub const NEEDS_ATTESTATION: &str = "app-check/needs-attestation";
/// The exchange refused the credentials (HTTP 400-403: a debug token the
/// console has not registered, or an app/project mismatch).
pub const INVALID_DEBUG_TOKEN: &str = "app-check/invalid-debug-token";
/// Any other non-2xx exchange reply.
pub const EXCHANGE_FAILED: &str = "app-check/exchange-failed";
/// A 2xx reply without a usable `{token, ttl}`.
pub const INVALID_RESPONSE: &str = "app-check/invalid-response";

/// The error a non-2xx exchange reply maps to: the status decides the
/// code, and Google's error body (`{"error": {"message": ...}}`) rides in
/// the message for diagnosis, as the client SDKs surface only the status.
pub(crate) fn reply_error(status: u16, body: &str) -> FirebaseError {
    let detail = serde_json::from_str::<JsonValue>(body)
        .ok()
        .and_then(|reply| {
            reply
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(JsonValue::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "the server gave no error message".to_owned());
    let (code, why) = if (400..=403).contains(&status) {
        (INVALID_DEBUG_TOKEN, "the exchange refused the debug token")
    } else {
        (EXCHANGE_FAILED, "the exchange failed")
    };
    FirebaseError::new(code, format!("{why} (HTTP {status}: {detail})"))
}

pub(crate) fn invalid_response(message: impl Into<String>) -> FirebaseError {
    FirebaseError::new(INVALID_RESPONSE, message)
}

pub(crate) fn needs_attestation() -> FirebaseError {
    FirebaseError::new(
        NEEDS_ATTESTATION,
        "the pure tier has no device attestor (DeviceCheck/App Attest on iOS, Play Integrity on \
         Android): call getToken with debug:true and a debug token registered in the Firebase \
         console",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_maps_to_invalid_debug_token_with_the_server_text() {
        let error = reply_error(
            403,
            r#"{"error":{"code":403,"message":"App attestation failed.","status":"PERMISSION_DENIED"}}"#,
        );
        assert_eq!(error.code, INVALID_DEBUG_TOKEN);
        assert!(error.message.contains("App attestation failed."));
        assert!(error.message.contains("HTTP 403"));
    }

    #[test]
    fn the_refusal_range_covers_400_to_403_only() {
        for status in [400_u16, 401, 403] {
            assert_eq!(reply_error(status, "{}").code, INVALID_DEBUG_TOKEN);
        }
        assert_eq!(reply_error(500, "{}").code, EXCHANGE_FAILED);
        assert_eq!(reply_error(429, "{}").code, EXCHANGE_FAILED);
    }

    #[test]
    fn a_body_without_an_error_message_still_maps() {
        let error = reply_error(500, "<html>busy</html>");
        assert_eq!(error.code, EXCHANGE_FAILED);
        assert!(error.message.contains("the server gave no error message"));
    }
}
