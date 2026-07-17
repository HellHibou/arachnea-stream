/// Reverses Unicode scalar values in every current text value.
pub(super) fn apply(texts: Vec<String>) -> Vec<String> {
    texts
        .into_iter()
        .map(|value| value.chars().rev().collect())
        .collect()
}
