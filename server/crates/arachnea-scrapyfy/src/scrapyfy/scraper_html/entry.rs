//! HTML entry definitions and field extractors.
//!
//! An [`HtmlScraperEntry`] describes a single field or group of fields
//! extracted from each result row via CSS selectors and ordered actions.
//! Raw YAML deserialization and the validated runtime type are both defined here.
//!
//! See [`HtmlScraperEntryRaw`] for the YAML shape and [`HtmlScraperEntry`]
//! for the validated runtime enum.

use std::any::Any;
use std::sync::Mutex;

use ::scraper::{selector::ToCss, ElementRef, Selector};
use anyhow::Result;
use serde::{Deserialize, Serialize, Serializer};

use crate::scrapyfy::query_helpers::{self, DynamicTemplateVariables};
use crate::scrapyfy::scraper::entry_trait::ScraperEntrySpec;
use crate::scrapyfy::scraper::post_build::ScraperPostBuild;
use crate::scrapyfy::scraper::query_trait::ScraperQuery;
use crate::scrapyfy::scraper::row_locator::ScraperType;
use crate::scrapyfy::EntrySubQueryRaw;
use crate::scrapyfy::*;

/// Raw configuration definition of one field or grouped field extracted from each result row.
///
/// Deserialized from YAML and converted into [`HtmlScraperEntry`] via
/// [`TryFrom`].
#[derive(Serialize, Deserialize)]
pub struct HtmlScraperEntryRaw {
    /// Entry name, supporting `>`-separated hierarchical paths.
    ///
    /// This name is used as the key in the output data structure.
    /// Hierarchical paths like "parent>child" create nested objects.
    name: String,
    /// JSON output type expected for this entry.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    output_type: Option<ScraperOutputType>,
    /// Optional CSS selector applied relative to each result row.
    ///
    /// If provided, this selector is used to find elements within each row.
    /// If `None`, the entry operates on the row element itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector: Option<String>,
    /// Resolved selector after collection-level placeholder substitution.
    ///
    /// This is computed at runtime by resolving any `{placeholder}` templates
    /// in the original selector using collection parameters.
    #[serde(skip)]
    resolved_selector: Option<String>,
    /// Whether to use the first match or all matches.
    ///
    /// - [`HtmlScraperSelectMode::First`]: Only the first matching element.
    /// - [`HtmlScraperSelectMode::All`]: All matching elements.
    #[serde(default)]
    select: HtmlScraperSelectMode,
    /// Ordered extraction steps executed on the selected node.
    ///
    /// These actions transform and extract data from the selected HTML elements.
    /// Actions are executed in sequence, with each action's output feeding into the next.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<ScraperAction>,
    /// Child entries for group entries (mutually exclusive with `actions`).
    ///
    /// When present, this entry becomes a group that applies its child entries
    /// to each matched element. Must be empty if `actions` is non-empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entries: Vec<HtmlScraperEntryRaw>,
    /// Transformations run after an object entry assembled all child fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    post_build: Vec<ScraperPostBuild>,
    /// Sub-queries executed on each value produced by this entry.
    ///
    /// These sub-queries fetch additional data based on values extracted from
    /// the current entry and merge the results into the output structure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sub_queries: Vec<EntrySubQueryRaw>,
}

impl HtmlScraperEntryRaw {
    /// Resolves collection-level placeholders used by selectors in this entry tree.
    ///
    /// Recursively processes the entry and all its child entries to replace
    /// `{placeholder}` templates with actual values from the collection parameters.
    ///
    /// # Arguments
    ///
    /// * `params` - Collection-level template parameters mapping placeholder names to values.
    ///
    /// # Errors
    ///
    /// Returns an error if a required placeholder is missing from `params`.
    /// The error includes the entry name and field name that failed resolution.
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
///
/// Controls how many elements matching a CSS selector are processed.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HtmlScraperSelectMode {
    /// Uses only the first matched element.
    ///
    /// When selected, only the first element matching the CSS selector
    /// will have its actions applied.
    First,
    /// Uses all matched elements and concatenates their extracted values.
    ///
    /// When selected, all elements matching the CSS selector will have
    /// their actions applied, and the extracted values will be concatenated.
    #[default]
    All,
}

/// One logical field extracted from a result row.
///
/// An entry defines how to extract data from HTML elements using CSS selectors
/// and a pipeline of actions. Entries can be either simple fields or groups
/// containing nested entries.
#[derive(Deserialize)]
#[serde(try_from = "HtmlScraperEntryRaw")]
pub enum HtmlScraperEntry {
    /// A leaf field that extracts one or more values via actions.
    ///
    /// Field entries extract data from matched HTML elements and produce
    /// scalar values (strings) in the output structure.
    Field {
        /// Output field name used in the extracted metadata map.
        ///
        /// This name becomes a key in the output JSON structure.
        name: String,
        /// Original CSS selector template preserved from YAML.
        ///
        /// The raw selector string as defined in the configuration,
        /// used for serialization and debugging.
        selector_template: Option<String>,
        /// JSON output type requested by this field entry.
        output_type: ScraperOutputType,
        /// Compiled CSS selector used at runtime.
        ///
        /// The parsed and validated selector used for actual HTML element matching.
        selector: Option<::scraper::Selector>,
        /// Whether to use the first match or all matches.
        ///
        /// Controls the selection behavior when multiple elements match the selector.
        select: HtmlScraperSelectMode,
        /// Ordered extraction steps executed on the selected node.
        ///
        /// Actions are applied in sequence to extract and transform values
        /// from the matched HTML elements.
        actions: Vec<ScraperAction>,
        /// Sub-queries attached to this entry (recursion across scraper types).
        ///
        /// Sub-queries allow fetching additional data based on values extracted
        /// from this entry. They run after values are extracted and replace the
        /// source URL with the returned values.
        sub_queries: Vec<Box<dyn ScraperQuery>>,
    },
    /// A group entry that contains child entries applied to each matched element.
    ///
    /// Group entries create nested objects in the output structure by applying
    /// their child entries to each matched HTML element.
    Group {
        /// Group name used to build the output path.
        ///
        /// This name becomes a key in the output JSON structure.
        /// Child entry names are nested under this key.
        name: String,
        /// Original CSS selector template preserved from YAML.
        ///
        /// The raw selector string as defined in the configuration,
        /// used for serialization and debugging.
        selector_template: Option<String>,
        /// JSON output type requested by this group entry.
        output_type: ScraperOutputType,
        /// Compiled CSS selector used at runtime.
        ///
        /// The parsed and validated selector used for actual HTML element matching.
        selector: Option<::scraper::Selector>,
        /// Whether to use the first match or all matches.
        ///
        /// Controls the selection behavior when multiple elements match the selector.
        select: HtmlScraperSelectMode,
        /// Child entries applied to each matched element.
        ///
        /// Each child entry extracts data from the matched HTML elements,
        /// creating nested structures in the output.
        entries: Vec<HtmlScraperEntry>,
        /// Transformations run after all child entries built the object.
        post_build: Vec<ScraperPostBuild>,
        /// Sub-queries attached to this entry (recursion across scraper types).
        ///
        /// Sub-queries allow fetching additional data based on values extracted
        /// from this group.
        sub_queries: Vec<Box<dyn ScraperQuery>>,
    },
}

impl HtmlScraperEntry {
    /// Creates a field entry and panics if the selector is invalid.
    ///
    /// This is a convenience constructor that wraps [`try_new`] and panics on error.
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
    /// This constructor validates the CSS selector before creating the entry,
    /// allowing callers to handle errors gracefully.
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
            ScraperOutputType::String,
            HtmlScraperSelectMode::First,
            actions,
        )
    }

    /// Returns the output field name used in the final metadata map.
    ///
    /// For both field and group variants, this returns the `name` field.
    #[cfg(any(test, feature = "test-support"))]
    pub fn name(&self) -> &str {
        match self {
            HtmlScraperEntry::Field { name, .. } => name,
            HtmlScraperEntry::Group { name, .. } => name,
        }
    }

    /// Returns the flattened list of leaf field names produced by this entry.
    ///
    /// Recursively collects all leaf field names from this entry and its children,
    /// including the full hierarchical path for nested group entries.
    ///
    /// # Returns
    ///
    /// A vector of strings representing the full paths to all leaf fields,
    /// with group hierarchies separated by ` > `.
    #[cfg(any(test, feature = "test-support"))]
    pub fn field_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_field_names(None, &mut names);
        names
    }

    /// Recursively collects leaf field names into `names`, prefixing each with the
    /// group hierarchy separated by ` > `.
    ///
    /// This is a helper method for [`field_names`] that builds the hierarchical paths.
    ///
    /// # Arguments
    ///
    /// * `prefix` - Optional group path prefix prepended to each collected name.
    ///   If `None`, the entry's own name is used as the base.
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
    /// Executes the entry's extraction logic on the given HTML element and appends
    /// the extracted values to the output data structure.
    ///
    /// - For field entries: Extracts values from matched elements and appends them.
    /// - For group entries: Creates nested objects by applying child entries to each matched element.
    ///
    /// # Arguments
    ///
    /// * `root` - Output data node where extracted values are appended.
    /// * `card` - Parent HTML element to search within (a result row).
    /// * `params` - Runtime template parameters forwarded to actions.
    /// * `request_url` - Fully resolved request URL forwarded to actions.
    /// * `response_body` - Optional raw response body available to actions.
    pub fn apply_to(
        &self,
        root: &mut ScraperDataNode,
        card: ElementRef,
        params: &HashMap<String, String>,
        dynamic_variables: &Mutex<DynamicTemplateVariables>,
        request_url: &str,
        response_body: Option<&str>,
    ) -> Result<()> {
        match self {
            HtmlScraperEntry::Field {
                name,
                selector_template: _,
                output_type,
                selector,
                select,
                actions,
                ..
            } => {
                let path: Vec<&str> = name.split('>').map(|s| s.trim()).collect();
                root.set_output_type(&path, *output_type);
                Self::for_each_selected(selector, *select, card, |selected| {
                    let mut values: Vec<String> = Vec::new();
                    let selected = Some(selected);
                    for action in actions {
                        values = action.apply_with_dynamic_variables(
                            &selected,
                            values,
                            params,
                            request_url,
                            response_body,
                            None,
                            Some(dynamic_variables),
                        )?;
                    }
                    if *select == HtmlScraperSelectMode::First {
                        if let Some(value) = values.into_iter().next() {
                            root.set_value_typed(&path, value, *output_type);
                        }
                    } else {
                        for value in values {
                            root.push_value_typed(&path, value, *output_type);
                        }
                    }
                    Ok(())
                })?;
                Ok(())
            }
            HtmlScraperEntry::Group {
                name,
                selector_template: _,
                output_type,
                selector,
                select,
                entries,
                post_build,
                ..
            } => {
                let path: Vec<&str> = name.split('>').map(|s| s.trim()).collect();
                root.set_output_type(&path, *output_type);
                Self::for_each_selected(selector, *select, card, |selected| {
                    let mut item = ScraperDataNode::default();
                    for entry in entries {
                        entry.apply_to(
                            &mut item,
                            selected,
                            params,
                            dynamic_variables,
                            request_url,
                            response_body,
                        )?;
                    }
                    for transformation in post_build {
                        transformation.apply(&mut item);
                    }
                    root.push_node_typed(&path, item, *output_type);
                    Ok(())
                })?;
                Ok(())
            }
        }
    }

    /// Parses an optional CSS selector string into a compiled [`Selector`](::scraper::Selector).
    ///
    /// Compiles the CSS selector string into a validated selector object that can be
    /// used for efficient HTML element matching.
    ///
    /// # Arguments
    ///
    /// * `_name` - Entry name kept for consistent call-site signatures but unused here.
    /// * `selector` - Raw CSS selector string to compile. If `None` or empty, returns `None`.
    ///
    /// # Returns
    ///
    /// `Some(Selector)` if the input is a valid non-empty CSS selector,
    /// or `None` if the input is `None` or empty.
    ///
    /// # Errors
    ///
    /// Returns an error if `selector` is non-empty but not a valid CSS selector.
    /// The error includes the invalid selector string and the parsing error details.
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
    /// Ensures that all actions configured for this entry are valid and supported
    /// in the entry context.
    ///
    /// # Arguments
    ///
    /// * `name` - Entry name used in diagnostic messages when validation fails.
    /// * `actions` - Ordered extraction steps to validate.
    ///
    /// # Errors
    ///
    /// Returns an error if any action fails validation for the `"entry"` context.
    /// The error includes the entry name and the specific validation failure.
    fn validate_actions(name: &str, actions: &[ScraperAction]) -> Result<()> {
        for action in actions {
            action.validate(name, "entry")?;
        }

        Ok(())
    }

    /// Validates and returns the output type for one field entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the YAML entry omits `type` or uses an object
    /// type on a scalar field.
    fn require_field_output_type(
        name: &str,
        output_type: Option<ScraperOutputType>,
        has_sub_queries: bool,
    ) -> Result<ScraperOutputType> {
        let Some(output_type) = output_type else {
            anyhow::bail!("Entry {} must define type", name);
        };

        if !has_sub_queries
            && matches!(
                output_type,
                ScraperOutputType::Object | ScraperOutputType::ObjectArray
            )
        {
            anyhow::bail!(
                "Entry {} uses {} but does not define entries",
                name,
                output_type.as_str()
            );
        }

        Ok(output_type)
    }

    /// Resolves the YAML output type for one group entry into its final shape.
    ///
    /// `type: object` means an object family: `select: first` produces one
    /// object, while the default `select: all` produces an array of objects.
    /// `type: object[]` is accepted as an explicit array alias.
    ///
    /// # Errors
    ///
    /// Returns an error when `type` is missing, when a scalar type is used on a
    /// group, or when `object[]` is combined with `select: first`.
    fn resolve_group_output_type(
        name: &str,
        output_type: Option<ScraperOutputType>,
        select: HtmlScraperSelectMode,
    ) -> Result<ScraperOutputType> {
        let Some(output_type) = output_type else {
            anyhow::bail!("Group entry {} must define type", name);
        };

        match (output_type, select) {
            (ScraperOutputType::ObjectArray, HtmlScraperSelectMode::First) => {
                anyhow::bail!(
                    "Group entry {} cannot combine type object[] with select: first",
                    name
                );
            }
            (ScraperOutputType::ObjectArray, _) => Ok(ScraperOutputType::ObjectArray),
            (ScraperOutputType::Object, HtmlScraperSelectMode::First) => {
                Ok(ScraperOutputType::Object)
            }
            (ScraperOutputType::Object, _) => Ok(ScraperOutputType::ObjectArray),
            _ => anyhow::bail!(
                "Group entry {} must use type object or object[], got {}",
                name,
                output_type.as_str()
            ),
        }
    }

    /// Normalizes an optional selector template by trimming and discarding empty strings.
    ///
    /// Cleans up the selector template string by removing leading/trailing whitespace
    /// and returning `None` if the result is empty.
    ///
    /// # Arguments
    ///
    /// * `selector` - Raw selector template string to normalize.
    ///
    /// # Returns
    ///
    /// `Some(String)` with the trimmed selector, or `None` if the input is `None` or empty.
    fn normalize_selector_template(selector: Option<&str>) -> Option<String> {
        selector
            .filter(|selector| !selector.trim().is_empty())
            .map(str::to_string)
    }

    /// Produces the CSS selector string used during serialization, preferring
    /// the original template when available and falling back to the compiled selector.
    ///
    /// When serializing an entry back to its YAML representation, this method
    /// determines which selector string to use. The original template is preferred
    /// to preserve the exact formatting from the configuration file.
    ///
    /// # Arguments
    ///
    /// * `selector_template` - Original template string preserved from YAML.
    ///   If present, this value is returned.
    /// * `selector` - Compiled CSS selector used at runtime.
    ///   Used as fallback if `selector_template` is `None`.
    ///
    /// # Returns
    ///
    /// The selector template if available, otherwise the CSS string representation
    /// of the compiled selector, or `None` if both are `None`.
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
    /// Creates a field entry that extracts values from HTML elements using the
    /// provided selector and action pipeline.
    ///
    /// # Arguments
    ///
    /// * `name` - Output field name used in the extracted metadata map.
    /// * `selector_template` - Original template string kept for serialization.
    ///   This is the raw selector as it appeared in the YAML configuration.
    /// * `resolved_selector` - Resolved CSS selector compiled at construction time.
    ///   This is the selector after any template placeholders have been resolved.
    /// * `select` - Whether to use the first match or all matches.
    /// * `actions` - Ordered extraction steps executed on the selected node.
    ///
    /// # Errors
    ///
    /// Returns an error if `resolved_selector` is not a valid CSS selector or if
    /// any action fails validation. Validation errors include the entry name.
    fn try_new_field_with_optional_selector(
        name: &str,
        selector_template: Option<&str>,
        resolved_selector: Option<&str>,
        output_type: ScraperOutputType,
        select: HtmlScraperSelectMode,
        actions: &[ScraperAction],
    ) -> Result<Self> {
        Self::validate_actions(name, actions)?;

        Ok(HtmlScraperEntry::Field {
            name: name.to_string(),
            selector_template: Self::normalize_selector_template(selector_template),
            output_type,
            selector: Self::parse_selector(name, resolved_selector)?,
            select,
            actions: actions.to_vec(),
            sub_queries: Vec::new(),
        })
    }

    /// Builds a [`HtmlScraperEntry::Group`] variant with an optional CSS selector.
    ///
    /// Creates a group entry that applies child entries to each matched HTML element,
    /// producing nested objects in the output structure.
    ///
    /// # Arguments
    ///
    /// * `name` - Group name used to build the output path.
    /// * `selector_template` - Original template string kept for serialization.
    ///   This is the raw selector as it appeared in the YAML configuration.
    /// * `resolved_selector` - Resolved CSS selector compiled at construction time.
    ///   This is the selector after any template placeholders have been resolved.
    /// * `select` - Whether to use the first match or all matches.
    /// * `entries` - Child entries applied to each matched element.
    ///
    /// # Errors
    ///
    /// Returns an error if `resolved_selector` is not a valid CSS selector.
    /// The error includes the invalid selector string and parsing error details.
    fn try_new_group_with_optional_selector(
        name: &str,
        selector_template: Option<&str>,
        resolved_selector: Option<&str>,
        output_type: ScraperOutputType,
        select: HtmlScraperSelectMode,
        entries: Vec<HtmlScraperEntry>,
        post_build: Vec<ScraperPostBuild>,
    ) -> Result<Self> {
        Ok(HtmlScraperEntry::Group {
            name: name.to_string(),
            selector_template: Self::normalize_selector_template(selector_template),
            output_type,
            selector: Self::parse_selector(name, resolved_selector)?,
            select,
            entries,
            post_build,
            sub_queries: Vec::new(),
        })
    }

    /// Iterates over the CSS selector matches on `card` and invokes `callback` for each.
    ///
    /// Applies the selector to the card element and invokes the callback for each match.
    /// When no selector is provided, the callback is invoked once on the card itself.
    ///
    /// # Arguments
    ///
    /// * `selector` - Compiled CSS selector applied to the card element.
    ///   If `None`, the callback is invoked on the card itself.
    /// * `select` - Whether to use the first match or all matches.
    ///   - [`HtmlScraperSelectMode::First`]: Only the first match is passed to the callback.
    ///   - [`HtmlScraperSelectMode::All`]: All matches are passed to the callback.
    /// * `card` - Root HTML element to search within.
    /// * `callback` - Function invoked for each selected element.
    ///   Receives an [`ElementRef`] for each matched element.
    fn for_each_selected<F>(
        selector: &Option<::scraper::Selector>,
        select: HtmlScraperSelectMode,
        card: ElementRef,
        mut callback: F,
    ) -> Result<()>
    where
        F: FnMut(ElementRef) -> Result<()>,
    {
        match (selector, select) {
            (Some(selector), HtmlScraperSelectMode::All) => {
                for selected in card.select(selector) {
                    callback(selected)?;
                }
            }
            (Some(selector), HtmlScraperSelectMode::First) => {
                if let Some(selected) = card.select(selector).next() {
                    callback(selected)?;
                }
            }
            (None, _) => callback(card)?,
        }
        Ok(())
    }
}

impl HtmlScraperEntry {
    /// Builds a runtime entry from raw config, converting sub-queries if any.
    ///
    /// Converts a raw YAML entry definition into a validated runtime entry.
    /// Handles the conversion of sub-queries from raw to runtime types.
    ///
    /// # Arguments
    ///
    /// * `config` - Raw YAML entry definition to convert.
    /// * `base_url` - Base URL used to create HTTP clients for sub-queries.
    ///   This is passed to sub-query constructors for building their HTTP clients.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The entry defines both `actions` and `entries` (mutually exclusive)
    /// - The entry defines neither `actions` nor `entries` (at least one required)
    /// - Any CSS selector is invalid
    /// - Any sub-query fails to convert
    fn from_raw_with_base_url(config: HtmlScraperEntryRaw, base_url: &str) -> Result<Self> {
        let HtmlScraperEntryRaw {
            name,
            output_type,
            selector,
            resolved_selector,
            select,
            actions,
            entries,
            post_build,
            sub_queries,
        } = config;

        let converted_sub_queries: Result<Vec<Box<dyn ScraperQuery>>> = sub_queries
            .into_iter()
            .map(|sq| sq.into_boxed_query(base_url))
            .collect();
        let sub_queries = converted_sub_queries?;

        match (actions.is_empty(), entries.is_empty()) {
            (false, true) => {
                if !post_build.is_empty() {
                    anyhow::bail!("Field entry {} cannot define post_build", name);
                }
                let output_type = HtmlScraperEntry::require_field_output_type(
                    &name,
                    output_type,
                    !sub_queries.is_empty(),
                )?;
                let mut entry = HtmlScraperEntry::try_new_field_with_optional_selector(
                    &name,
                    selector.as_deref(),
                    resolved_selector.as_deref().or(selector.as_deref()),
                    output_type,
                    select,
                    &actions,
                )?;
                if let HtmlScraperEntry::Field {
                    sub_queries: ref mut sq,
                    ..
                } = entry
                {
                    *sq = sub_queries;
                }
                Ok(entry)
            }
            (true, false) => {
                let output_type =
                    HtmlScraperEntry::resolve_group_output_type(&name, output_type, select)?;
                let mut entry = {
                    let entries = entries
                        .into_iter()
                        .map(|e| HtmlScraperEntry::from_raw_with_base_url(e, base_url))
                        .collect::<Result<Vec<_>>>()?;
                    for transformation in &post_build {
                        transformation.validate(&name)?;
                    }
                    HtmlScraperEntry::try_new_group_with_optional_selector(
                        &name,
                        selector.as_deref(),
                        resolved_selector.as_deref().or(selector.as_deref()),
                        output_type,
                        select,
                        entries,
                        post_build,
                    )?
                };
                if let HtmlScraperEntry::Group {
                    sub_queries: ref mut sq,
                    ..
                } = entry
                {
                    *sq = sub_queries;
                }
                Ok(entry)
            }
            (true, true) if !sub_queries.is_empty() => {
                if !post_build.is_empty() {
                    anyhow::bail!("Field entry {} cannot define post_build", name);
                }
                let output_type = HtmlScraperEntry::require_field_output_type(
                    &name,
                    output_type,
                    !sub_queries.is_empty(),
                )?;
                // Entry with only sub_queries and no actions/entries: it's a passthrough
                Ok(HtmlScraperEntry::Field {
                    name: name.clone(),
                    selector_template: Self::normalize_selector_template(selector.as_deref()),
                    output_type,
                    selector: Self::parse_selector(
                        &name,
                        resolved_selector.as_deref().or(selector.as_deref()),
                    )?,
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
    /// Delegates to [`from_raw_with_base_url`] with an empty string as the base URL.
    ///
    /// # Arguments
    ///
    /// * `config` - Raw YAML entry definition to convert.
    ///
    /// # Errors
    ///
    /// Returns an error when the entry defines both `actions` and `entries`
    /// (mutually exclusive), or when it defines neither (at least one required).
    /// Also returns errors for invalid CSS selectors or sub-query conversion failures.
    fn try_from(config: HtmlScraperEntryRaw) -> Result<Self> {
        Self::from_raw_with_base_url(config, "")
    }
}

impl ScraperEntrySpec for HtmlScraperEntry {
    /// Returns a reference to this entry as a trait object.
    ///
    /// # Returns
    ///
    /// A reference to this entry as `&dyn Any`, allowing downcasting.
    fn as_any(&self) -> &dyn Any {
        self
    }

    /// Returns a mutable reference to this entry as a trait object.
    ///
    /// # Returns
    ///
    /// A mutable reference to this entry as `&mut dyn Any`, allowing downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Returns the entry name.
    ///
    /// # Returns
    ///
    /// The name field from either the Field or Group variant.
    fn name(&self) -> &str {
        match self {
            HtmlScraperEntry::Field { name, .. } => name,
            HtmlScraperEntry::Group { name, .. } => name,
        }
    }

    /// Returns the scraper type for this entry.
    ///
    /// # Returns
    ///
    /// Always returns [`ScraperType::Html`] for HTML scraper entries.
    fn entry_type(&self) -> ScraperType {
        ScraperType::Html
    }

    /// Returns the JSON pointer for this entry.
    ///
    /// # Returns
    ///
    /// Always returns `None` for HTML scraper entries, as they use CSS selectors
    /// rather than JSON pointers.
    fn pointer(&self) -> Option<&str> {
        None
    }

    /// Returns the CSS selector for this entry.
    ///
    /// # Returns
    ///
    /// Always returns `None` for HTML scraper entries. Use [`name()`] for the entry name
    /// or access the `selector_template` field directly for the CSS selector string.
    fn selector(&self) -> Option<&str> {
        None
    }

    /// Returns the selection mode for this entry.
    ///
    /// # Returns
    ///
    /// The `select` field from either the Field or Group variant,
    /// controlling whether to use the first match or all matches.
    fn select(&self) -> HtmlScraperSelectMode {
        match self {
            HtmlScraperEntry::Field { select, .. } => *select,
            HtmlScraperEntry::Group { select, .. } => *select,
        }
    }

    /// Returns the actions configured for this entry.
    ///
    /// # Returns
    ///
    /// For Field entries, returns the configured actions.
    /// For Group entries, always returns an empty slice.
    fn actions(&self) -> &[ScraperAction] {
        match self {
            HtmlScraperEntry::Field { actions, .. } => actions,
            HtmlScraperEntry::Group { .. } => &[],
        }
    }

    /// Returns the child entries for this entry.
    ///
    /// # Returns
    ///
    /// For Field entries, always returns an empty vector.
    /// For Group entries, returns references to all child entries.
    fn sub_entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        match self {
            HtmlScraperEntry::Field { .. } => Vec::new(),
            HtmlScraperEntry::Group { entries, .. } => {
                entries.iter().map(|e| e as &dyn ScraperEntrySpec).collect()
            }
        }
    }

    /// Returns whether this entry is a group.
    ///
    /// # Returns
    ///
    /// `true` if this is a [`HtmlScraperEntry::Group`] variant, `false` otherwise.
    fn is_group(&self) -> bool {
        matches!(self, HtmlScraperEntry::Group { .. })
    }

    /// Returns the sub-queries attached to this entry.
    ///
    /// # Returns
    ///
    /// A vector of references to all sub-queries attached to this entry.
    /// For both Field and Group variants, returns the configured sub-queries.
    fn sub_queries(&self) -> Vec<&dyn crate::scrapyfy::scraper::query_trait::ScraperQuery> {
        match self {
            HtmlScraperEntry::Field { sub_queries, .. } => sub_queries
                .iter()
                .map(|b| &**b as &dyn ScraperQuery)
                .collect(),
            HtmlScraperEntry::Group { sub_queries, .. } => sub_queries
                .iter()
                .map(|b| &**b as &dyn ScraperQuery)
                .collect(),
        }
    }
}

impl From<&HtmlScraperEntry> for HtmlScraperEntryRaw {
    /// Converts a runtime entry back into its raw YAML-compatible representation.
    ///
    /// Serializes the validated runtime entry back to a form that can be
    /// serialized to YAML. This preserves the original structure but loses
    /// runtime-only information like compiled selectors.
    ///
    /// # Arguments
    ///
    /// * `entry` - Runtime entry to convert.
    ///
    /// # Returns
    ///
    /// A raw entry struct suitable for YAML serialization.
    fn from(entry: &HtmlScraperEntry) -> Self {
        match entry {
            HtmlScraperEntry::Field {
                name,
                selector_template,
                output_type,
                selector,
                select,
                actions,
                ..
            } => Self {
                name: name.clone(),
                output_type: Some(*output_type),
                selector: HtmlScraperEntry::serialize_selector(selector_template, selector),
                resolved_selector: None,
                select: *select,
                actions: actions.clone(),
                entries: Vec::new(),
                post_build: Vec::new(),
                sub_queries: Vec::new(),
            },
            HtmlScraperEntry::Group {
                name,
                selector_template,
                output_type,
                selector,
                select,
                entries,
                post_build,
                ..
            } => Self {
                name: name.clone(),
                output_type: Some(*output_type),
                selector: HtmlScraperEntry::serialize_selector(selector_template, selector),
                resolved_selector: None,
                select: *select,
                actions: Vec::new(),
                entries: entries.iter().map(HtmlScraperEntryRaw::from).collect(),
                post_build: post_build.clone(),
                sub_queries: Vec::new(),
            },
        }
    }
}

impl Serialize for HtmlScraperEntry {
    /// Serializes the entry through its raw YAML representation.
    ///
    /// Converts the runtime entry to its raw form and then serializes it,
    /// ensuring the output matches the expected YAML configuration format.
    ///
    /// # Arguments
    ///
    /// * `serializer` - The serde serializer to use.
    ///
    /// # Returns
    ///
    /// The serialized representation of the entry as YAML-compatible data.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        HtmlScraperEntryRaw::from(self).serialize(serializer)
    }
}
