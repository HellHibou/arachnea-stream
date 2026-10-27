use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::scrapyfy::query_helpers::{
    validate_dynamic_template_variable_name, DynamicTemplateVariables,
};

/// Cache of compiled regular expressions keyed by their source pattern.
static REGEX_CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();

/// Cached regex used to detect numeric capture placeholders such as `{1}`.
static CAPTURE_PLACEHOLDER_REGEX: OnceLock<Regex> = OnceLock::new();

/// Applies the `extract_variables` scraper action.
///
/// The action scans every current value with `pattern`, renders `name` and
/// `value` from regex captures, and stores the resulting pair in the shared
/// dynamic variable set. Current values are returned unchanged so the action can
/// be inserted into an existing pipeline as a controlled side effect.
pub(super) fn apply(
    texts: Vec<String>,
    pattern: &str,
    name: &str,
    value: &str,
    on_duplicate: ExtractVariablesDuplicatePolicy,
    params: &HashMap<String, String>,
    dynamic_variables: &Mutex<DynamicTemplateVariables>,
) -> Result<Vec<String>> {
    let regex = get_cached_regex(pattern)
        .with_context(|| format!("Invalid extract_variables regex pattern: {}", pattern))?;

    for text in &texts {
        for captures in regex.captures_iter(text) {
            let rendered_name = render_capture_template(name, &captures).trim().to_string();
            let rendered_value = render_capture_template(value, &captures).trim().to_string();
            let mut dynamic_variables = dynamic_variables.lock().map_err(|error| {
                anyhow::anyhow!("Dynamic variable store lock failed: {}", error)
            })?;
            match on_duplicate {
                ExtractVariablesDuplicatePolicy::Error => {
                    dynamic_variables.insert(params, rendered_name, rendered_value)?;
                }
                ExtractVariablesDuplicatePolicy::Replace => {
                    dynamic_variables.insert_or_replace(params, rendered_name, rendered_value)?;
                }
            }
        }
    }

    Ok(texts)
}

/// Validates the configuration of an `extract_variables` action.
pub(super) fn validate(owner_name: &str, owner: &str, pattern: &str, name: &str) -> Result<()> {
    Regex::new(pattern).with_context(|| {
        format!(
            "Invalid extract_variables regex pattern for {} {}",
            owner, owner_name
        )
    })?;

    validate_dynamic_template_variable_name(name).with_context(|| {
        format!(
            "Invalid extract_variables name template for {} {}",
            owner, owner_name
        )
    })?;

    Ok(())
}

/// Duplicate handling policy for `extract_variables`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExtractVariablesDuplicatePolicy {
    /// Reject duplicate dynamic variables.
    Error,
    /// Replace previous dynamic variable values.
    Replace,
}

impl ExtractVariablesDuplicatePolicy {
    /// Parses the YAML `on_duplicate` value.
    pub(super) fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("error") {
            "error" => Ok(Self::Error),
            "replace" => Ok(Self::Replace),
            other => anyhow::bail!(
                "Unsupported extract_variables on_duplicate value {}; supported values are error and replace",
                other
            ),
        }
    }
}

/// Renders `{1}`, `{2}`, ... placeholders from regex captures.
fn render_capture_template(format: &str, captures: &regex::Captures<'_>) -> String {
    let placeholder_re = CAPTURE_PLACEHOLDER_REGEX
        .get_or_init(|| Regex::new(r"\{([0-9]+)\}").expect("Invalid capture placeholder regex"));

    placeholder_re
        .replace_all(format, |placeholder: &regex::Captures<'_>| {
            placeholder[1]
                .parse::<usize>()
                .ok()
                .and_then(|index| captures.get(index))
                .map(|matched| matched.as_str().trim().to_string())
                .unwrap_or_default()
        })
        .to_string()
}

/// Returns the compiled regular expression for `pattern`, caching successful
/// compilations for reuse across calls.
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

#[cfg(test)]
#[path = "../../../tests/scrapyfy/actions/extract_variables.rs"]
mod tests;
