/// Applies the `max` scraper action by keeping only the highest positive
/// integer parsed from the current value list.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline. Each
///   value is trimmed and parsed as `usize`; non-numeric or non-positive
///   values are ignored.
///
/// # Returns
///
/// A single-element list containing the maximum value, or an empty list when
/// no entry could be parsed as a positive integer.
pub(super) fn apply(texts: Vec<String>) -> Vec<String> {
    texts
        .into_iter()
        .filter_map(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .max()
        .map(|value| vec![value.to_string()])
        .unwrap_or_default()
}
