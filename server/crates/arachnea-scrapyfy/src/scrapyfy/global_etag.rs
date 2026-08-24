//! Global ETag construction and decoding for aggregated scraper responses.
//!
//! The global ETag is a `;`-separated sequence of per-source validator
//! fragments (`E:...`, `C:...`, or `N:...`). For multi-source requests an
//! optional leading `S:<hash>` block encodes the XXH3-128 digest (full base62
//! encoding) of the alphabetically sorted service names so a change in the
//! source list invalidates every cached response without decomposing the JSON
//! payload.

use std::collections::HashMap;

use super::{hash62, NO_VALIDATION_PREFIX};

/// Separator between blocks of the global ETag.
pub const GLOBAL_ETAG_SEPARATOR: char = ';';

/// Prefix marking the multi-source services hash block.
const SERVICES_HASH_PREFIX: &str = "S:";

/// Computes the stable services hash used by the multi-source `S:` block.
///
/// # Arguments
///
/// * `services` - Service names involved in the request; order is irrelevant,
///   they are sorted alphabetically before hashing.
///
/// # Returns
///
/// The full base62 encoding of the XXH3-128 digest of the concatenated
/// sorted service names.
pub fn services_hash(services: &[&str]) -> String {
    let mut names = services.to_vec();
    names.sort();
    names.dedup();
    let concatenated = names.concat();
    hash62(concatenated.as_bytes())
}

/// Returns the deterministic fallback fragment for a source without any
/// recorded validation outcome (static queries, fetch-less sources).
///
/// The leading `N:` fragment carries the source YAML-hash when the caller can
/// provide it, so a scraper edit still invalidates the fragment and forces a
/// client reload.
fn fallback_fragment(source: &str, yaml_hash: Option<&str>) -> String {
    match yaml_hash {
        Some(hash) if !hash.is_empty() => format!(
            "{NO_VALIDATION_PREFIX}{hash}-{}",
            hash62(source.as_bytes())
        ),
        _ => format!("{NO_VALIDATION_PREFIX}{}", hash62(source.as_bytes())),
    }
}

/// Builds the global ETag from the per-source validation fragments.
///
/// Fragments are concatenated following the alphabetically sorted service
/// names so the value stays deterministic across identical requests.
///
/// # Arguments
///
/// * `services` - Service names involved in the request.
/// * `fragments` - Up-to-date fragment per source name, when validated.
/// * `yaml_hashes` - Base62 YAML-hash per source name; used to build the
///   fallback fragment for sources without a recorded validation outcome.
///
/// # Returns
///
/// The global ETag string, or `None` when no service is involved.
pub fn build_global_etag(
    services: &[&str],
    fragments: &HashMap<String, String>,
    yaml_hashes: &HashMap<String, String>,
) -> Option<String> {
    if services.is_empty() {
        return None;
    }

    let mut names = services.to_vec();
    names.sort();
    names.dedup();

    let mut etag = String::new();
    if names.len() > 1 {
        etag.push_str(SERVICES_HASH_PREFIX);
        etag.push_str(&services_hash(&names));
    }
    for name in &names {
        if !etag.is_empty() {
            etag.push(GLOBAL_ETAG_SEPARATOR);
        }
        let fragment = fragments
            .get(*name)
            .cloned()
            .unwrap_or_else(|| fallback_fragment(name, yaml_hashes.get(*name).map(String::as_str)));
        etag.push_str(&fragment);
    }

    Some(etag)
}

/// Normalizes an incoming global ETag value.
///
/// Strips surrounding quotes and the weak-validator `W/` prefix.
pub fn normalize_client_etag(value: &str) -> &str {
    let value = value.trim();
    let value = value.strip_prefix("W/").unwrap_or(value);
    value.trim_matches('"')
}

/// Decodes a client-provided global ETag into per-source fragments.
///
/// # Arguments
///
/// * `client_etag` - Raw `If-None-Match` header value previously returned by
///   [`build_global_etag`].
/// * `services` - Service names involved in the current request.
///
/// # Returns
///
/// A fragment per service name, matched by position over the alphabetically
/// sorted service list. Returns `None` when the ETag cannot be trusted
/// (malformed shape, services-hash mismatch, or wrong fragment count), in
/// which case the caller must treat every source as fresh.
pub fn decode_client_fragments(
    client_etag: &str,
    services: &[&str],
) -> Option<HashMap<String, String>> {
    if services.is_empty() {
        return None;
    }

    let mut names = services.to_vec();
    names.sort();
    names.dedup();

    let mut parts: Vec<&str> = normalize_client_etag(client_etag)
        .split(GLOBAL_ETAG_SEPARATOR)
        .collect();

    // Multi-source requests must start with a valid services hash block.
    if names.len() > 1 {
        let first = parts.first()?;
        let expected = format!("{SERVICES_HASH_PREFIX}{}", services_hash(&names));
        if *first != expected.as_str() {
            return None;
        }
        parts.remove(0);
    } else if parts.len() == 2 && parts[0].starts_with(SERVICES_HASH_PREFIX) {
        // Tolerate a stale single-source client that still carries an S: block.
        parts.remove(0);
    }

    if parts.len() != names.len() {
        return None;
    }

    Some(
        names
            .iter()
            .map(|name| (*name).to_string())
            .zip(parts.iter().map(|part| (*part).to_string()))
            .collect::<HashMap<String, String>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an empty per-service YAML-hash map for tests not exercising it.
    fn empty_hashes() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn services_hash_is_order_insensitive_and_stable() {
        let left = services_hash(&["b-source", "a-source"]);
        let right = services_hash(&["a-source", "b-source"]);
        assert_eq!(left, right);
        assert!(!left.is_empty());
        assert!(left.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn global_etag_multi_source_starts_with_services_block() {
        let mut fragments = HashMap::new();
        fragments.insert("b".to_string(), "E:3hY0a9c2X-w456".to_string());
        fragments.insert("a".to_string(), "C:3dY0aBc2Xf-0-abcd1234".to_string());

        let etag = build_global_etag(&["b", "a"], &fragments, &empty_hashes())
            .expect("etag should build");
        let hash = services_hash(&["a", "b"]);
        assert_eq!(etag, format!("S:{hash};C:3dY0aBc2Xf-0-abcd1234;E:3hY0a9c2X-w456"));
    }

    #[test]
    fn global_etag_single_source_has_no_services_block() {
        let mut fragments = HashMap::new();
        fragments.insert("only".to_string(), "E:3hY0a9c2X-w456".to_string());

        let etag = build_global_etag(&["only"], &fragments, &empty_hashes())
            .expect("etag should build");
        assert_eq!(etag, "E:3hY0a9c2X-w456");
    }

    #[test]
    fn fallback_fragment_includes_yaml_hash_when_known() {
        // Sources without a validated outcome still get the YAML hash so a
        // scraper edit invalidates their stable fragment.
        let mut yaml_hashes = HashMap::new();
        yaml_hashes.insert("static".to_string(), "3hY0a9c2Xf".to_string());
        let etag = build_global_etag(&["static"], &HashMap::new(), &yaml_hashes)
            .expect("etag should build");
        assert!(etag.starts_with("N:3hY0a9c2Xf-"));
    }

    #[test]
    fn decode_round_trips_built_etag() {
        let services = vec!["c", "a", "b"];
        let mut fragments = HashMap::new();
        fragments.insert("a".to_string(), "E:3hY0a9c2X-w1".to_string());
        fragments.insert("b".to_string(), "E:3hY0a9c2X-w2".to_string());
        fragments.insert("c".to_string(), "C:3hY0a9c2X-0-deadbeef".to_string());

        let etag = build_global_etag(&services, &fragments, &empty_hashes())
            .expect("etag should build");
        let decoded = decode_client_fragments(&etag, &services).expect("should decode");
        assert_eq!(decoded.get("a").map(String::as_str), Some("E:3hY0a9c2X-w1"));
        assert_eq!(decoded.get("b").map(String::as_str), Some("E:3hY0a9c2X-w2"));
        assert_eq!(decoded.get("c").map(String::as_str), Some("C:3hY0a9c2X-0-deadbeef"));
    }

    #[test]
    fn decode_rejects_services_hash_mismatch() {
        let services = vec!["a", "b"];
        let decoded = decode_client_fragments("S:00000000;E:w1;E:w2", &services);
        assert!(decoded.is_none());
    }

    #[test]
    fn decode_rejects_wrong_fragment_count() {
        let services = vec!["a", "b"];
        let decoded = decode_client_fragments("E:w1", &services);
        assert!(decoded.is_none());
    }
}
