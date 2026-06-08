use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Cache of compiled regular expressions keyed by their source pattern.
static REGEX_CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();

/// Cached regex used to detect `{name}` placeholders inside the output format.
static PLACEHOLDER_REGEX: OnceLock<Regex> = OnceLock::new();

/// Applies the `regex_find_all` scraper action: runs `pattern` against every
/// current value and renders an output for each match using `format`.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `pattern` - Regular expression applied to each value.
/// * `format` - Output template used for every regex match. Numeric
///   placeholders such as `{1}` map to regex capture groups; named
///   placeholders such as `{request_url}` or any key in `params` are also
///   supported.
/// * `params` - Runtime values used to resolve named placeholders.
/// * `request_url` - Final request URL used to resolve the `{request_url}`
///   placeholder.
///
/// # Returns
///
/// A new value list where every entry is the rendered match output, trimmed
/// and filtered to skip empty results. The original `texts` list is returned
/// unchanged when `pattern` fails to compile.
pub(super) fn apply(
    texts: Vec<String>,
    pattern: &str,
    format: &str,
    params: &HashMap<String, String>,
    request_url: &str,
) -> Vec<String> {
    let Some(regex) = get_cached_regex(pattern) else {
        return texts;
    };

    let placeholder_re = PLACEHOLDER_REGEX
        .get_or_init(|| Regex::new(r"\{([A-Za-z0-9_]+)\}").expect("Invalid placeholder regex"));

    // Important: we intentionally do not use query_helpers::replace_template_placeholders
    // here, because we also want `{1}`, `{2}`, ... to map to regex capture groups.
    let mut out: Vec<String> = Vec::new();

    for value in texts {
        for captures in regex.captures_iter(&value) {
            let rendered = placeholder_re
                .replace_all(format, |caps: &regex::Captures| {
                    let key = &caps[1];

                    // Numeric placeholders: {1}, {2}, ...
                    if key.chars().all(|c| c.is_ascii_digit()) {
                        if let Ok(index) = key.parse::<usize>() {
                            return captures
                                .get(index)
                                .map(|m| m.as_str().trim().to_string())
                                .unwrap_or_default();
                        }
                        return String::new();
                    }

                    // Named placeholders
                    if key == "request_url" {
                        return request_url.to_string();
                    }

                    params
                        .get(key)
                        .cloned()
                        // Keep verbatim when missing (debug-friendly / consistent behavior)
                        .unwrap_or_else(|| caps[0].to_string())
                })
                .to_string();

            let trimmed = rendered.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        }
    }

    out
}

/// Validates the configuration of a `regex_find_all` action by ensuring the
/// pattern compiles.
///
/// # Arguments
///
/// * `name` - Field or sub-query name that owns the action.
/// * `owner` - Human-readable owner kind used in validation errors.
/// * `pattern` - Regular expression to compile.
///
/// # Errors
///
/// Returns an error when `pattern` is not a valid regular expression.
pub(super) fn validate(name: &str, owner: &str, pattern: &str) -> Result<()> {
    Regex::new(pattern)
        .with_context(|| format!("Invalid regex pattern for {} {}", owner, name))?;
    Ok(())
}

/// Returns the compiled regular expression for `pattern`, caching successful
/// compilations for reuse across calls.
///
/// # Arguments
///
/// * `pattern` - Regular expression source to compile.
///
/// # Returns
///
/// The compiled [`Regex`], or `None` when the pattern fails to compile.
fn get_cached_regex(pattern: &str) -> Option<Regex> {
    let cache = REGEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Ok(cache_guard) = cache.lock() {
        if let Some(regex) = cache_guard.get(pattern) {
            return Some(regex.clone());
        }
    }

    let compiled = Regex::new(pattern).ok()?;

    if let Ok(mut cache_guard) = cache.lock() {
        cache_guard.insert(pattern.to_string(), compiled.clone());
    }

    Some(compiled)
}
