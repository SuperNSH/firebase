//! Firebase Core for Native Shell Rust apps.
//!
//! Holds what every product shares: app and options with the sandbox
//! defaults, the emulator switches, the stable error codes, the event bus
//! and the tier facade ([`FirebaseHost`]). The pure tier's HTTP transport
//! runs over the engine's `fetch`; the native tier's calls cross the
//! channel the embedding host injects. The TypeScript package
//! `packages/nsh-firebase/src/core` mirrors this behavior name for name.

pub mod app;
pub mod bus;
pub mod error;
pub mod host;
pub mod options;
pub mod transport;

pub use crate::{
    app::{FirebaseApp, get_app, initialize_app},
    bus::{EventBus, ListenerId},
    error::FirebaseError,
    host::{FirebaseHost, HostChannel, NativeHost, PureHost},
    options::{EmulatorEndpoint, EmulatorOptions, FirebaseOptions, from_config},
    transport::{join_url, send, socket_request},
};
