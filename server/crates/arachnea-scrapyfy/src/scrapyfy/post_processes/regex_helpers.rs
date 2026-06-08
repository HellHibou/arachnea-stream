use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use regex::Regex;

use super::super::scraper_data_node::ScraperDataNode;
use super::types::ScraperRegexItemEntry;

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
pub(super) fn get_cached_regex(pattern: &str) -> Option<Regex> {
    static REGEX_CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();

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

/// Iterates `regex` captures over `source_text` and produces one item per
/// match, where each `ScraperRegexItemEntry` contributes one or more scalar
/// values to the item.
///
/// # Arguments
///
/// * `source_text` - Text searched by the compiled regex.
/// * `pattern` - Regular expression source, used to look up the cached
///   compiled regex.
/// * `entries` - Field definitions describing the captured values to extract.
/// * `params` - Runtime values used to resolve placeholders inside action
///   templates.
/// * `request_url` - Final URL used to resolve the `{request_url}` placeholder.
/// * `response_body` - Optional response body forwarded to actions that need
///   to read the original payload.
///
/// # Returns
///
/// One [`ScraperDataNode`] per regex match, or an empty vector when the
/// pattern does not compile.
pub(super) fn build_regex_items(
    source_text: &str,
    pattern: &str,
    entries: &[ScraperRegexItemEntry],
    params: &HashMap<String, String>,
    request_url: &str,
    response_body: Option<&str>,
) -> Vec<ScraperDataNode> {
    let Some(regex) = get_cached_regex(pattern) else {
        return Vec::new();
    };

    let entry_paths: Vec<Vec<&str>> = entries
        .iter()
        .map(|entry| super::node_helpers::split_path(&entry.name))
        .collect();

    let mut items = Vec::new();

    for captures in regex.captures_iter(source_text) {
        let mut item = ScraperDataNode::default();

        for (entry, path) in entries.iter().zip(entry_paths.iter()) {
            let mut values = entry
                .capture_group
                .and_then(|group| {
                    captures
                        .get(group)
                        .map(|value| vec![value.as_str().to_string()])
                })
                .unwrap_or_default();

            for action in &entry.actions {
                values = action.apply(&None, values, params, request_url, response_body, None);
            }

            for value in values {
                item.push_value(path, value);
            }
        }

        items.push(item);
    }

    items
}
