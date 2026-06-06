/// Applies the `suffix` scraper action by appending `suffix` to every current
/// value.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `suffix` - Suffix appended after each value (e.g. `?utm_source=feed`).
///
/// # Returns
///
/// A new value list where each entry is the original value followed by
/// `suffix`.
pub(super) fn apply(texts: Vec<String>, suffix: &str) -> Vec<String> {
    texts
        .into_iter()
        .map(|value| format!("{}{}", value, suffix))
        .collect()
}
