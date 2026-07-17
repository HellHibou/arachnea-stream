/// Parses each current value as JSON and extracts one scalar by JSON Pointer.
pub(super) fn apply(texts: Vec<String>, path: &str) -> Vec<String> {
    texts
        .into_iter()
        .filter_map(|text| {
            let json: serde_json::Value = serde_json::from_str(&text).ok()?;
            match json.pointer(path)? {
                serde_json::Value::String(value) => Some(value.clone()),
                serde_json::Value::Number(value) => Some(value.to_string()),
                serde_json::Value::Bool(value) => Some(value.to_string()),
                _ => None,
            }
        })
        .collect()
}
