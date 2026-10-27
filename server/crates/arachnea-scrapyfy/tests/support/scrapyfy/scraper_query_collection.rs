use super::*;

impl ScraperQueryCollection {
    /// Returns one query definition by name for test assertions.
    ///
    /// # Arguments
    ///
    /// * `query_name` - Name of the query to look up.
    pub fn get_query(&self, query_name: &str) -> Option<&ScraperQueryDefinition> {
        self.queries.get(query_name)
    }
}
impl ScraperQueryDefinition {
    /// Returns the flattened list of leaf field names produced by this query.
    pub fn get_field_names(&self) -> Vec<String> {
        match self {
            ScraperQueryDefinition::Html(query) => query.get_field_names(),
            ScraperQueryDefinition::Json(query) => query.get_field_names(),
            ScraperQueryDefinition::Static(query) => query.get_field_names(),
            ScraperQueryDefinition::Text(query) => query.get_field_names(),
        }
    }
}
