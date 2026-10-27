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
