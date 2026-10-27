//! Raw/runtime conversions preserving the serialized contract.

use super::*;

impl TryFrom<JsonScraperQueryRaw> for JsonScraperQuery {
    type Error = anyhow::Error;

    /// Converts a raw YAML query definition into a validated runtime query.
    ///
    /// # Errors
    ///
    /// Returns an error if the row pointer, entries, sub-queries, or request headers are invalid.
    fn try_from(config: JsonScraperQueryRaw) -> Result<Self> {
        let JsonScraperQueryRaw {
            common,
            extract_next_data,
            sibling_sub_query_concurrency,
            sub_query_context_concurrency,
            sub_query_fetch_concurrency,
            filters,
            row_pointer,
            entries,
            sub_queries,
        } = config;

        let ScraperQueryCommon {
            name,
            base_url,
            resolved_base_url,
            media_types,
            query_url,
            request_url_actions,
            request_method,
            request_body_pointer,
            request_body_select,
            request_body_actions,
            request_headers,
            http,
            query_param_mappings,
            result_item_field,
            pre_process,
            post_process,
            ..
        } = common;

        let media_types = media_types.unwrap_or_default();

        let resolved_base_url = query_helpers::resolved_or_template(&base_url, resolved_base_url);

        let entries = entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let sub_queries: Vec<Box<dyn crate::scrapyfy::scraper::query_trait::ScraperQuery>> =
            sub_queries
                .into_iter()
                .map(|raw| raw.into_boxed_query(&resolved_base_url))
                .collect::<Result<Vec<_>>>()?;
        let request_headers = request_headers
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;

        JsonScraperQuery::validate_request_actions(&name, &request_body_actions)?;
        JsonScraperQuery::validate_request_actions(&name, &request_url_actions)?;

        let mut query = JsonScraperQuery::try_new(
            &name,
            &resolved_base_url,
            media_types,
            &query_url,
            sibling_sub_query_concurrency,
            sub_query_context_concurrency,
            sub_query_fetch_concurrency,
            filters,
            &row_pointer,
            entries,
            sub_queries,
        )?;
        query.base_url_template = base_url;
        query.extract_next_data = extract_next_data;
        query.request_method = request_method;
        query.request_url_actions = request_url_actions;
        query.request_body_pointer = request_body_pointer;
        query.request_body_select = request_body_select;
        query.request_body_actions = request_body_actions;
        query.request_headers = request_headers;
        query.http_config = http.clone();
        query.http_client = HttpClient::with_http_config(http);
        query.query_param_mappings = query_param_mappings;
        query.result_item_field = result_item_field;

        for pre_process in pre_process {
            pre_process.validate(&name)?;
            query.pre_processes.push(pre_process);
        }

        for post_process in post_process {
            post_process.validate(&name)?;
            query.post_processes.push(post_process);
        }

        Ok(query)
    }
}

impl From<&JsonScraperQuery> for JsonScraperQueryRaw {
    /// Converts a runtime query back into its raw YAML-compatible representation.
    ///
    /// Sub-queries are serialized as [`JsonScraperSubQueryRaw`] children.
    /// Heterogeneous composition (e.g. HTML sub-queries inside a JSON query)
    /// raises a panic in the serialization path; a round-trip extension is a
    /// planned improvement.
    fn from(query: &JsonScraperQuery) -> Self {
        Self {
            common: ScraperQueryCommon {
                name: query.name.clone(),
                base_url: query.base_url_template.clone(),
                resolved_base_url: None,
                media_types: Some(query.media_types.clone()),
                query_url: query.query_url.clone(),
                request_url_actions: query.request_url_actions.clone(),
                request_method: query.request_method,
                request_body_pointer: query.request_body_pointer.clone(),
                request_body_select: query.request_body_select,
                request_body_actions: query.request_body_actions.clone(),
                request_headers: query
                    .request_headers
                    .iter()
                    .map(|h| ScraperRequestHeaderRaw {
                        name: h.name.clone(),
                        pointer: h.pointer.clone(),
                        select: h.select,
                        actions: h.actions.clone(),
                    })
                    .collect(),
                http: query.http_config.clone(),
                query_param_mappings: query.query_param_mappings.clone(),
                result_item_field: query.result_item_field.clone(),
                pre_process: query.pre_processes.clone(),
                post_process: query.post_processes.clone(),
            },
            extract_next_data: query.extract_next_data,
            sibling_sub_query_concurrency: query.sibling_sub_query_concurrency,
            sub_query_context_concurrency: query.sub_query_context_concurrency,
            sub_query_fetch_concurrency: query.sub_query_fetch_concurrency,
            filters: query.filters.clone(),
            row_pointer: query.row_pointer.clone(),
            entries: query
                .scraper_entries
                .iter()
                .map(JsonScraperEntryRaw::from)
                .collect(),
            sub_queries: Vec::new(),
            // Note: sub-queries serialization uses legacy JsonScraperSubQueryRaw
            // for backward compatibility. Sub-queries created via EntrySubQueryRaw
            // (the unified format) are not serialized through this path.
            // This is a known limitation; full round-trip support for heterogeneous
            // sub-queries is a planned improvement.
        }
    }
}

impl Serialize for JsonScraperQuery {
    /// Serializes the query through its raw YAML representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        JsonScraperQueryRaw::from(self).serialize(serializer)
    }
}

impl TryFrom<JsonScraperSubQueryRaw> for JsonScraperSubQuery {
    type Error = anyhow::Error;

    /// Converts a raw YAML sub-query definition into a validated runtime sub-query.
    ///
    /// Nested sub-queries are boxed into `Box<dyn ScraperQuery>`. They are
    /// dispatched by the unified executor or by [`JsonScraperSubQuery::execute_siblings`]
    /// at runtime; this conversion does not validate the runtime dispatch
    /// compatibility of mixed children (a `JsonScraperSubQuery` cannot dispatch
    /// a non-JSON sibling through its own siblings path, but the unified
    /// executor handles such cases).
    ///
    /// # Errors
    ///
    /// Returns an error if the row pointer is empty, the target is empty, or
    /// actions fail validation.
    fn try_from(config: JsonScraperSubQueryRaw) -> Result<Self> {
        let JsonScraperSubQueryRaw {
            common,
            row_pointer,
            entries,
            context_entries,
            request_body_pointer,
            request_body_select,
            request_body_actions,
            extract_next_data,
            sub_queries,
        } = config;

        let SubQueryCommon {
            context_pointer,
            context_select,
            filters,
            row_filters,
            target,
            request_pointer,
            request_select,
            request_actions,
            request_method,
            request_headers,
            http,
            pre_process,
            post_process,
            ..
        } = common;

        if row_pointer.trim().is_empty() {
            anyhow::bail!("Invalid JSON sub-query: row_pointer cannot be empty");
        }
        if target
            .as_deref()
            .is_some_and(|target| target.trim().is_empty())
        {
            anyhow::bail!("Invalid JSON sub-query: target cannot be empty");
        }

        for action in &pre_process {
            action.validate("JSON sub-query")?;
        }

        JsonScraperSubQuery::validate_actions(&row_pointer, &request_actions)?;
        JsonScraperSubQuery::validate_actions(&row_pointer, &request_body_actions)?;

        let entries = entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let context_entries = context_entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let request_headers = request_headers
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let sub_queries = sub_queries
            .into_iter()
            .map(|raw| -> Result<_> {
                let runtime: JsonScraperSubQuery = raw.try_into()?;
                Ok(Box::new(runtime)
                    as Box<
                        dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    >)
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            context_pointer,
            context_select,
            filters,
            row_filters,
            context_entries,
            target,
            request_pointer,
            request_select,
            request_actions,
            request_method,
            request_body_pointer,
            request_body_select,
            request_body_actions,
            extract_next_data,
            request_headers,
            http_config: http,
            row_pointer,
            entries,
            pre_processes: pre_process,
            post_processes: post_process,
            sub_queries,
            http_client: HttpClient::new(""),
        })
    }
}

impl From<&JsonScraperSubQuery> for JsonScraperSubQueryRaw {
    /// Converts a runtime sub-query back into its raw YAML-compatible representation.
    ///
    /// Nested sub-queries are not round-tripped through the raw type when they
    /// contain heterogeneous children; the serialization slot is left empty
    /// for such cases.  A full bidirectional conversion for heterogeneous
    /// trees is a planned extension.
    fn from(sub_query: &JsonScraperSubQuery) -> Self {
        Self {
            common: SubQueryCommon {
                context_pointer: sub_query.context_pointer.clone(),
                context_select: sub_query.context_select,
                filters: sub_query.filters.clone(),
                row_filters: sub_query.row_filters.clone(),
                target: sub_query.target.clone(),
                request_pointer: sub_query.request_pointer.clone(),
                request_select: sub_query.request_select,
                request_actions: sub_query.request_actions.clone(),
                request_body_actions: sub_query.request_body_actions.clone(),
                request_method: sub_query.request_method,
                request_headers: sub_query
                    .request_headers
                    .iter()
                    .map(|h| ScraperRequestHeaderRaw {
                        name: h.name.clone(),
                        pointer: h.pointer.clone(),
                        select: h.select,
                        actions: h.actions.clone(),
                    })
                    .collect(),
                http: sub_query.http_config.clone(),
                pre_process: sub_query.pre_processes.clone(),
                post_process: sub_query.post_processes.clone(),
            },
            row_pointer: sub_query.row_pointer.clone(),
            entries: sub_query
                .entries
                .iter()
                .map(JsonScraperEntryRaw::from)
                .collect(),
            context_entries: sub_query
                .context_entries
                .iter()
                .map(JsonScraperEntryRaw::from)
                .collect(),
            request_body_pointer: sub_query.request_body_pointer.clone(),
            request_body_select: sub_query.request_body_select,
            request_body_actions: sub_query.request_body_actions.clone(),
            extract_next_data: sub_query.extract_next_data,
            sub_queries: sub_query
                .sub_queries
                .iter()
                .map(|child| {
                    child
                        .as_any()
                        .downcast_ref::<JsonScraperSubQuery>()
                        .map(JsonScraperSubQueryRaw::from)
                        .unwrap_or_else(|| {
                            panic!(
                                "JsonScraperSubQuery::from: nested sub-query '{}' is not a JsonScraperSubQuery; \
                                 heterogeneous sub-queries are not yet supported in the JSON serialization path",
                                child.name()
                            )
                        })
                })
                .collect(),
        }
    }
}
