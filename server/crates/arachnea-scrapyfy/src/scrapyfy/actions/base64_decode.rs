use base64::Engine;

/// Applies the `base64_decode` scraper action by base64-decoding each current value.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
///
/// # Returns
///
/// A new value list where every value is base64-decoded from its original form.
/// Values that fail to decode are kept as-is.
pub(super) fn apply(texts: Vec<String>) -> Vec<String> {
    texts
        .into_iter()
        .map(|value| {
            base64::engine::general_purpose::STANDARD
                .decode(value.trim())
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or(value)
        })
        .collect()
}

