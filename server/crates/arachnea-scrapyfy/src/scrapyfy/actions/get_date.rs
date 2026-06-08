use anyhow::{bail, Result};
use chrono::{DateTime, Duration, Local, NaiveDateTime};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Supported input formats accepted by `get_date`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GetDateSource {
    /// Matches dates like `2026-03-25` or `2026-03-25T20:15:00+01:00`.
    YyyyMmDd,
    /// Matches dates like `25/03/2026` or `25.03.2026`.
    DdMmYyyy,
    /// Matches dates like `25/03/26` or `25.03.26`.
    DdMmYy,
    /// Matches dates like `mar. 25 mars 2026`, using the configured `months`.
    DdMonthYyyy,
    /// Matches a day count such as `3` and returns today's date plus that many days.
    DaysFromToday,
    /// Matches date-times like `2026-03-25T20:15:00+01:00` or `2026-03-25 20:15:00`.
    YyyyMmDdHhMmSs,
}

/// One or many input formats accepted by `get_date`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GetDateSources {
    /// A single input format tried first.
    One(GetDateSource),
    /// Multiple input formats tried in declaration order.
    Many(Vec<GetDateSource>),
}

impl GetDateSources {
    /// Applies `callback` to each configured source in order and returns the
    /// first `Some` value produced.
    ///
    /// # Arguments
    ///
    /// * `callback` - Function called for every `GetDateSource` until it
    ///   returns `Some`.
    ///
    /// # Returns
    ///
    /// The first `Some` value emitted by `callback`, or `None` when no source
    /// produced a value.
    fn find_map<T, F>(&self, mut callback: F) -> Option<T>
    where
        F: FnMut(GetDateSource) -> Option<T>,
    {
        match self {
            Self::One(source) => callback(*source),
            Self::Many(sources) => sources.iter().copied().find_map(&mut callback),
        }
    }

    /// Returns `true` when at least one of the configured sources is
    /// `GetDateSource::DdMonthYyyy`, which requires a non-empty `months` map.
    fn contains_named_month_source(&self) -> bool {
        self.find_map(|source| (source == GetDateSource::DdMonthYyyy).then_some(()))
            .is_some()
    }

    /// Returns `true` when this source list is empty (only meaningful for the
    /// `Many` variant with no entries).
    fn is_empty(&self) -> bool {
        matches!(self, Self::Many(sources) if sources.is_empty())
    }
}

/// Applies the `get_date` scraper action to every current value, keeping the
/// original text when no format matches.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `format` - One or more input formats tried in declaration order.
/// * `months` - Month lookup required when `format` includes
///   `GetDateSource::DdMonthYyyy`.
///
/// # Returns
///
/// A new value list where each entry is either the normalized ISO date or the
/// original text when no format produced a match.
pub(super) fn apply(
    texts: Vec<String>,
    format: &GetDateSources,
    months: &HashMap<String, u32>,
) -> Vec<String> {
    texts
        .into_iter()
        .map(|value| get_date(&value, format, months).unwrap_or(value))
        .collect()
}

/// Validates the configuration of a `get_date` action.
///
/// # Arguments
///
/// * `name` - Field or sub-query name that owns the action.
/// * `owner` - Human-readable owner kind used in validation errors.
/// * `format` - Configured source list to validate.
/// * `months` - Configured month lookup table to validate.
///
/// # Errors
///
/// Returns an error when `format` is empty, when `DdMonthYyyy` is requested
/// without a `months` map, or when an entry in `months` is invalid.
pub(super) fn validate(
    name: &str,
    owner: &str,
    format: &GetDateSources,
    months: &HashMap<String, u32>,
) -> Result<()> {
    if format.is_empty() {
        bail!(
            "get_date for {} {} requires at least one format",
            owner,
            name
        );
    }

    if format.contains_named_month_source() && months.is_empty() {
        bail!(
            "get_date for {} {} requires a non-empty months map when using dd_month_yyyy",
            owner,
            name
        );
    }

    for (month_name, month_number) in months {
        if month_name.trim().is_empty() {
            bail!(
                "get_date for {} {} contains an empty month name",
                owner,
                name
            );
        }

        if !(1..=12).contains(month_number) {
            bail!(
                "get_date for {} {} contains invalid month number {} for {}",
                owner,
                name,
                month_number,
                month_name
            );
        }
    }

    Ok(())
}

/// Tries every configured `GetDateSource` against `value` and returns the
/// first normalized ISO date produced.
///
/// # Arguments
///
/// * `value` - Raw text to parse (will be trimmed).
/// * `format` - Source list tried in declaration order.
/// * `months` - Month lookup used by `GetDateSource::DdMonthYyyy`.
///
/// # Returns
///
/// A normalized date string (`YYYY-MM-DD` or `YYYY-MM-DD HH:MM:SS`) or `None`
/// when no source matched.
fn get_date(
    value: &str,
    format: &GetDateSources,
    months: &HashMap<String, u32>,
) -> Option<String> {
    let trimmed = value.trim();

    format.find_map(|format| match format {
        GetDateSource::YyyyMmDd => normalize_iso_like_date(trimmed),
        GetDateSource::DdMmYyyy => normalize_day_month_year_date(trimmed),
        GetDateSource::DdMmYy => normalize_day_month_short_year_date(trimmed),
        GetDateSource::DdMonthYyyy => normalize_named_month_date(trimmed, months),
        GetDateSource::DaysFromToday => normalize_days_from_today(trimmed),
        GetDateSource::YyyyMmDdHhMmSs => normalize_iso_like_date_time(trimmed),
    })
}

/// Extracts and normalizes a date matching `YYYY-MM-DD` from `value`.
///
/// # Arguments
///
/// * `value` - Text searched with the ISO-like date regex.
///
/// # Returns
///
/// The normalized `YYYY-MM-DD` date or `None` when no match was found.
fn normalize_iso_like_date(value: &str) -> Option<String> {
    let captures = iso_like_date_regex().captures(value)?;
    let year = captures.get(2)?.as_str().parse::<u32>().ok()?;
    let month = captures.get(3)?.as_str().parse::<u32>().ok()?;
    let day = captures.get(4)?.as_str().parse::<u32>().ok()?;

    format_iso_date(year, month, day)
}

/// Extracts and normalizes a date-time matching `YYYY-MM-DD HH:MM:SS` or
/// RFC 3339 from `value`.
///
/// # Arguments
///
/// * `value` - Text searched with the ISO-like date-time regex or the
///   `chrono` parsers.
///
/// # Returns
///
/// The normalized `YYYY-MM-DD HH:MM:SS` date-time or `None` when no match
/// was found or the captured values form an invalid date.
fn normalize_iso_like_date_time(value: &str) -> Option<String> {
    if let Ok(date_time) = DateTime::parse_from_rfc3339(value) {
        return Some(date_time.format("%F %T").to_string());
    }

    if let Ok(date_time) = NaiveDateTime::parse_from_str(value, "%F %T") {
        return Some(date_time.format("%F %T").to_string());
    }

    let captures = iso_like_date_time_regex().captures(value)?;
    let year = captures.get(2)?.as_str().parse::<u32>().ok()?;
    let month = captures.get(3)?.as_str().parse::<u32>().ok()?;
    let day = captures.get(4)?.as_str().parse::<u32>().ok()?;
    let hour = captures.get(5)?.as_str().parse::<u32>().ok()?;
    let min = captures.get(6)?.as_str().parse::<u32>().ok()?;
    let sec = captures.get(7)?.as_str().parse::<u32>().ok()?;

    if !is_valid_date(year, month, day) {
        return None;
    }

    if hour > 23 || min > 59 || sec > 59 {
        return None;
    }

    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hour, min, sec
    ))
}

/// Extracts and normalizes a date matching `DD/MM/YYYY` or `DD.MM.YYYY`.
///
/// # Arguments
///
/// * `value` - Text searched with the day/month/year regex.
///
/// # Returns
///
/// The normalized `YYYY-MM-DD` date or `None` when no match was found.
fn normalize_day_month_year_date(value: &str) -> Option<String> {
    let captures = day_month_year_regex().captures(value)?;
    let day = captures.get(1)?.as_str().parse::<u32>().ok()?;
    let month = captures.get(2)?.as_str().parse::<u32>().ok()?;
    let year = captures.get(3)?.as_str().parse::<u32>().ok()?;

    format_iso_date(year, month, day)
}

/// Extracts and normalizes a date matching `DD/MM/YY` or `DD.MM.YY`, applying
/// the same 70-year pivot as POSIX `%y` (70–99 → 19xx, 00–69 → 20xx).
///
/// # Arguments
///
/// * `value` - Text searched with the day/month/short-year regex.
///
/// # Returns
///
/// The normalized `YYYY-MM-DD` date or `None` when no match was found.
fn normalize_day_month_short_year_date(value: &str) -> Option<String> {
    let captures = day_month_short_year_regex().captures(value)?;
    let day = captures.get(1)?.as_str().parse::<u32>().ok()?;
    let month = captures.get(2)?.as_str().parse::<u32>().ok()?;
    let short_year = captures.get(3)?.as_str().parse::<u32>().ok()?;
    let year = if short_year >= 70 {
        1900 + short_year
    } else {
        2000 + short_year
    };

    format_iso_date(year, month, day)
}

/// Extracts and normalizes a date that includes a named month (e.g. `mar. 25
/// mars 2026`) by looking the month up in the configured `months` table.
///
/// # Arguments
///
/// * `value` - Raw text containing a day, a month name, and a year.
/// * `months` - Mapping from normalized month name to month number (1–12).
///
/// # Returns
///
/// The normalized `YYYY-MM-DD` date or `None` when the text did not contain
/// a recognizable trailing day/month/year triple.
fn normalize_named_month_date(value: &str, months: &HashMap<String, u32>) -> Option<String> {
    let normalized = normalize_date_text(value);
    let tokens = normalized
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();

    if tokens.len() < 3 {
        return None;
    }

    let date_tokens = &tokens[tokens.len().saturating_sub(3)..];
    let day = date_tokens.first()?.parse::<u32>().ok()?;
    let month = lookup_month(months, date_tokens.get(1)?)?;
    let year = date_tokens.get(2)?.parse::<u32>().ok()?;

    format_iso_date(year, month, day)
}

/// Returns today plus `value` parsed as a signed day offset.
///
/// # Arguments
///
/// * `value` - Day count to add to the current local date.
///
/// # Returns
///
/// The resulting local date formatted as `YYYY-MM-DD`, or `None` when the
/// input is not a valid integer or causes a date overflow.
fn normalize_days_from_today(value: &str) -> Option<String> {
    let days = value.trim().parse::<i64>().ok()?;
    let today = Local::now().date_naive();
    let target_date = today.checked_add_signed(Duration::days(days))?;

    Some(target_date.format("%F").to_string())
}

/// Resolves `token` to a configured month number using case- and
/// punctuation-insensitive matching.
///
/// # Arguments
///
/// * `months` - Configured month lookup table.
/// * `token` - Raw token to look up.
///
/// # Returns
///
/// The configured month number (1–12) or `None` when no entry matches.
fn lookup_month(months: &HashMap<String, u32>, token: &str) -> Option<u32> {
    let normalized_token = normalize_date_text(token);

    months.iter().find_map(|(month_name, month_number)| {
        (normalize_date_text(month_name) == normalized_token).then_some(*month_number)
    })
}

/// Normalizes free-form date text by lowercasing it and stripping commas and
/// periods so day/month names can be compared loosely.
fn normalize_date_text(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .replace([',', '.'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Renders the given `year`, `month`, `day` triple as `YYYY-MM-DD`.
///
/// # Arguments
///
/// * `year` - Four-digit year.
/// * `month` - Month number (1–12).
/// * `day` - Day of the month (1–31, range depends on the month).
///
/// # Returns
///
/// The formatted `YYYY-MM-DD` string, or `None` when the triple does not
/// form a valid calendar date.
fn format_iso_date(year: u32, month: u32, day: u32) -> Option<String> {
    if !is_valid_date(year, month, day) {
        return None;
    }

    Some(format!("{:04}-{:02}-{:02}", year, month, day))
}

/// Returns `true` when `year`, `month`, `day` describe an existing calendar
/// date (with leap-year aware February handling).
fn is_valid_date(year: u32, month: u32, day: u32) -> bool {
    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }

    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return false,
    };

    (1..=max_day).contains(&day)
}

/// Returns `true` when `year` is a leap year according to the proleptic
/// Gregorian calendar.
fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Returns the cached regex used to detect ISO-like `YYYY-MM-DD` substrings.
fn iso_like_date_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(^|[^0-9])(\d{4})-(\d{2})-(\d{2})([^0-9]|$)")
            .expect("Invalid ISO-like date regex")
    })
}

/// Returns the cached regex used to detect ISO-like `YYYY-MM-DD HH:MM:SS`
/// substrings with optional timezone information.
fn iso_like_date_time_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(^|[^0-9])(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2}):(\d{2})(?:Z|[+-]\d{2}:?\d{2})?([^0-9]|$)")
            .expect("Invalid ISO-like date time regex")
    })
}

/// Returns the cached regex used to detect `DD/MM/YYYY` or `DD.MM.YYYY` substrings.
fn day_month_year_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\b(\d{1,2})[/.](\d{1,2})[/.](\d{4})\b")
            .expect("Invalid day/month/year date regex")
    })
}

/// Returns the cached regex used to detect `DD/MM/YY` or `DD.MM.YY` substrings.
fn day_month_short_year_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\b(\d{1,2})[/.](\d{1,2})[/.](\d{2})\b")
            .expect("Invalid day/month/short-year date regex")
    })
}
