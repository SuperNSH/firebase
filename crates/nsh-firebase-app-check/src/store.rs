//! Persistence: the key-value seam the token cache lives behind (an app
//! implements it over its own device storage), the in-memory one the
//! tests use, the adapter over the app's `nsh/storage`, and the cache
//! record both this crate and the TypeScript package
//! (`packages/nsh-firebase/src/app-check`) persist as one JSON shape.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::json;

/// Where the App Check keeps its cached token. The calls are synchronous
/// and may run on the engine thread (the app owns any off-thread
/// writing); expiry, not removal, is what refreshes the record.
pub trait KeyValueStore: Send + Sync {
    /// The value of `key`, `None` when it has none.
    fn get(&self, key: &str) -> Option<String>;
    /// Sets `key` to `value`.
    fn set(&self, key: &str, value: String);
}

/// Keeps entries in process memory; a cache in it does not survive a
/// restart. What the tests use, and what a cache-only app may pass.
#[derive(Default)]
pub struct MemoryStore {
    entries: Mutex<BTreeMap<String, String>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seeds entries, for tests that hand a store its state.
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
}

/// Stores the cache in the app's own storage (`nsh/storage`), the same
/// entries a TSX app's `nsh/storage` holds. Its functions run on the
/// engine thread (a component's body, handler or task), like `Storage`.
pub struct EngineStore;

impl KeyValueStore for EngineStore {
    fn get(&self, key: &str) -> Option<String> {
        nsh_core::storage::Storage::get_item(key)
    }

    fn set(&self, key: &str, value: String) {
        // A full storage keeps the old record; the cache then just does
        // not survive the launch, which the next exchange repairs.
        let _ = nsh_core::storage::Storage::set_item(key, value);
    }
}

/// One name per app, so two apps of a process do not share a cache.
pub(crate) fn store_key(app_name: &str) -> String {
    format!("nsh.firebase.app-check.{app_name}")
}

/// The cached token. Field names are the camelCase ones the TypeScript
/// package writes, so either tier restores the other's record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CachedToken {
    pub token: String,
    pub expires_at_ms: u64,
}

impl CachedToken {
    pub(crate) fn encode(&self) -> String {
        json!({
            "v": 1,
            "token": self.token,
            "expiresAtMs": self.expires_at_ms,
        })
        .to_string()
    }

    /// `None` for anything not the version-1 shape: a corrupt record is
    /// treated as no cache, not as an error to surface at startup.
    pub(crate) fn decode(text: &str) -> Option<Self> {
        let record: serde_json::Value = serde_json::from_str(text).ok()?;
        let record = record.as_object()?;
        if record.get("v").and_then(serde_json::Value::as_u64) != Some(1) {
            return None;
        }
        Some(Self {
            token: record
                .get("token")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)?,
            expires_at_ms: record
                .get("expiresAtMs")
                .and_then(serde_json::Value::as_u64)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_round_trips_through_camel_case_json() {
        let text = CachedToken {
            token: "tok".to_owned(),
            expires_at_ms: 1_000,
        }
        .encode();
        assert!(text.contains("\"expiresAtMs\""));
        let back = CachedToken::decode(&text).unwrap();
        assert_eq!(back.token, "tok");
        assert_eq!(back.expires_at_ms, 1_000);
    }

    #[test]
    fn a_corrupt_or_wrong_version_record_is_absent() {
        assert_eq!(CachedToken::decode("not json"), None);
        assert_eq!(CachedToken::decode(r#"{"v":2}"#), None);
        assert_eq!(CachedToken::decode(r#"{"v":1,"token":"t"}"#), None);
        assert_eq!(
            CachedToken::decode(r#"{"v":1,"token":"t","expiresAtMs":-5}"#),
            None
        );
    }

    #[test]
    fn the_store_key_names_the_app() {
        assert_eq!(store_key("[DEFAULT]"), "nsh.firebase.app-check.[DEFAULT]");
    }

    #[test]
    fn the_memory_store_keeps_what_it_is_given() {
        let store = MemoryStore::seeded([("k".to_owned(), "v".to_owned())]);
        assert_eq!(store.get("k").as_deref(), Some("v"));
        store.set("k", "v2".to_owned());
        assert_eq!(store.get("k").as_deref(), Some("v2"));
    }
}
