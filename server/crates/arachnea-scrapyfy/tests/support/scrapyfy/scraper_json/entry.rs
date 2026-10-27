use super::*;

impl JsonScraperEntry {
    /// Returns the flattened list of leaf field names produced by this entry.
    ///
    /// Collects all field names from this entry and its nested sub-entries.
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
    fn name(&self) -> &str {
        match self {
            JsonScraperEntry::Field { name, .. } => name,
            JsonScraperEntry::Group { name, .. } => name,
        }
    }
}
