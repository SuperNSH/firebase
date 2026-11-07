//! The storage trait the client id persists through, plus the in-memory
//! implementation. A production host backs it with the platform's store
//! (the same seam the auth area persists tokens through); a new client
//! over the same storage keeps the same client id.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Key-value persistence the analytics client may keep small state in.
/// Infallible by contract: a disk-backed implementation serves its last
/// known value on read errors and drops what it cannot write.
pub trait KeyValueStorage: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
}

/// The process-lifetime implementation tests (and default constructors)
/// use.
#[derive(Default)]
pub struct MemoryStorage(Mutex<HashMap<String, String>>);

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KeyValueStorage for MemoryStorage {
    fn get(&self, key: &str) -> Option<String> {
        self.0
            .lock()
            .expect("memory storage lock")
            .get(key)
            .cloned()
    }

    fn set(&self, key: &str, value: &str) {
        self.0
            .lock()
            .expect("memory storage lock")
            .insert(key.to_owned(), value.to_owned());
    }
}

/// The storage key the client id lives under.
pub const CLIENT_ID_KEY: &str = "nsh.firebase.analytics.client_id";

/// The stored client id, or a freshly generated one written back. Two
/// clients over the same storage share one id, which is what makes the
/// GA4 stream see one install.
pub fn ensure_client_id(storage: &dyn KeyValueStorage) -> String {
    if let Some(existing) = storage.get(CLIENT_ID_KEY)
        && !existing.is_empty()
    {
        return existing;
    }
    let id = random_client_id();
    storage.set(CLIENT_ID_KEY, &id);
    id
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A UUID-v4-shaped identifier. Not cryptographic: a per-install tag
/// mixed from the clock, a process counter and an ASLR address, which is
/// enough to tell installs apart without pulling a random-number crate.
pub fn random_client_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|since| since.as_nanos() as u64)
        .unwrap_or_default();
    let stack = &nanos as *const u64 as usize as u64;
    let mut x = nanos
        ^ COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ stack.rotate_left(17);
    let mut next = || {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut a = next();
    let mut b = next();
    // Version 4 in the third group's top nibble, the RFC 4122 variant
    // (10xx) in the fourth's, so UUID parsers accept it.
    a = (a & !(0xFu64 << 12)) | (4 << 12);
    b = (b & !(0xCu64 << 60)) | (0x8 << 60);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        a & 0xFFFF,
        (b >> 48) & 0xFFFF,
        b & 0xFFFF_FFFF_FFFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_clients_over_one_storage_share_the_client_id() {
        let storage = MemoryStorage::new();
        let first = ensure_client_id(&storage);
        let second = ensure_client_id(&storage);
        assert_eq!(first, second);
    }

    #[test]
    fn fresh_storage_gives_a_fresh_well_shaped_id() {
        let id = ensure_client_id(&MemoryStorage::new());
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        // Version 4 at index 14, variant 10xx at index 19.
        assert_eq!(id.as_bytes()[14], b'4');
        assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        assert_ne!(id, ensure_client_id(&MemoryStorage::new()));
    }
}
