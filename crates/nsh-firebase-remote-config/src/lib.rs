//! Firebase Remote Config for Native Shell Rust apps.
//!
//! What the pure tier honestly does: app defaults
//! ([`RemoteConfig::set_defaults`] / [`RemoteConfig::get_defaults`]),
//! typed reads ([`RemoteConfig::get`] / `get_string` / `get_boolean` /
//! `get_number`, with the web SDK's coercion rules), and template
//! parsing/validation ([`parse_template`], the REST `RemoteConfig`
//! resource shape from the v1 discovery document of
//! `firebaseremoteconfig.googleapis.com`).
//!
//! What it honestly does not do: fetch. The REST API is authorized with
//! an OAuth 2.0 access token from a service account (scope
//! `https://www.googleapis.com/auth/firebase.remoteconfig`), not the web
//! API key, so a pure-tier NSH app cannot fetch a real template, and the
//! emulator suite has no Remote Config service.
//! [`RemoteConfig::fetch_and_activate`] refuses with
//! [`codes::UNAVAILABLE`] unless a caller supplies a [`Transport`] that
//! returns a template from wherever it *can* authorize. The native tier
//! routes around this entirely: [`NativeRemoteConfig`] calls the
//! `firebase.remote-config.*` handlers an embedding host registers.
//! The TypeScript package `packages/nsh-firebase/src/remote-config`
//! mirrors this behavior name for name.
//!
//! # Native handler contract (fixed)
//!
//! - `firebase.remote-config.setDefaults({map}) -> {}`
//! - `firebase.remote-config.getDefaults() -> {map}`
//! - `firebase.remote-config.fetchAndActivate() -> {activated: bool, lastFetchStatus: string}` (rejects clearly when the backend refuses a fake project)
//! - `firebase.remote-config.get({key}) -> {value: string|null}`
//!
//! On the wire: `setDefaults` carries `{"map": <defaults>}`, `get`
//! carries `{"key": <key>}`, `getDefaults` and `fetchAndActivate` carry
//! no arguments; a malformed answer is a `core/host-error`.

pub mod client;
pub mod clock;
pub mod codes;
pub mod template;
pub mod transport;

pub use crate::{
    client::{
        FETCH_FAILURE, FETCH_SUCCESS, FetchAndActivateResult, NATIVE_FETCH_AND_ACTIVATE,
        NATIVE_GET, NATIVE_GET_DEFAULTS, NATIVE_SET_DEFAULTS, NO_FETCH_YET, NativeRemoteConfig,
        RemoteConfig,
    },
    clock::{Clock, ManualClock, SystemClock},
    codes::{INVALID_TEMPLATE, UNAVAILABLE},
    template::{
        RemoteConfigCondition, RemoteConfigParameter, RemoteConfigTemplate, parse_template,
    },
    transport::Transport,
};
