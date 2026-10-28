use regex::Regex;

/// Replaces every match of `pattern` with `replacement` in each value.
///
/// # Arguments
///
/// * `texts` - Values produced by earlier actions in the pipeline.
/// * `pattern` - Regular-expression pattern used for matching or extraction.
/// * `replacement` - Regex replacement template applied to each matching input value.
pub(super) fn apply(texts: Vec<String>, pattern: &str, replacement: &str) -> Vec<String> {
    let Ok(regex) = Regex::new(pattern) else {
        return texts;
    };

    texts
        .into_iter()
        .map(|value| regex.replace_all(&value, replacement).into_owned())
        .collect()
}

/// Validates a configured replacement pattern.
///
/// # Arguments
///
/// * `pattern` - Regular-expression pattern used for matching or extraction.
pub(super) fn validate(pattern: &str) -> Result<(), regex::Error> {
    Regex::new(pattern).map(|_| ())
}
