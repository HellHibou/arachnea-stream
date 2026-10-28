//! Catalog queries, source filtering and aggregation.

use super::*;

impl StreamScraper {
    /// Executes the `search` query using URL-encoded terms.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    /// * `query` - Raw search string that will be URL-encoded before execution.
    /// * `media_types` - Optional media type filters applied to query selection and result filtering.
    /// * `themes` - Optional theme filters applied to scraped results.
    /// * `page` - 1-based page number requested by the caller.
    /// * `source_params` - Source-specific runtime parameters returned by the previous page.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn search(
        &self,
        context: RequestControlerContext,
        query: String,
        media_types: Vec<String>,
        themes: Vec<String>,
        page: usize,
        source_params: ScraperSourceParams,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let page = page.max(1);
        let scrapper_list = if page > 1 && !source_params.is_empty() {
            Some(source_params.keys().cloned().collect::<Vec<_>>())
        } else {
            None
        };

        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("search_query".to_string(), query.clone());
        params.insert(
            "search_query_json".to_string(),
            serde_json::to_string(&query)?,
        );
        params.insert("search_terms".to_string(), encode(&query).to_string());
        params.insert("media_types".to_string(), media_types.join(","));
        params.insert("themes".to_string(), themes.join(","));
        params.insert("page".to_string(), page.to_string());
        self.enrich_runtime_params(&mut params);

        let media = if media_types.is_empty() {
            None
        } else {
            Some(&media_types)
        };

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::ClientCache),
                STREAM_SERVICE_GROUP_NAME,
                "search",
                &params,
                Some(&source_params),
                scrapper_list.as_ref(),
                media,
                None,
                None,
                "search",
            )
            .await;
        let global_etag = result.take_global_etag();
        Ok((result, global_etag))
    }

    /// Convenience helper for the `get_entry` query using the provided absolute entry URL.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    /// * `query_source` - Query source name.
    /// * `query_url` - Absolute URL of the entry page to fetch.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution cannot be started.
    pub async fn get_entry(
        &self,
        context: RequestControlerContext,
        query_source: String,
        query_url: String,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("query_url".to_string(), query_url);
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::ClientCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_entry",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
                "get_entry",
            )
            .await;
        let global_etag = result.take_global_etag();

        let mut root = ScraperDataNode {
            children: result.data.into_iter().next().unwrap_or_default(),
            ..Default::default()
        };
        populate_stream_resolver_web_links(&mut root);

        Ok((
            ScraperAggregationResult::new(root.children, result.errors)
                .with_validations(result.validations)
                .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }

    /// Convenience helper for the `get_season` query using the provided absolute season URL.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    /// * `query_source` - Query source name.
    /// * `query_url` - Absolute URL of the season payload to fetch.
    /// * `page` - 1-based page number requested from the backend source.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution cannot be started.
    pub async fn get_season(
        &self,
        context: RequestControlerContext,
        query_source: String,
        query_url: String,
        page: usize,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("query_url".to_string(), query_url);
        params.insert("page".to_string(), page.max(1).to_string());
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::ClientCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_season",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
                "get_season",
            )
            .await;
        let global_etag = result.take_global_etag();

        Ok((
            ScraperAggregationResult::new(
                result.data.into_iter().next().unwrap_or_default(),
                result.errors,
            )
            .with_validations(result.validations)
            .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }

    /// Loads the aggregated live catalog across every configured source.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn list_lives(
        &self,
        context: RequestControlerContext,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);
        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::FullCache),
                STREAM_SERVICE_GROUP_NAME,
                "list_lives",
                &params,
                None,
                None,
                None,
                None,
                Some("source"),
                "list_lives",
            )
            .await;
        let global_etag = result.take_global_etag();
        Ok((result, global_etag))
    }

    /// Loads one live payload for the provided source and channel identifier.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    /// * `query_source` - Query source name.
    /// * `channel` - Source-specific live channel identifier.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution cannot be started.
    pub async fn get_live(
        &self,
        context: RequestControlerContext,
        query_source: String,
        channel: String,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("channel".to_string(), channel);
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::default(),
                STREAM_SERVICE_GROUP_NAME,
                "get_live",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
                "get_live",
            )
            .await;
        let global_etag = result.take_global_etag();

        Ok((
            ScraperAggregationResult::new(
                result.data.into_iter().next().unwrap_or_default(),
                result.errors,
            )
            .with_validations(result.validations)
            .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }

    /// Loads the home catalog for all loaded sources or an explicit selection.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    /// * `sources` - Exact source identifiers. `None` keeps the legacy selection;
    ///   an empty list returns an empty success without provider calls or ETag.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn load_home(
        &self,
        context: RequestControlerContext,
        sources: Option<Vec<String>>,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let mut errors = Vec::new();
        let selection = if let Some(mut sources) = sources {
            sources.sort();
            sources.dedup();
            let mut valid = Vec::new();
            for source in sources {
                if self.scraper_agregator.source_has_query(
                    STREAM_SERVICE_GROUP_NAME,
                    &source,
                    "load_home",
                ) {
                    valid.push(source);
                } else {
                    let code = HOME_ERROR_CODES.next_code();
                    let message = format!(
                        "source `{source}` is unknown, unavailable or does not support load_home"
                    );
                    tracing::error!(target: "arachnea_stream::stream_scraper", error_code = %code, operation = "load_home", source = %source, error = %message);
                    errors.push(ScraperExecutionError {
                        code,
                        operation: "load_home".to_string(),
                        source: Some(source),
                        origin: ScraperErrorOrigin::Backend,
                        message,
                    });
                }
            }
            if valid.is_empty() {
                return Ok((ScraperAggregationResult::new(Vec::new(), errors), None));
            }
            Some(valid)
        } else {
            None
        };
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);
        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::FullCache),
                STREAM_SERVICE_GROUP_NAME,
                "load_home",
                &params,
                None,
                selection.as_ref(),
                None,
                None,
                Some("source"),
                "load_home",
            )
            .await;
        result.errors.extend(errors);
        let global_etag = if selection.is_none() || result.errors.is_empty() {
            result.take_global_etag()
        } else {
            result.global_etag = None;
            None
        };
        Ok((result, global_etag))
    }

    /// Loads display metadata for every configured source.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_service(
        &self,
        context: RequestControlerContext,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::NoCache),
                STREAM_SERVICE_GROUP_NAME,
                "service_stream_metadata",
                &params,
                None,
                None,
                None,
                None,
                None,
                "service_stream_metadata",
            )
            .await;
        for row in &mut result.data {
            let supported_queries = row
                .get("id")
                .and_then(ScraperDataNode::value_as_string)
                .and_then(|id| {
                    self.scraper_agregator
                        .query_services()
                        .find(|group| group.name == STREAM_SERVICE_GROUP_NAME)
                        .and_then(|group| group.services.iter().find(|source| source.name() == id))
                })
                .map(|source| source.query_names())
                .unwrap_or_default();
            row.insert(
                "supported_queries".to_string(),
                ScraperDataNode::from_values_typed(
                    supported_queries,
                    ScraperOutputType::StringArray,
                ),
            );
        }
        let global_etag = result.take_global_etag();
        Ok((result, global_etag))
    }

    /// Loads category payloads for the provided YAML-defined source descriptors.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    /// * `sources` - Source descriptors returned under `category.sources` by `load_home`.
    /// * `page` - 1-based page number requested from the backend source.
    /// * `source_params` - Optional per-source runtime parameters overriding global values.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    ///
    /// # Errors
    ///
    /// Returns an error if a required source name is missing from the request.
    pub async fn get_category(
        &self,
        context: RequestControlerContext,
        sources: Vec<HashMap<String, String>>,
        page: usize,
        source_params: ScraperSourceParams,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let page = page.max(1);
        let mut source_params = source_params;
        let mut scrapper_list = Vec::new();

        for mut source in sources {
            source.insert("page".to_string(), page.to_string());

            let query_source = source
                .remove("name")
                .or_else(|| source.remove("source"))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Missing category source name"))?;

            let existing_params = source_params.remove(&query_source).unwrap_or_default();
            source.extend(existing_params);
            source_params.insert(query_source.clone(), source);
            scrapper_list.push(query_source);
        }

        if scrapper_list.is_empty() {
            scrapper_list.extend(source_params.keys().cloned());
        }

        if scrapper_list.is_empty() {
            anyhow::bail!("Missing category source name");
        }

        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("page".to_string(), page.to_string());
        self.enrich_runtime_params(&mut params);

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::FullCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_category",
                &params,
                Some(&source_params),
                Some(&scrapper_list),
                None,
                None,
                Some("source"),
                "get_category",
            )
            .await;
        let global_etag = result.take_global_etag();
        Ok((result, global_etag))
    }

    /// Loads one paged section payload for the provided source and section link.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    /// * `query_source` - Query source name.
    /// * `query_url` - Source-specific section URL or API endpoint.
    /// * `page` - 1-based page number requested from the backend source.
    /// * `source_params` - Optional per-source runtime parameters overriding global values.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_section(
        &self,
        context: RequestControlerContext,
        query_source: String,
        query_url: String,
        page: usize,
        mut source_params: ScraperSourceParams,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("page".to_string(), page.max(1).to_string());
        self.enrich_runtime_params(&mut params);

        if !query_url.trim().is_empty() {
            params.insert("query_url".to_string(), query_url.clone());
            params.insert("link".to_string(), query_url);
        }

        for params_for_source in source_params.values_mut() {
            if let Some(link) = params_for_source.get("link").cloned() {
                params_for_source
                    .entry("query_url".to_string())
                    .or_insert(link);
            }
        }

        let scrapper_list = if !query_source.trim().is_empty() {
            Some(vec![query_source])
        } else if !source_params.is_empty() {
            Some(source_params.keys().cloned().collect())
        } else {
            None
        };

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::FullCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_section",
                &params,
                Some(&source_params),
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
                "get_section",
            )
            .await;
        let global_etag = result.take_global_etag();

        let mut root = ScraperDataNode::default();
        for row in result.data {
            root.merge_first(&ScraperDataNode {
                children: row,
                ..Default::default()
            });
        }
        root.keep_first_values();

        Ok((
            ScraperAggregationResult::new(root.children, result.errors)
                .with_validations(result.validations)
                .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }

    /// Loads one deferred recommendations payload for the provided source and recommendations link.
    ///
    /// This is a generic facade over the `get_recommendations` YAML query: no
    /// provider-specific logic lives in Rust. The response reuses the
    /// `get_section` JSON shape (`entries`, `current_page`, `have_more`).
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    /// * `query_source` - Query source name.
    /// * `query_url` - Source-specific recommendations URL or API endpoint.
    /// * `page` - 1-based page number requested from the backend source.
    /// * `source_params` - Optional per-source runtime parameters overriding global values.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_recommendations(
        &self,
        context: RequestControlerContext,
        query_source: String,
        query_url: String,
        page: usize,
        mut source_params: ScraperSourceParams,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("page".to_string(), page.max(1).to_string());
        self.enrich_runtime_params(&mut params);

        if !query_url.trim().is_empty() {
            params.insert("query_url".to_string(), query_url.clone());
            params.insert("link".to_string(), query_url);
        }

        for params_for_source in source_params.values_mut() {
            if let Some(link) = params_for_source.get("link").cloned() {
                params_for_source
                    .entry("query_url".to_string())
                    .or_insert(link);
            }
        }

        let scrapper_list = if !query_source.trim().is_empty() {
            Some(vec![query_source])
        } else if !source_params.is_empty() {
            Some(source_params.keys().cloned().collect())
        } else {
            None
        };

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::FullCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_recommendations",
                &params,
                Some(&source_params),
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
                "get_recommendations",
            )
            .await;
        let global_etag = result.take_global_etag();

        let mut root = ScraperDataNode::default();
        for row in result.data {
            root.merge_first(&ScraperDataNode {
                children: row,
                ..Default::default()
            });
        }
        root.keep_first_values();

        Ok((
            ScraperAggregationResult::new(root.children, result.errors)
                .with_validations(result.validations)
                .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }

    /// Loads banners for the provided source and banner link.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    /// * `query_source` - Query source name.
    /// * `query_url` - Source-specific banner URL or API endpoint.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_banners(
        &self,
        context: RequestControlerContext,
        query_source: String,
        query_url: String,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);

        if !query_url.trim().is_empty() {
            params.insert("query_url".to_string(), query_url.clone());
            params.insert("link".to_string(), query_url);
        }

        let scrapper_list = if query_source.trim().is_empty() {
            None
        } else {
            Some(vec![query_source])
        };

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::ClientCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_banners",
                &params,
                None,
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
                "get_banners",
            )
            .await;
        let global_etag = result.take_global_etag();

        let mut root = ScraperDataNode::default();
        for row in result.data {
            root.merge_first(&ScraperDataNode {
                children: row,
                ..Default::default()
            });
        }
        root.keep_first_values();

        Ok((
            ScraperAggregationResult::new(root.children, result.errors)
                .with_validations(result.validations)
                .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }
}
