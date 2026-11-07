//! The auth stable codes and the mapping from Identity Toolkit and Secure
//! Token wire codes to them (the surfaces documented in docs/research.md
//! §3). Transport failures keep the core's stable codes
//! (`core/network`, `core/access-denied`): only a server reply maps to an
//! `auth/*` code.

use nsh_firebase_core::FirebaseError;

/// The email is already registered (`EMAIL_EXISTS`).
pub const EMAIL_ALREADY_IN_USE: &str = "auth/email-already-in-use";
/// No account has that email (`EMAIL_NOT_FOUND`), or, on a refresh, that
/// the account was deleted (`USER_NOT_FOUND` on Secure Token).
pub const USER_NOT_FOUND: &str = "auth/user-not-found";
/// The password is wrong for the account (`INVALID_PASSWORD`).
pub const WRONG_PASSWORD: &str = "auth/wrong-password";
/// The server rejected the credentials without saying which side was
/// wrong (`INVALID_LOGIN_CREDENTIALS`, `INVALID_IDP_RESPONSE`).
pub const INVALID_CREDENTIAL: &str = "auth/invalid-credential";
/// The email is not a valid address (`INVALID_EMAIL`).
pub const INVALID_EMAIL: &str = "auth/invalid-email";
/// The password is too short (`WEAK_PASSWORD`).
pub const WEAK_PASSWORD: &str = "auth/weak-password";
/// The account is disabled (`USER_DISABLED`).
pub const USER_DISABLED: &str = "auth/user-disabled";
/// The provider is not enabled for the project (`OPERATION_NOT_ALLOWED`).
pub const OPERATION_NOT_ALLOWED: &str = "auth/operation-not-allowed";
/// Too many tries; try later (`TOO_MANY_ATTEMPTS_TRY_LATER`).
pub const TOO_MANY_REQUESTS: &str = "auth/too-many-requests";
/// The custom token is not one the project accepts (`INVALID_CUSTOM_TOKEN`).
pub const INVALID_CUSTOM_TOKEN: &str = "auth/invalid-custom-token";
/// The token was minted for another project (`CREDENTIAL_MISMATCH`).
pub const CUSTOM_TOKEN_MISMATCH: &str = "auth/custom-token-mismatch";
/// The refresh token is not one the project issued
/// (`INVALID_REFRESH_TOKEN`, `INVALID_GRANT_TYPE`, `MISSING_REFRESH_TOKEN`).
pub const INVALID_REFRESH_TOKEN: &str = "auth/invalid-refresh-token";
/// The session is gone server-side: the refresh token expired or the user
/// was deleted (`TOKEN_EXPIRED`, `USER_NOT_FOUND` on Secure Token). The
/// auth signs the user out on this one.
pub const USER_TOKEN_EXPIRED: &str = "auth/user-token-expired";
/// The API key names another project (`PROJECT_NUMBER_MISMATCH`).
pub const PROJECT_NOT_FOUND: &str = "auth/project-not-found";
/// The API key is not valid (`INVALID_API_KEY`).
pub const INVALID_API_KEY: &str = "auth/invalid-api-key";
/// The arguments are wrong before any request (an empty email, a missing
/// token).
pub const INVALID_ARGUMENT: &str = "auth/invalid-argument";
/// No user is signed in (refresh with nothing to refresh).
pub const NO_CURRENT_USER: &str = "auth/no-current-user";
/// A refresh is already in flight; wait for it instead of starting one.
pub const REFRESH_IN_PROGRESS: &str = "auth/refresh-in-progress";
/// The server's success reply lacks a field the session needs.
pub const INVALID_RESPONSE: &str = "auth/invalid-response";
/// The server failed on its side (5xx).
pub const INTERNAL_ERROR: &str = "auth/internal-error";
/// A 4xx the table above has no code for; the wire code is in the message.
pub const REQUEST_FAILED: &str = "auth/request-failed";

/// The codes whose session cannot recover: refresh maps them to a
/// sign-out. USER_NOT_FOUND arrives here only from Secure Token, where it
/// means the account was deleted; identitytoolkit sign-ins say
/// EMAIL_NOT_FOUND instead.
pub(crate) fn is_session_lost(code: &str) -> bool {
    code == USER_TOKEN_EXPIRED || code == INVALID_REFRESH_TOKEN || code == USER_NOT_FOUND
}

/// Builds the error for a non-2xx reply: the wire error body's `message`
/// is the machine code, alone or ahead of a human part (`WEAK_PASSWORD :
/// Password should be at least 6 characters`).
pub(crate) fn wire_error(status: u16, message: &str) -> FirebaseError {
    let wire = message.split(" : ").next().unwrap_or(message).trim();
    let (code, human) = match wire {
        "EMAIL_EXISTS" => (
            EMAIL_ALREADY_IN_USE,
            "that email is already registered; sign in instead",
        ),
        "EMAIL_NOT_FOUND" => (USER_NOT_FOUND, "no account with that email"),
        "USER_NOT_FOUND" => (USER_NOT_FOUND, "no such account on the server"),
        "INVALID_PASSWORD" => (WRONG_PASSWORD, "that password is wrong for the account"),
        "INVALID_LOGIN_CREDENTIALS" | "INVALID_IDP_RESPONSE" => {
            (INVALID_CREDENTIAL, "the server rejected those credentials")
        }
        "INVALID_EMAIL" => (INVALID_EMAIL, "that email is not a valid address"),
        "WEAK_PASSWORD" => (WEAK_PASSWORD, "that password is too weak"),
        "USER_DISABLED" => (USER_DISABLED, "that account is disabled"),
        "OPERATION_NOT_ALLOWED" => (
            OPERATION_NOT_ALLOWED,
            "the sign-in provider is not enabled for this project",
        ),
        "TOO_MANY_ATTEMPTS_TRY_LATER" => (
            TOO_MANY_REQUESTS,
            "too many attempts from this client; try again later",
        ),
        "INVALID_CUSTOM_TOKEN" => (INVALID_CUSTOM_TOKEN, "the server did not accept that token"),
        "CREDENTIAL_MISMATCH" => (
            CUSTOM_TOKEN_MISMATCH,
            "that token was minted for another project",
        ),
        "INVALID_REFRESH_TOKEN" | "INVALID_GRANT_TYPE" | "MISSING_REFRESH_TOKEN" => (
            INVALID_REFRESH_TOKEN,
            "the refresh token is not one this project issued",
        ),
        "TOKEN_EXPIRED" => (
            USER_TOKEN_EXPIRED,
            "the session expired server-side; sign in again",
        ),
        "PROJECT_NUMBER_MISMATCH" => (PROJECT_NOT_FOUND, "the API key names another project"),
        "INVALID_API_KEY" => (INVALID_API_KEY, "the API key is not valid"),
        _ if status >= 500 => (INTERNAL_ERROR, "the auth server failed"),
        _ => (REQUEST_FAILED, message),
    };
    FirebaseError::new(code, format!("{human} ({wire})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_codes_map_to_their_stable_codes() {
        let cases = [
            ("EMAIL_EXISTS", EMAIL_ALREADY_IN_USE),
            ("EMAIL_NOT_FOUND", USER_NOT_FOUND),
            ("USER_NOT_FOUND", USER_NOT_FOUND),
            ("INVALID_PASSWORD", WRONG_PASSWORD),
            ("INVALID_LOGIN_CREDENTIALS", INVALID_CREDENTIAL),
            ("INVALID_EMAIL", INVALID_EMAIL),
            ("USER_DISABLED", USER_DISABLED),
            ("OPERATION_NOT_ALLOWED", OPERATION_NOT_ALLOWED),
            ("TOO_MANY_ATTEMPTS_TRY_LATER", TOO_MANY_REQUESTS),
            ("INVALID_CUSTOM_TOKEN", INVALID_CUSTOM_TOKEN),
            ("CREDENTIAL_MISMATCH", CUSTOM_TOKEN_MISMATCH),
            ("INVALID_REFRESH_TOKEN", INVALID_REFRESH_TOKEN),
            ("MISSING_REFRESH_TOKEN", INVALID_REFRESH_TOKEN),
            ("TOKEN_EXPIRED", USER_TOKEN_EXPIRED),
            ("PROJECT_NUMBER_MISMATCH", PROJECT_NOT_FOUND),
            ("INVALID_API_KEY", INVALID_API_KEY),
        ];
        for (wire, stable) in cases {
            let error = wire_error(400, wire);
            assert_eq!(error.code, stable, "{wire} must map to {stable}");
            // The wire code stays in the message for diagnosis.
            assert!(error.message.contains(wire), "{wire} must be visible");
        }
    }

    #[test]
    fn a_wire_code_with_a_human_part_keeps_only_the_code() {
        let error = wire_error(
            400,
            "WEAK_PASSWORD : Password should be at least 6 characters",
        );
        assert_eq!(error.code, WEAK_PASSWORD);
        assert!(error.message.contains("WEAK_PASSWORD"));
    }

    #[test]
    fn a_server_failure_is_internal_and_an_unknown_4xx_keeps_its_message() {
        assert_eq!(wire_error(503, "").code, INTERNAL_ERROR);
        let odd = wire_error(400, "SOMETHING_NEW");
        assert_eq!(odd.code, REQUEST_FAILED);
        assert!(odd.message.contains("SOMETHING_NEW"));
    }

    #[test]
    fn only_the_lost_session_codes_sign_the_user_out() {
        assert!(is_session_lost(USER_TOKEN_EXPIRED));
        assert!(is_session_lost(INVALID_REFRESH_TOKEN));
        // USER_NOT_FOUND is consulted only on the Secure Token reply,
        // where it means the account was deleted server-side.
        assert!(is_session_lost(USER_NOT_FOUND));
        assert!(!is_session_lost(USER_DISABLED));
        assert!(!is_session_lost("core/network"));
    }
}
