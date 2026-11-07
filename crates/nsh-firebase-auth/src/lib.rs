//! Firebase Auth for Native Shell Rust apps, over the Identity Toolkit
//! and Secure Token REST the emulator suite also serves (the shapes are
//! documented in docs/research.md §3). Email+password sign-up and sign-in,
//! anonymous, custom token, a credential from an ID token another library
//! minted, a clock-based refresh schedule (no hidden timers), the
//! `currentUser` signal, and persistence behind [`KeyValueStore`]. The
//! TypeScript package `packages/nsh-firebase/src/auth` mirrors this
//! behavior name for name.
//!
//! The flows run on the engine thread (call them in `spawn` from a
//! component's handler), where the engine's `fetch` and clock live.
//! Transport failures surface with the core's stable codes
//! (`core/network`, `core/access-denied`); a server reply maps to an
//! `auth/*` code ([`error`]). The persisted record is one JSON document
//! both this crate and the TypeScript package read.

pub mod auth;
pub mod error;
pub mod store;
pub mod wire;

pub use crate::auth::{Auth, AuthUser, Credential, REFRESH_MARGIN_MS};
pub use crate::store::{EngineStore, KeyValueStore, MemoryStore};
