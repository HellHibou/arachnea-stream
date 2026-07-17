/// Adds `value` to every UTF-8 byte in each current value.
///
/// The action preserves the original value when the shifted bytes are not valid
/// UTF-8. This keeps the current string-based action pipeline lossless for
/// unsupported binary values.
pub(super) fn apply(texts: Vec<String>, value: i8) -> Vec<String> {
    texts
        .into_iter()
        .map(|text| {
            let shifted: Vec<u8> = text
                .as_bytes()
                .iter()
                .map(|byte| byte.wrapping_add_signed(value))
                .collect();
            String::from_utf8(shifted).unwrap_or(text)
        })
        .collect()
}
