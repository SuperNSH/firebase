//! Firebase Installations for Native Shell Rust apps.
//!
//! The pure tier drives the Installations REST
//! (`firebaseinstallations.googleapis.com`) with the web API key: a
//! locally generated FID (the js-SDK's 22-character base64url shape),
//! the register call, `authTokens:generate` with the `FIS_v2` refresh
//! scheme, `{fid, refreshToken}` persisted behind [`KeyValueStore`] and
//! restored on init, and the auth token cached in memory with the
//! js-SDK's one-hour expiry margin. No emulator serves this product and
//! a fake `demo-` project is refused by the real endpoint, so the
//! sandbox proof is the request/response shape against a stubbed
//! network plus the clear `installations/request-failed` mapping — the
//! same honest criterion the messaging area proved for its token
//! request. The native tier routes through the `firebase.installations.*`
//! handlers below when a host answers. The TypeScript package
//! `packages/nsh-firebase/src/installations` mirrors this behavior name
//! for name.
//!
//! # The native handler contract (verbatim, coordinator-fixed)
//!
//! - `firebase.installations.getId()` -> `{id}` (rejects clearly under
//!   the fake project)
//! - `firebase.installations.getToken()` -> `{token}` (rejects clearly
//!   under the fake project)

pub mod error;
pub mod fid;
pub mod installations;
pub mod store;
pub mod wire;

pub use crate::error::{
    INTERNAL_ERROR, INVALID_RESPONSE, MISSING_APP_CONFIG, REGISTER_IN_PROGRESS, REQUEST_FAILED,
};
pub use crate::fid::{VALID_FID_PATTERN, generate_fid, is_valid_fid};
pub use crate::installations::{Installations, TOKEN_EXPIRATION_BUFFER_MS};
pub use crate::store::{EngineStore, KeyValueStore, MemoryStore};
pub use crate::wire::{
    GET_ID, GET_TOKEN, INSTALLATIONS_API_URL, INTERNAL_AUTH_VERSION, SDK_VERSION, endpoint,
};
