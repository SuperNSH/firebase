//! Client-generated push ids with the Firebase ordering properties.
//!
//! The algorithm is the official client's `nextPushId`
//! (`@firebase/database`, `packages/database/src/core/util/NextPushId.ts`,
//! the keys the REST `POST` of
//! <https://firebase.google.com/docs/database/rest/save-data> describes):
//! 8 characters of the millisecond timestamp in the 64-character
//! web-safe-ordered alphabet, then 12 characters (72 bits) of
//! per-client randomness. Ids sort lexicographically in time order and
//! keep increasing when two are made within the same millisecond.

use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::Mutex;

/// The SDK's `PUSH_CHARS`, "modeled after base64 web-safe chars, but
/// ordered by ASCII" (its words): 64 characters, so a base-64 digit is
/// one of them.
const PUSH_CHARS: &[u8; 64] = b"-0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";

/// How many ids a push id has: 8 of timestamp, 12 of randomness.
const RAND_CHARS: usize = 12;

struct Generator {
    /// The timestamp of the last id, to detect two pushes in one
    /// millisecond.
    last_ms: i64,
    /// The last randomness, "incremented" by one on a duplicate time, so
    /// the later id still sorts after.
    rand: [u8; RAND_CHARS],
}

static GENERATOR: Mutex<Generator> = Mutex::new(Generator {
    last_ms: 0,
    rand: [0; RAND_CHARS],
});

/// One push id for `now_ms` (the app clock's millisecond: a manual test
/// clock makes same-millisecond pushes repeatable).
pub fn push_id(now_ms: i64) -> String {
    let mut maker = GENERATOR.lock().expect("push id generator");
    let duplicate = now_ms == maker.last_ms;
    maker.last_ms = now_ms;
    let mut now = now_ms;
    let mut chars = [0u8; 8 + RAND_CHARS];
    for slot in chars[..8].iter_mut().rev() {
        *slot = PUSH_CHARS[(now % 64) as usize];
        // Division, not a shift: JS numbers lose the high bits shifting.
        now /= 64;
    }
    assert!(
        now == 0,
        "cannot push at a time past the 8 timestamp characters"
    );
    if duplicate {
        // Same millisecond: keep the randomness, incremented by one, so
        // this id sorts right after the last one.
        let mut i = RAND_CHARS - 1;
        while i > 0 && maker.rand[i] == 63 {
            maker.rand[i] = 0;
            i -= 1;
        }
        maker.rand[i] = (maker.rand[i] + 1) % 64;
    } else {
        maker.rand = random_72_bits();
    }
    for (slot, index) in chars[8..].iter_mut().zip(maker.rand.iter()) {
        *slot = PUSH_CHARS[*index as usize];
    }
    String::from_utf8(chars.to_vec()).expect("push id characters are ASCII")
}

/// The 72 bits of per-client randomness: [`RandomState`] is seeded from
/// the OS once per id, so two clients collide with probability 64^-12.
fn random_72_bits() -> [u8; RAND_CHARS] {
    let state = RandomState::new();
    let mut rand = [0u8; RAND_CHARS];
    for (i, slot) in rand.iter_mut().enumerate() {
        let mut hasher = state.build_hasher();
        hasher.write_u8(i as u8);
        *slot = (hasher.finish() % 64) as u8;
    }
    rand
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALPHABET: &str = "-0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";

    #[test]
    fn an_id_is_twenty_alphabet_characters() {
        let id = push_id(1_767_258_000_123);
        assert_eq!(id.len(), 20, "{id}");
        assert!(id.chars().all(|c| ALPHABET.contains(c)), "{id}");
    }

    #[test]
    fn the_timestamp_is_the_first_eight_characters() {
        // Decoding the prefix back through the base-64 alphabet gives the
        // millisecond the id was made at.
        let now = 1_767_258_000_123_i64;
        let id = push_id(now);
        let mut decoded = 0_i64;
        for c in id[..8].chars() {
            decoded = decoded * 64 + ALPHABET.find(c).unwrap() as i64;
        }
        assert_eq!(decoded, now);
    }

    #[test]
    fn ids_within_one_millisecond_keep_increasing() {
        let now = 1_767_258_000_500_i64;
        let first = push_id(now);
        let second = push_id(now);
        let third = push_id(now);
        assert!(first < second, "{first} !< {second}");
        assert!(second < third, "{second} !< {third}");
    }

    #[test]
    fn a_later_millisecond_sorts_after_the_last_id() {
        let now = 1_767_258_000_900_i64;
        let last = push_id(now);
        let next_ms = push_id(now + 1);
        assert!(last < next_ms, "{last} !< {next_ms}");
    }
}
