use std::collections::HashMap;

use super::super::query_helpers;

/// Applies the `build_url` scraper action by collecting multiple JSON fields
/// from the response and substituting them into the configured `base`
/// template.
///
/// # Arguments
///
/// * `response_json` - Parsed JSON response, or `None` when the action runs
///   outside a JSON scraper.
/// * `base` - URL template containing `{placeholder}` tokens.
/// * `fields` - Map of placeholder name to JSON pointer path. The value
///   resolved at each path is inserted into the template with the matching
///   placeholder name.
/// * `params` - Existing runtime parameters propagated to the template
///   resolution.
///
/// # Returns
///
/// A single-element list containing the rendered URL.
pub(super) fn apply(
    response_json: Option<&serde_json::Value>,
    base: &str,
    fields: &HashMap<String, String>,
    params: &HashMap<String, String>,
) -> Vec<String> {
    let mut runtime_params = params.clone();

    // First build a set of field values from JSON response
    if let Some(json) = response_json {
        for (placeholder, path) in fields {
            if let Some(value) = json.pointer(path) {
                let extracted = match value {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                };
                runtime_params.insert(placeholder.to_string(), extracted);
            }
        }
    }

    // Now use replace_template_placeholders to handle all placeholders at once
    let (result, _missing) =
        query_helpers::replace_template_placeholders(base, &runtime_params);
    vec![result]
}
