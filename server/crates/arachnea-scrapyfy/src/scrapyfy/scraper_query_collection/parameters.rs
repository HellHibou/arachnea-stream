//! Collection parameter resolution and pagination metadata.

use super::*;

impl ScraperQueryCollection {
    /// Process all collection parameters sequentially in YAML declaration order.
    ///
    /// For each parameter:
    /// 1. Resolve `{placeholder}` references using already-resolved parameters.
    /// 2. Apply its actions (e.g. map) to transform the resolved value.
    ///
    /// This ensures that a parameter defined later in the YAML file can reference
    /// an earlier parameter's final (action-processed) value.
    ///
    /// # Arguments
    ///
    /// * `parameters` - Collection-level parameters to validate and resolve.
    ///
    /// # Errors
    ///
    /// Returns an error if a parameter name is empty or if duplicate names exist.
    pub(super) fn build_parameter_defaults(
        parameters: &[ScraperQueryCollectionParameter],
    ) -> Result<HashMap<String, String>> {
        let mut resolved = HashMap::new();

        for parameter in parameters {
            let (parameter_name, mut current_value) = parameter.validate_and_normalize()?;

            // 1. Resolve {placeholders} using already-processed parameters
            let (resolved_value, _) = replace_template_placeholders(&current_value, &resolved);
            current_value = resolved_value;

            // 2. Apply actions in order on the resolved value
            let mut values = vec![current_value];
            for action in &parameter.actions {
                values = action.apply(&None, values, &resolved, "", None, None);
            }

            // Take the first value after all actions
            let final_value = values.into_iter().next().unwrap_or_default();

            if resolved
                .insert(parameter_name.clone(), final_value)
                .is_some()
            {
                anyhow::bail!("Duplicate collection parameter: {}", parameter_name);
            }
        }

        Ok(resolved)
    }

    /// Builds service metadata defaults from the collection identity and description.
    ///
    /// # Arguments
    ///
    /// * `id` - Source identifier.
    /// * `title` - Human-readable source title.
    /// * `logo` - Logo URL or path.
    /// * `description` - Multi-language description map serialized as JSON.
    pub(super) fn build_service_metadata_defaults(
        id: &str,
        title: &str,
        logo: &str,
        description: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let mut defaults = HashMap::new();
        defaults.insert("service_id".to_string(), id.to_string());
        defaults.insert("service_title".to_string(), title.to_string());
        defaults.insert("service_logo".to_string(), logo.to_string());
        defaults.insert(
            "service_description".to_string(),
            serde_json::to_string(description)?,
        );
        Ok(defaults)
    }

    /// Merges collection parameter defaults with runtime parameters, then
    /// enriches the result with pagination and query separator helpers.
    ///
    /// Parameter defaults are already fully resolved and have had their actions
    /// applied during collection loading. Runtime parameters may still contain
    /// `{placeholder}` references that need resolution.
    ///
    /// # Arguments
    ///
    /// * `runtime_params` - Caller-provided parameters that override defaults.
    pub(super) fn build_execution_params(
        &self,
        runtime_params: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        let mut merged_params = self.parameter_defaults.clone();

        for (key, value) in runtime_params {
            merged_params.insert(key.clone(), value.clone());
        }

        Self::enrich_pagination_params(&mut merged_params);
        Self::enrich_query_separator_params(&mut merged_params);

        crate::scrapyfy::query_helpers::resolve_nested_template_params(&merged_params)
    }

    /// Derives `page_index` and `offset` from `page` and `page_size` parameters.
    ///
    /// # Arguments
    ///
    /// * `params` - Mutable parameter map enriched in place.
    pub(super) fn enrich_pagination_params(params: &mut HashMap<String, String>) {
        let page = params
            .get("page")
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0);
        let page_size = params
            .get("page_size")
            .or_else(|| params.get("pageSize"))
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0);

        if let Some(page) = page {
            params.insert("page".to_string(), page.to_string());
            params
                .entry("page_index".to_string())
                .or_insert_with(|| (page - 1).to_string());

            if let Some(page_size) = page_size {
                params
                    .entry("page_size".to_string())
                    .or_insert_with(|| page_size.to_string());
                params
                    .entry("offset".to_string())
                    .or_insert_with(|| ((page - 1) * page_size).to_string());
            }
        }
    }

    /// Inserts a `query_separator` parameter (`?`, `&`, or empty) derived from
    /// the `query_url` or `link` parameter.
    ///
    /// # Arguments
    ///
    /// * `params` - Mutable parameter map enriched in place.
    pub(super) fn enrich_query_separator_params(params: &mut HashMap<String, String>) {
        let Some(query_url) = params
            .get("query_url")
            .or_else(|| params.get("link"))
            .map(String::as_str)
        else {
            return;
        };

        let separator = query_parameter_separator(query_url);
        params
            .entry("query_separator".to_string())
            .or_insert_with(|| separator.to_string());
    }
}

/// Returns the URL query-parameter separator needed to append further parameters.
///
/// # Arguments
///
/// * `url` - URL string to inspect.
pub(super) fn query_parameter_separator(url: &str) -> &'static str {
    let url_without_fragment = url.split('#').next().unwrap_or(url);

    if url_without_fragment.ends_with('?') || url_without_fragment.ends_with('&') {
        ""
    } else if url_without_fragment.contains('?') {
        "&"
    } else {
        "?"
    }
}
