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
                    .expect("encoded value must use the base62 alphabet") as u128
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
