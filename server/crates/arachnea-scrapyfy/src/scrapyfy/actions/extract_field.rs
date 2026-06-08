/// Applies the `extract_field` scraper action by resolving `path` against
/// `response_json` and appending the resulting scalar as a string.
///
/// # Arguments
///
/// * `path` - JSON pointer (e.g. `/program/id`) used to look up the value.
/// * `response_json` - Parsed JSON response, or `None` when the action runs
///   outside a JSON scraper.
///
/// # Returns
///
/// A new value list containing the string representation of the JSON value
/// (empty when the path is missing or maps to `null`).
pub(super) fn apply(path: &str, response_json: Option<&serde_json::Value>) -> Vec<String> {
    let mut extracted = Vec::new();
    if let Some(json) = response_json {
        if let Some(value) = json.pointer(path) {
            match value {
                serde_json::Value::String(s) => extracted.push(s.clone()),
                serde_json::Value::Number(n) => {
                    if let Some(n) = n.as_i64() {
                        extracted.push(n.to_string());
                    }
                }
                serde_json::Value::Null => {}
                other => extracted.push(other.to_string()),
            }
        }
    }
    extracted
}
