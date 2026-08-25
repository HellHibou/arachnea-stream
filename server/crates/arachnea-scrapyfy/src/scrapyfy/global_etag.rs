//! Global ETag construction and decoding for aggregated scraper responses.
//!
//! The global ETag is a `;`-separated sequence of self-describing
//! `name=fragment` pairs covering **only** the sources that produced a
//! validation fragment (`E:...` or `C:...`). Sources without a recorded
//! validation outcome — static queries, fetch-less or failed sources —
//! contribute no tag at all and are simply absent from the value, so a
//! returned ETag never claims freshness it cannot back.

use std::collections::HashMap;

/// Separator between the `name=fragment` blocks of the global ETag.
pub const GLOBAL_ETAG_SEPARATOR: char = ';';

fn encode_name(name: &str) -> String {
    urlencoding::encode(name).to_string()
}

fn decode_name(raw: &str) -> String {
    urlencoding::decode(raw)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| raw.to_string())
}

/// Builds the global ETag from the per-source validation fragments.
///
/// Pairs are emitted following the alphabetically sorted source names so the
/// value stays deterministic across identical requests. Sources without a
/// recorded fragment are excluded: they cannot be revalidated, therefore they
/// must not participate in the returned tag.
///
/// # Arguments
///
/// * `fragments` - Up-to-date validator fragment per source name.
///
/// # Returns
///
/// The global ETag string, or `None` when no source carries a fragment.
pub fn build_global_etag(fragments: &HashMap<String, String>) -> Option<String> {
    if fragments.is_empty() {
        return None;
    }

    let mut names: Vec<&String> = fragments.keys().collect();
    names.sort();

    let pairs: Vec<String> = names
        .iter()
        .map(|name| {
            let name: &String = name;
            format!("{}={}", encode_name(name), fragments[name].as_str())
        })
        .collect();

    Some(pairs.join(GLOBAL_ETAG_SEPARATOR.to_string().as_str()))
}

/// Normalizes a raw client ETag header value before comparison.
///
/// Trims surrounding whitespace, drops the weak-validator `W/` prefix and the
/// enclosing double quotes so stored fragments can be compared verbatim.
///
/// # Arguments
///
/// * `value` - Raw `If-None-Match` header value.
pub fn normalize_client_etag(value: &str) -> &str {
    let value = value.trim();
    let value = value.strip_prefix("W/").unwrap_or(value);
    match (value.starts_with('"'), value.ends_with('"'), value.len()) {
        (true, true, length) if length >= 2 => &value[1..length - 1],
        _ => value,
    }
}

/// Decodes the per-source validator fragments carried by a client global ETag.
///
/// Malformed parts are silently ignored so an outdated tag shape (or a
/// truncated header) degrades to a partial map instead of failing the whole
/// request; missing sources are then simply treated as fresh by the caller.
///
/// # Arguments
///
/// * `etag` - Raw client global ETag value.
///
/// # Returns
///
/// The decoded fragment per source name. Empty when the tag carries no
/// usable pair.
pub fn decode_client_fragments(etag: &str) -> HashMap<String, String> {
    use super::fragment_yaml_hash;

    etag.split(GLOBAL_ETAG_SEPARATOR)
        .filter_map(|part| part.split_once('='))
        .map(|(raw_name, fragment)| (decode_name(raw_name), fragment.to_string()))
        .filter(|(name, fragment)| {
            !name.is_empty() && fragment_yaml_hash(fragment).is_some()
        })
        .collect()
}

#[cfg(test)]
mod tests {
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
            decode_client_fragments(&etag).get("a;b=c").map(String::as_str),
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
        assert_eq!(decoded.get("c").map(String::as_str), Some("C:3hY0a9c2X-0-deadbeef"));
    }

    #[test]
    fn decode_ignores_malformed_parts() {
        let decoded = decode_client_fragments(
            "S:00000000;E:bad;;valid=E:3hY0aBc2Xf-w2;=;broken=E:;",
        );
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded.get("valid").map(String::as_str), Some("E:3hY0aBc2Xf-w2"));
    }

    #[test]
    fn decode_of_legacy_positional_etag_is_empty() {
        // Tags built before the named-pairs format carry no `name=` pair and
        // degrade to an empty map (full re-fetch) instead of failing.
        assert!(decode_client_fragments("S:00000000;C:3dY0aBc2Xf-0-abcd1234;E:3hY0a9c2X-w456")
            .is_empty());
    }
}

