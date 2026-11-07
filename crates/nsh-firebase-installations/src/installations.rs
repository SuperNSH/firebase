//! The installations itself: [`Installations`]. In the native tier (a
//! host answers) every call routes through the `firebase.installations.*`
//! handlers per the contract below; in the pure tier it drives the
//! Installations REST on the engine thread (`spawn` in a component's
//! handler), where the engine's `fetch` and clock live. The TypeScript
//! package `packages/nsh-firebase/src/installations` mirrors this
//! behavior name for name, down to the persisted JSON.
//!
//! # The native handler contract (verbatim, coordinator-fixed)
//!
//! - `firebase.installations.getId()` -> `{id}` (rejects clearly under
//!   the fake project)
//! - `firebase.installations.getToken()` -> `{token}` (rejects clearly
//!   under the fake project)
//!
//! # Wire shapes (verified against firebase-js-sdk `packages/installations`)
//!
//! - register: `POST https://firebaseinstallations.googleapis.com/v1/
//!   projects/{projectId}/installations`, headers `Content-Type:
//!   application/json`, `Accept: application/json`,
//!   `x-goog-api-key: {apiKey}`; body `{fid, authVersion: "FIS_v2",
//!   appId, sdkVersion: "w:<version>"}`; reply `{name?, fid?,
//!   refreshToken, authToken: {token, expiresIn: "<secs>s"}}`.
//! - token: `POST .../projects/{projectId}/installations/{fid}/
//!   authTokens:generate`, the same headers plus
//!   `Authorization: FIS_v2 {refreshToken}`; body
//!   `{installation: {sdkVersion, appId}}`; reply
//!   `{token, expiresIn: "<secs>s"}`.
//!
//! The native contract carries no delete handler, so this facade has no
//! delete; the references' REST `DELETE .../installations/{fid}` is out
//! of scope. The native tier's FIS id belongs to the SDK, so nothing is
//! persisted there; a pure-tier record a previous launch wrote stays
//! harmless in the store.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use nsh_core::time::unix_ms;
use nsh_firebase_core::error::HOST_ERROR;
use nsh_firebase_core::{FirebaseApp, FirebaseError, FirebaseHost, send};
use serde_json::Value as JsonValue;

use crate::error::{REGISTER_IN_PROGRESS, reply_error};
use crate::fid::generate_fid;
use crate::store::{KeyValueStore, StoredInstallation, store_key};
use crate::wire::{self};

/// A cached token goes stale this long before its expiry, like the
/// js-SDK's one-hour `TOKEN_EXPIRATION_BUFFER`.
pub const TOKEN_EXPIRATION_BUFFER_MS: u64 = 60 * 60 * 1000;

/// Clears its flag however the register ends, including a dropped
/// (cancelled) future: nothing can leave the single-flight flag stuck.
struct RegisterGuard<'a> {
    flag: &'a AtomicBool,
}

impl<'a> RegisterGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Result<Self, FirebaseError> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self { flag })
            .map_err(|_| {
                FirebaseError::new(
                    REGISTER_IN_PROGRESS,
                    "an installation registration is already in flight; wait for it",
                )
            })
    }
}

impl Drop for RegisterGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone)]
struct CachedToken {
    token: String,
    expires_at_ms: u64,
}

/// The installations of one app. Create it where a store lives (an app
/// component's body); the constructor reads the store once, before the
/// first frame.
pub struct Installations {
    app: Arc<FirebaseApp>,
    store: Arc<dyn KeyValueStore>,
    host: Arc<dyn FirebaseHost>,
    stored: Mutex<Option<StoredInstallation>>,
    cached: Mutex<Option<CachedToken>>,
    registering: AtomicBool,
}

impl std::fmt::Debug for Installations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Installations")
            .field("app", &self.app.name)
            .field("registered", &self.id().is_some())
            .finish_non_exhaustive()
    }
}

impl Installations {
    pub fn new(
        app: Arc<FirebaseApp>,
        store: Arc<dyn KeyValueStore>,
        host: Arc<dyn FirebaseHost>,
    ) -> Self {
        let installations = Self {
            app,
            store,
            host,
            stored: Mutex::new(None),
            cached: Mutex::new(None),
            registering: AtomicBool::new(false),
        };
        installations.restore();
        installations
    }

    /// Re-reads the store into the in-memory record. A record the other
    /// tier (or an older launch) wrote restores too; a corrupt one is
    /// treated as unregistered.
    pub fn restore(&self) {
        let stored = self
            .store
            .get(&store_key(&self.app.name))
            .as_deref()
            .and_then(StoredInstallation::decode);
        *self.stored.lock().expect("stored lock") = stored;
    }

    /// The registered FID, `None` while unregistered (the pure tier's
    /// record; the native tier's SDK id is not persisted here).
    pub fn id(&self) -> Option<String> {
        self.stored
            .lock()
            .expect("stored lock")
            .as_ref()
            .map(|stored| stored.fid.clone())
    }

    /// The refresh token of the registered installation, `None` while
    /// unregistered.
    pub fn refresh_token(&self) -> Option<String> {
        self.stored
            .lock()
            .expect("stored lock")
            .as_ref()
            .map(|stored| stored.refresh_token.clone())
    }

    /// Whether the initial restore finished. `Installations::new`
    /// restores (sync), so this is `true` from the first read; the TSX
    /// `restoreReady()` resolves a promise instead, its store being
    /// async.
    pub fn restore_ready(&self) -> bool {
        true
    }

    /// The installation id: registers on first use in the pure tier,
    /// routes through the host in the native tier. Under a fake
    /// `demo-` project the register call refuses server-side and this
    /// rejects with a clear `installations/request-failed` — the shape
    /// still round-trips, which the stubbed-network tests prove.
    pub async fn get_id(&self) -> Result<String, FirebaseError> {
        if self.host.available() {
            return self.call_host(wire::GET_ID, "id", "an id").await;
        }
        match self.id() {
            Some(fid) => Ok(fid),
            None => Ok(self.register().await?.fid),
        }
    }

    /// An installation auth token: the cached one while it stays fresh
    /// past [`TOKEN_EXPIRATION_BUFFER_MS`], a minted one on
    /// `force_refresh` or expiry. Routes through the host in the native
    /// tier (no args, no cache there: the SDK keeps its own).
    pub async fn get_token(&self, force_refresh: bool) -> Result<String, FirebaseError> {
        if self.host.available() {
            return self.call_host(wire::GET_TOKEN, "token", "a token").await;
        }
        if self.id().is_none() {
            self.register().await?;
        }
        if !force_refresh
            && let Some(cached) = self.cached.lock().expect("cached lock").as_ref()
            // Fresh while the margin still covers the expiry.
            && now_ms() + TOKEN_EXPIRATION_BUFFER_MS < cached.expires_at_ms
        {
            return Ok(cached.token.clone());
        }
        let stored = self.stored.lock().expect("stored lock").clone();
        let stored = stored.ok_or_else(|| {
            FirebaseError::new(
                crate::error::INVALID_RESPONSE,
                "the registration vanished mid-flight; register again",
            )
        })?;
        let request = wire::token_request(
            &self.app.options.project_id,
            &self.app.options.api_key,
            &self.app.options.app_id,
            &stored.fid,
            &stored.refresh_token,
        )?;
        let body = self.send_body("Generate Auth Token", request).await?;
        let auth = wire::parse_token_reply(&body)?;
        *self.cached.lock().expect("cached lock") = Some(CachedToken {
            token: auth.token.clone(),
            expires_at_ms: now_ms().saturating_add(auth.expires_in_ms),
        });
        Ok(auth.token)
    }

    /// Whether the cached token reaches its refresh point on the clock's
    /// time now (or there is none). The schedule-check: call it whenever
    /// the app chooses and [`Installations::get_token`] when it answers
    /// `true`.
    pub fn token_due(&self) -> bool {
        match self.cached.lock().expect("cached lock").as_ref() {
            // Due once the one-hour margin reaches the expiry.
            Some(cached) => now_ms() + TOKEN_EXPIRATION_BUFFER_MS >= cached.expires_at_ms,
            None => true,
        }
    }

    /// When the cached token expires, in wall-clock milliseconds; `None`
    /// while nothing is cached.
    pub fn token_expires_at_ms(&self) -> Option<u64> {
        self.cached
            .lock()
            .expect("cached lock")
            .as_ref()
            .map(|cached| cached.expires_at_ms)
    }

    /// `POST projects/{projectId}/installations` with a generated FID,
    /// persisting `{fid, refreshToken}` and caching the reply's auth
    /// token. One runs at a time: a second concurrent call rejects with
    /// `installations/register-in-progress`.
    async fn register(&self) -> Result<StoredInstallation, FirebaseError> {
        let _guard = RegisterGuard::acquire(&self.registering)?;
        let fid = generate_fid();
        let request = wire::register_request(
            &self.app.options.project_id,
            &self.app.options.api_key,
            &self.app.options.app_id,
            &fid,
        )?;
        let body = self.send_body("Create Installation", request).await?;
        let (stored, auth) = wire::parse_register_reply(&body, &fid)?;
        self.store.set(&store_key(&self.app.name), stored.encode());
        *self.stored.lock().expect("stored lock") = Some(stored.clone());
        *self.cached.lock().expect("cached lock") = Some(CachedToken {
            token: auth.token,
            expires_at_ms: now_ms().saturating_add(auth.expires_in_ms),
        });
        Ok(stored)
    }

    /// Sends one request and gives the reply body; a non-2xx maps through
    /// [`crate::error::reply_error`], a transport failure keeps the
    /// core's stable codes.
    async fn send_body(
        &self,
        request_name: &str,
        request: nsh_core::platform::HttpRequest,
    ) -> Result<String, FirebaseError> {
        let response = send(request).await?;
        let status = response.status();
        let body = response.text();
        if !response.ok() {
            return Err(reply_error(request_name, status, &body));
        }
        Ok(body)
    }

    /// The native tier's path: the host answers `{id}` or `{token}`;
    /// anything else (a missing field, a refusal, a failed handler) is
    /// the host's own error, unchanged — the fake project's rejection
    /// rides it.
    async fn call_host(
        &self,
        name: &str,
        field: &str,
        noun: &str,
    ) -> Result<String, FirebaseError> {
        let reply = self.host.call(name, JsonValue::Null).await?;
        reply
            .get(field)
            .and_then(JsonValue::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                FirebaseError::new(HOST_ERROR, format!("`{name}` answered without {noun}"))
            })
    }
}

fn now_ms() -> u64 {
    // The engine thread's clock: a manual one in tests, so this never
    // moves on its own there.
    unix_ms().max(0.0) as u64
}
