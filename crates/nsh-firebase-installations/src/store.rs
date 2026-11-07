//! Persistence: the small key-value store an app implements over its own
//! device storage, the in-memory one the tests use, the adapter over the
//! app's `nsh/storage`, and the installation record both this crate and
//! the TypeScript package (`packages/nsh-firebase/src/installations`)
//! persist as one JSON shape. The auth crate keeps its own `KeyValueStore`
//! trait of the same shape; installations does not depend on auth.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::json;

/// Where the installations keep their record. Implement it over whatever
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

/// Keeps entries in process memory; a record in it does not survive a
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

/// Stores the record in the app's own storage (`nsh/storage`), the same
/// entries a TSX app's `nsh/storage` holds. Its functions run on the
/// engine thread (a component's body, handler or task), like `Storage`.
pub struct EngineStore;

impl KeyValueStore for EngineStore {
    fn get(&self, key: &str) -> Option<String> {
        nsh_core::storage::Storage::get_item(key)
    }

    fn set(&self, key: &str, value: String) {
        // A full storage keeps the old record; the FID then just does not
        // survive the launch, which register on the next one repairs.
        let _ = nsh_core::storage::Storage::set_item(key, value);
    }

    fn remove(&self, key: &str) {
        nsh_core::storage::Storage::remove_item(key);
    }
}

/// One name per app, so two apps of a process do not share a record.
pub(crate) fn store_key(app_name: &str) -> String {
    format!("nsh.firebase.installations.{app_name}")
}

/// The persisted record: only what the flows cannot rebuild — the FID and
/// its refresh token. The auth token stays in process memory (it is
/// short-lived and re-generatable). Field names are the camelCase ones
/// the TypeScript package writes, so either side restores the other's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredInstallation {
    pub fid: String,
    pub refresh_token: String,
}

impl StoredInstallation {
    pub(crate) fn encode(&self) -> String {
        json!({
            "v": 1,
            "fid": self.fid,
            "refreshToken": self.refresh_token,
        })
        .to_string()
    }

    /// `None` for anything not the version-1 shape: a corrupt record is
    /// treated as unregistered, not as an error to surface at startup.
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
            fid: string("fid")?,
            refresh_token: string("refreshToken")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> StoredInstallation {
        StoredInstallation {
            fid: "c0123456789abcdef-_xyz".to_owned(),
            refresh_token: "refresh-1".to_owned(),
        }
    }

    #[test]
    fn the_record_round_trips_through_camel_case_json() {
        let text = record().encode();
        assert!(text.contains("\"refreshToken\""));
        assert!(text.contains("\"v\":1"));
        let back = StoredInstallation::decode(&text).unwrap();
        assert_eq!(back, record());
    }

    #[test]
    fn a_corrupt_or_wrong_version_record_is_absent() {
        assert_eq!(StoredInstallation::decode("not json"), None);
        assert_eq!(StoredInstallation::decode(r#"{"v":2}"#), None);
        assert_eq!(
            StoredInstallation::decode(r#"{"v":1,"fid":"c0123456789abcdef-_xyz"}"#),
            None
        );
    }

    #[test]
    fn the_store_key_names_the_app() {
        assert_eq!(
            store_key("[DEFAULT]"),
            "nsh.firebase.installations.[DEFAULT]"
        );
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
