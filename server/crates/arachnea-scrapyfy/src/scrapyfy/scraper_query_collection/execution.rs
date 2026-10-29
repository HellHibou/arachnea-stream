//! Polymorphic execution of concrete query definitions.

use super::*;

impl ScraperQueryDefinition {
    /// Executes the query and returns the extracted rows.
    ///
    /// All query types (HTML, JSON, Static) are dispatched through the
    /// unified polymorphic executor `scraper::query_executor::execute_query_items`.
    /// When `result_item_field` is set, the returned rows are flattened by
    /// promoting the group's items to top-level entries.
    ///
    /// # Arguments
    ///
    /// * `params` - Runtime parameters available to query templates and action pipelines.
    /// * `fields_filters` - Field filters applied to produced rows and included in their cache identity.
    pub async fn execute_query(
        &self,
        params: &HashMap<String, String>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        // Apply query_param_mappings before creating the context
        let (query_ref, result_item_field, execution_params) = match self {
            ScraperQueryDefinition::Html(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
            ScraperQueryDefinition::Json(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
            ScraperQueryDefinition::Static(query) => {
                // Static queries don't have query_param_mappings, just use params as-is
                // but still resolve nested templates
                let execution_params =
                    crate::scrapyfy::query_helpers::resolve_nested_template_params(params);
                (
                    query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
            ScraperQueryDefinition::Text(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
        };

        let dynamic_template_variables =
            std::sync::Mutex::new(crate::scrapyfy::query_helpers::DynamicTemplateVariables::new());
        let mut execution_http_config = query_ref.http_config().clone();
        execution_http_config.resolve_collection_params(
            "runtime query",
            query_ref.name(),
            &execution_params,
        )?;
        let execution_http_client = query_ref.http_client().configured(execution_http_config);
        let context = crate::scrapyfy::scraper::query_executor::QueryContext {
            params: &execution_params,
            dynamic_template_variables: &dynamic_template_variables,
            request_url: "",
            response_body: None,
            http_client: &execution_http_client,
            fields_filters,
            parent_response: None,
            validation: None,
        };

        let rows =
            crate::scrapyfy::scraper::query_executor::execute_query_items(query_ref, &context)
                .await?;

        Ok(match result_item_field {
            Some(result_item_field) => flatten_result_item_field(rows, result_item_field),
            None => rows,
        })
    }

    /// Executes the query with optional conditional validation of its root request.
    ///
    /// When `client_fragment` is provided, conditional headers (`If-None-Match`
    /// / `If-Modified-Since`) are applied to the source's first request and a
    /// `304` or identical content hash skips parsing entirely.
    ///
    /// # Arguments
    ///
    /// * `params` - Runtime values used to format the query URL template.
    /// * `fields_filters` - Root fields filter list or None.
    /// * `client_fragment` - Client-provided validator fragment for this source.
    /// * `yaml_hash` - Base62 hash of the source YAML document, embedded in
    ///   every outgoing validator fragment.
    ///
    /// # Returns
    ///
    /// The extracted rows alongside the root-fetch outcome, when the query
    /// performed an HTTP fetch.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution fails.
    pub async fn execute_query_with_validation(
        &self,
        params: &HashMap<String, String>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
        client_fragment: Option<&str>,
        yaml_hash: &str,
    ) -> Result<(
        Vec<HashMap<String, ScraperDataNode>>,
        Option<crate::scrapyfy::RootFetchOutcome>,
    )> {
        // Apply query_param_mappings before creating the context
        let (query_ref, result_item_field, execution_params) = match self {
            ScraperQueryDefinition::Html(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
            ScraperQueryDefinition::Json(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
            ScraperQueryDefinition::Static(query) => {
                let execution_params =
                    crate::scrapyfy::query_helpers::resolve_nested_template_params(params);
                (
                    query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
            ScraperQueryDefinition::Text(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
        };

        let dynamic_template_variables =
            std::sync::Mutex::new(crate::scrapyfy::query_helpers::DynamicTemplateVariables::new());
        let mut execution_http_config = query_ref.http_config().clone();
        execution_http_config.resolve_collection_params(
            "runtime query",
            query_ref.name(),
            &execution_params,
        )?;
        let execution_http_client = query_ref.http_client().configured(execution_http_config);
        let validation_slot = crate::scrapyfy::ValidationSlot::new(
            client_fragment.map(str::to_string),
            yaml_hash.to_string(),
        );
        let context = crate::scrapyfy::scraper::query_executor::QueryContext {
            params: &execution_params,
            dynamic_template_variables: &dynamic_template_variables,
            request_url: "",
            response_body: None,
            http_client: &execution_http_client,
            fields_filters,
            parent_response: None,
            validation: Some(&validation_slot),
        };

        let rows =
            crate::scrapyfy::scraper::query_executor::execute_query_items(query_ref, &context)
                .await?;
        let outcome = validation_slot.take();

        let rows = match result_item_field {
            Some(result_item_field) => flatten_result_item_field(rows, result_item_field),
            None => rows,
        };
        Ok((rows, outcome))
    }
}
