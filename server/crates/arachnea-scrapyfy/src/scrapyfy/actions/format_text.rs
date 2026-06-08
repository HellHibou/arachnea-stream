use std::collections::HashMap;

use super::super::query_helpers;

/// Applies the `format_text` scraper action, rendering every current value
/// against the configured template.
///
/// The template replaces:
/// - `{}` with the current value when one exists,
/// - `{request_url}` with the final request URL,
/// - named placeholders such as `{base_url}` or `{locale}` using runtime params.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline. When the
///   list is empty, the rendered template is pushed once without `{}`
///   substitution.
/// * `format` - Template string containing placeholders.
/// * `params` - Runtime values used to resolve named placeholders.
/// * `request_url` - Final URL used to fetch the current page; replaces the
///   `{request_url}` placeholder.
///
/// # Returns
///
/// A new value list where each entry is the rendered template output.
pub(super) fn apply(
    texts: Vec<String>,
    format: &str,
    params: &HashMap<String, String>,
    request_url: &str,
) -> Vec<String> {
    let mut new_texts: Vec<String> = Vec::new();
    let (format, _missing_keys) = query_helpers::replace_template_placeholders(format, params);
    let format = format.replace("{request_url}", request_url);

    if !texts.is_empty() {
        for value in texts {
            new_texts.push(format.replace("{}", &value));
        }
    } else {
        new_texts.push(format);
    }

    new_texts
}
