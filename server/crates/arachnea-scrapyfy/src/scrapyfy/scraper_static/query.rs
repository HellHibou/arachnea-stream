use std::any::Any;
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize, Serializer};
use serde_yaml::Value;
use std::collections::HashMap;

use crate::scrapyfy::query_helpers;
use crate::scrapyfy::query_helpers::DynamicTemplateVariables;
use crate::scrapyfy::scraper::config::{ScraperRequestHeader, ScraperRequestMethod};
use crate::scrapyfy::scraper::{RowLocator, ScraperEntrySpec, ScraperQuery, ScraperType};
use crate::scrapyfy::scraper_data_node::ScraperDataNode;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::*;

/// Raw configuration definition of one static field or list of static objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticScraperEntryRaw {
    /// Entry name, supporting `>`-separated hierarchical paths.
    pub name: String,
    /// JSON output type expected for this static entry.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub output_type: Option<ScraperOutputType>,
    /// Optional scalar YAML value rendered through template placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// Optional actions applied after rendering `value`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ScraperAction>,
    /// Optional list of static objects whose values are also template-rendered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<HashMap<String, Value>>,
}

/// Raw configuration definition for a static query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticScraperQueryRaw {
    /// Query identifier used as the lookup key in a collection.
    pub name: String,
    /// Content types this query produces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_types: Vec<String>,
    /// Static entries rendered at query execution time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<StaticScraperEntryRaw>,
}

impl StaticScraperQueryRaw {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Validates that all template placeholders in entry values can be resolved
    /// against the collection-level parameters.
    ///
    /// # Arguments
    ///
    /// * `params` - Collection-level template parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if a placeholder is missing from `params`.
    pub(crate) fn resolve_collection_params(
        &mut self,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        for entry in &self.entries {
            if let Some(value) = &entry.value {
                for value in yaml_value_to_templates(value) {
                    query_helpers::resolve_required_template(
                        "static query",
                        &self.name,
                        &entry.name,
                        &value,
                        params,
                    )?;
                }
            }

            for action in &entry.actions {
                action.validate(&entry.name, "static entry")?;
            }

            for item in &entry.items {
                for (item_name, item_value) in item {
                    if let Some(value) = yaml_value_to_template(item_value) {
                        query_helpers::resolve_required_template(
                            "static query",
                            &self.name,
                            item_name,
                            &value,
                            params,
                        )?;
                    }
                }
            }
        }

        Ok(())
    }
}

/// Runtime query that returns YAML-declared rows without an HTTP request.
#[derive(Clone, Deserialize)]
#[serde(try_from = "StaticScraperQueryRaw")]
pub struct StaticScraperQuery {
    name: String,
    media_types: Vec<String>,
    entries: Vec<StaticScraperEntryRaw>,
    /// HTTP configuration placeholder — static queries never issue HTTP requests
    /// but the [`ScraperQuery`] trait surface requires a stable reference.
    http_config: ScraperHttpConfig,
    /// HTTP client placeholder for the [`ScraperQuery`] trait.
    http_client: HttpClient,
}

impl StaticScraperQuery {
    /// Creates a static query from validated configuration.
    ///
    /// # Arguments
    ///
    /// * `name` - Query identifier used as the lookup key in a collection.
    /// * `media_types` - Content types this query produces.
    /// * `entries` - Static entries rendered at query execution time.
    ///
    /// # Errors
    ///
    /// Returns an error if the name is empty, an entry name is empty, or an
    /// entry defines both `value` and `items`.
    pub fn try_new(
        name: String,
        media_types: Vec<String>,
        entries: Vec<StaticScraperEntryRaw>,
    ) -> Result<Self> {
        if name.trim().is_empty() {
            anyhow::bail!("Static query name cannot be empty");
        }

        for entry in &entries {
            if entry.name.trim().is_empty() {
                anyhow::bail!("Static query {} has an empty entry name", name);
            }

            let Some(output_type) = entry.output_type else {
                anyhow::bail!(
                    "Static query {} entry {} must define type",
                    name,
                    entry.name
                );
            };

            if entry.value.is_some() && !entry.items.is_empty() {
                anyhow::bail!(
                    "Static query {} entry {} cannot define both value and items",
                    name,
                    entry.name
                );
            }

            if !entry.items.is_empty() && output_type.is_scalar() {
                anyhow::bail!(
                    "Static query {} entry {} must use type object or object[] for items",
                    name,
                    entry.name
                );
            }
        }

        Ok(Self {
            name,
            media_types,
            entries,
            http_config: ScraperHttpConfig::default(),
            http_client: HttpClient::new(""),
        })
    }

    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Rebinds the placeholder HTTP client to shared runtime handles.
    pub(crate) fn set_runtime_handles(
        &mut self,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
    ) {
        self.http_client = HttpClient::with_http_config_proxy_handle_and_local_country(
            self.http_config.clone(),
            proxy_handle,
            local_country,
        );
    }

    /// Returns whether the query matches at least one requested media type.
    pub fn is_media_type(&self, media_types: &[String]) -> bool {
        self.media_types.is_empty()
            || media_types.is_empty()
            || self
                .media_types
                .iter()
                .any(|media_type| media_types.contains(media_type))
    }

    /// Returns the flattened list of leaf field names produced by this query.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_field_names(&self) -> Vec<String> {
        let mut names = Vec::new();

        for entry in &self.entries {
            if let Some(value) = &entry.value {
                match entry.output_type {
                    Some(ScraperOutputType::Object | ScraperOutputType::ObjectArray) => {
                        collect_static_object_field_names(&entry.name, value, &mut names);
                    }
                    _ => names.push(entry.name.clone()),
                }
            }

            for item in &entry.items {
                for item_name in item.keys() {
                    names.push(format!("{} > {}", entry.name, item_name));
                }
            }
        }

        names.sort();
        names.dedup();
        names
    }
}

#[cfg(any(test, feature = "test-support"))]
fn collect_static_object_field_names(prefix: &str, value: &Value, names: &mut Vec<String>) {
    match value {
        Value::Mapping(mapping) => {
            for (key, child) in mapping {
                let Some(key) = key.as_str() else {
                    continue;
                };
                let child_prefix = format!("{} > {}", prefix, key);
                match child {
                    Value::Mapping(_) | Value::Sequence(_) => {
                        collect_static_object_field_names(&child_prefix, child, names);
                    }
                    _ => names.push(child_prefix),
                }
            }
        }
        Value::Sequence(items) => {
            for item in items {
                collect_static_object_field_names(prefix, item, names);
            }
        }
        _ => {}
    }
}

impl TryFrom<StaticScraperQueryRaw> for StaticScraperQuery {
    type Error = anyhow::Error;

    /// Converts a raw static query definition into a validated runtime query.
    ///
    /// # Errors
    ///
    /// Returns an error if the name is empty or entries are inconsistent.
    fn try_from(config: StaticScraperQueryRaw) -> Result<Self> {
        StaticScraperQuery::try_new(config.name, config.media_types, config.entries)
    }
}

impl From<&StaticScraperQuery> for StaticScraperQueryRaw {
    /// Converts a runtime static query back into its raw YAML representation.
    fn from(query: &StaticScraperQuery) -> Self {
        Self {
            name: query.name.clone(),
            media_types: query.media_types.clone(),
            entries: query.entries.clone(),
        }
    }
}

impl Serialize for StaticScraperQuery {
    /// Serializes the static query through its raw YAML representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        StaticScraperQueryRaw::from(self).serialize(serializer)
    }
}

impl StaticScraperEntryRaw {
    /// Applies this static entry to the provided root data node.
    ///
    /// Resolves the optional `value` template (or the per-item map when `items`
    /// is set) against the provided runtime parameters and pushes the rendered
    /// values into the root at the `>`-separated `name` path. Mirrors the
    /// legacy `StaticScraperQuery::execute_query` semantics consumed before
    /// the unified executor took over.
    ///
    /// # Arguments
    ///
    /// * `root` - Mutable root data node receiving the rendered values.
    /// * `params` - Runtime parameters used to resolve template placeholders.
    ///
    /// # Errors
    ///
    /// Returns an error if a template placeholder cannot be resolved.
    pub fn apply_to(
        &self,
        root: &mut ScraperDataNode,
        params: &HashMap<String, String>,
        dynamic_variables: &Mutex<DynamicTemplateVariables>,
    ) -> Result<()> {
        let entry_path = split_static_path(&self.name);
        let output_type = self
            .output_type
            .ok_or_else(|| anyhow::anyhow!("Static entry {} must define type", self.name))?;

        if let Some(value) = &self.value {
            root.set_output_type(&entry_path, output_type);
            let mut values = render_yaml_values(value, params)?;
            for action in &self.actions {
                values = action.apply_with_dynamic_variables(
                    &None,
                    values,
                    params,
                    "",
                    None,
                    None,
                    Some(dynamic_variables),
                )?;
            }

            if matches!(
                output_type,
                ScraperOutputType::Object | ScraperOutputType::ObjectArray
            ) {
                let node = rendered_json_values_to_node(&self.name, &values, output_type)?;
                set_static_node(root, &entry_path, node);
                return Ok(());
            }

            for value in values {
                root.push_value_typed(&entry_path, value, output_type);
            }
        }

        let item_output_type = match output_type {
            ScraperOutputType::Object | ScraperOutputType::ObjectArray => {
                ScraperOutputType::ObjectArray
            }
            _ => output_type,
        };

        if !self.items.is_empty() {
            root.set_output_type(&entry_path, item_output_type);
        }

        for item in &self.items {
            let mut item_node = ScraperDataNode::default();
            for (item_name, item_value) in item {
                if let Some(value) = render_yaml_value(item_value, params)? {
                    let item_path = split_static_path(item_name);
                    item_node.push_value_typed(&item_path, value, ScraperOutputType::String);
                }
            }
            root.push_node_typed(&entry_path, item_node, item_output_type);
        }

        Ok(())
    }
}

impl ScraperEntrySpec for StaticScraperEntryRaw {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// Returns the entry name.
    fn name(&self) -> &str {
        &self.name
    }

    /// Returns the entry type discriminant (Static).
    fn entry_type(&self) -> ScraperType {
        ScraperType::Static
    }

    /// Returns `None` (static entries have no JSON pointer).
    fn pointer(&self) -> Option<&str> {
        None
    }

    /// Returns `None` (static entries have no CSS selector).
    fn selector(&self) -> Option<&str> {
        None
    }

    /// Returns the default selection mode (static entries resolve to a single value).
    fn select(&self) -> HtmlScraperSelectMode {
        HtmlScraperSelectMode::First
    }

    /// Returns an empty action list (static entries do not run actions).
    fn actions(&self) -> &[ScraperAction] {
        &self.actions
    }

    /// Returns the nested sub-entries (always empty for static entries).
    fn sub_entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        Vec::new()
    }

    /// Returns `true` when this entry defines a list of static items.
    fn is_group(&self) -> bool {
        !self.items.is_empty()
    }

    /// Returns an empty sub-query list (static entries do not attach sub-queries).
    fn sub_queries(&self) -> Vec<&dyn ScraperQuery> {
        Vec::new()
    }
}

impl ScraperQuery for StaticScraperQuery {
    fn scraper_type(&self) -> ScraperType {
        ScraperType::Static
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn media_types(&self) -> &[String] {
        &self.media_types
    }

    fn is_media_type(&self, media_types: &[String]) -> bool {
        StaticScraperQuery::is_media_type(self, media_types)
    }

    fn base_url(&self) -> &str {
        ""
    }

    fn query_url(&self) -> &str {
        ""
    }

    fn request_method(&self) -> ScraperRequestMethod {
        ScraperRequestMethod::Get
    }

    fn request_pointer(&self) -> Option<&str> {
        None
    }

    fn request_select(&self) -> HtmlScraperSelectMode {
        HtmlScraperSelectMode::First
    }

    fn request_actions(&self) -> &[ScraperAction] {
        const EMPTY: &[ScraperAction] = &[];
        EMPTY
    }

    fn request_headers(&self) -> &[ScraperRequestHeader] {
        const EMPTY: &[ScraperRequestHeader] = &[];
        EMPTY
    }

    fn http_config(&self) -> &ScraperHttpConfig {
        &self.http_config
    }

    fn http_client(&self) -> &HttpClient {
        &self.http_client
    }

    fn extract_next_data(&self) -> bool {
        false
    }

    fn row_locator(&self) -> RowLocator {
        RowLocator::Single
    }

    fn post_processes(&self) -> &[crate::scrapyfy::ScraperPostProcess] {
        const EMPTY: &[crate::scrapyfy::ScraperPostProcess] = &[];
        EMPTY
    }

    fn result_item_field(&self) -> Option<&str> {
        None
    }

    fn entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        self.entries
            .iter()
            .map(|entry| entry as &dyn ScraperEntrySpec)
            .collect()
    }

    fn sub_queries(&self) -> Vec<&dyn ScraperQuery> {
        Vec::new()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Splits a `>`-separated path into trimmed, non-empty segments.
///
/// # Arguments
///
/// * `path` - Hierarchical path string.
fn split_static_path(path: &str) -> Vec<&str> {
    path.split('>')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// Renders a single template string through the query parameter set.
///
/// # Arguments
///
/// * `value` - Template string containing `{placeholder}` tokens.
/// * `params` - Runtime parameters used for placeholder resolution.
fn render_static_value(value: &str, params: &HashMap<String, String>) -> Result<String> {
    query_helpers::format_query_template("", value, params)
}

/// Renders one YAML value into an optional string, returning `None` for null values.
///
/// # Arguments
///
/// * `value` - YAML value to convert and render.
/// * `params` - Runtime parameters used for placeholder resolution.
fn render_yaml_value(value: &Value, params: &HashMap<String, String>) -> Result<Option<String>> {
    let Some(value) = yaml_value_to_template(value) else {
        return Ok(None);
    };

    Ok(Some(render_static_value(&value, params)?))
}

/// Renders a YAML value into a list of strings, expanding sequences element by element.
///
/// # Arguments
///
/// * `value` - YAML value to convert and render.
/// * `params` - Runtime parameters used for placeholder resolution.
fn render_yaml_values(value: &Value, params: &HashMap<String, String>) -> Result<Vec<String>> {
    yaml_value_to_templates(value)
        .into_iter()
        .map(|value| render_static_value(&value, params))
        .collect()
}

/// Extracts template strings from a YAML value, expanding sequences into individual entries.
///
/// # Arguments
///
/// * `value` - YAML value to extract templates from.
fn yaml_value_to_templates(value: &Value) -> Vec<String> {
    match value {
        Value::Null => Vec::new(),
        Value::Sequence(values) => values.iter().filter_map(yaml_value_to_template).collect(),
        _ => yaml_value_to_template(value).into_iter().collect(),
    }
}

/// Converts a single YAML value into a template string, returning `None` for null
/// and serializing complex types as JSON.
///
/// # Arguments
///
/// * `value` - YAML value to convert.
fn yaml_value_to_template(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => Some(value.clone()),
        Value::Sequence(_) | Value::Mapping(_) | Value::Tagged(_) => {
            serde_json::to_string(value).ok()
        }
    }
}

fn set_static_node(root: &mut ScraperDataNode, path: &[&str], node: ScraperDataNode) {
    if path.is_empty() {
        return;
    }

    let mut current = root;
    for segment in &path[..path.len().saturating_sub(1)] {
        current = current.children.entry((*segment).to_string()).or_default();
    }

    current
        .children
        .insert(path[path.len().saturating_sub(1)].to_string(), node);
}

fn rendered_json_values_to_node(
    entry_name: &str,
    values: &[String],
    output_type: ScraperOutputType,
) -> Result<ScraperDataNode> {
    if values.is_empty() {
        let mut node = ScraperDataNode::default();
        node.set_self_output_type(output_type);
        return Ok(node);
    }

    if matches!(output_type, ScraperOutputType::Object) && values.len() != 1 {
        anyhow::bail!(
            "Static entry {} cannot serialize {} values as object",
            entry_name,
            values.len()
        );
    }

    if matches!(output_type, ScraperOutputType::Object) {
        let value: serde_json::Value = serde_json::from_str(&values[0]).with_context(|| {
            format!(
                "Static entry {} expected a JSON object value for type object",
                entry_name
            )
        })?;
        let mut node = json_value_to_node(value, Some(ScraperOutputType::Object))?;
        node.set_self_output_type(ScraperOutputType::Object);
        return Ok(node);
    }

    let mut node = ScraperDataNode::default();
    node.set_self_output_type(ScraperOutputType::ObjectArray);
    for value in values {
        let value: serde_json::Value = serde_json::from_str(value).with_context(|| {
            format!(
                "Static entry {} expected JSON object values for type object[]",
                entry_name
            )
        })?;
        let item = json_value_to_node(value, Some(ScraperOutputType::Object))?;
        node.items.push(item);
    }
    Ok(node)
}

fn json_value_to_node(
    value: serde_json::Value,
    forced_type: Option<ScraperOutputType>,
) -> Result<ScraperDataNode> {
    match value {
        serde_json::Value::Null => {
            let mut node = ScraperDataNode::default();
            if let Some(output_type) = forced_type {
                node.set_self_output_type(output_type);
            }
            Ok(node)
        }
        serde_json::Value::Bool(value) => Ok(ScraperDataNode::from_values_typed(
            vec![value.to_string()],
            forced_type.unwrap_or(ScraperOutputType::Boolean),
        )),
        serde_json::Value::Number(value) => Ok(ScraperDataNode::from_values_typed(
            vec![value.to_string()],
            forced_type.unwrap_or(ScraperOutputType::Number),
        )),
        serde_json::Value::String(value) => Ok(ScraperDataNode::from_values_typed(
            vec![value],
            forced_type.unwrap_or(ScraperOutputType::String),
        )),
        serde_json::Value::Array(values) => {
            let mut node = ScraperDataNode::default();
            node.set_self_output_type(forced_type.unwrap_or(ScraperOutputType::ObjectArray));
            for value in values {
                node.items.push(json_value_to_node(value, None)?);
            }
            Ok(node)
        }
        serde_json::Value::Object(values) => {
            let mut node = ScraperDataNode::default();
            node.set_self_output_type(forced_type.unwrap_or(ScraperOutputType::Object));
            for (key, value) in values {
                node.children.insert(key, json_value_to_node(value, None)?);
            }
            Ok(node)
        }
    }
}
