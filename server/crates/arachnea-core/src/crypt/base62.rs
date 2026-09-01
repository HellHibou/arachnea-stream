//! Base62 encoding primitives.
//!
//! Base62 encodes unsigned integers using digits, uppercase letters, then
//! lowercase letters (`0-9A-Za-z`). It produces compact, URL-safe,
//! case-sensitive representations suitable for opaque identifiers such as
//! cache validators.

/// Base62 alphabet (digits, uppercase, then lowercase).
const BASE62_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Encodes an unsigned integer in base62, most significant digit first.
///
/// # Arguments
///
/// * `value` - Unsigned integer to encode.
///
/// # Returns
///
/// The base62 representation of `value`. Zero encodes as `"0"`.
///
/// # Examples
///
/// ```
/// use arachnea_core::crypt::base62::encode;
///
/// assert_eq!(encode(0), "0");
/// assert_eq!(encode(61), "z");
/// assert_eq!(encode(62), "10");
/// ```
pub fn encode(value: u128) -> String {
    if value == 0 {
        return "0".to_string();
    }

    let mut digits = Vec::new();
    let mut remaining = value;
    while remaining > 0 {
        digits.push(BASE62_ALPHABET[(remaining % 62) as usize]);
        remaining /= 62;
    }
    digits.reverse();
    String::from_utf8(digits).expect("base62 alphabet is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_zero() {
        assert_eq!(encode(0), "0");
    }

    #[test]
    fn encodes_single_digit_values() {
        assert_eq!(encode(9), "9");
        assert_eq!(encode(10), "A");
        assert_eq!(encode(35), "Z");
        assert_eq!(encode(36), "a");
        assert_eq!(encode(61), "z");
    }

    #[test]
    fn encodes_multi_digit_values() {
        assert_eq!(encode(62), "10");
        assert_eq!(encode(63), "11");
        assert_eq!(encode(3844), "100");
    }

    #[test]
    fn round_trips_through_decoding() {
        // Decode using positional weights to validate the encoding.
        fn decode(encoded: &str) -> u128 {
            encoded
                .bytes()
                .map(|byte| {
                    BASE62_ALPHABET
                        .iter()
                        .position(|candidate| *candidate == byte)
                        .expect("encoded value must use the base62 alphabet")
                        as u128
                })
                .fold(0u128, |accumulator, digit| accumulator * 62 + digit)
        }

        for value in [0u128, 1, 61, 62, 12345, u64::MAX as u128, u128::MAX] {
            assert_eq!(decode(&encode(value)), value);
        }
    }

    #[test]
    fn encodes_u128_max_with_expected_length() {
        // 128 bits over log2(62) ≈ 5.954 bits per character → at most 22 chars.
        let encoded = encode(u128::MAX);
        assert!(!encoded.is_empty());
        assert!(encoded.len() <= 22);
        assert!(encoded.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
