/// Reverses Unicode scalar values in every current text value.
///
/// # Arguments
///
/// * `texts` - Values produced by earlier actions in the pipeline.
pub(super) fn apply(texts: Vec<String>) -> Vec<String> {
    texts
        .into_iter()
        .map(|value| value.chars().rev().collect())
        .collect()
}
