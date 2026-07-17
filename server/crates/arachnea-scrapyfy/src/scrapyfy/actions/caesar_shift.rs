/// Applies a configurable Caesar shift to ASCII letters in every value.
pub(super) fn apply(texts: Vec<String>, shift: i8) -> Vec<String> {
    texts
        .into_iter()
        .map(|value| {
            value
                .chars()
                .map(|character| match character {
                    'A'..='Z' => shift_letter(character, 'A', shift),
                    'a'..='z' => shift_letter(character, 'a', shift),
                    _ => character,
                })
                .collect()
        })
        .collect()
}

fn shift_letter(character: char, base: char, shift: i8) -> char {
    let offset = character as i16 - base as i16;
    let shifted = (offset + shift as i16).rem_euclid(26);
    char::from_u32(base as u32 + shifted as u32).unwrap_or(character)
}
