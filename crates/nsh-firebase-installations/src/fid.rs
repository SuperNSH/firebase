//! FID generation, mirroring the js-SDK's `generateFid`
//! (firebase-js-sdk `packages/installations/src/helpers/generate-fid.ts`):
//! 17 random bytes, the first byte's top 4 bits replaced with the FID
//! header `0b0111`, base64url, cut to 22 characters — a valid FID is
//! exactly 22 base64url characters (132 bits, 16.5 bytes), so it starts
//! with `c`, `d`, `e` or `f` and holds only `[\w-]` after it.

/// The regex the js-SDK validates with (`/^[cdef][\w-]{21}$/`), spelled
/// out.
pub const VALID_FID_PATTERN: &str = "^[cdef][A-Za-z0-9_-]{21}$";

/// The table `bufferToBase64UrlSafe` encodes with (RFC 4648 §5, `-`/`_`
/// for `+`/`/`).
const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// One FID. Two calls collide with probability 2^-132; the identifier is
/// not a secret, so OS-seeded hashing stands in for a random-number
/// crate (the database push-id and analytics client-id do the same).
pub fn generate_fid() -> String {
    let mut bytes = [0_u8; 17];
    random_bytes(&mut bytes);
    // Replace the first 4 random bits with the constant FID header.
    bytes[0] = 0b0111_0000 + (bytes[0] % 0b0001_0000);
    let fid = base64url(&bytes);
    fid[..22].to_owned()
}

/// Whether `fid` is a shape the service takes: 22 base64url characters
/// with the FID header. A record that fails it is corrupt, not absent.
pub fn is_valid_fid(fid: &str) -> bool {
    let bytes = fid.as_bytes();
    bytes.len() == 22
        && matches!(bytes[0], b'c'..=b'f')
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
}

fn random_bytes(out: &mut [u8]) {
    use std::hash::{BuildHasher, Hasher, RandomState};
    // RandomState seeds from the OS per construction; a fresh hasher per
    // byte keeps one compromised stream from correlating the rest.
    for (i, slot) in out.iter_mut().enumerate() {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u8(i as u8);
        hasher.write_u64(nanos());
        *slot = (hasher.finish() >> (i % 8 * 8)) as u8;
    }
}

fn nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|since| since.as_nanos() as u64)
        .unwrap_or_default()
}

/// RFC 4648 §5 without padding: 17 bytes land at 23 characters (the last
/// carries 4 bits), and the caller cuts at 22 like the js-SDK's
/// `substr(0, 22)` does.
fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[n as usize & 63] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_fid_is_22_base64url_characters_with_the_header() {
        let fid = generate_fid();
        assert_eq!(fid.len(), 22, "{fid}");
        assert!(
            fid.chars().next().is_some_and(|c| matches!(c, 'c'..='f')),
            "{fid} must start with the 0b0111 header's c-f"
        );
        assert!(
            fid.chars()
                .skip(1)
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{fid} must be base64url"
        );
        assert!(is_valid_fid(&fid), "{fid}");
    }

    #[test]
    fn two_generated_fids_differ() {
        assert_ne!(generate_fid(), generate_fid());
    }

    #[test]
    fn the_validation_rejects_wrong_shapes() {
        assert!(!is_valid_fid(""));
        // Wrong length, wrong header nibble (a standard base64 'a' lead),
        // and a character outside the alphabet.
        assert!(!is_valid_fid("a0123456789abcdef-_xyz1"));
        assert!(!is_valid_fid("c0123456789abcdef-_xy"));
        assert!(!is_valid_fid("c0123456789abcdef-_xy$"));
    }

    #[test]
    fn base64url_matches_the_reference_vectors() {
        // "foobar" is the RFC 4648 test vector; the fb-ff-be bytes cover
        // both `+`→`-` and `/`→`_` (standard base64 would give "+/+8").
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64url(&[0xfb, 0xff, 0xbe]), "-_-_");
    }
}
