//! The remote-config-specific stable codes, shared with
//! `packages/nsh-firebase/src/remote-config`. Argument validation keeps
//! the package-wide `core/invalid-argument`.

/// Stable code: fetch is impossible here without a caller-supplied
/// [`crate::Transport`] — the REST API is authorized with an OAuth 2.0
/// service-account token, not the web API key, and the emulator suite has
/// no Remote Config service.
pub const UNAVAILABLE: &str = "remote-config/unavailable";
/// Stable code: the fetched document is not a valid Remote Config
/// template (the REST `RemoteConfig` resource shape).
pub const INVALID_TEMPLATE: &str = "remote-config/invalid-template";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_are_stable() {
        assert_eq!(UNAVAILABLE, "remote-config/unavailable");
        assert_eq!(INVALID_TEMPLATE, "remote-config/invalid-template");
    }
}
