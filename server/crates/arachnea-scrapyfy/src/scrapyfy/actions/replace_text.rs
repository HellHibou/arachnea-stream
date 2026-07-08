/// Applies the `replace_text` scraper action by performing string replacement
/// on every current value.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `search` - Substring to search for in each value.
/// * `replace` - Replacement string.
///
/// # Returns
///
/// A new value list where each entry has all occurrences of `search` replaced
/// with `replace`.
pub(super) fn apply(texts: Vec<String>, search: &str, replace: &str) -> Vec<String> {
    texts
        .into_iter()
        .map(|value| value.replace(search, replace))
        .collect()
}
