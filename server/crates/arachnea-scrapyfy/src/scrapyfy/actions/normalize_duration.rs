use regex::Regex;
use std::sync::OnceLock;

/// Applies the `normalize_duration` scraper action by parsing human-readable
/// duration strings (e.g. `39 min`, `1 h 05 min`) into a seconds count.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
///
/// # Returns
///
/// A new value list where each entry is either the parsed duration in seconds
/// (as a string) or the original text when no recognizable duration token was
/// found.
pub(super) fn apply(texts: Vec<String>) -> Vec<String> {
    texts
        .into_iter()
        .map(|value| normalize_duration(&value).unwrap_or(value))
        .collect()
}

/// Parses `value` as a duration string and returns the equivalent number of
/// seconds.
///
/// # Arguments
///
/// * `value` - Raw text containing one or more `amount unit` tokens (e.g.
///   `1 h 05 min 30 sec`). Supported units: `h`, `min`/`m`, `sec`/`s`.
///
/// # Returns
///
/// The total number of seconds or `None` when the text does not contain any
/// recognizable unit.
fn normalize_duration(value: &str) -> Option<String> {
    parse_duration_seconds(value).map(|seconds| seconds.to_string())
}

/// Sums every `amount unit` match in `value` to compute the total duration in
/// seconds.
///
/// # Arguments
///
/// * `value` - Raw text searched with the duration regex.
///
/// # Returns
///
/// The total duration in seconds, or `None` when no token matches.
fn parse_duration_seconds(value: &str) -> Option<u64> {
    let regex = duration_regex();

    let mut total_seconds: u64 = 0;
    let mut matched = false;

    for captures in regex.captures_iter(value) {
        let amount = captures.get(1)?.as_str().parse::<u64>().ok()?;
        let unit = captures.get(2)?.as_str().to_lowercase();

        matched = true;
        match unit.as_str() {
            "h" => total_seconds += amount * 3600,
            "min" | "m" => total_seconds += amount * 60,
            "sec" | "s" => total_seconds += amount,
            _ => return None,
        }
    }

    if !matched {
        return None;
    }

    Some(total_seconds)
}

/// Returns the cached regex used to detect `<amount> <unit>` pairs in
/// human-readable duration strings.
fn duration_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)(\d+)\s*(h|min|m|sec|s)?").expect("Invalid duration regex")
    })
}
