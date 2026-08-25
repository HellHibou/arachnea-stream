//! Conditional request validation primitives (ETag fragments).
//!
//! Each source exposes a stable *fragment* encoding the validator mechanism
//! used for its root request. Every fragment begins with the base62 hash of
//! the source's YAML file so an edit of the scraper invalidates the fragment
//! even when the remote content is unchanged:
//!
//! - `E:<yamlhash>-<etag>` — HTTP ETag validated through `If-None-Match`.
//! - `C:<yamlhash>-<last_modified_unix>-<hash>` — content hash (XXH3-128
//!   encoded in full base62) optionally combined with `If-Modified-Since`.
//!
//! Sources without remote validation support (static queries, fetch-less
//! sources) carry no fragment and are excluded from the global ETag — they are
//! always re-fetched on every execution. The legacy `N:` prefix is accepted
//! when parsing client-provided fragments so an outdated value degrades to a
//! full re-fetch instead of an error, but it is never emitted anymore.
//!
//! Fragments are stable while both the YAML file and the remote content do not
//! change, and are concatenated by the caller into the global aggregated ETag.

use std::time::{SystemTime, UNIX_EPOCH};

use arachnea_core::crypt::base62;
use httpdate::parse_http_date;
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::xxh3_128;

/// Prefix marking an ETag-based validator fragment.
pub const ETAG_FRAGMENT_PREFIX: &str = "E:";
/// Prefix marking a content-hash validator fragment.
pub const CONTENT_FRAGMENT_PREFIX: &str = "C:";
/// Prefix marking a source that does not support conditional validation.
pub const NO_VALIDATION_PREFIX: &str = "N:";

/// Incoming conditional-validation request derived from a client fragment.
///
/// Carries the headers that must be applied to the root request of one source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionalRequest {
    /// Value sent as `If-None-Match`, when the client provided an ETag fragment.
    pub if_none_match: Option<String>,
    /// Value sent as `If-Modified-Since`, when the client provided a timestamp.
    pub if_modified_since: Option<String>,
    /// Expected base62 XXH3-128 digest of the response body, when known.
    pub content_hash: Option<String>,
}

impl ConditionalRequest {
    /// Returns whether this request carries at least one validator.
    pub fn has_validator(&self) -> bool {
        self.if_none_match.is_some() || self.if_modified_since.is_some()
    }
}

/// Outcome recorded for the root fetch of one source query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootFetchOutcome {
    /// Whether the remote content was unchanged (`304` or identical hash).
    pub not_modified: bool,
    /// Up-to-date validator fragment for this source (`E:...` or `C:...`).
    pub etag_fragment: String,
}

/// Shared slot carrying the incoming client fragment and receiving the
/// root-fetch outcome of one query execution.
///
/// Also carries the source YAML-hash used to compose up-to-date fragments;
/// when the incoming fragment does not carry the same hash, the caller must
/// drop it so a YAML edit forces a full re-fetch (see `fragment_yaml_hash`).
pub struct ValidationSlot {
    /// Conditional request derived from the incoming fragment, when any.
    request: Option<ConditionalRequest>,
    /// Raw incoming fragment, reused as the up-to-date fragment on `304`.
    incoming_fragment: Option<String>,
    /// Base62 hash of the source YAML document, embedded in outgoing fragments.
    yaml_hash: String,
    /// Outcome recorded by the executor, keeping the first value written.
    outcome: std::sync::Mutex<Option<RootFetchOutcome>>,
}

impl ValidationSlot {
    /// Creates a validation slot for one source execution.
    ///
    /// # Arguments
    ///
    /// * `incoming_fragment` - Client-provided fragment for this source, when any.
    /// * `yaml_hash` - Base62 hash of the source YAML document.
    pub fn new(incoming_fragment: Option<String>, yaml_hash: String) -> Self {
        let request = incoming_fragment
            .as_deref()
            .and_then(conditional_from_fragment);
        Self {
            request,
            incoming_fragment,
            yaml_hash,
            outcome: std::sync::Mutex::new(None),
        }
    }

    /// Returns the conditional request derived from the incoming fragment.
    pub fn request(&self) -> Option<&ConditionalRequest> {
        self.request.as_ref()
    }

    /// Returns the source YAML-hash used to compose up-to-date fragments.
    pub fn yaml_hash(&self) -> &str {
        &self.yaml_hash
    }

    /// Returns the raw incoming fragment, when provided by the client.
    pub fn incoming_fragment(&self) -> Option<&str> {
        self.incoming_fragment.as_deref()
    }

    /// Records the outcome, keeping the first value written.
    pub fn record(&self, outcome: RootFetchOutcome) {
        if let Ok(mut guard) = self.outcome.lock() {
            if guard.is_none() {
                *guard = Some(outcome);
            }
        }
    }

    /// Takes the recorded outcome, if any.
    pub fn take(&self) -> Option<RootFetchOutcome> {
        self.outcome.lock().ok().and_then(|mut guard| guard.take())
    }
}

/// Computes the full base62 encoding of the XXH3-128 digest of `data`.
///
/// XXH3 is used instead of a cryptographic hash because fragments only need a
/// fast, stable change detector — not tamper resistance. The digest is encoded
/// in full (up to 22 base62 characters for 128 bits), without truncation.
pub fn hash62(data: &[u8]) -> String {
    base62::encode(xxh3_128(data))
}

/// Encodes a fragment component so it never contains the `;` separator.
fn encode_component(value: &str) -> String {
    urlencoding::encode(value).to_string()
}

/// Decodes a previously encoded fragment component.
fn decode_component(value: &str) -> String {
    urlencoding::decode(value)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| value.to_string())
}

/// Builds the up-to-date fragment from a root response.
///
/// Prefers the server-provided `ETag` header; falls back to a content-hash
/// fragment combining `Last-Modified` (when present) with the body hash.
///
/// # Arguments
///
/// * `yaml_hash` - Base62 hash of the source YAML document, embedded as the
///   leading fragment segment.
/// * `etag_header` - Raw `ETag` response header value, when present.
/// * `last_modified_header` - Raw `Last-Modified` response header value, when present.
/// * `body` - Response body used for the content-hash fallback.
pub fn fragment_from_response(
    yaml_hash: &str,
    etag_header: Option<&str>,
    last_modified_header: Option<&str>,
    body: &str,
) -> String {
    if let Some(etag) = etag_header.map(str::trim).filter(|value| !value.is_empty()) {
        return format!(
            "{ETAG_FRAGMENT_PREFIX}{yaml_hash}-{}",
            encode_component(etag)
        );
    }

    let last_modified = last_modified_header
        .and_then(|value| parse_http_date(value.trim()).ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    format!(
        "{CONTENT_FRAGMENT_PREFIX}{yaml_hash}-{last_modified}-{}",
        hash62(body.as_bytes())
    )
}

/// Extracts the base62 YAML-hash segment of a fragment.
///
/// The YAML hash is pure base62 so it never contains `-` and is never empty;
/// the leading `-` therefore always separates it from the rest, even when the
/// percent-encoded remote ETag carries literal `-` characters.
///
/// # Arguments
///
/// * `fragment` - Fragment previously returned by [`fragment_from_response`].
///
/// # Returns
///
/// The YAML-hash segment, or `None` when the fragment is empty, uses an
/// unknown prefix, or lacks the leading `<yamlhash>-` segment (for example a
/// legacy fragment produced before the YAML hash was introduced).
pub fn fragment_yaml_hash(fragment: &str) -> Option<&str> {
    let fragment = fragment.trim();
    let rest = fragment
        .strip_prefix(ETAG_FRAGMENT_PREFIX)
        .or_else(|| fragment.strip_prefix(CONTENT_FRAGMENT_PREFIX))
        .or_else(|| fragment.strip_prefix(NO_VALIDATION_PREFIX))?;
    let (yaml_hash, _) = rest.split_once('-')?;
    if yaml_hash.is_empty() {
        return None;
    }
    Some(yaml_hash)
}

/// Parses a client-provided fragment into the conditional request to apply.
///
/// # Arguments
///
/// * `fragment` - Fragment previously returned by [`fragment_from_response`].
///
/// # Returns
///
/// The conditional request to send, or `None` when the fragment is malformed
/// or describes a source without remote validation.
pub fn conditional_from_fragment(fragment: &str) -> Option<ConditionalRequest> {
    let fragment = fragment.trim();
    if let Some(rest) = fragment.strip_prefix(ETAG_FRAGMENT_PREFIX) {
        let (_yaml_hash, encoded_etag) = rest.split_once('-')?;
        let etag = decode_component(encoded_etag);
        if etag.is_empty() {
            return None;
        }
        return Some(ConditionalRequest {
            if_none_match: Some(etag),
            ..Default::default()
        });
    }

    if let Some(_rest) = fragment.strip_prefix(NO_VALIDATION_PREFIX) {
        // Sources without remote validation support are always re-fetched;
        // their fragment is stable so the global ETag stays deterministic.
        return None;
    }

    if let Some(rest) = fragment.strip_prefix(CONTENT_FRAGMENT_PREFIX) {
        let (_yaml_hash, remainder) = rest.split_once('-')?;
        let (timestamp, hash) = remainder.split_once('-')?;
        let timestamp: u64 = timestamp.parse().ok()?;
        if hash.is_empty() {
            return None;
        }
        let if_modified_since = if timestamp > 0 {
            Some(format_http_date(timestamp))
        } else {
            None
        };
        return Some(ConditionalRequest {
            if_none_match: None,
            if_modified_since,
            content_hash: Some(hash.to_string()),
        });
    }

    None
}

/// Formats a unix timestamp (seconds) as an HTTP-date string.
fn format_http_date(timestamp: u64) -> String {
    let time = UNIX_EPOCH + std::time::Duration::from_secs(timestamp);
    httpdate::fmt_http_date(time)
}

/// Returns the current unix timestamp in seconds.
#[allow(dead_code)]
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
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
        let fragment =
            fragment_from_response(YAML_HASH, None, Some("Wed, 21 Oct 2015 07:28:00 GMT"), "body");
        let conditional = conditional_from_fragment(&fragment).expect("fragment should parse");
        assert!(conditional.if_modified_since.is_some());
        assert_eq!(
            conditional.content_hash.as_deref(),
            Some(&hash62(b"body")[..])
        );
    }

    #[test]
    fn fragment_yaml_hash_extracts_for_all_prefixes() {
        assert_eq!(fragment_yaml_hash("E:3dY0aBc2Xf-%22w456%22"), Some(YAML_HASH));
        assert_eq!(fragment_yaml_hash("C:3dY0aBc2Xf-0-deadbeef"), Some(YAML_HASH));
        assert_eq!(fragment_yaml_hash("N:3dY0aBc2Xf-aBcDe"), Some(YAML_HASH));
    }

    #[test]
    fn legacy_fragments_without_yaml_hash_are_divergent() {
        // Fragments produced before the YAML hash was introduced do not match
        // the current grammar and must never be applied as conditional headers.
        assert!(conditional_from_fragment("E:w456").is_none());
        assert!(fragment_yaml_hash("E:w456").is_none());
        assert!(fragment_yaml_hash("C:0-abcd1234").is_none());
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
}
