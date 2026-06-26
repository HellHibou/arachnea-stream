use std::any::Any;

use anyhow::Result;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::scrapyfy::scraper::entry_trait::ScraperEntrySpec;
use crate::scrapyfy::scraper::query_trait::ScraperQuery;
use crate::scrapyfy::scraper::row_locator::ScraperType;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::EntrySubQueryRaw;
use crate::scrapyfy::*;

/// Raw configuration definition of one field or grouped field extracted from a JSON result row.
///
/// Deserializes from YAML to define how to extract a single field or a group of fields
/// from a JSON row during scraping.
#[derive(Serialize, Deserialize)]
pub struct JsonScraperEntryRaw {
    /// Field or group name.
    ///
    /// The identifier used for the extracted value in the output.
    name: String,

    /// JSON output type expected for this entry.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    output_type: Option<ScraperOutputType>,

    /// Optional JSON pointer selecting the value from the row.
    ///
    /// JSON pointer or pointer-like path used to locate the value in the JSON row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pointer: Option<String>,

    /// Selection mode for the pointer.
    ///
    /// Determines whether to use the first match or all matches:
    /// - [`HtmlScraperSelectMode::First`]: Only the first matching value.
    /// - [`HtmlScraperSelectMode::All`]: All matching values.
    #[serde(default)]
    select: HtmlScraperSelectMode,

    /// Actions applied to the extracted values.
    ///
    /// Transformations applied to the extracted values before storing them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<ScraperAction>,

    /// Nested entries for group entries.
    ///
    /// When this entry is a group, this contains the sub-entries to extract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entries: Vec<JsonScraperEntryRaw>,

    /// Sub-queries executed on each value produced by this entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sub_queries: Vec<EntrySubQueryRaw>,
}

/// One logical field extracted from a JSON result row.
///
/// Represents a validated entry that can extract either a simple field value
/// or a nested group of fields from a JSON row.
#[derive(Deserialize)]
#[serde(try_from = "JsonScraperEntryRaw")]
pub enum JsonScraperEntry {
    /// A simple field entry that extracts a single value or multiple values.
    ///
    /// Extracts values using a JSON pointer and applies actions to transform them.
    Field {
        /// Field name.
        name: String,

        /// JSON output type requested by this field entry.
        output_type: ScraperOutputType,

        /// Optional JSON pointer selecting the value from the row.
        pointer: Option<String>,

        /// Selection mode for the pointer.
        select: HtmlScraperSelectMode,

        /// Actions applied to the extracted values.
        actions: Vec<ScraperAction>,

        /// Sub-queries attached to this entry (recursion across scraper types).
        ///
        /// Always empty at the moment — the field is wired in step 14, the
        /// YAML exposure lands with the executor in step 15+.
        sub_queries: Vec<Box<dyn ScraperQuery>>,
    },

    /// A group entry that extracts multiple nested fields.
    ///
    /// Creates a nested object structure with the extracted sub-entries.
    Group {
        /// Group name.
        name: String,

        /// JSON output type requested by this group entry.
        output_type: ScraperOutputType,

        /// Optional JSON pointer selecting the values from the row.
        pointer: Option<String>,

        /// Selection mode for the pointer.
        select: HtmlScraperSelectMode,

        /// Nested entries to extract for each matched value.
        entries: Vec<JsonScraperEntry>,

        /// Sub-queries attached to this entry (recursion across scraper types).
        ///
        /// Always empty at the moment — the field is wired in step 14, the
        /// YAML exposure lands with the executor in step 15+.
        sub_queries: Vec<Box<dyn ScraperQuery>>,
    },
}

impl JsonScraperEntry {
    /// Returns the flattened list of leaf field names produced by this entry.
    ///
    /// Collects all field names from this entry and its nested sub-entries.
    #[cfg(any(test, feature = "test-support"))]
    pub fn field_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_field_names(None, &mut names);
        names
    }

    /// Applies this entry to the provided JSON row.
    ///
    /// Group entries append explicit array items, while field entries append values.
    /// When `select` is `first`, only the first value is kept.
    ///
    /// # Arguments
    ///
    /// * `root` - Data node receiving the extracted values.
    /// * `row` - JSON row to extract values from.
    /// * `params` - Runtime template parameters.
    /// * `request_url` - URL of the request for action pipelines.
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
                output_type,
                pointer,
                select,
                actions,
                ..
            } => {
                let path: Vec<&str> = name.split('>').map(|segment| segment.trim()).collect();
                let is_first = *select == HtmlScraperSelectMode::First;
                root.set_output_type(&path, *output_type);

                for selected in select_json_values(row, pointer.as_deref(), *select) {
                    let mut values = json_value_to_strings(selected);
                    for action in actions {
                        values = action.apply(&None, values, params, request_url, None, Some(row));
                    }

                    if is_first {
                        if let Some(value) = values.into_iter().next() {
                            root.set_value_typed(&path, value, *output_type);
                        }
                    } else {
                        for value in values {
                            root.push_value_typed(&path, value, *output_type);
                        }
                    }
                }
            }
            JsonScraperEntry::Group {
                name,
                output_type,
                pointer,
                select,
                entries,
                ..
            } => {
                let path: Vec<&str> = name.split('>').map(|segment| segment.trim()).collect();
                root.set_output_type(&path, *output_type);

                for selected in select_json_values(row, pointer.as_deref(), *select) {
                    let mut item = ScraperDataNode::default();
                    for entry in entries {
                        entry.apply_to(&mut item, selected, params, request_url);
                    }

                    root.push_node_typed(&path, item, *output_type);
                }
            }
        }
    }

    /// Returns `true` when this entry is a grouped/nested JSON entry.
    pub fn is_group(&self) -> bool {
        matches!(self, JsonScraperEntry::Group { .. })
    }

    /// Recursively collects leaf field names into `names`, prefixing each with the
    /// group hierarchy separated by ` > `.
    ///
    /// # Arguments
    ///
    /// * `prefix` - Optional group path prefix prepended to each collected name.
    /// * `names` - Accumulator receiving the collected field names.
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

    /// Returns the name of this entry.
    #[cfg(any(test, feature = "test-support"))]
    fn name(&self) -> &str {
        match self {
            JsonScraperEntry::Field { name, .. } => name,
            JsonScraperEntry::Group { name, .. } => name,
        }
    }

    /// Validates every action in the entry against the entry-level contract.
    ///
    /// # Arguments
    ///
    /// * `name` - Entry name used in diagnostic messages.
    /// * `actions` - Ordered extraction steps to validate.
    ///
    /// # Errors
    ///
    /// Returns an error if any action fails validation for the `"entry"` context.
    fn validate_actions(name: &str, actions: &[ScraperAction]) -> Result<()> {
        for action in actions {
            action.validate(name, "entry")?;
        }

        Ok(())
    }

    /// Validates and returns the output type for one scalar JSON entry.
    ///
    /// # Errors
    ///
    /// Returns an error when `type` is missing or when an object type is used
    /// without nested `entries`.
    fn require_field_output_type(
        name: &str,
        output_type: Option<ScraperOutputType>,
        has_sub_queries: bool,
    ) -> Result<ScraperOutputType> {
        let Some(output_type) = output_type else {
            anyhow::bail!("JSON entry {} must define type", name);
        };

        if !has_sub_queries
            && matches!(
                output_type,
                ScraperOutputType::Object | ScraperOutputType::ObjectArray
            )
        {
            anyhow::bail!(
                "JSON entry {} uses {} but does not define entries",
                name,
                output_type.as_str()
            );
        }

        Ok(output_type)
    }

    /// Resolves the output type for one grouped JSON entry.
    ///
    /// # Errors
    ///
    /// Returns an error when `type` is missing, scalar, or contradicts
    /// `select: first`.
    fn resolve_group_output_type(
        name: &str,
        output_type: Option<ScraperOutputType>,
        select: HtmlScraperSelectMode,
    ) -> Result<ScraperOutputType> {
        let Some(output_type) = output_type else {
            anyhow::bail!("JSON group entry {} must define type", name);
        };

        match (output_type, select) {
            (ScraperOutputType::ObjectArray, HtmlScraperSelectMode::First) => {
                anyhow::bail!(
                    "JSON group entry {} cannot combine type object[] with select: first",
                    name
                );
            }
            (ScraperOutputType::ObjectArray, _) => Ok(ScraperOutputType::ObjectArray),
            (ScraperOutputType::Object, HtmlScraperSelectMode::First) => {
                Ok(ScraperOutputType::Object)
            }
            (ScraperOutputType::Object, _) => Ok(ScraperOutputType::ObjectArray),
            _ => anyhow::bail!(
                "JSON group entry {} must use type object or object[], got {}",
                name,
                output_type.as_str()
            ),
        }
    }
}

impl TryFrom<JsonScraperEntryRaw> for JsonScraperEntry {
    type Error = anyhow::Error;

    /// Converts a raw YAML entry definition into a validated runtime entry.
    ///
    /// # Errors
    ///
    /// Returns an error when a group entry defines actions alongside sub-entries.
    fn try_from(config: JsonScraperEntryRaw) -> Result<Self> {
        let JsonScraperEntryRaw {
            name,
            output_type,
            pointer,
            select,
            actions,
            entries,
            sub_queries,
        } = config;

        let converted_sub_queries: Result<Vec<Box<dyn ScraperQuery>>> = sub_queries
            .into_iter()
            .map(|sq| sq.into_boxed_query(""))
            .collect();
        let sub_queries = converted_sub_queries?;

        if entries.is_empty() {
            JsonScraperEntry::validate_actions(&name, &actions)?;
            let output_type = JsonScraperEntry::require_field_output_type(
                &name,
                output_type,
                !sub_queries.is_empty(),
            )?;

            return Ok(Self::Field {
                name,
                output_type,
                pointer,
                select,
                actions,
                sub_queries,
            });
        }

        if !actions.is_empty() {
            anyhow::bail!("JSON group entry {} cannot define actions", name);
        }

        let output_type = JsonScraperEntry::resolve_group_output_type(&name, output_type, select)?;

        let entries = entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;

        Ok(Self::Group {
            name,
            output_type,
            pointer,
            select,
            entries,
            sub_queries,
        })
    }
}

impl From<&JsonScraperEntry> for JsonScraperEntryRaw {
    /// Converts a runtime entry back into its raw YAML-compatible representation.
    fn from(entry: &JsonScraperEntry) -> Self {
        match entry {
            JsonScraperEntry::Field {
                name,
                output_type,
                pointer,
                select,
                actions,
                ..
            } => Self {
                name: name.clone(),
                output_type: Some(*output_type),
                pointer: pointer.clone(),
                select: *select,
                actions: actions.clone(),
                entries: Vec::new(),
                sub_queries: Vec::new(),
            },
            JsonScraperEntry::Group {
                name,
                output_type,
                pointer,
                select,
                entries,
                ..
            } => Self {
                name: name.clone(),
                output_type: Some(*output_type),
                pointer: pointer.clone(),
                select: *select,
                actions: Vec::new(),
                entries: entries.iter().map(JsonScraperEntryRaw::from).collect(),
                sub_queries: Vec::new(),
            },
        }
    }
}

impl Serialize for JsonScraperEntry {
    /// Serializes the entry through its raw YAML representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        JsonScraperEntryRaw::from(self).serialize(serializer)
    }
}

/// Selects JSON values using a pointer-like syntax with wildcard support.
///
/// The pointer is split on `/` and each token is resolved against the current
/// set of JSON values. Tokens may contain array-filter syntax such as
/// `*[role=mea]` or `0[status=active]`.
///
/// # Arguments
///
/// * `root` - Root JSON value to navigate from.
/// * `pointer` - Slash-separated path, optionally starting with `/`.
/// * `select` - Whether to return only the first match or all matches.
///
/// # Examples
///
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

/// Navigates the JSON tree using the full pointer, expanding wildcards and
/// array filters at each level.
///
/// This is the internal implementation that returns all matching values.
///
/// # Arguments
///
/// * `root` - Root JSON value to navigate from.
/// * `pointer` - Slash-separated path, optionally starting with `/`.
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

/// Decodes RFC 6901 pointer escape sequences (`~1` → `/`, `~0` → `~`).
///
/// # Arguments
///
/// * `token` - Raw pointer token to decode.
///
/// # Returns
///
/// The decoded token with escape sequences replaced.
fn decode_pointer_token(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

/// Parses a token that may contain array filtering syntax like `*[role=mea]`
/// or `0[status=active]`.
///
/// Returns `(selector, filter)` if filtering syntax is detected, `None` otherwise.
///
/// # Arguments
///
/// * `token` - Pointer token that may contain `[filter]` suffix.
///
/// # Returns
///
/// A tuple of (array selector, filter condition) if filtering syntax is present.
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

/// Checks if a JSON value matches a filter condition like `"role=mea"`.
///
/// # Arguments
///
/// * `value` - JSON object to test against the filter.
/// * `filter` - Condition string in `field=expected` format.
///
/// # Returns
///
/// `true` if the value matches the filter condition, `false` otherwise.
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

/// Parses a filter condition like `"role=mea"` into `(field, value)`.
///
/// # Arguments
///
/// * `filter` - Condition string in `field=expected` format.
///
/// # Returns
///
/// A tuple of (field name, expected value) if the condition is valid.
fn parse_filter_condition(filter: &str) -> Option<(String, String)> {
    if let Some(eq_pos) = filter.find('=') {
        let field = filter[..eq_pos].trim().to_string();
        let value = filter[eq_pos + 1..].trim().to_string();
        Some((field, value))
    } else {
        None
    }
}

/// Converts a JSON value into a flat list of string representations.
///
/// Strings and numbers are returned as-is, booleans are stringified,
/// arrays are recursively flattened, objects are serialized to JSON, and
/// null values produce an empty list.
///
/// # Arguments
///
/// * `value` - JSON value to convert.
///
/// # Returns
///
/// A vector of string representations of the value.
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

impl ScraperEntrySpec for JsonScraperEntry {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &str {
        match self {
            JsonScraperEntry::Field { name, .. } => name,
            JsonScraperEntry::Group { name, .. } => name,
        }
    }

    fn entry_type(&self) -> ScraperType {
        ScraperType::Json
    }

    fn pointer(&self) -> Option<&str> {
        match self {
            JsonScraperEntry::Field { pointer, .. } => pointer.as_deref(),
            JsonScraperEntry::Group { pointer, .. } => pointer.as_deref(),
        }
    }

    fn selector(&self) -> Option<&str> {
        None
    }

    fn select(&self) -> HtmlScraperSelectMode {
        match self {
            JsonScraperEntry::Field { select, .. } => *select,
            JsonScraperEntry::Group { select, .. } => *select,
        }
    }

    fn actions(&self) -> &[ScraperAction] {
        match self {
            JsonScraperEntry::Field { actions, .. } => actions,
            JsonScraperEntry::Group { .. } => &[],
        }
    }

    fn sub_entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        match self {
            JsonScraperEntry::Field { .. } => Vec::new(),
            JsonScraperEntry::Group { entries, .. } => {
                entries.iter().map(|e| e as &dyn ScraperEntrySpec).collect()
            }
        }
    }

    fn is_group(&self) -> bool {
        matches!(self, JsonScraperEntry::Group { .. })
    }

    fn sub_queries(&self) -> Vec<&dyn crate::scrapyfy::scraper::query_trait::ScraperQuery> {
        match self {
            JsonScraperEntry::Field { sub_queries, .. } => sub_queries
                .iter()
                .map(|b| &**b as &dyn ScraperQuery)
                .collect(),
            JsonScraperEntry::Group { sub_queries, .. } => sub_queries
                .iter()
                .map(|b| &**b as &dyn ScraperQuery)
                .collect(),
        }
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
