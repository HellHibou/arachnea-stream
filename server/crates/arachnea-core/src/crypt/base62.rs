//! Base62 encoding primitives.
//!
//! Base62 encodes unsigned integers using digits, uppercase letters, then
//! lowercase letters (`0-9A-Za-z`). It produces compact, URL-safe,
//! case-sensitive representations suitable for opaque identifiers such as
//! cache validators.

/// Base62 alphabet (digits, uppercase, then lowercase).
use crate::BASE62_ALPHABET;

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
#[path = "../../tests/crypt/base62.rs"]
mod tests;
