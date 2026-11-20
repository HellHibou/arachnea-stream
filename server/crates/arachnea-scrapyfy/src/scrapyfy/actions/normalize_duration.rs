use regex::Regex;
use std::sync::OnceLock;

/// Applies the `normalize_duration` scraper action by parsing human-readable
/// duration strings (e.g. `39 min`, `1 h 05 min`, `1 h 05`) into a seconds
/// count. Also accepts standalone minute-second and hour-minute-second strings.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
///
/// # Returns
///
/// A new value list where each entry is either the parsed duration in seconds
/// (as a string) or the original text when no duration was recognized.
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
/// * `value` - Standalone clock duration or text containing `amount unit` tokens (e.g.
///   `1 h 05 min 30 sec`). Supported units: `h`, `min`/`m`, `sec`/`s`. A bare
///   number directly following an hours token is read as minutes, so compact
///   formats such as `1 h 05` resolve to one hour and five minutes.
///
/// # Returns
///
/// The total number of seconds or `None` when no duration was recognized.
fn normalize_duration(value: &str) -> Option<String> {
    parse_clock_duration(value.trim())
        .or_else(|| parse_duration_seconds(value))
        .map(|seconds| seconds.to_string())
}

/// Parses a standalone clock duration with checked arithmetic.
/// Seconds and the middle component of a three-part duration are two digits
/// between 00 and 59. The leading component is an unsigned integer.
///
/// # Arguments
///
/// * `value` - Trimmed input in minute-second or hour-minute-second form.
fn parse_clock_duration(value: &str) -> Option<u64> {
    let (leading, seconds) = value.rsplit_once(':')?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b':')
    {
        return None;
    }

    let seconds_value: u64 = seconds.parse().ok()?;
    if seconds.len() != 2 || seconds_value >= 60 {
        return None;
    }

    let minutes = if let Some((hours, minutes)) = leading.split_once(':') {
        let hours: u64 = hours.parse().ok()?;
        let minutes_value: u64 = minutes.parse().ok()?;
        if minutes.len() != 2 || minutes_value >= 60 {
            return None;
        }
        hours.checked_mul(60)?.checked_add(minutes_value)?
    } else {
        leading.parse().ok()?
    };

    minutes.checked_mul(60)?.checked_add(seconds_value)
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
    let mut after_hours = false;

    for captures in regex.captures_iter(value) {
        let amount = captures.get(1)?.as_str().parse::<u64>().ok()?;
        let unit = captures
            .get(2)
            .map(|capture| capture.as_str().to_lowercase());

        match unit.as_deref() {
            Some("h") => {
                total_seconds += amount * 3600;
                after_hours = true;
                matched = true;
            }
            Some("min" | "m") => {
                total_seconds += amount * 60;
                after_hours = false;
                matched = true;
            }
            Some("sec" | "s") => {
                total_seconds += amount;
                after_hours = false;
                matched = true;
            }
            // A unit-less number right after `h` is the minutes part of compact
            // formats such as `1 h 05` or `1h05`, which omit the `min` unit.
            None if after_hours => {
                total_seconds += amount * 60;
                after_hours = false;
                matched = true;
            }
            // Unrelated numbers must not discard a duration that was already
            // recognized earlier in the same text.
            None => after_hours = false,
            Some(_) => return None,
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
    /// Cached matcher for human-readable duration amounts and units.
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)(\d+)\s*(h|min|m|sec|s)?").expect("Invalid duration regex")
    })
}
