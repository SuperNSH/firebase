//! Persistence: the small key-value store an app implements over its own
//! device storage, the in-memory one the tests use, the adapter over the
//! app's `nsh/storage`, and the session record both this crate and the
//! TypeScript package (`packages/nsh-firebase/src/auth`) persist as one
//! JSON shape.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::json;

use crate::auth::AuthUser;

/// Where the auth keeps its session record. Implement it over whatever
/// the app persists with; the calls are synchronous and may run on the
/// engine thread (the app owns any off-thread writing).
pub trait KeyValueStore: Send + Sync {
    /// The value of `key`, `None` when it has none.
    fn get(&self, key: &str) -> Option<String>;
    /// Sets `key` to `value`.
    fn set(&self, key: &str, value: String);
    /// Removes `key`.
    fn remove(&self, key: &str);
}

/// Keeps entries in process memory; a session in it does not survive a
/// restart. What the tests use, and what a cache-only app may pass.
#[derive(Default)]
pub struct MemoryStore {
    entries: Mutex<BTreeMap<String, String>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seeds one entry, for tests that hand a store its state.
    pub fn seeded(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            entries: Mutex::new(entries.into_iter().collect()),
        }
    }
}

impl KeyValueStore for MemoryStore {
    fn get(&self, key: &str) -> Option<String> {
        self.entries
            .lock()
            .expect("memory store lock")
            .get(key)
            .cloned()
    }

    fn set(&self, key: &str, value: String) {
        self.entries
            .lock()
            .expect("memory store lock")
            .insert(key.to_owned(), value);
    }

    fn remove(&self, key: &str) {
        self.entries.lock().expect("memory store lock").remove(key);
    }
}

/// Stores the session in the app's own storage (`nsh/storage`), the same
/// entries a TSX app's `nsh/storage` holds. Its functions run on the
/// engine thread (a component's body, handler or task), like `Storage`.
pub struct EngineStore;

impl KeyValueStore for EngineStore {
    fn get(&self, key: &str) -> Option<String> {
        nsh_core::storage::Storage::get_item(key)
    }

    fn set(&self, key: &str, value: String) {
        // A full storage keeps the old record; the session then just does
        // not survive the launch, which sign-in on the next one repairs.
        let _ = nsh_core::storage::Storage::set_item(key, value);
    }

    fn remove(&self, key: &str) {
        nsh_core::storage::Storage::remove_item(key);
    }
}

/// One name per app, so two apps of a process do not share a session.
pub(crate) fn store_key(app_name: &str) -> String {
    format!("nsh.firebase.auth.{app_name}")
}

/// The persisted session record. Field names are the camelCase ones the
/// TypeScript package writes, so either side restores the other's record.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StoredSession {
    pub uid: String,
    pub email: Option<String>,
    pub is_anonymous: bool,
    pub id_token: String,
    pub refresh_token: String,
    pub expires_at_ms: u64,
    pub next_refresh_at_ms: u64,
}

impl StoredSession {
    pub(crate) fn of(user: &AuthUser) -> Self {
        Self {
            uid: user.uid.clone(),
            email: user.email.clone(),
            is_anonymous: user.is_anonymous,
            id_token: user.id_token.clone(),
            refresh_token: user.refresh_token.clone(),
            expires_at_ms: user.expires_at_ms,
            next_refresh_at_ms: user.next_refresh_at_ms,
        }
    }

    pub(crate) fn encode(&self) -> String {
        json!({
            "v": 1,
            "uid": self.uid,
            "email": self.email,
            "isAnonymous": self.is_anonymous,
            "idToken": self.id_token,
            "refreshToken": self.refresh_token,
            "expiresAtMs": self.expires_at_ms,
            "nextRefreshAtMs": self.next_refresh_at_ms,
        })
        .to_string()
    }

    /// `None` for anything not the version-1 shape: a corrupt record is
    /// treated as signed out, not as an error to surface at startup.
    pub(crate) fn decode(text: &str) -> Option<Self> {
        let record: serde_json::Value = serde_json::from_str(text).ok()?;
        let record = record.as_object()?;
        if record.get("v").and_then(serde_json::Value::as_u64) != Some(1) {
            return None;
        }
        let string = |name: &str| -> Option<String> {
            record
                .get(name)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        Some(Self {
            uid: string("uid")?,
            email: record
                .get("email")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            is_anonymous: record
                .get("isAnonymous")
                .and_then(serde_json::Value::as_bool)?,
            id_token: string("idToken")?,
            refresh_token: string("refreshToken")?,
            expires_at_ms: record
                .get("expiresAtMs")
                .and_then(serde_json::Value::as_u64)?,
            next_refresh_at_ms: record
                .get("nextRefreshAtMs")
                .and_then(serde_json::Value::as_u64)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> StoredSession {
        StoredSession {
            uid: "u1".to_owned(),
            email: Some("e@x.test".to_owned()),
            is_anonymous: false,
            id_token: "it".to_owned(),
            refresh_token: "rt".to_owned(),
            expires_at_ms: 1_000_000,
            next_refresh_at_ms: 990_000,
        }
    }

    #[test]
    fn the_record_round_trips_through_camel_case_json() {
        let text = record().encode();
        assert!(text.contains("\"isAnonymous\""));
        assert!(text.contains("\"refreshToken\""));
        let back = StoredSession::decode(&text).unwrap();
        assert_eq!(back.uid, "u1");
        assert_eq!(back.email.as_deref(), Some("e@x.test"));
        assert_eq!(back.next_refresh_at_ms, 990_000);
    }

    #[test]
    fn a_corrupt_or_wrong_version_record_is_absent() {
        assert_eq!(StoredSession::decode("not json"), None);
        assert_eq!(StoredSession::decode(r#"{"v":2}"#), None);
        assert_eq!(StoredSession::decode(r#"{"v":1,"uid":"u1"}"#), None);
    }

    #[test]
    fn the_store_key_names_the_app() {
        assert_eq!(store_key("[DEFAULT]"), "nsh.firebase.auth.[DEFAULT]");
    }

    #[test]
    fn the_memory_store_keeps_what_it_is_given() {
        let store = MemoryStore::seeded([("k".to_owned(), "v".to_owned())]);
        assert_eq!(store.get("k").as_deref(), Some("v"));
        store.set("k", "v2".to_owned());
        assert_eq!(store.get("k").as_deref(), Some("v2"));
        store.remove("k");
        assert_eq!(store.get("k"), None);
    }
}
