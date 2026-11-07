//! The auth itself: `AuthUser`, `Credential` and [`Auth`]. Every network
//! method is an async fn the app drives on the engine thread (`spawn` in
//! a component's handler): that is where the engine's `fetch` and clock
//! live, and the clock is what [`Auth::refresh_due`] compares against.
//! The TypeScript package `packages/nsh-firebase/src/auth` mirrors this
//! behavior name for name, down to the persisted JSON.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use nsh_core::element::create_root;
use nsh_core::prelude::{Owner, Signal, signal};
use nsh_core::time::unix_ms;
use nsh_firebase_core::error::UNAVAILABLE;
use nsh_firebase_core::{FirebaseError, send};
use serde_json::Value as JsonValue;
use serde_json::json;

use crate::error::{
    INVALID_ARGUMENT, INVALID_RESPONSE, NO_CURRENT_USER, REFRESH_IN_PROGRESS, is_session_lost,
};
use crate::store::{KeyValueStore, StoredSession, store_key};
use crate::wire::{identity_request, parse_session_reply, refresh_request, uid_from_id_token};

/// Refreshing starts this long before expiry, like the Firebase SDKs'
/// five-minute margin.
pub const REFRESH_MARGIN_MS: u64 = 5 * 60 * 1000;

/// The signed-in user. `expires_at_ms` and `next_refresh_at_ms` are
/// absolute wall-clock milliseconds (`unix_ms` on the app's clock, so a
/// manual clock moves them only when the test moves it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthUser {
    pub uid: String,
    pub email: Option<String>,
    pub is_anonymous: bool,
    pub id_token: String,
    pub refresh_token: String,
    pub expires_at_ms: u64,
    pub next_refresh_at_ms: u64,
}

impl From<StoredSession> for AuthUser {
    fn from(stored: StoredSession) -> Self {
        Self {
            uid: stored.uid,
            email: stored.email,
            is_anonymous: stored.is_anonymous,
            id_token: stored.id_token,
            refresh_token: stored.refresh_token,
            expires_at_ms: stored.expires_at_ms,
            next_refresh_at_ms: stored.next_refresh_at_ms,
        }
    }
}

/// What [`Auth::sign_in_with_credential`] accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// Email and password (`accounts:signInWithPassword`).
    Password { email: String, password: String },
    /// A custom token the project's backend minted
    /// (`accounts:signInWithCustomToken`).
    CustomToken(String),
    /// An ID token minted by another library for this project
    /// (`accounts:verifyCustomToken`).
    IdToken(String),
}

/// Clears its flag however the refresh ends, including a dropped (cancelled)
/// future: nothing can leave the single-flight flag stuck.
struct RefreshGuard<'a> {
    flag: &'a AtomicBool,
}

impl<'a> RefreshGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Result<Self, FirebaseError> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self { flag })
            .map_err(|_| {
                FirebaseError::new(
                    REFRESH_IN_PROGRESS,
                    "a token refresh is already in flight; wait for it",
                )
            })
    }
}

impl Drop for RefreshGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

/// The auth for one app. Create it where an owner exists (a component's
/// body): the `currentUser` signal outlives that component either way —
/// it is created under its own root, and `restore` reads the store once
/// here, before the first frame.
pub struct Auth {
    app: Arc<nsh_firebase_core::FirebaseApp>,
    store: Arc<dyn KeyValueStore>,
    user: Signal<Option<AuthUser>>,
    _owner: Owner,
    refreshing: AtomicBool,
    ready: AtomicBool,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth")
            .field("app", &self.app.name)
            .field("signed_in", &self.current_user().is_some())
            .finish_non_exhaustive()
    }
}

impl Auth {
    pub fn new(app: Arc<nsh_firebase_core::FirebaseApp>, store: Arc<dyn KeyValueStore>) -> Self {
        // The root is never disposed: an Auth handed around the app must
        // not lose its signal when the component that made it is replaced.
        let (_owner, user) = create_root(|| signal(None::<AuthUser>));
        let auth = Self {
            app,
            store,
            user,
            _owner,
            refreshing: AtomicBool::new(false),
            ready: AtomicBool::new(false),
        };
        auth.restore();
        auth
    }

    /// The `currentUser` signal, `None` while signed out. A binding that
    /// reads it updates through every sign-in, refresh and sign-out.
    pub fn user(&self) -> Signal<Option<AuthUser>> {
        self.user
    }

    /// The user right now, if one is signed in.
    pub fn current_user(&self) -> Option<AuthUser> {
        self.user.get()
    }

    /// Whether the initial restore finished. `Auth::new` restores, so this
    /// is `true` from the first read (the TSX `authStateReady` resolves the
    /// same way; its storage is async, so the promise still matters there).
    pub fn state_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// Re-reads the store into `currentUser`. A record the other tier (or
    /// an older launch) wrote restores too; a corrupt one is signed out.
    pub fn restore(&self) {
        let user = self
            .store
            .get(&store_key(&self.app.name))
            .as_deref()
            .and_then(StoredSession::decode)
            .map(AuthUser::from);
        self.user.set(user);
        self.ready.store(true, Ordering::Release);
    }

    /// Creates the account (`accounts:signUp`) and signs in with it.
    pub async fn create_user_with_email_and_password(
        &self,
        email: &str,
        password: &str,
    ) -> Result<AuthUser, FirebaseError> {
        require_text(email, "email")?;
        require_text(password, "password")?;
        let session = self
            .post_identity(
                "accounts:signUp",
                &json!({ "email": email, "password": password, "returnSecureToken": true }),
            )
            .await?;
        self.adopt(session, false)
    }

    /// Signs in with email and password (`accounts:signInWithPassword`).
    pub async fn sign_in_with_email_and_password(
        &self,
        email: &str,
        password: &str,
    ) -> Result<AuthUser, FirebaseError> {
        require_text(email, "email")?;
        require_text(password, "password")?;
        let session = self
            .post_identity(
                "accounts:signInWithPassword",
                &json!({ "email": email, "password": password, "returnSecureToken": true }),
            )
            .await?;
        self.adopt(session, false)
    }

    /// Signs in anonymously: `accounts:signUp` with no email and no
    /// password, the shape the docs give anonymous sign-up (and the
    /// emulator serves); there is no `accounts:signInAnonymously`.
    pub async fn sign_in_anonymously(&self) -> Result<AuthUser, FirebaseError> {
        let session = self
            .post_identity("accounts:signUp", &json!({ "returnSecureToken": true }))
            .await?;
        self.adopt(session, true)
    }

    /// Exchanges a custom token for a session
    /// (`accounts:signInWithCustomToken`).
    pub async fn sign_in_with_custom_token(&self, token: &str) -> Result<AuthUser, FirebaseError> {
        require_text(token, "token")?;
        let session = self
            .post_identity(
                "accounts:signInWithCustomToken",
                &json!({ "token": token, "returnSecureToken": true }),
            )
            .await?;
        self.adopt(session, false)
    }

    /// Signs in with a credential: a password pair, a custom token, or an
    /// ID token another library minted for this project (sent to
    /// `accounts:verifyCustomToken`).
    pub async fn sign_in_with_credential(
        &self,
        credential: &Credential,
    ) -> Result<AuthUser, FirebaseError> {
        match credential {
            Credential::Password { email, password } => {
                self.sign_in_with_email_and_password(email, password).await
            }
            Credential::CustomToken(token) => self.sign_in_with_custom_token(token).await,
            Credential::IdToken(id_token) => {
                require_text(id_token, "idToken")?;
                let session = self
                    .post_identity(
                        "accounts:verifyCustomToken",
                        &json!({ "token": id_token, "returnSecureToken": true }),
                    )
                    .await?;
                self.adopt(session, false)
            }
        }
    }

    /// Whether the current user's token reaches its refresh point on the
    /// clock's time now. The schedule-check: call it whenever the app
    /// chooses (on start, on a screen, on a timer it owns) and
    /// [`Auth::refresh_now`] when it answers `true`.
    pub fn refresh_due(&self) -> bool {
        self.current_user()
            .is_some_and(|user| now_ms() >= user.next_refresh_at_ms)
    }

    /// When the next refresh is due, in wall-clock milliseconds; `None`
    /// while signed out.
    pub fn next_refresh_at_ms(&self) -> Option<u64> {
        self.current_user().map(|user| user.next_refresh_at_ms)
    }

    /// Refreshes now (`securetoken.googleapis.com/v1/token`), whatever the
    /// schedule says. One runs at a time: a second concurrent call rejects
    /// with `auth/refresh-in-progress`. A session the server has lost
    /// (`auth/user-token-expired`, `auth/invalid-refresh-token`,
    /// `auth/user-not-found`) signs the user out: it cannot recover.
    pub async fn refresh_now(&self) -> Result<AuthUser, FirebaseError> {
        let _guard = RefreshGuard::acquire(&self.refreshing)?;
        let current = self.current_user().ok_or_else(|| {
            FirebaseError::new(NO_CURRENT_USER, "no user is signed in; nothing to refresh")
        })?;
        let base = self.auth_base_url()?;
        let request = refresh_request(&base, &self.app.options.api_key, &current.refresh_token);
        let session = match self.send_and_parse(request, true).await {
            Err(error) if is_session_lost(&error.code) => {
                self.set_user(None);
                return Err(error);
            }
            other => other?,
        };
        // The refresh reply names no email and may name no uid and no new
        // refresh token; those carry over from the session it renewed.
        let uid = session
            .local_id
            .or_else(|| uid_from_id_token(&session.id_token))
            .unwrap_or_else(|| current.uid.clone());
        let refresh_token = if session.refresh_token.is_empty() {
            current.refresh_token.clone()
        } else {
            session.refresh_token
        };
        let now = now_ms();
        let expires_at_ms = now.saturating_add(session.expires_in_secs.saturating_mul(1000));
        let user = AuthUser {
            uid,
            email: current.email,
            is_anonymous: current.is_anonymous,
            id_token: session.id_token,
            refresh_token,
            expires_at_ms,
            next_refresh_at_ms: expires_at_ms.saturating_sub(REFRESH_MARGIN_MS),
        };
        self.set_user(Some(user.clone()));
        Ok(user)
    }

    /// Signs out: the signal clears and the record leaves the store.
    /// These providers have nothing to revoke server-side.
    pub fn sign_out(&self) {
        self.set_user(None);
    }

    /// Puts a fresh session in the signal and the store.
    fn adopt(
        &self,
        session: crate::wire::Session,
        is_anonymous: bool,
    ) -> Result<AuthUser, FirebaseError> {
        let uid = session
            .local_id
            .or_else(|| uid_from_id_token(&session.id_token))
            .ok_or_else(|| {
                FirebaseError::new(
                    INVALID_RESPONSE,
                    "neither the reply nor the idToken names the user",
                )
            })?;
        let now = now_ms();
        let expires_at_ms = now.saturating_add(session.expires_in_secs.saturating_mul(1000));
        let user = AuthUser {
            uid,
            email: session.email,
            is_anonymous,
            id_token: session.id_token,
            refresh_token: session.refresh_token,
            expires_at_ms,
            next_refresh_at_ms: expires_at_ms.saturating_sub(REFRESH_MARGIN_MS),
        };
        self.set_user(Some(user.clone()));
        Ok(user)
    }

    fn set_user(&self, user: Option<AuthUser>) {
        self.user.set(user.clone());
        let key = store_key(&self.app.name);
        match user {
            Some(user) => self.store.set(&key, StoredSession::of(&user).encode()),
            None => self.store.remove(&key),
        }
    }

    async fn post_identity(
        &self,
        operation: &str,
        body: &JsonValue,
    ) -> Result<crate::wire::Session, FirebaseError> {
        let base = self.auth_base_url()?;
        let request = identity_request(&base, &self.app.options.api_key, operation, body)?;
        self.send_and_parse(request, false).await
    }

    async fn send_and_parse(
        &self,
        request: nsh_core::platform::HttpRequest,
        accept_access_token: bool,
    ) -> Result<crate::wire::Session, FirebaseError> {
        // A transport failure keeps the core's stable codes; only a
        // server reply maps to an auth code.
        let response = send(request).await?;
        if !response.ok() {
            let status = response.status();
            let body = response.text();
            let wire = serde_json::from_str::<JsonValue>(&body)
                .ok()
                .and_then(|reply| {
                    reply
                        .get("error")
                        .and_then(|error| error.get("message"))
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned)
                });
            return Err(match wire {
                Some(wire) => crate::error::wire_error(status, &wire),
                None if status >= 500 => FirebaseError::new(
                    crate::error::INTERNAL_ERROR,
                    format!("the auth server failed (HTTP {status})"),
                ),
                None => FirebaseError::new(
                    crate::error::REQUEST_FAILED,
                    format!("the server replied {status} without an error message"),
                ),
            });
        }
        parse_session_reply(&response.text(), accept_access_token)
    }

    fn auth_base_url(&self) -> Result<String, FirebaseError> {
        self.app.options.auth_base_url().ok_or_else(|| {
            FirebaseError::new(
                UNAVAILABLE,
                "auth has no base URL: set authUrl or the auth emulator switch in the app options",
            )
        })
    }
}

fn require_text(value: &str, what: &str) -> Result<(), FirebaseError> {
    if value.trim().is_empty() {
        Err(FirebaseError::new(
            INVALID_ARGUMENT,
            format!("{what} is empty"),
        ))
    } else {
        Ok(())
    }
}

fn now_ms() -> u64 {
    // The engine thread's clock: a manual one in tests, so this never
    // moves on its own there.
    unix_ms().max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_arguments_are_rejected_before_any_request() {
        assert_eq!(
            require_text("", "email").unwrap_err().code,
            INVALID_ARGUMENT
        );
        assert_eq!(
            require_text("   ", "password").unwrap_err().code,
            INVALID_ARGUMENT
        );
        require_text("e@x.test", "email").unwrap();
    }

    #[test]
    fn the_refresh_flag_blocks_a_second_refresh_and_drops_clear() {
        let flag = AtomicBool::new(false);
        {
            let _guard = RefreshGuard::acquire(&flag).unwrap();
            let second = match RefreshGuard::acquire(&flag) {
                Err(error) => error.code,
                Ok(_) => panic!("a second refresh must not start while one runs"),
            };
            assert_eq!(second, REFRESH_IN_PROGRESS);
        }
        // Dropping the first guard cleared the flag: a refresh can start.
        RefreshGuard::acquire(&flag).unwrap();
    }
}
