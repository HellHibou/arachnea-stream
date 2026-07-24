use anyhow::{bail, Result};

/// Applies the `eval_math` scraper action to every current value.
///
/// The evaluator is intentionally small and deterministic: it accepts unsigned
/// integers, parentheses, `+`, and bitwise XOR `^`. In normal mode `+` is numeric
/// addition. In `js_string_concat` mode, top-level `+` terms are evaluated
/// independently and concatenated as decimal strings, matching expressions that
/// originally followed a JavaScript string prefix such as `":" + (...) + (...)`.
pub(super) fn apply(texts: Vec<String>, js_string_concat: bool) -> Result<Vec<String>> {
    texts
        .into_iter()
        .map(|text| evaluate_expression(&text, js_string_concat).map(|value| value.to_string()))
        .collect()
}

fn evaluate_expression(expression: &str, js_string_concat: bool) -> Result<i64> {
    if expression.trim().is_empty() {
        bail!("eval_math expression cannot be empty");
    }

    if js_string_concat {
        let mut rendered = String::new();
        for term in split_top_level_addition(expression)? {
            let mut parser = Parser::new(term);
            rendered.push_str(&parser.parse()?.to_string());
        }
        return rendered.parse::<i64>().map_err(|error| {
            anyhow::anyhow!("Failed to parse concatenated eval_math result: {}", error)
        });
    }

    Parser::new(expression).parse()
}

fn split_top_level_addition(expression: &str) -> Result<Vec<&str>> {
    let mut terms = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;

    for (index, byte) in expression.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| anyhow::anyhow!("eval_math expression has unmatched `)`"))?;
            }
            b'+' if depth == 0 => {
                let term = expression[start..index].trim();
                if term.is_empty() {
                    bail!("eval_math expression contains an empty addition term");
                }
                terms.push(term);
                start = index + 1;
            }
            _ => {}
        }
    }

    if depth != 0 {
        bail!("eval_math expression has unmatched `(`");
    }

    let term = expression[start..].trim();
    if term.is_empty() {
        bail!("eval_math expression contains an empty addition term");
    }
    terms.push(term);
    Ok(terms)
}

struct Parser<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            position: 0,
        }
    }

    fn parse(&mut self) -> Result<i64> {
        let value = self.parse_addition()?;
        self.skip_whitespace();
        if self.position != self.input.len() {
            bail!(
                "eval_math expression contains unexpected character `{}`",
                self.input[self.position] as char
            );
        }
        Ok(value)
    }

    fn parse_addition(&mut self) -> Result<i64> {
        let mut value = self.parse_xor()?;
        loop {
            self.skip_whitespace();
            if !self.consume(b'+') {
                break;
            }
            value = value
                .checked_add(self.parse_xor()?)
                .ok_or_else(|| anyhow::anyhow!("eval_math addition overflow"))?;
        }
        Ok(value)
    }

    fn parse_xor(&mut self) -> Result<i64> {
        let mut value = self.parse_primary()?;
        loop {
            self.skip_whitespace();
            if !self.consume(b'^') {
                break;
            }
            value ^= self.parse_primary()?;
        }
        Ok(value)
    }

    fn parse_primary(&mut self) -> Result<i64> {
        self.skip_whitespace();
        if self.consume(b'(') {
            let value = self.parse_addition()?;
            self.skip_whitespace();
            if !self.consume(b')') {
                bail!("eval_math expression has unmatched `(`");
            }
            return Ok(value);
        }

        self.parse_integer()
    }

    fn parse_integer(&mut self) -> Result<i64> {
        self.skip_whitespace();
        let start = self.position;
        while self
            .input
            .get(self.position)
            .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }

        if start == self.position {
            bail!("eval_math expression expected an integer");
        }

        std::str::from_utf8(&self.input[start..self.position])?
            .parse::<i64>()
            .map_err(|error| anyhow::anyhow!("Invalid eval_math integer: {}", error))
    }

    fn skip_whitespace(&mut self) {
        while self
            .input
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.input.get(self.position) == Some(&expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_evaluates_addition_and_xor() {
        let values = apply(vec!["(80^0)+(8^0)".to_string()], false).unwrap();

        assert_eq!(values, vec!["88".to_string()]);
    }

    #[test]
    fn apply_can_concatenate_top_level_addition_terms_like_javascript_string_prefix() {
        let values = apply(vec!["(80^0)+(8^0)".to_string()], true).unwrap();

        assert_eq!(values, vec!["808".to_string()]);
    }

    #[test]
    fn apply_rejects_unsupported_operator() {
        let error = apply(vec!["2*3".to_string()], false).unwrap_err();

        assert!(error.to_string().contains("unexpected character"));
    }
}
