use super::*;

impl HtmlScraperQuery {
    /// Returns the flattened list of leaf field names produced by this query.
    ///
    /// Recursively collects all leaf field names from all configured entries,
    /// including nested group entries. This is useful for understanding the
    /// output structure of the query.
    ///
    /// # Returns
    ///
    /// A vector of strings representing all leaf field paths in the output,
    /// with group hierarchies separated by ` > `.
    pub fn get_field_names(&self) -> Vec<String> {
        self.scraper_entries()
            .iter()
            .flat_map(HtmlScraperEntry::field_names)
            .collect()
    }
}
