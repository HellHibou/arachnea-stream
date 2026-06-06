use anyhow::Result;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use super::*;
use crate::scrapyfy::html_scraper_entry::HtmlScraperSelectMode;

/// Raw configuration definition of one field or grouped field extracted from a JSON result row.
#[derive(Serialize, Deserialize)]
pub struct JsonScraperEntryRaw {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pointer: Option<String>,
    #[serde(default)]
    select: HtmlScraperSelectMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<ScraperAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entries: Vec<JsonScraperEntryRaw>,
}

/// One logical field extracted from a JSON result row.
#[derive(Deserialize)]
#[serde(try_from = "JsonScraperEntryRaw")]
pub enum JsonScraperEntry {
    Field {
        name: String,
        pointer: Option<String>,
        select: HtmlScraperSelectMode,
        actions: Vec<ScraperAction>,
    },
    Group {
        name: String,
        pointer: Option<String>,
        select: HtmlScraperSelectMode,
        entries: Vec<JsonScraperEntry>,
    },
}

impl JsonScraperEntry {
    /// Returns the flattened list of leaf field names produced by this entry.
    #[cfg(any(test, feature = "test-support"))]
    pub fn field_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_field_names(None, &mut names);
        names
    }

    /// Applies this entry to the provided JSON row.
    ///
    /// Group entries append explicit array items, while field entries append values.
    pub fn apply_to(
        &self,
        root: &mut ScraperDataNode,
        row: &Value,
        params: &HashMap<String, String>,
        request_url: &str,
    ) {
        match self {
            JsonScraperEntry::Field {
                name,
                pointer,
                select,
                actions,
            } => {
                let path: Vec<&str> = name.split('>').map(|segment| segment.trim()).collect();

                for selected in select_json_values(row, pointer.as_deref(), *select) {
                    let mut values = json_value_to_strings(selected);
                    for action in actions {
                        values = action.apply(&None, values, params, request_url, None, Some(row));
                    }

                    for value in values {
                        root.push_value(&path, value);
                    }
                }
            }
            JsonScraperEntry::Group {
                name,
                pointer,
                select,
                entries,
            } => {
                let path: Vec<&str> = name.split('>').map(|segment| segment.trim()).collect();

                for selected in select_json_values(row, pointer.as_deref(), *select) {
                    let mut item = ScraperDataNode::default();
                    for entry in entries {
                        entry.apply_to(&mut item, selected, params, request_url);
                    }

                    root.push_node(&path, item);
                }
            }
        }
    }

    /// Returns `true` when this entry is a grouped/nested JSON entry.
    pub fn is_group(&self) -> bool {
        matches!(self, JsonScraperEntry::Group { .. })
    }

    #[cfg(any(test, feature = "test-support"))]
    fn collect_field_names(&self, prefix: Option<&str>, names: &mut Vec<String>) {
        let full_name = match prefix {
            Some(prefix) => format!("{} > {}", prefix, self.name()),
            None => self.name().to_string(),
        };

        match self {
            JsonScraperEntry::Field { .. } => names.push(full_name),
            JsonScraperEntry::Group { entries, .. } => {
                for entry in entries {
                    entry.collect_field_names(Some(&full_name), names);
                }
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    fn name(&self) -> &str {
        match self {
            JsonScraperEntry::Field { name, .. } => name,
            JsonScraperEntry::Group { name, .. } => name,
        }
    }

    fn validate_actions(name: &str, actions: &[ScraperAction]) -> Result<()> {
        for action in actions {
            action.validate(name, "entry")?;
        }

        Ok(())
    }
}

impl TryFrom<JsonScraperEntryRaw> for JsonScraperEntry {
    type Error = anyhow::Error;

    fn try_from(config: JsonScraperEntryRaw) -> Result<Self> {
        let JsonScraperEntryRaw {
            name,
            pointer,
            select,
            actions,
            entries,
        } = config;

        if entries.is_empty() {
            JsonScraperEntry::validate_actions(&name, &actions)?;

            return Ok(Self::Field {
                name,
                pointer,
                select,
                actions,
            });
        }

        if !actions.is_empty() {
            anyhow::bail!("JSON group entry {} cannot define actions", name);
        }

        let entries = entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;

        Ok(Self::Group {
            name,
            pointer,
            select,
            entries,
        })
    }
}

impl From<&JsonScraperEntry> for JsonScraperEntryRaw {
    fn from(entry: &JsonScraperEntry) -> Self {
        match entry {
            JsonScraperEntry::Field {
                name,
                pointer,
                select,
                actions,
            } => Self {
                name: name.clone(),
                pointer: pointer.clone(),
                select: *select,
                actions: actions.clone(),
                entries: Vec::new(),
            },
            JsonScraperEntry::Group {
                name,
                pointer,
                select,
                entries,
            } => Self {
                name: name.clone(),
                pointer: pointer.clone(),
                select: *select,
                actions: Vec::new(),
                entries: entries.iter().map(JsonScraperEntryRaw::from).collect(),
            },
        }
    }
}

impl Serialize for JsonScraperEntry {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        JsonScraperEntryRaw::from(self).serialize(serializer)
    }
}

/// Selects JSON values using a pointer-like syntax with wildcard support.
///
/// Examples:
/// - `/data/0/title`
/// - `/data/*/content/*`
pub(crate) fn select_json_values<'a>(
    root: &'a Value,
    pointer: Option<&str>,
    select: HtmlScraperSelectMode,
) -> Vec<&'a Value> {
    let mut selected = select_json_values_all(root, pointer);
    if select == HtmlScraperSelectMode::First {
        selected.truncate(1);
    }

    selected
}

fn select_json_values_all<'a>(root: &'a Value, pointer: Option<&str>) -> Vec<&'a Value> {
    let Some(pointer) = pointer.map(str::trim).filter(|pointer| !pointer.is_empty()) else {
        return vec![root];
    };

    let mut current = vec![root];
    for token in pointer.trim_start_matches('/').split('/') {
        if token.is_empty() {
            continue;
        }

        let token = decode_pointer_token(token);
        let mut next = Vec::new();

        for value in current {
            if let Some((array_selector, filter)) = parse_array_filter(&token) {
                if let Value::Array(items) = value {
                    match array_selector.as_str() {
                        "*" => {
                            for item in items {
                                if matches_filter(item, &filter) {
                                    next.push(item);
                                }
                            }
                        }
                        _ => {
                            if let Ok(index) = array_selector.parse::<usize>() {
                                if let Some(item) = items.get(index) {
                                    if matches_filter(item, &filter) {
                                        next.push(item);
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                match (token.as_str(), value) {
                    ("*", Value::Array(items)) => next.extend(items.iter()),
                    ("*", Value::Object(entries)) => next.extend(entries.values()),
                    (_, Value::Array(items)) => {
                        if let Ok(index) = token.parse::<usize>() {
                            if let Some(item) = items.get(index) {
                                next.push(item);
                            }
                        }
                    }
                    (_, Value::Object(entries)) => {
                        if let Some(item) = entries.get(token.as_str()) {
                            next.push(item);
                        }
                    }
                    _ => {}
                }
            }
        }

        current = next;
        if current.is_empty() {
            break;
        }
    }

    current
}

fn decode_pointer_token(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

/// Parses a token that may contain array filtering syntax like "*[role=mea]" or "0[status=active]".
/// Returns (selector, filter) if filtering syntax is detected, None otherwise.
fn parse_array_filter(token: &str) -> Option<(String, String)> {
    if let Some(start) = token.find('[') {
        if let Some(end) = token.rfind(']') {
            if end > start {
                let selector = token[..start].to_string();
                let filter = token[start + 1..end].to_string();
                return Some((selector, filter));
            }
        }
    }
    None
}

/// Checks if a JSON value matches a filter condition like "role=mea".
fn matches_filter(value: &Value, filter: &str) -> bool {
    if let Some((field, expected)) = parse_filter_condition(filter) {
        if let Some(obj) = value.as_object() {
            if let Some(actual_value) = obj.get(&field) {
                match actual_value {
                    Value::String(s) => return s == &expected,
                    Value::Number(n) => return n.to_string() == expected,
                    Value::Bool(b) => return b.to_string() == expected,
                    _ => return false,
                }
            }
        }
    }
    false
}

/// Parses a filter condition like "role=mea" into (field, value).
fn parse_filter_condition(filter: &str) -> Option<(String, String)> {
    if let Some(eq_pos) = filter.find('=') {
        let field = filter[..eq_pos].trim().to_string();
        let value = filter[eq_pos + 1..].trim().to_string();
        Some((field, value))
    } else {
        None
    }
}

pub(crate) fn json_value_to_strings(value: &Value) -> Vec<String> {
    match value {
        Value::Null => Vec::new(),
        Value::Bool(value) => vec![value.to_string()],
        Value::Number(value) => vec![value.to_string()],
        Value::String(value) => vec![value.clone()],
        Value::Array(values) => values.iter().flat_map(json_value_to_strings).collect(),
        Value::Object(_) => vec![value.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_select_json_values_with_filter() {
        let data = json!({
            "images": [
                {"role": "banner", "external_key": "key1"},
                {"role": "mea", "external_key": "key2"},
                {"role": "thumbnail", "external_key": "key3"},
                {"role": "mea", "external_key": "key4"}
            ]
        });

        // Test filtering by role=mea
        let results = select_json_values(
            &data,
            Some("/images/*[role=mea]/external_key"),
            HtmlScraperSelectMode::All,
        );
        let strings: Vec<String> = results
            .into_iter()
            .flat_map(json_value_to_strings)
            .collect();

        assert_eq!(strings, vec!["key2", "key4"]);
    }

    #[test]
    fn test_select_json_values_without_filter() {
        let data = json!({
            "images": [
                {"external_key": "key1"},
                {"external_key": "key2"}
            ]
        });

        // Test without filter (should get all)
        let results = select_json_values(
            &data,
            Some("/images/*/external_key"),
            HtmlScraperSelectMode::All,
        );
        let strings: Vec<String> = results
            .into_iter()
            .flat_map(json_value_to_strings)
            .collect();

        assert_eq!(strings, vec!["key1", "key2"]);
    }
}
