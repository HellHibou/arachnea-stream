//! HTML entry definitions and field extractors.
//!
//! An [`HtmlScraperEntry`] describes a single field or group of fields
//! extracted from each result row via CSS selectors and ordered actions.
//! Raw YAML deserialization and the validated runtime type are both defined here.
//!
//! See [`HtmlScraperEntryRaw`] for the YAML shape and [`HtmlScraperEntry`]
//! for the validated runtime enum.

use std::any::Any;

use anyhow::Result;
use ::scraper::{selector::ToCss, ElementRef, Selector};
use serde::{Deserialize, Serialize, Serializer};

use crate::scrapyfy::*;
use crate::scrapyfy::query_helpers;
use crate::scrapyfy::scraper::entry_trait::ScraperEntrySpec;
use crate::scrapyfy::scraper::query_trait::ScraperQuery;
use crate::scrapyfy::scraper::row_locator::ScraperType;
use crate::scrapyfy::scraper_query_collection::EntrySubQueryRaw;

/// Raw configuration definition of one field or grouped field extracted from each result row.
///
/// Deserialized from YAML and converted into [`HtmlScraperEntry`] via
/// [`TryFrom`].
#[derive(Serialize, Deserialize)]
pub struct HtmlScraperEntryRaw {
    /// Entry name, supporting `>`-separated hierarchical paths.
    name: String,
    /// Optional CSS selector applied relative to each result row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector: Option<String>,
    /// Resolved selector after collection-level placeholder substitution.
    #[serde(skip)]
    resolved_selector: Option<String>,
    /// Whether to use the first match or all matches.
    #[serde(default)]
    select: HtmlScraperSelectMode,
    /// Ordered extraction steps executed on the selected node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<ScraperAction>,
    /// Child entries for group entries (mutually exclusive with `actions`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entries: Vec<HtmlScraperEntryRaw>,
    /// Sub-queries executed on each value produced by this entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sub_queries: Vec<EntrySubQueryRaw>,
}

impl HtmlScraperEntryRaw {
    /// Resolves collection-level placeholders used by selectors in this entry tree.
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
        if let Some(selector) = &self.selector {
            self.resolved_selector = Some(query_helpers::resolve_required_template(
                "HTML entry",
                &self.name,
                "selector",
                selector,
                params,
            )?);
        }

        for entry in &mut self.entries {
            entry.resolve_collection_params(params)?;
        }

        Ok(())
    }
}

/// Selection behavior for entry selectors.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HtmlScraperSelectMode {
    /// Uses only the first matched element.
    First,
    /// Uses all matched elements and concatenates their extracted values.
    #[default]
    All,
}

/// One logical field extracted from a result row.
#[derive(Deserialize)]
#[serde(try_from = "HtmlScraperEntryRaw")]
pub enum HtmlScraperEntry {
    /// A leaf field that extracts one or more values via actions.
    Field {
        /// Output field name used in the extracted metadata map.
        name: String,
        /// Original CSS selector template preserved from YAML.
        selector_template: Option<String>,
        /// Compiled CSS selector used at runtime.
        selector: Option<::scraper::Selector>,
        /// Whether to use the first match or all matches.
        select: HtmlScraperSelectMode,
        /// Ordered extraction steps executed on the selected node.
        actions: Vec<ScraperAction>,
        /// Sub-queries attached to this entry (recursion across scraper types).
        ///
        /// Always empty at the moment — the field is wired in step 14, the
        /// YAML exposure lands with the executor in step 15+.
        sub_queries: Vec<Box<dyn ScraperQuery>>,
    },
    /// A group entry that contains child entries applied to each matched element.
    Group {
        /// Group name used to build the output path.
        name: String,
        /// Original CSS selector template preserved from YAML.
        selector_template: Option<String>,
        /// Compiled CSS selector used at runtime.
        selector: Option<::scraper::Selector>,
        /// Whether to use the first match or all matches.
        select: HtmlScraperSelectMode,
        /// Child entries applied to each matched element.
        entries: Vec<HtmlScraperEntry>,
        /// Sub-queries attached to this entry (recursion across scraper types).
        ///
        /// Always empty at the moment — the field is wired in step 14, the
        /// YAML exposure lands with the executor in step 15+.
        sub_queries: Vec<Box<dyn ScraperQuery>>,
    },
}

impl HtmlScraperEntry {
    /// Creates a field entry and panics if the selector is invalid.
    ///
    /// # Arguments
    ///
    /// * `name` - Output field name used in the extracted metadata map.
    /// * `selector` - CSS selector applied relative to each result row.
    /// * `actions` - Ordered extraction steps executed on the selected node.
    ///
    /// # Panics
    ///
    /// Panics if `selector` is not a valid CSS selector.
    #[allow(dead_code)]
    pub fn new(name: &str, selector: &str, actions: &[ScraperAction]) -> Self {
        Self::try_new(name, selector, actions)
            .unwrap_or_else(|_| panic!("Invalid format: {}", selector))
    }

    /// Creates a field entry and validates the selector ahead of execution.
    ///
    /// # Arguments
    ///
    /// * `name` - Output field name used in the extracted metadata map.
    /// * `selector` - CSS selector applied relative to each result row.
    /// * `actions` - Ordered extraction steps executed on the selected node.
    ///
    /// # Errors
    ///
    /// Returns an error if `selector` is not a valid CSS selector.
    pub fn try_new(name: &str, selector: &str, actions: &[ScraperAction]) -> Result<Self> {
        Self::try_new_field_with_optional_selector(
            name,
            Some(selector),
            Some(selector),
            HtmlScraperSelectMode::First,
            actions,
        )
    }

    /// Returns the output field name used in the final metadata map.
    #[cfg(any(test, feature = "test-support"))]
    pub fn name(&self) -> &str {
        match self {
            HtmlScraperEntry::Field { name, .. } => name,
            HtmlScraperEntry::Group { name, .. } => name,
        }
    }

    /// Returns the flattened list of leaf field names produced by this entry.
    #[cfg(any(test, feature = "test-support"))]
    pub fn field_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_field_names(None, &mut names);
        names
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
            HtmlScraperEntry::Field { .. } => names.push(full_name),
            HtmlScraperEntry::Group { entries, .. } => {
                for entry in entries {
                    entry.collect_field_names(Some(&full_name), names);
                }
            }
        }
    }

    /// Applies this entry to the provided result root.
    ///
    /// Group entries append explicit array items, while field entries append values.
    ///
    /// # Arguments
    ///
    /// * `root` - Output data node where extracted values are appended.
    /// * `card` - Parent HTML element to search within.
    /// * `params` - Runtime template parameters forwarded to actions.
    /// * `request_url` - Fully resolved request URL forwarded to actions.
    /// * `response_body` - Optional raw response body available to actions.
    pub fn apply_to(
        &self,
        root: &mut ScraperDataNode,
        card: ElementRef,
        params: &HashMap<String, String>,
        request_url: &str,
        response_body: Option<&str>,
    ) {
        match self {
            HtmlScraperEntry::Field {
                name,
                selector_template: _,
                selector,
                select,
                actions,
                ..
            } => {
                let path: Vec<&str> = name.split('>').map(|s| s.trim()).collect();
                Self::for_each_selected(selector, *select, card, |selected| {
                    let mut values: Vec<String> = Vec::new();
                    let selected = Some(selected);
                    for action in actions {
                        values = action.apply(
                            &selected,
                            values,
                            params,
                            request_url,
                            response_body,
                            None,
                        );
                    }
                    for value in values {
                        root.push_value(&path, value);
                    }
                });
            }
            HtmlScraperEntry::Group {
                name,
                selector_template: _,
                selector,
                select,
                entries,
                ..
            } => {
                let path: Vec<&str> = name.split('>').map(|s| s.trim()).collect();
                Self::for_each_selected(selector, *select, card, |selected| {
                    let mut item = ScraperDataNode::default();
                    for entry in entries {
                        entry.apply_to(&mut item, selected, params, request_url, response_body);
                    }
                    root.push_node(&path, item);
                });
            }
        }
    }

    /// Parses an optional CSS selector string into a compiled [`Selector`](::scraper::Selector).
    ///
    /// Returns `None` when the input is `None` or empty.
    ///
    /// # Arguments
    ///
    /// * `_name` - Entry name kept for consistent call-site signatures but unused here.
    /// * `selector` - Raw CSS selector string to compile.
    ///
    /// # Errors
    ///
    /// Returns an error if `selector` is non-empty but not a valid CSS selector.
    fn parse_selector(_name: &str, selector: Option<&str>) -> Result<Option<::scraper::Selector>> {
        let selector = match selector {
            Some(selector) if !selector.trim().is_empty() => Some(
                Selector::parse(selector)
                    .map_err(|err| anyhow::anyhow!("Invalid selector {}: {:?}", selector, err))?,
            ),
            _ => None,
        };

        Ok(selector)
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

    /// Normalizes an optional selector template by trimming and discarding empty strings.
    ///
    /// # Arguments
    ///
    /// * `selector` - Raw selector template string to normalize.
    fn normalize_selector_template(selector: Option<&str>) -> Option<String> {
        selector
            .filter(|selector| !selector.trim().is_empty())
            .map(str::to_string)
    }

    /// Produces the CSS selector string used during serialization, preferring
    /// the original template when available and falling back to the compiled selector.
    ///
    /// # Arguments
    ///
    /// * `selector_template` - Original template string preserved from YAML.
    /// * `selector` - Compiled CSS selector used at runtime.
    fn serialize_selector(
        selector_template: &Option<String>,
        selector: &Option<::scraper::Selector>,
    ) -> Option<String> {
        selector_template
            .clone()
            .or_else(|| selector.as_ref().map(Selector::to_css_string))
    }

    /// Builds a [`HtmlScraperEntry::Field`] variant with an optional CSS selector.
    ///
    /// # Arguments
    ///
    /// * `name` - Output field name used in the extracted metadata map.
    /// * `selector_template` - Original template string kept for serialization.
    /// * `resolved_selector` - Resolved CSS selector compiled at construction time.
    /// * `select` - Whether to use the first match or all matches.
    /// * `actions` - Ordered extraction steps executed on the selected node.
    ///
    /// # Errors
    ///
    /// Returns an error if `resolved_selector` is not a valid CSS selector or if
    /// any action fails validation.
    fn try_new_field_with_optional_selector(
        name: &str,
        selector_template: Option<&str>,
        resolved_selector: Option<&str>,
        select: HtmlScraperSelectMode,
        actions: &[ScraperAction],
    ) -> Result<Self> {
        Self::validate_actions(name, actions)?;

        Ok(HtmlScraperEntry::Field {
            name: name.to_string(),
            selector_template: Self::normalize_selector_template(selector_template),
            selector: Self::parse_selector(name, resolved_selector)?,
            select,
            actions: actions.to_vec(),
            sub_queries: Vec::new(),
        })
    }

    /// Builds a [`HtmlScraperEntry::Group`] variant with an optional CSS selector.
    ///
    /// # Arguments
    ///
    /// * `name` - Group name used to build the output path.
    /// * `selector_template` - Original template string kept for serialization.
    /// * `resolved_selector` - Resolved CSS selector compiled at construction time.
    /// * `select` - Whether to use the first match or all matches.
    /// * `entries` - Child entries applied to each matched element.
    ///
    /// # Errors
    ///
    /// Returns an error if `resolved_selector` is not a valid CSS selector.
    fn try_new_group_with_optional_selector(
        name: &str,
        selector_template: Option<&str>,
        resolved_selector: Option<&str>,
        select: HtmlScraperSelectMode,
        entries: Vec<HtmlScraperEntry>,
    ) -> Result<Self> {
        Ok(HtmlScraperEntry::Group {
            name: name.to_string(),
            selector_template: Self::normalize_selector_template(selector_template),
            selector: Self::parse_selector(name, resolved_selector)?,
            select,
            entries,
            sub_queries: Vec::new(),
        })
    }

    /// Iterates over the CSS selector matches on `card` and invokes `callback` for each.
    ///
    /// When no selector is provided the callback is invoked once on the card itself.
    ///
    /// # Arguments
    ///
    /// * `selector` - Compiled CSS selector applied to the card element.
    /// * `select` - Whether to use the first match or all matches.
    /// * `card` - Root HTML element to search within.
    /// * `callback` - Function invoked for each selected element.
    fn for_each_selected<F>(
        selector: &Option<::scraper::Selector>,
        select: HtmlScraperSelectMode,
        card: ElementRef,
        mut callback: F,
    ) where
        F: FnMut(ElementRef),
    {
        match (selector, select) {
            (Some(selector), HtmlScraperSelectMode::All) => {
                for selected in card.select(selector) {
                    callback(selected);
                }
            }
            (Some(selector), HtmlScraperSelectMode::First) => {
                if let Some(selected) = card.select(selector).next() {
                    callback(selected);
                }
            }
            (None, _) => callback(card),
        }
    }
}

impl HtmlScraperEntry {
    /// Builds a runtime entry from raw config, converting sub-queries if any.
    ///
    /// # Arguments
    ///
    /// * `config` - Raw YAML entry definition.
    /// * `base_url` - Base URL used to create HTTP clients for sub-queries.
    ///
    /// # Errors
    ///
    /// Returns an error if entries or sub-queries are invalid.
    fn from_raw_with_base_url(config: HtmlScraperEntryRaw, base_url: &str) -> Result<Self> {
        let HtmlScraperEntryRaw {
            name,
            selector,
            resolved_selector,
            select,
            actions,
            entries,
            sub_queries,
        } = config;

        let converted_sub_queries: Result<Vec<Box<dyn ScraperQuery>>> = sub_queries
            .into_iter()
            .map(|sq| sq.into_boxed_query(base_url))
            .collect();
        let sub_queries = converted_sub_queries?;

        match (actions.is_empty(), entries.is_empty()) {
            (false, true) => {
                let mut entry = HtmlScraperEntry::try_new_field_with_optional_selector(
                    &name,
                    selector.as_deref(),
                    resolved_selector.as_deref().or(selector.as_deref()),
                    select,
                    &actions,
                )?;
                if let HtmlScraperEntry::Field { sub_queries: ref mut sq, .. } = entry {
                    *sq = sub_queries;
                }
                Ok(entry)
            }
            (true, false) => {
                let mut entry = {
                    let entries = entries
                        .into_iter()
                        .map(|e| HtmlScraperEntry::from_raw_with_base_url(e, base_url))
                        .collect::<Result<Vec<_>>>()?;
                    HtmlScraperEntry::try_new_group_with_optional_selector(
                        &name,
                        selector.as_deref(),
                        resolved_selector.as_deref().or(selector.as_deref()),
                        select,
                        entries,
                    )?
                };
                if let HtmlScraperEntry::Group { sub_queries: ref mut sq, .. } = entry {
                    *sq = sub_queries;
                }
                Ok(entry)
            }
            (true, true) if !sub_queries.is_empty() => {
                // Entry with only sub_queries and no actions/entries: it's a passthrough
                Ok(HtmlScraperEntry::Field {
                    name: name.clone(),
                    selector_template: Self::normalize_selector_template(selector.as_deref()),
                    selector: Self::parse_selector(&name, resolved_selector.as_deref().or(selector.as_deref()))?,
                    select,
                    actions: Vec::new(),
                    sub_queries,
                })
            }
            (true, true) => anyhow::bail!("Entry {} must define either actions or entries", name),
            (false, false) => {
                anyhow::bail!("Entry {} cannot define both actions and entries", name)
            }
        }
    }
}

impl TryFrom<HtmlScraperEntryRaw> for HtmlScraperEntry {
    type Error = anyhow::Error;

    /// Converts a raw YAML entry definition into a validated runtime entry.
    ///
    /// Uses `""` as default base URL for sub-query HTTP clients.
    ///
    /// # Errors
    ///
    /// Returns an error when the entry defines both `actions` and `entries`,
    /// or when it defines neither.
    fn try_from(config: HtmlScraperEntryRaw) -> Result<Self> {
        Self::from_raw_with_base_url(config, "")
    }
}

impl ScraperEntrySpec for HtmlScraperEntry {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &str {
        match self {
            HtmlScraperEntry::Field { name, .. } => name,
            HtmlScraperEntry::Group { name, .. } => name,
        }
    }

    fn entry_type(&self) -> ScraperType {
        ScraperType::Html
    }

    fn pointer(&self) -> Option<&str> {
        None
    }

    fn selector(&self) -> Option<&str> {
        None
    }

    fn select(&self) -> HtmlScraperSelectMode {
        match self {
            HtmlScraperEntry::Field { select, .. } => *select,
            HtmlScraperEntry::Group { select, .. } => *select,
        }
    }

    fn actions(&self) -> &[ScraperAction] {
        match self {
            HtmlScraperEntry::Field { actions, .. } => actions,
            HtmlScraperEntry::Group { .. } => &[],
        }
    }

    fn sub_entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        match self {
            HtmlScraperEntry::Field { .. } => Vec::new(),
            HtmlScraperEntry::Group { entries, .. } => {
                entries.iter().map(|e| e as &dyn ScraperEntrySpec).collect()
            }
        }
    }

    fn is_group(&self) -> bool {
        matches!(self, HtmlScraperEntry::Group { .. })
    }

    fn sub_queries(&self) -> Vec<&dyn crate::scrapyfy::scraper::query_trait::ScraperQuery> {
        match self {
            HtmlScraperEntry::Field { sub_queries, .. } => {
                sub_queries.iter().map(|b| &**b as &dyn ScraperQuery).collect()
            }
            HtmlScraperEntry::Group { sub_queries, .. } => {
                sub_queries.iter().map(|b| &**b as &dyn ScraperQuery).collect()
            }
        }
    }
}

impl From<&HtmlScraperEntry> for HtmlScraperEntryRaw {
    /// Converts a runtime entry back into its raw YAML-compatible representation.
    fn from(entry: &HtmlScraperEntry) -> Self {
        match entry {
            HtmlScraperEntry::Field {
                name,
                selector_template,
                selector,
                select,
                actions,
                ..
            } => Self {
                name: name.clone(),
                selector: HtmlScraperEntry::serialize_selector(selector_template, selector),
                resolved_selector: None,
                select: *select,
                actions: actions.clone(),
                entries: Vec::new(),
                sub_queries: Vec::new(),
            },
            HtmlScraperEntry::Group {
                name,
                selector_template,
                selector,
                select,
                entries,
                ..
            } => Self {
                name: name.clone(),
                selector: HtmlScraperEntry::serialize_selector(selector_template, selector),
                resolved_selector: None,
                select: *select,
                actions: Vec::new(),
                entries: entries.iter().map(HtmlScraperEntryRaw::from).collect(),
                sub_queries: Vec::new(),
            },
        }
    }
}

impl Serialize for HtmlScraperEntry {
    /// Serializes the entry through its raw YAML representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        HtmlScraperEntryRaw::from(self).serialize(serializer)
    }
}