//! Text scraper entry definitions and field extractors.
//!
//! A [`TextScraperEntry`] describes a single field or group of fields
//! extracted from each text row. Fields are selected by 1-based index after
//! splitting the row by the query's `field_delimiter`.

use std::any::Any;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::scraper::entry_trait::ScraperEntrySpec;
use crate::scrapyfy::scraper::query_trait::ScraperQuery;
use crate::scrapyfy::scraper::row_locator::ScraperType;
use crate::scrapyfy::scraper_data_node::{ScraperDataNode, ScraperOutputType};
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::*;

/// Raw configuration definition of one field or grouped field extracted from each text row.
///
/// Deserialized from YAML and converted into [`TextScraperEntry`] via [`TryFrom`].
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TextScraperEntryRaw {
    /// Entry name, supporting `>`-separated hierarchical paths.
    pub name: String,
    /// JSON output type expected for this entry.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub output_type: Option<ScraperOutputType>,
    /// Index 1-based du champ après split par `field_delimiter`.
    /// Exemple: `field: 2` sélectionne le 2ème champ splitté.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<usize>,
    /// Ordered extraction steps executed on the selected value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ScraperAction>,
    /// Child entries for group entries (type: object[]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<TextScraperEntryRaw>,
}

impl TextScraperEntryRaw {
    /// Resolves collection-level placeholders used by actions in this entry tree.
    ///
    /// # Arguments
    ///
    /// * `params` - Collection-level template parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if a required placeholder is missing from `params`.
    pub(crate) fn resolve_collection_params(
        &mut self,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        for entry in &mut self.entries {
            entry.resolve_collection_params(params)?;
        }
        Ok(())
    }
}

/// One logical field extracted from a text row.
///
/// An entry defines how to extract data from a split text line using a 1-based
/// field index and a pipeline of actions. Entries can be either simple fields
/// or groups containing nested entries.
#[derive(Deserialize, Clone, Debug)]
#[serde(try_from = "TextScraperEntryRaw")]
pub enum TextScraperEntry {
    /// A leaf field that extracts one value via field index and actions.
    Field {
        /// Output field name used in the extracted metadata map.
        name: String,
        /// JSON output type requested by this field entry.
        output_type: ScraperOutputType,
        /// 1-based index of the field after splitting the row.
        field_index: Option<usize>,
        /// Ordered extraction steps executed on the selected value.
        actions: Vec<ScraperAction>,
    },
    /// A group entry that contains child entries applied to each text row.
    Group {
        /// Group name used to build the output path.
        name: String,
        /// JSON output type requested by this group entry.
        output_type: ScraperOutputType,
        /// Child entries applied to each text row.
        entries: Vec<TextScraperEntry>,
    },
}

impl TextScraperEntry {
    /// Applies this entry to the provided result root for one parsed row.
    ///
    /// # Arguments
    ///
    /// * `root` - Output data node where extracted values are appended.
    /// * `fields` - The fields of the current text row (after split by field_delimiter).
    /// * `params` - Runtime template parameters forwarded to actions.
    /// * `request_url` - Fully resolved request URL forwarded to actions.
    pub fn apply_to(
        &self,
        root: &mut ScraperDataNode,
        fields: &[&str],
        params: &HashMap<String, String>,
        request_url: &str,
    ) {
        match self {
            TextScraperEntry::Field {
                name,
                output_type,
                field_index,
                actions,
            } => {
                let path: Vec<&str> = name.split('>').map(|s| s.trim()).collect();
                root.set_output_type(&path, *output_type);

                let mut values: Vec<String> = Vec::new();
                if let Some(idx) = field_index {
                    if *idx > 0 {
                        if let Some(value) = fields.get(*idx - 1) {
                            values.push((*value).to_string());
                        }
                    }
                }

                for action in actions {
                    values = action.apply(&None, values, params, request_url, None, None);
                }

                for value in values {
                    root.push_value_typed(&path, value, *output_type);
                }
            }
            TextScraperEntry::Group {
                name,
                output_type,
                entries,
            } => {
                let path: Vec<&str> = name.split('>').map(|s| s.trim()).collect();
                root.set_output_type(&path, *output_type);
                let mut item = ScraperDataNode::default();
                for entry in entries {
                    entry.apply_to(&mut item, fields, params, request_url);
                }
                root.push_node_typed(&path, item, *output_type);
            }
        }
    }

    /// Validates the entry tree, ensuring field entries define a type and
    /// groups define child entries.
    ///
    /// # Errors
    ///
    /// Returns an error if a group entry lacks children. Field entries are
    /// guaranteed to carry a type by [`TryFrom`] conversion, so no extra
    /// check is required here.
    pub fn validate(&self) -> Result<()> {
        if let TextScraperEntry::Group { name, entries, .. } = self {
            if entries.is_empty() {
                anyhow::bail!("Text group entry {} must define entries", name);
            }
            for entry in entries {
                entry.validate()?;
            }
        }
        Ok(())
    }

    /// Returns the entry name.
    pub fn name(&self) -> &str {
        match self {
            TextScraperEntry::Field { name, .. } => name,
            TextScraperEntry::Group { name, .. } => name,
        }
    }
}

impl TryFrom<TextScraperEntryRaw> for TextScraperEntry {
    type Error = anyhow::Error;

    /// Converts a raw YAML entry definition into a validated runtime entry.
    ///
    /// # Errors
    ///
    /// Returns an error when a field entry omits `type` or a group lacks children.
    fn try_from(config: TextScraperEntryRaw) -> Result<Self> {
        let TextScraperEntryRaw {
            name,
            output_type,
            field,
            actions,
            entries,
        } = config;

        if entries.is_empty() {
            let output_type = output_type
                .ok_or_else(|| anyhow::anyhow!("Text entry {} must define type", name))?;
            Ok(TextScraperEntry::Field {
                name,
                output_type,
                field_index: field,
                actions,
            })
        } else {
            let output_type = output_type.unwrap_or(ScraperOutputType::ObjectArray);
            let entries = entries
                .into_iter()
                .map(TextScraperEntry::try_from)
                .collect::<Result<Vec<_>>>()?;
            Ok(TextScraperEntry::Group {
                name,
                output_type,
                entries,
            })
        }
    }
}

impl ScraperEntrySpec for TextScraperEntry {
    /// Returns a reference to this entry as a trait object.
    fn as_any(&self) -> &dyn Any {
        self
    }

    /// Returns a mutable reference to this entry as a trait object.
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Returns the entry name.
    fn name(&self) -> &str {
        self.name()
    }

    /// Returns the scraper type for this entry.
    fn entry_type(&self) -> ScraperType {
        ScraperType::Text
    }

    /// Returns the JSON pointer for this entry (always `None` for text entries).
    fn pointer(&self) -> Option<&str> {
        None
    }

    /// Returns the CSS selector for this entry (always `None` for text entries).
    fn selector(&self) -> Option<&str> {
        None
    }

    /// Returns the selection mode for this entry (always All for text entries).
    fn select(&self) -> HtmlScraperSelectMode {
        HtmlScraperSelectMode::All
    }

    /// Returns the actions configured for this entry.
    fn actions(&self) -> &[ScraperAction] {
        match self {
            TextScraperEntry::Field { actions, .. } => actions,
            TextScraperEntry::Group { .. } => &[],
        }
    }

    /// Returns the child entries for this entry.
    fn sub_entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        match self {
            TextScraperEntry::Field { .. } => Vec::new(),
            TextScraperEntry::Group { entries, .. } => {
                entries.iter().map(|e| e as &dyn ScraperEntrySpec).collect()
            }
        }
    }

    /// Returns whether this entry is a group.
    fn is_group(&self) -> bool {
        matches!(self, TextScraperEntry::Group { .. })
    }

    /// Returns the sub-queries attached to this entry (always empty for text entries).
    fn sub_queries(&self) -> Vec<&dyn ScraperQuery> {
        Vec::new()
    }

    /// Returns the 1-based field index for text mode, if configured.
    fn field(&self) -> Option<usize> {
        match self {
            TextScraperEntry::Field { field_index, .. } => *field_index,
            TextScraperEntry::Group { .. } => None,
        }
    }
}

impl From<&TextScraperEntry> for TextScraperEntryRaw {
    /// Converts a runtime entry back into its raw YAML-compatible representation.
    fn from(entry: &TextScraperEntry) -> Self {
        match entry {
            TextScraperEntry::Field {
                name,
                output_type,
                field_index,
                actions,
            } => Self {
                name: name.clone(),
                output_type: Some(*output_type),
                field: *field_index,
                actions: actions.clone(),
                entries: Vec::new(),
            },
            TextScraperEntry::Group {
                name,
                output_type,
                entries,
            } => Self {
                name: name.clone(),
                output_type: Some(*output_type),
                field: None,
                actions: Vec::new(),
                entries: entries.iter().map(TextScraperEntryRaw::from).collect(),
            },
        }
    }
}
