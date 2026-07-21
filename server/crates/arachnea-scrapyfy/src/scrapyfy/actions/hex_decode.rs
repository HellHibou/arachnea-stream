/// Decodes hexadecimal UTF-8 text values.
///
/// Values containing non-hexadecimal characters, an odd number of digits, or
/// bytes that are not valid UTF-8 are discarded.
pub(super) fn apply(texts: Vec<String>) -> Vec<String> {
    texts
        .into_iter()
        .filter_map(|value| decode(value.trim()))
        .collect()
}

fn decode(value: &str) -> Option<String> {
    if value.len() % 2 != 0 || !value.is_ascii() {
        return None;
    }

    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        })
        .collect::<Option<Vec<_>>>()?;

    String::from_utf8(bytes).ok()
}
