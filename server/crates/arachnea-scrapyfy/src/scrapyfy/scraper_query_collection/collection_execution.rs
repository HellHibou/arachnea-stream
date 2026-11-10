//! Collection query lookup and execution context preparation.

use super::*;

impl ScraperQueryCollection {
    /// Measures the serialized root POST body without sending a request.
    ///
    /// # Arguments
    ///
    /// * `query_name` - Query using the same body actions as execution.
    /// * `params` - Runtime parameters merged over collection defaults.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown query or an unmeasurable request.
    /// Only the byte count is returned; credentials and payloads stay internal.
    pub fn measure_query_body(
        &self,
        query_name: &str,
        params: &HashMap<String, String>,
    ) -> Result<u64> {
        let mut params = self.build_execution_params(params);
        params
            .entry("source".into())
            .or_insert_with(|| self.name.clone());
        let definition = self
            .queries
            .get(query_name)
            .context("unknown query for body measurement")?;
        let (query, mappings): (&dyn ScraperQuery, _) = match definition {
            ScraperQueryDefinition::Json(query) => (query, &query.query_param_mappings),
            ScraperQueryDefinition::Html(query) => (query, &query.query_param_mappings),
            ScraperQueryDefinition::Text(query) => (query, &query.query_param_mappings),
            ScraperQueryDefinition::Static(_) => anyhow::bail!("static queries have no HTTP body"),
        };
        let params = crate::scrapyfy::query_helpers::build_query_execution_params(
            query.base_url(),
            &params,
            mappings,
        );
        crate::scrapyfy::scraper::query_executor::measure_root_body(query, &params)
    }

    /// Executes one named query within this collection.
    ///
    /// # Arguments
    ///
    /// * `query_name` - Name of the query to execute.
    /// * `params` - Runtime values used to format the query URL template. Collection-level
    ///   default parameters are merged first, and runtime values override them.
    /// * `query_media_type_filter` - Filter on media_type query.
    /// * `fields_filters` - Root fields filter list or None.
    ///
    /// The returned entries are tree-shaped when fields include `>` in their names.
    ///
    /// # Errors
    ///
    /// Returns an error if `query_name` is unknown or if query execution fails.
    pub async fn execute_query(
        &self,
        query_name: &str,
        params: &HashMap<String, String>,
        query_media_type_filter: Option<&Vec<String>>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        self.execute_query_with_validation(
            query_name,
            params,
            query_media_type_filter,
            fields_filters,
            None,
            self.yaml_hash(),
        )
        .await
        .map(|(rows, _)| rows)
    }

    /// Executes one named query within this collection with optional
    /// conditional validation of the source's root request.
    ///
    /// # Arguments
    ///
    /// * `query_name` - Name of the query to execute.
    /// * `params` - Runtime values used to format the query URL template.
    /// * `query_media_type_filter` - Filter on media_type query.
    /// * `fields_filters` - Root fields filter list or None.
    /// * `client_fragment` - Client-provided validator fragment for this source.
    /// * `yaml_hash` - Hash of the owning YAML configuration included in the source validator identity.
    ///
    /// # Returns
    ///
    /// The extracted rows alongside the root-fetch outcome, when the query
    /// performed an HTTP fetch.
    ///
    /// # Errors
    ///
    /// Returns an error if `query_name` is unknown or if query execution fails.
    pub async fn execute_query_with_validation(
        &self,
        query_name: &str,
        params: &HashMap<String, String>,
        query_media_type_filter: Option<&Vec<String>>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
        client_fragment: Option<&str>,
        yaml_hash: &str,
    ) -> Result<(
        Vec<HashMap<String, ScraperDataNode>>,
        Option<crate::scrapyfy::RootFetchOutcome>,
    )> {
        let mut merged_params = self.build_execution_params(params);
        merged_params
            .entry("source".to_string())
            .or_insert_with(|| self.name.clone());

        match self.queries.get(query_name) {
            Some(value) => {
                if query_media_type_filter.is_none()
                    || value.is_media_type(query_media_type_filter.unwrap())
                {
                    value
                        .execute_query_with_validation(
                            &merged_params,
                            fields_filters,
                            client_fragment,
                            yaml_hash,
                        )
                        .await
                } else {
                    Ok((Vec::new(), None))
                }
            }
            _none => Ok((Vec::new(), None)),
        }
    }
}
