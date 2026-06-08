/// Applies the `ratio` scraper action by multiplying every numeric value in
/// `texts` by `ratio` and rounding the result to the nearest integer.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline. Each
///   value is trimmed and parsed as `f64`.
/// * `ratio` - Multiplier applied to every successfully parsed value.
///
/// # Returns
///
/// A new value list where each numeric input is replaced by the rounded
/// product. Non-numeric values are kept as-is so downstream actions can
/// surface them as errors.
pub(super) fn apply(texts: Vec<String>, ratio: &f64) -> Vec<String> {
    let mut new_texts: Vec<String> = Vec::new();

    for value in texts {
        // Trim only for parsing (no need to create new Strings unnecessarily)
        let value_trimmed = value.trim();

        // Try to parse value as f64
        if let Ok(val_f64) = value_trimmed.parse::<f64>() {
            let result = val_f64 * ratio;
            let rounded = result.round();
            new_texts.push(rounded.to_string());
        } else {
            // If value cannot be converted, return the original String unchanged
            new_texts.push(value);
        }
    }

    new_texts
}
