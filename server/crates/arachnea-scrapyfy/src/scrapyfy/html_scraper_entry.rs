use anyhow::Result;
use scraper::{selector::ToCss, ElementRef, Selector};
use serde::{Deserialize, Serialize, Serializer};

use super::*;
use crate::scrapyfy::query_helpers;

/// Raw configuration definition of one field or grouped field extracted from each result row.
#[derive(Serialize, Deserialize)]
pub struct HtmlScraperEntryRaw {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector: Option<String>,
    #[serde(skip)]
    resolved_selector: Option<String>,
    #[serde(default)]
    select: HtmlScraperSelectMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<ScraperAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entries: Vec<HtmlScraperEntryRaw>,
}

impl HtmlScraperEntryRaw {
    /// Resolves collection-level placeholders used by selectors in this entry tree.
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
    #[default]
    First,
    /// Uses all matched elements and concatenates their extracted values.
    All,
}

/// One logical field extracted from a result row.
#[derive(Deserialize)]
#[serde(try_from = "HtmlScraperEntryRaw")]
pub enum HtmlScraperEntry {
    Field {
        name: String,
        selector_template: Option<String>,
        selector: Option<scraper::Selector>,
        select: HtmlScraperSelectMode,
        actions: Vec<ScraperAction>,
    },
    Group {
        name: String,
        selector_template: Option<String>,
        selector: Option<scraper::Selector>,
        select: HtmlScraperSelectMode,
        entries: Vec<HtmlScraperEntry>,
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

    fn parse_selector(_name: &str, selector: Option<&str>) -> Result<Option<scraper::Selector>> {
        let selector = match selector {
            Some(selector) if !selector.trim().is_empty() => Some(
                Selector::parse(selector)
                    .map_err(|err| anyhow::anyhow!("Invalid selector {}: {:?}", selector, err))?,
            ),
            _ => None,
        };

        Ok(selector)
    }

    fn validate_actions(name: &str, actions: &[ScraperAction]) -> Result<()> {
        for action in actions {
            action.validate(name, "entry")?;
        }

        Ok(())
    }

    fn normalize_selector_template(selector: Option<&str>) -> Option<String> {
        selector
            .filter(|selector| !selector.trim().is_empty())
            .map(str::to_string)
    }

    fn serialize_selector(
        selector_template: &Option<String>,
        selector: &Option<scraper::Selector>,
    ) -> Option<String> {
        selector_template
            .clone()
            .or_else(|| selector.as_ref().map(Selector::to_css_string))
    }

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
        })
    }

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
        })
    }

    fn for_each_selected<F>(
        selector: &Option<scraper::Selector>,
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

impl TryFrom<HtmlScraperEntryRaw> for HtmlScraperEntry {
    type Error = anyhow::Error;

    fn try_from(config: HtmlScraperEntryRaw) -> Result<Self> {
        let HtmlScraperEntryRaw {
            name,
            selector,
            resolved_selector,
            select,
            actions,
            entries,
        } = config;

        match (actions.is_empty(), entries.is_empty()) {
            (false, true) => HtmlScraperEntry::try_new_field_with_optional_selector(
                &name,
                selector.as_deref(),
                resolved_selector.as_deref().or(selector.as_deref()),
                select,
                &actions,
            ),
            (true, false) => {
                let entries = entries
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<Vec<_>>>()?;
                HtmlScraperEntry::try_new_group_with_optional_selector(
                    &name,
                    selector.as_deref(),
                    resolved_selector.as_deref().or(selector.as_deref()),
                    select,
                    entries,
                )
            }
            (true, true) => anyhow::bail!("Entry {} must define either actions or entries", name),
            (false, false) => {
                anyhow::bail!("Entry {} cannot define both actions and entries", name)
            }
        }
    }
}

impl From<&HtmlScraperEntry> for HtmlScraperEntryRaw {
    fn from(entry: &HtmlScraperEntry) -> Self {
        match entry {
            HtmlScraperEntry::Field {
                name,
                selector_template,
                selector,
                select,
                actions,
            } => Self {
                name: name.clone(),
                selector: HtmlScraperEntry::serialize_selector(selector_template, selector),
                resolved_selector: None,
                select: *select,
                actions: actions.clone(),
                entries: Vec::new(),
            },
            HtmlScraperEntry::Group {
                name,
                selector_template,
                selector,
                select,
                entries,
            } => Self {
                name: name.clone(),
                selector: HtmlScraperEntry::serialize_selector(selector_template, selector),
                resolved_selector: None,
                select: *select,
                actions: Vec::new(),
                entries: entries.iter().map(HtmlScraperEntryRaw::from).collect(),
            },
        }
    }
}

impl Serialize for HtmlScraperEntry {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        HtmlScraperEntryRaw::from(self).serialize(serializer)
    }
}
