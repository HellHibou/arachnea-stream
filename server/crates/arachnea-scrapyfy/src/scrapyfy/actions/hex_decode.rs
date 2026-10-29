/// Decodes hexadecimal UTF-8 text values.
///
/// Values containing non-hexadecimal characters, an odd number of digits, or
/// bytes that are not valid UTF-8 are discarded.
///
/// # Arguments
///
/// * `texts` - Values produced by earlier actions in the pipeline.
pub(super) fn apply(texts: Vec<String>) -> Vec<String> {
    texts
        .into_iter()
        .filter_map(|value| decode(value.trim()))
        .collect()
}

/// Decodes an even-length hexadecimal string as UTF-8, returning `None` for invalid input.
///
/// # Arguments
///
/// * `value` - Even-length ASCII hexadecimal text expected to decode as UTF-8.
fn decode(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) || !value.is_ascii() {
        return None;
    }

    let bytes = value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        })
        .collect::<Option<Vec<_>>>()?;

    String::from_utf8(bytes).ok()
}
