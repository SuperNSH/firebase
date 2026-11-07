//! The installations stable codes and the mapping from an Installations
//! REST error reply to them (the shapes are documented in docs/research.md
//! §4). Transport failures keep the core's stable codes
//! (`core/network`, `core/access-denied`): only a server reply maps to an
//! `installations/*` code.

use nsh_firebase_core::FirebaseError;

/// The options lack a value the calls need (`apiKey`, `appId`) — the
/// js-SDK's `installations/missing-app-config-values`.
pub const MISSING_APP_CONFIG: &str = "installations/missing-app-config-values";
/// A 4xx reply: the server refused (a fake `demo-` project lands here).
pub const REQUEST_FAILED: &str = "installations/request-failed";
/// The server failed on its side (5xx).
pub const INTERNAL_ERROR: &str = "installations/internal-error";
/// The server's success reply lacks a field the flow needs.
pub const INVALID_RESPONSE: &str = "installations/invalid-response";
/// A registration is already in flight; wait for it instead of starting
/// one.
pub const REGISTER_IN_PROGRESS: &str = "installations/register-in-progress";

/// Builds the error for a non-2xx reply. A 4xx body's `error` object
/// (`{code, message, status}`) goes into the message in the js-SDK's
/// `request-failed` shape, so the server's own code and status stay
/// diagnosable; a 5xx is the server's failure, not the request's.
pub(crate) fn reply_error(request_name: &str, status: u16, body: &str) -> FirebaseError {
    if status >= 500 {
        return FirebaseError::new(
            INTERNAL_ERROR,
            format!("the installations server failed (HTTP {status})"),
        );
    }
    let parsed = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|reply| {
            let error = reply.get("error")?;
            let code = error.get("code")?.as_u64()?;
            let message = error.get("message")?.as_str()?;
            let wire_status = error
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            Some(format!(
                "{request_name} request failed with error \"{code} {wire_status}: {message}\""
            ))
        });
    FirebaseError::new(
        REQUEST_FAILED,
        parsed.unwrap_or_else(|| {
            format!("the server replied {status} without an error message")
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_4xx_error_body_maps_to_request_failed_with_the_server_parts() {
        let error = reply_error(
            "Create Installation",
            403,
            r#"{"error":{"code":403,"message":"Firebase Installations service is disabled for this project","status":"PERMISSION_DENIED"}}"#,
        );
        assert_eq!(error.code, REQUEST_FAILED);
        assert_eq!(
            error.message,
            "Create Installation request failed with error \
             \"403 PERMISSION_DENIED: Firebase Installations service is disabled for this project\""
        );
    }

    #[test]
    fn a_missing_status_or_code_still_names_the_request() {
        let error = reply_error(
            "Create Installation",
            400,
            r#"{"error":{"code":400,"message":"API key not valid"}}"#,
        );
        assert_eq!(error.code, REQUEST_FAILED);
        assert!(error.message.contains("400 : API key not valid"), "{error}");

        let no_error = reply_error("Create Installation", 400, r#"{"kind":"odd"}"#);
        assert_eq!(no_error.code, REQUEST_FAILED);
        assert!(
            no_error
                .message
                .contains("replied 400 without an error message"),
            "{no_error}"
        );
    }

    #[test]
    fn a_5xx_is_internal_whatever_the_body_says() {
        let error = reply_error("Generate Auth Token", 503, r#"{"error":{"code":503}}"#);
        assert_eq!(error.code, INTERNAL_ERROR);
        assert!(error.message.contains("HTTP 503"), "{error}");
    }
}
