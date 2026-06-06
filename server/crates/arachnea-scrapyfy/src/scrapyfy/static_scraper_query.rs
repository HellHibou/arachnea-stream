use anyhow::Result;
use serde::{Deserialize, Serialize, Serializer};
use serde_yaml::Value;
use std::collections::HashMap;

use super::*;
use crate::scrapyfy::query_helpers;

/// Raw configuration definition of one static field or list of static objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticScraperEntryRaw {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<HashMap<String, Value>>,
}

/// Raw configuration definition for a static query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticScraperQueryRaw {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<StaticScraperEntryRaw>,
}

impl StaticScraperQueryRaw {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.name.clone()
    }

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
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "StaticScraperQueryRaw")]
pub struct StaticScraperQuery {
    name: String,
    media_types: Vec<String>,
    entries: Vec<StaticScraperEntryRaw>,
}

impl StaticScraperQuery {
    /// Creates a static query from validated configuration.
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

            if entry.value.is_some() && !entry.items.is_empty() {
                anyhow::bail!(
                    "Static query {} entry {} cannot define both value and items",
                    name,
                    entry.name
                );
            }
        }

        Ok(Self {
            name,
            media_types,
            entries,
        })
    }

    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.name.clone()
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
            if entry.value.is_some() {
                names.push(entry.name.clone());
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

    /// Executes the static query and returns one row.
    pub async fn execute_query(
        &self,
        params: &HashMap<String, String>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        let mut root = ScraperDataNode::default();

        for entry in &self.entries {
            let entry_path = split_static_path(&entry.name);

            if let Some(value) = &entry.value {
                for value in render_yaml_values(value, params)? {
                    root.push_value(&entry_path, value);
                }
            }

            for item in &entry.items {
                let mut item_node = ScraperDataNode::default();
                for (item_name, item_value) in item {
                    if let Some(value) = render_yaml_value(item_value, params)? {
                        let item_path = split_static_path(item_name);
                        item_node.push_value(&item_path, value);
                    }
                }
                root.push_node(&entry_path, item_node);
            }
        }

        if fields_filters
            .map(|filters| query_helpers::is_root_filtered(&root, filters))
            .unwrap_or(false)
        {
            return Ok(Vec::new());
        }

        Ok(vec![root.children])
    }
}

impl TryFrom<StaticScraperQueryRaw> for StaticScraperQuery {
    type Error = anyhow::Error;

    fn try_from(config: StaticScraperQueryRaw) -> Result<Self> {
        StaticScraperQuery::try_new(config.name, config.media_types, config.entries)
    }
}

impl From<&StaticScraperQuery> for StaticScraperQueryRaw {
    fn from(query: &StaticScraperQuery) -> Self {
        Self {
            name: query.name.clone(),
            media_types: query.media_types.clone(),
            entries: query.entries.clone(),
        }
    }
}

impl Serialize for StaticScraperQuery {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        StaticScraperQueryRaw::from(self).serialize(serializer)
    }
}

fn split_static_path(path: &str) -> Vec<&str> {
    path.split('>')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn render_static_value(value: &str, params: &HashMap<String, String>) -> Result<String> {
    query_helpers::format_query_template("", value, params)
}

fn render_yaml_value(value: &Value, params: &HashMap<String, String>) -> Result<Option<String>> {
    let Some(value) = yaml_value_to_template(value) else {
        return Ok(None);
    };

    Ok(Some(render_static_value(&value, params)?))
}

fn render_yaml_values(value: &Value, params: &HashMap<String, String>) -> Result<Vec<String>> {
    yaml_value_to_templates(value)
        .into_iter()
        .map(|value| render_static_value(&value, params))
        .collect()
}

fn yaml_value_to_templates(value: &Value) -> Vec<String> {
    match value {
        Value::Null => Vec::new(),
        Value::Sequence(values) => values.iter().filter_map(yaml_value_to_template).collect(),
        _ => yaml_value_to_template(value).into_iter().collect(),
    }
}

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
