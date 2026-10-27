use super::*;

#[test]
fn build_global_etag_sorts_pairs_by_source_name() {
    let mut fragments = HashMap::new();
    fragments.insert("b".to_string(), "E:3hY0a9c2X-w456".to_string());
    fragments.insert("a".to_string(), "C:3dY0aBc2Xf-0-abcd1234".to_string());

    let etag = build_global_etag(&fragments).expect("etag should build");
    assert_eq!(etag, "a=C:3dY0aBc2Xf-0-abcd1234;b=E:3hY0a9c2X-w456");
}

#[test]
fn global_etag_single_source_has_no_separator() {
    let mut fragments = HashMap::new();
    fragments.insert("only".to_string(), "E:3hY0a9c2X-w456".to_string());

    let etag = build_global_etag(&fragments).expect("etag should build");
    assert_eq!(etag, "only=E:3hY0a9c2X-w456");
}

#[test]
fn sources_without_fragment_are_excluded_from_the_tag() {
    // Only validated sources take part in the tag; there is no fallback
    // fragment for fetch-less or failed sources.
    assert!(build_global_etag(&HashMap::new()).is_none());
}

#[test]
fn build_encodes_source_names_safely() {
    let mut fragments = HashMap::new();
    fragments.insert("a;b=c".to_string(), "E:3hY0a9c2X-w1".to_string());

    let etag = build_global_etag(&fragments).expect("etag should build");
    assert_eq!(etag, "a%3Bb%3Dc=E:3hY0a9c2X-w1");
    assert_eq!(
        decode_client_fragments(&etag)
            .get("a;b=c")
            .map(String::as_str),
        Some("E:3hY0a9c2X-w1")
    );
}

#[test]
fn decode_round_trips_built_etag() {
    let mut fragments = HashMap::new();
    fragments.insert("a".to_string(), "E:3hY0a9c2X-w1".to_string());
    fragments.insert("b".to_string(), "E:3hY0a9c2X-w2".to_string());
    fragments.insert("c".to_string(), "C:3hY0a9c2X-0-deadbeef".to_string());

    let etag = build_global_etag(&fragments).expect("etag should build");
    let decoded = decode_client_fragments(&etag);
    assert_eq!(decoded.get("a").map(String::as_str), Some("E:3hY0a9c2X-w1"));
    assert_eq!(decoded.get("b").map(String::as_str), Some("E:3hY0a9c2X-w2"));
    assert_eq!(
        decoded.get("c").map(String::as_str),
        Some("C:3hY0a9c2X-0-deadbeef")
    );
}

#[test]
fn decode_ignores_malformed_parts() {
    let decoded = decode_client_fragments("S:00000000;E:bad;;valid=E:3hY0aBc2Xf-w2;=;broken=E:;");
    assert_eq!(decoded.len(), 1);
    assert_eq!(
        decoded.get("valid").map(String::as_str),
        Some("E:3hY0aBc2Xf-w2")
    );
}

#[test]
fn decode_of_legacy_positional_etag_is_empty() {
    // Tags built before the named-pairs format carry no `name=` pair and
    // degrade to an empty map (full re-fetch) instead of failing.
    assert!(
        decode_client_fragments("S:00000000;C:3dY0aBc2Xf-0-abcd1234;E:3hY0a9c2X-w456").is_empty()
    );
}
