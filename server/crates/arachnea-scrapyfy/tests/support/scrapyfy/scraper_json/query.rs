use super::*;

impl JsonScraperQuery {
    /// Returns the flattened list of leaf field names produced by this query.
    ///
    /// Only the [`JsonScraperSubQuery`] children contribute to the list; nested
    /// sub-queries of other concrete types (e.g. HTML) are silently skipped.
    pub fn get_field_names(&self) -> Vec<String> {
        let mut names = self
            .scraper_entries()
            .iter()
            .flat_map(JsonScraperEntry::field_names)
            .collect::<Vec<_>>();

        for sub_query in self.sub_queries() {
            if let Some(json_sub) = sub_query.as_any().downcast_ref::<JsonScraperSubQuery>() {
                names.extend(json_sub.field_names());
            }
        }

        names
    }
}
impl JsonScraperSubQuery {
    /// Returns the flattened list of leaf field names produced by this sub-query tree.
    ///
    /// Only the [`JsonScraperSubQuery`] children contribute to the list; nested
    /// sub-queries of other concrete types (e.g. HTML) are silently skipped
    /// because they are not part of the JSON sub-query field-name contract.
    pub fn field_names(&self) -> Vec<String> {
        let mut names = self
            .entries
            .iter()
            .flat_map(JsonScraperEntry::field_names)
            .collect::<Vec<_>>();

        names.extend(
            self.context_entries
                .iter()
                .flat_map(JsonScraperEntry::field_names),
        );

        for sub_query in &self.sub_queries {
            if let Some(json_sub) = sub_query.as_any().downcast_ref::<JsonScraperSubQuery>() {
                names.extend(json_sub.field_names());
            }
        }

        names
    }
}
