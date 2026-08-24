//! Cryptographic and hashing helpers shared by backend crates.

use xxhash_rust::xxh3::xxh3_64;

/// Base62 encoding primitives.
pub mod base62;

pub use base62::encode as encode_base62;

/// Computes the compact base62 encoding of the XXH3-64 digest of `data`.
///
/// XXH3 is used instead of a cryptographic hash because this identifier only
/// needs a fast, stable change detector — not tamper resistance. The 64-bit
/// digest is encoded in full base62 (up to 11 characters, `"0"` for a zero
/// digest).
///
/// # Examples
///
/// ```
/// use arachnea_core::crypt::hash62_64;
///
/// let first = hash62_64(b"service.yaml");
/// let second = hash62_64(b"service.yaml");
/// assert_eq!(first, second);
/// assert_ne!(first, hash62_64(b"service-2.yaml"));
/// ```
pub fn hash62_64(data: &[u8]) -> String {
    base62::encode(xxh3_64(data) as u128)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash62_64_is_deterministic_compact_and_base62() {
        let first = hash62_64(b"service.yaml");
        let second = hash62_64(b"service.yaml");
        assert_eq!(first, second);
        assert!(!first.is_empty());
        // 64 bits over log2(62) ≈ 5.954 bits per character → at most 11 chars.
        assert!(first.len() <= 11);
        assert!(first.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(first, hash62_64(b"different.yaml"));
    }
}