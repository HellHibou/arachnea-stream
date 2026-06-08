/// Applies the `split` scraper action by expanding every value in `texts`
/// using `separator` and collecting the non-empty trimmed fragments.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `separator` - Substring used to split every value with `str::split`.
///
/// # Returns
///
/// A new value list containing every non-empty trimmed fragment produced by
/// splitting each input value.
pub(super) fn apply(texts: Vec<String>, separator: &str) -> Vec<String> {
    // Split expands the values already collected by previous actions.
    let mut new_texts: Vec<String> = Vec::new();
    for entry1 in texts {
        for entry2 in entry1
            .split(separator)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            new_texts.push(entry2);
        }
    }

    new_texts
}
