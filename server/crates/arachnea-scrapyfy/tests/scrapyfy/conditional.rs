use super::*;

/// Fixed YAML hash used by fragment tests.
const YAML_HASH: &str = "3dY0aBc2Xf";

#[test]
fn hash62_is_deterministic_full_base62() {
    let first = hash62(b"example-body");
    let second = hash62(b"example-body");
    assert_eq!(first, second);
    assert!(!first.is_empty());
    assert!(first.len() <= 22);
    assert!(first.chars().all(|c| c.is_ascii_alphanumeric()));
    assert_ne!(first, hash62(b"different-body"));
}

#[test]
fn etag_fragment_round_trip() {
    let quoted_etag = r#""w456""#;
    let fragment = fragment_from_response(YAML_HASH, Some(quoted_etag), None, "body");
    assert_eq!(fragment, "E:3dY0aBc2Xf-%22w456%22");

    let conditional = conditional_from_fragment(&fragment).expect("fragment should parse");
    assert_eq!(conditional.if_none_match.as_deref(), Some(quoted_etag));
    assert!(conditional.content_hash.is_none());
}

#[test]
fn etag_fragment_round_trip_keeps_dashes_in_encoded_etag() {
    // The remote ETag may contain literal `-`; only the first `-` after
    // the YAML hash separates the hash segment.
    let quoted_etag = r#""w/123-abc""#;
    let fragment = fragment_from_response(YAML_HASH, Some(quoted_etag), None, "body");
    assert_eq!(fragment, "E:3dY0aBc2Xf-%22w%2F123-abc%22");

    let conditional = conditional_from_fragment(&fragment).expect("fragment should parse");
    assert_eq!(conditional.if_none_match.as_deref(), Some(quoted_etag));

    assert_eq!(fragment_yaml_hash(&fragment), Some(YAML_HASH));
}

#[test]
fn content_fragment_round_trip_without_last_modified() {
    let body = "example-body";
    let fragment = fragment_from_response(YAML_HASH, None, None, body);
    assert!(fragment.starts_with("C:3dY0aBc2Xf-0-"));

    let conditional = conditional_from_fragment(&fragment).expect("fragment should parse");
    assert_eq!(
        conditional.content_hash.as_deref(),
        Some(&hash62(body.as_bytes())[..])
    );
    assert!(conditional.if_modified_since.is_none());
    assert_eq!(fragment_yaml_hash(&fragment), Some(YAML_HASH));
}

#[test]
fn content_fragment_keeps_last_modified_timestamp() {
    let fragment = fragment_from_response(
        YAML_HASH,
        None,
        Some("Wed, 21 Oct 2015 07:28:00 GMT"),
        "body",
    );
    let conditional = conditional_from_fragment(&fragment).expect("fragment should parse");
    assert!(conditional.if_modified_since.is_some());
    assert_eq!(
        conditional.content_hash.as_deref(),
        Some(&hash62(b"body")[..])
    );
}

#[test]
fn fragment_yaml_hash_extracts_for_all_prefixes() {
    assert_eq!(
        fragment_yaml_hash("E:3dY0aBc2Xf-%22w456%22"),
        Some(YAML_HASH)
    );
    assert_eq!(
        fragment_yaml_hash("C:3dY0aBc2Xf-0-deadbeef"),
        Some(YAML_HASH)
    );
    assert_eq!(fragment_yaml_hash("N:3dY0aBc2Xf-aBcDe"), Some(YAML_HASH));
}

#[test]
fn legacy_fragments_without_yaml_hash_are_divergent() {
    // Fragments produced before the YAML hash was introduced do not match
    // the current grammar and must never be applied as conditional headers.
    assert!(conditional_from_fragment("E:w456").is_none());
    assert!(fragment_yaml_hash("E:w456").is_none());
    assert_eq!(fragment_yaml_hash("C:0-abcd1234"), Some("0"));
    assert!(fragment_yaml_hash("N:abcd1234").is_none());
}

#[test]
fn malformed_fragments_are_rejected() {
    assert!(conditional_from_fragment("").is_none());
    assert!(conditional_from_fragment("X:abc").is_none());
    assert!(conditional_from_fragment("C:notanumber-abcd1234").is_none());
    assert!(conditional_from_fragment("E:").is_none());
    assert!(conditional_from_fragment("-w456").is_none());
}

#[test]
fn yaml_hash_mismatch_is_detected() {
    let fragment = fragment_from_response("AAAAAAAAAAA", Some(r#""w456""#), None, "body");
    assert_eq!(fragment_yaml_hash(&fragment), Some("AAAAAAAAAAA"));
    assert_ne!(fragment_yaml_hash(&fragment), Some(YAML_HASH));
}
