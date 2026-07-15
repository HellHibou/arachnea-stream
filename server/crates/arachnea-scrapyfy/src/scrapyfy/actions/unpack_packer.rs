use regex::{Captures, Regex};
use std::sync::LazyLock;

static PACKER_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)eval\s*\(\s*function\s*\(\s*p\s*,\s*a\s*,\s*c\s*,\s*k\s*,\s*e\s*,\s*d\s*\)\s*\{.*?\}\s*\(",
    )
    .expect("Packer prefix regex is valid")
});
static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9_]+\b").expect("word regex is valid"));

/// Unpacks literal Dean Edwards Packer blocks without evaluating JavaScript.
///
/// Only calls whose payload, radix, symbol count, dictionary, and `split` separator are literals
/// are accepted. Malformed or unsupported input produces no output.
pub(super) fn apply(texts: Vec<String>) -> Vec<String> {
    texts.into_iter().filter_map(|text| unpack(&text)).collect()
}

fn unpack(source: &str) -> Option<String> {
    let prefix = PACKER_PREFIX.find(source)?;
    let mut input = &source[prefix.end()..];

    let payload = parse_string(&mut input)?;
    consume_char(&mut input, ',')?;
    let radix = parse_number(&mut input)?;
    consume_char(&mut input, ',')?;
    let count = parse_number(&mut input)?;
    consume_char(&mut input, ',')?;
    let dictionary = parse_string(&mut input)?;

    consume_literal(&mut input, ".split")?;
    consume_char(&mut input, '(')?;
    let separator = parse_string(&mut input)?;
    consume_char(&mut input, ')')?;

    if !(2..=62).contains(&radix) {
        return None;
    }

    let mut symbols = dictionary
        .split(&separator)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if symbols.len() < count {
        return None;
    }
    symbols.truncate(count);

    Some(
        WORD.replace_all(&payload, |captures: &Captures<'_>| {
            let word = &captures[0];
            decode_index(word, radix)
                .and_then(|index| symbols.get(index))
                .filter(|symbol| !symbol.is_empty())
                .cloned()
                .unwrap_or_else(|| word.to_string())
        })
        .into_owned(),
    )
}

fn skip_whitespace(input: &mut &str) {
    *input = input.trim_start();
}

fn consume_char(input: &mut &str, expected: char) -> Option<()> {
    skip_whitespace(input);
    let remaining = input.strip_prefix(expected)?;
    *input = remaining;
    Some(())
}

fn consume_literal(input: &mut &str, expected: &str) -> Option<()> {
    skip_whitespace(input);
    let remaining = input.strip_prefix(expected)?;
    *input = remaining;
    Some(())
}

fn parse_number(input: &mut &str) -> Option<usize> {
    skip_whitespace(input);
    let end = input
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(input.len());
    let (number, remaining) = input.split_at(end);
    if number.is_empty() {
        return None;
    }
    *input = remaining;
    number.parse().ok()
}

fn parse_string(input: &mut &str) -> Option<String> {
    skip_whitespace(input);
    let quote = input.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    *input = &input[quote.len_utf8()..];

    let source = *input;
    let mut value = String::new();
    let mut index = 0;
    while index < source.len() {
        let character = source[index..].chars().next()?;
        index += character.len_utf8();
        if character == quote {
            *input = &source[index..];
            return Some(value);
        }
        if character != '\\' {
            value.push(character);
            continue;
        }

        let escaped = source[index..].chars().next()?;
        index += escaped.len_utf8();
        match escaped {
            'x' => {
                let hex = source.get(index..index + 2)?;
                let code = u8::from_str_radix(hex, 16).ok()?;
                value.push(char::from(code));
                index += 2;
            }
            'u' => {
                let hex = source.get(index..index + 4)?;
                let code = u32::from_str_radix(hex, 16).ok()?;
                value.push(char::from_u32(code)?);
                index += 4;
            }
            'n' => value.push('\n'),
            'r' => value.push('\r'),
            't' => value.push('\t'),
            other => value.push(other),
        }
    }
    None
}

fn decode_index(word: &str, radix: usize) -> Option<usize> {
    let mut value = 0usize;
    for character in word.chars() {
        let digit = match character {
            '0'..='9' => character as usize - '0' as usize,
            'a'..='z' => character as usize - 'a' as usize + 10,
            'A'..='Z' => character as usize - 'A' as usize + 36,
            _ => return None,
        };
        if digit >= radix {
            return None;
        }
        value = value.checked_mul(radix)?.checked_add(digit)?;
    }
    Some(value)
}
