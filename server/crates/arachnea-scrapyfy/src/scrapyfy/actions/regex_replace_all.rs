use regex::Regex;

/// Replaces every match of `pattern` with `replacement` in each value.
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
pub(super) fn validate(pattern: &str) -> Result<(), regex::Error> {
    Regex::new(pattern).map(|_| ())
}
