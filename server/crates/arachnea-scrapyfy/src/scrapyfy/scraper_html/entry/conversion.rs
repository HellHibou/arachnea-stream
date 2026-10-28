//! Raw/runtime conversions preserving the serialized contract.

use super::*;

impl TryFrom<HtmlScraperEntryRaw> for HtmlScraperEntry {
    type Error = anyhow::Error;

    /// Converts a raw YAML entry definition into a validated runtime entry.
    ///
    /// Delegates to `HtmlScraperEntry::from_raw_with_base_url` with an empty string as the base URL.
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
