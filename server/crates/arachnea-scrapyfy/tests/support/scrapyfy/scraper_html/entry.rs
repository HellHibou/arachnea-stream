use super::*;

impl HtmlScraperEntry {
    /// Returns the output field name used in the final metadata map.
    ///
    /// For both field and group variants, this returns the `name` field.
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
}
