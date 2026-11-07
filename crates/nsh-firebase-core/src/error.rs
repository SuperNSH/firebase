//! The shared error type and the stable codes.

/// Stable code: the sandbox refuses the request (`capabilities.network`
/// or `capabilities.host` does not grant it); the platform's native code
/// is `ERR_ACCESS_DENIED`.
pub const ACCESS_DENIED: &str = "core/access-denied";
/// Stable code: the host's handler failed, or there is none
/// (`ERR_NSH_HOST_ERROR`).
pub const HOST_ERROR: &str = "core/host-error";
/// Stable code: this platform has no host, or the product has no pure
/// path (`ERR_FEATURE_UNAVAILABLE_ON_PLATFORM`).
pub const UNAVAILABLE: &str = "core/unavailable";
/// Stable code: the host did not answer in time (`ETIMEDOUT`).
pub const TIMEOUT: &str = "core/timeout";
/// Stable code: a request failed in flight (offline, reset connection).
pub const NETWORK: &str = "core/network";
/// Stable code: the options or the arguments are wrong.
pub const INVALID_ARGUMENT: &str = "core/invalid-argument";
/// Stable code: the named app does not exist.
pub const NO_APP: &str = "core/no-app";

/// The error every product rejects and throws. `code` is stable and
/// shared with the TypeScript package; `native_code` keeps the platform's
/// own `ERR_*` code when one was mapped, so a caller can tell a sandbox
/// refusal (`ERR_ACCESS_DENIED`) from a lost connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirebaseError {
    pub code: String,
    pub message: String,
    pub native_code: Option<String>,
}

impl FirebaseError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            native_code: None,
        }
    }

    /// Maps a platform failure code (`ERR_ACCESS_DENIED`,
    /// `ERR_NSH_HOST_ERROR`, `ERR_FEATURE_UNAVAILABLE_ON_PLATFORM`,
    /// `ETIMEDOUT`) to its stable code, keeping the platform's message.
    pub fn from_host_code(native_code: &str, message: impl Into<String>) -> Self {
        let stable = match native_code {
            "ERR_ACCESS_DENIED" => ACCESS_DENIED,
            "ERR_NSH_HOST_ERROR" => HOST_ERROR,
            "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM" => UNAVAILABLE,
            "ETIMEDOUT" => TIMEOUT,
            _ => NETWORK,
        };
        Self {
            code: stable.to_owned(),
            message: message.into(),
            native_code: Some(native_code.to_owned()),
        }
    }

    /// Maps the text a failed engine `fetch` gives: a sandbox refusal
    /// names the grant it missed ("... is not granted: add ..."), every
    /// other failure is in flight.
    pub fn from_transport_message(message: &str) -> Self {
        let refused = message.contains("is not granted") || message.contains("ERR_ACCESS_DENIED");
        let code = if refused { ACCESS_DENIED } else { NETWORK };
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
            native_code: refused.then(|| "ERR_ACCESS_DENIED".to_owned()),
        }
    }
}

impl std::fmt::Display for FirebaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.native_code {
            Some(native) => write!(f, "{} ({}): {}", self.code, native, self.message),
            None => write!(f, "{}: {}", self.code, self.message),
        }
    }
}

impl std::error::Error for FirebaseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_codes_map_to_their_stable_codes() {
        let denied = FirebaseError::from_host_code("ERR_ACCESS_DENIED", "refused");
        assert_eq!(denied.code, ACCESS_DENIED);
        assert_eq!(denied.native_code.as_deref(), Some("ERR_ACCESS_DENIED"));
        assert_eq!(
            FirebaseError::from_host_code("ERR_NSH_HOST_ERROR", "boom").code,
            HOST_ERROR
        );
        assert_eq!(
            FirebaseError::from_host_code("ERR_FEATURE_UNAVAILABLE_ON_PLATFORM", "none").code,
            UNAVAILABLE
        );
        assert_eq!(
            FirebaseError::from_host_code("ETIMEDOUT", "slow").code,
            TIMEOUT
        );
    }

    #[test]
    fn an_unknown_host_code_is_a_network_failure() {
        let error = FirebaseError::from_host_code("ERR_WEIRD", "?");
        assert_eq!(error.code, NETWORK);
        // The unknown platform code stays recorded, so Display shows it.
        assert_eq!(error.native_code.as_deref(), Some("ERR_WEIRD"));
    }

    #[test]
    fn a_refusal_message_maps_to_access_denied() {
        let error = FirebaseError::from_transport_message(
            "fetch GET https://evil.io/ is not granted: add `evil.io` to capabilities.network",
        );
        assert_eq!(error.code, ACCESS_DENIED);
        assert_eq!(error.native_code.as_deref(), Some("ERR_ACCESS_DENIED"));
    }

    #[test]
    fn display_keeps_the_native_code_visible() {
        let error = FirebaseError::from_host_code("ERR_ACCESS_DENIED", "refused");
        assert_eq!(
            error.to_string(),
            "core/access-denied (ERR_ACCESS_DENIED): refused"
        );
    }
}
