//! Source execution, ordered aggregation and proxy discovery batches.

use super::*;

impl ScraperAgregator {
    /// Streams unconditional proxy-source batches in completion order.
    /// This discovery-only path leaves ordinary aggregation, ordering and caching unchanged.
    ///
    /// # Arguments
    /// - `group_name`: Loaded proxy source group.
    /// - `query_name`: Source query to execute without conditional validators.
    /// - `params`: Owned source parameters, normally the requested country.
    #[cfg(feature = "arachnea-proxy")]
    pub(crate) fn proxy_source_batches<'a>(
        &'a self,
        group_name: &str,
        query_name: &'a str,
        params: HashMap<String, String>,
    ) -> futures::stream::BoxStream<'a, Result<Vec<HashMap<String, ScraperDataNode>>>> {
        use futures::{FutureExt, StreamExt};
        let Some(group) = self.queries_collection.get(group_name) else {
            return futures::stream::once(async {
                anyhow::bail!("proxy source group is not loaded")
            })
            .boxed();
        };
        let pending = futures::stream::FuturesUnordered::new();
        for query in &group.services {
            let params = params.clone();
            pending.push(
                async move {
                    #[cfg(feature = "performance-diagnostics")]
                    let mut observation = ProxySourceObservation {
                        source: query.name(),
                        query: query_name,
                        country: params.get("country").cloned(),
                        started: std::time::Instant::now(),
                        outcome: "cancelled",
                        rows: 0,
                    };
                    let result = query
                        .execute_query_with_validation(
                            query_name,
                            &params,
                            None,
                            None,
                            None,
                            query.yaml_hash(),
                        )
                        .await;
                    let (mut rows, _) = match result {
                        Ok(result) => result,
                        Err(error) => {
                            #[cfg(feature = "performance-diagnostics")]
                            {
                                observation.outcome = "failed";
                            }
                            return Err(
                                error.context(format!("proxy source {} failed", query.name()))
                            );
                        }
                    };
                    for row in &mut rows {
                        row.insert(
                            "proxy_source".into(),
                            ScraperDataNode::from_values_typed(
                                vec![query.name().to_string()],
                                ScraperOutputType::String,
                            ),
                        );
                    }
                    #[cfg(feature = "performance-diagnostics")]
                    {
                        observation.rows = rows.len();
                    }
                    #[cfg(feature = "performance-diagnostics")]
                    {
                        observation.outcome = "completed";
                    }
                    Ok(rows)
                }
                .boxed(),
            );
        }
        pending.boxed()
    }

    /// Executes a single aggregation pass of one query on every selected source.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name whose sources should be queried.
    /// * `query_name` - Name of the query to execute on every configured source.
    /// * `params` - Runtime values passed to each source-specific query template.
    /// * `source_params` - Optional per-source parameter overrides merged into `params`.
    /// * `scrapper_list` - Optional list of source names to execute. When `None`, every
    ///   configured source in the group is queried.
    /// * `query_media_type_filter` - Filter on media_type query.
    /// * `fields_filters` - Root fields filter list or None.
    /// * `source_field_name` - Optional metadata key used to store the originating source name.
    /// * `operation` - Logical operation name used in error correlation codes.
    /// * `client_fragments` - Optional per-source validator fragments provided by the
    ///   client (source name → fragment). When set for a source, conditional headers
    ///   are applied to that source's root request and unchanged content skips parsing.
    /// * `server_cache_interaction` - How this pass interacts with the server-side
    ///   cache (`None`, `Validate` or `ForceRefresh`). See [`ServerCacheInteraction`].
    ///
    /// The returned entries are already tree-shaped when fields include `>` in their names.
    ///
    /// Per-source and pre-execution errors are collected in
    /// [`ScraperAggregationResult::errors`] and logged with a correlation code.
    /// Per-source validation outcomes are collected in
    /// [`ScraperAggregationResult::validations`].
    pub(super) async fn execute_query_pass(
        &self,
        group_name: &str,
        query_name: &str,
        params: &HashMap<String, String>,
        source_params: Option<&ScraperSourceParams>,
        scrapper_list: Option<&Vec<String>>,
        query_media_type_filter: Option<&Vec<String>>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
        source_field_name: Option<&str>,
        operation: &str,
        client_fragments: Option<&HashMap<String, String>>,
        server_cache_interaction: ServerCacheInteraction,
    ) -> ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>> {
        let Some(query_service) = self.queries_collection.get(group_name) else {
            let code = ERROR_CODE_GEN.next_code();
            let message = format!("query group `{group_name}` not found");
            tracing::error!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
                error_code = %code,
                operation = operation,
                source = ?Option::<String>::None,
                error = %message,
                "query execution could not start",
            );
            return ScraperAggregationResult::new(
                Vec::new(),
                vec![ScraperExecutionError {
                    code,
                    operation: operation.to_string(),
                    source: None,
                    origin: ScraperErrorOrigin::Backend,
                    message,
                }],
            );
        };

        let filtered_queries: Vec<&ScraperQueryCollection> = query_service
            .services
            .iter()
            .filter(|query_collection| match scrapper_list {
                Some(scrapper_list) => scrapper_list
                    .iter()
                    .any(|scrapper_name| scrapper_name == query_collection.name()),
                _none => true,
            })
            .collect();

        if filtered_queries.is_empty() {
            let code = ERROR_CODE_GEN.next_code();
            let message = format!("no configured sources selected in query group `{group_name}`");
            tracing::error!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
                error_code = %code,
                operation = operation,
                source = ?Option::<String>::None,
                error = %message,
                "query execution could not start",
            );
            return ScraperAggregationResult::new(
                Vec::new(),
                vec![ScraperExecutionError {
                    code,
                    operation: operation.to_string(),
                    source: None,
                    origin: ScraperErrorOrigin::Backend,
                    message,
                }],
            );
        }

        let results = join_all(filtered_queries.iter().map(|query_collection| {
            let mut execution_params = params.clone();

            if let Some(source_params) = source_params {
                if let Some(params_for_source) = source_params.get(query_collection.name()) {
                    execution_params.extend(params_for_source.clone());
                }
            }

            let client_fragment = client_fragments
                .and_then(|fragments| fragments.get(query_collection.name()).map(String::as_str))
                .filter(|fragment| {
                    // Only apply the client fragment when its YAML hash still
                    // matches the current collection; an edited scraper must
                    // trigger a full re-fetch without conditional headers.
                    fragment_yaml_hash(fragment)
                        .is_some_and(|hash| hash == query_collection.yaml_hash())
                });
            let cache_key = match server_cache_interaction {
                ServerCacheInteraction::None => None,
                _ => Some(server_cache_key(
                    group_name,
                    query_collection.name(),
                    query_name,
                    &execution_params,
                    query_media_type_filter,
                    fields_filters,
                    query_collection.yaml_hash(),
                )),
            };
            let server_cache = self.server_cache.clone();
            let execute: futures::future::BoxFuture<'_, _> = Box::pin(async move {
                match server_cache_interaction {
                    ServerCacheInteraction::None => {
                        query_collection
                            .execute_query_with_validation(
                                query_name,
                                &execution_params,
                                query_media_type_filter,
                                fields_filters,
                                client_fragment,
                                query_collection.yaml_hash(),
                            )
                            .await
                    }
                    ServerCacheInteraction::Validate => {
                        let Some(key) = cache_key else {
                            return query_collection
                                .execute_query_with_validation(
                                    query_name,
                                    &execution_params,
                                    query_media_type_filter,
                                    fields_filters,
                                    client_fragment,
                                    query_collection.yaml_hash(),
                                )
                                .await;
                        };
                        // The stored fragment takes precedence over the client
                        // fragment: the cached rows back it, so a `304` against
                        // it proves those rows are still current.
                        let cached = server_cache.load_entry(&key).await;
                        tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
                            group_name,
                            source = query_collection.name(),
                            query = query_name,
                            cached_entry = cached.is_some(),
                            "server cache validation of source"
                        );
                        let effective_fragment = cached
                            .as_ref()
                            .map(|entry| entry.etag_fragment.as_str())
                            .or(client_fragment);
                        match query_collection
                            .execute_query_with_validation(
                                query_name,
                                &execution_params,
                                query_media_type_filter,
                                fields_filters,
                                effective_fragment,
                                query_collection.yaml_hash(),
                            )
                            .await
                        {
                            Ok((rows, outcome)) => match outcome {
                                // Unchanged remote content: never return rows
                                // here — phase 2 resolves them (cache first),
                                // exactly like the historical client-ETag flow,
                                // so they cannot be duplicated in the aggregate.
                                Some(outcome) if outcome.not_modified => {
                                    Ok((Vec::new(), Some(outcome)))
                                }
                                Some(outcome) => {
                                    if !rows.is_empty() {
                                        server_cache
                                            .store_entry(&key, &rows, &outcome.etag_fragment)
                                            .await;
                                    } else if cached.is_some() {
                                        // Content vanished: drop the stale entry
                                        // instead of keeping unusable rows.
                                        server_cache.invalidate(&key).await;
                                    }
                                    Ok((rows, Some(outcome)))
                                }
                                // Static query without validation outcome.
                                None => Ok((rows, None)),
                            },
                            Err(error) => {
                                if cached.is_some() {
                                    server_cache.invalidate(&key).await;
                                }
                                Err(error)
                            }
                        }
                    }
                    ServerCacheInteraction::ForceRefresh => {
                        let result = query_collection
                            .execute_query_with_validation(
                                query_name,
                                &execution_params,
                                query_media_type_filter,
                                fields_filters,
                                None,
                                query_collection.yaml_hash(),
                            )
                            .await;
                        match (&cache_key, &result) {
                            (Some(key), Ok((rows, Some(outcome))))
                                if !rows.is_empty() && !outcome.not_modified =>
                            {
                                server_cache
                                    .store_entry(key, rows, &outcome.etag_fragment)
                                    .await;
                            }
                            (_, Err(_)) => {
                                if let Some(key) = &cache_key {
                                    server_cache.invalidate(key).await;
                                }
                            }
                            _ => {}
                        }
                        result
                    }
                }
            });
            async move {
                #[cfg(feature = "arachnea-proxy")]
                {
                    let diagnostics = arachnea_proxy::core::ProxyFallbackDiagnostics::default();
                    let result = diagnostics.scope(execute).await;
                    (result, diagnostics.messages())
                }
                #[cfg(not(feature = "arachnea-proxy"))]
                {
                    (execute.await, Vec::<String>::new())
                }
            }
        }))
        .await;

        let mut aggregated_results: Vec<HashMap<String, ScraperDataNode>> = Vec::new();
        let mut errors: Vec<ScraperExecutionError> = Vec::new();
        let mut validations: HashMap<String, ScraperSourceValidation> = HashMap::new();

        for (query, (result, fallback_messages)) in filtered_queries.into_iter().zip(results) {
            for message in fallback_messages {
                let code = ERROR_CODE_GEN.next_code();
                tracing::warn!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
                    error_code = %code,
                    operation,
                    source = query.name(),
                    error = %message,
                    "source query used direct proxy fallback",
                );
                errors.push(ScraperExecutionError {
                    code,
                    operation: operation.to_string(),
                    source: Some(query.name().to_string()),
                    origin: ScraperErrorOrigin::Backend,
                    message,
                });
            }
            match result {
                Ok((entries, outcome)) => {
                    if let Some(outcome) = outcome {
                        validations.insert(
                            query.name().to_string(),
                            ScraperSourceValidation {
                                status: if outcome.not_modified {
                                    ScraperSourceStatus::Stale
                                } else {
                                    ScraperSourceStatus::Fresh
                                },
                                etag: outcome.etag_fragment,
                            },
                        );
                    }
                    for mut entry in entries {
                        if let Some(source_field_name) = source_field_name {
                            // Preserve the origin of each row when the caller requests it.
                            entry.insert(
                                source_field_name.to_string(),
                                ScraperDataNode::from_values_typed(
                                    vec![query.name().to_string()],
                                    ScraperOutputType::String,
                                ),
                            );
                        }

                        aggregated_results.push(entry);
                    }
                }
                Err(error) => {
                    let code = ERROR_CODE_GEN.next_code();
                    let source_name = query.name().to_string();
                    tracing::error!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
                        error_code = %code,
                        operation = operation,
                        source = %source_name,
                        error = %error,
                        "source query failed",
                    );
                    errors.push(ScraperExecutionError {
                        code,
                        operation: operation.to_string(),
                        source: Some(source_name),
                        origin: ScraperErrorOrigin::Backend,
                        message: format!("{error:#}"),
                    });
                }
            }
        }

        ScraperAggregationResult::new(aggregated_results, errors).with_validations(validations)
    }

    /// Executes the same query on all sources within a group with optional
    /// conditional ETag validation, optional server-side caching, and merges
    /// the resulting entries.
    ///
    /// The behavior is driven by [`QueryParameters::cache_type`]:
    ///
    /// - `NoCache`: a single unconditional pass runs and no global ETag is
    ///   produced (historical `enable_etag = false`).
    /// - `ClientCache` (the default): the client global ETag is read from the
    ///   request context's `If-None-Match` header. Phase 1 validates every
    ///   involved source in parallel using the client fragments decoded from
    ///   that global ETag; unchanged sources skip parsing entirely. A second
    ///   pass re-fetches stale sources without conditional headers so the
    ///   aggregated payload stays complete. The result carries the up-to-date
    ///   global ETag, always built from the phase-1 fragments.
    /// - `ServerCache`: same two-phase flow, but the per-source conditional
    ///   requests are driven by the fragments stored in the server-side cache.
    ///   Stale sources are served from the cache when it holds their rows, so
    ///   only unknown sources hit the remotes again. No global ETag is exposed
    ///   to the client.
    /// - `FullCache`: server-cache flow with the rebuilt global ETag exposed
    ///   to the client, keeping the full `304 Not Modified` short-circuit.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request, providing the
    ///   received HTTP headers (`If-None-Match`).
    /// * `query_parameters` - Runtime options controlling the execution.
    /// * `group_name` - Group name whose sources should be queried.
    /// * `query_name` - Name of the query to execute on every configured source.
    /// * `params` - Runtime values passed to each source-specific query template.
    /// * `source_params` - Optional per-source parameter overrides merged into `params`.
    /// * `scrapper_list` - Optional list of source names to execute. When `None`, every
    ///   configured source in the group is queried.
    /// * `query_media_type_filter` - Filter on media_type query.
    /// * `fields_filters` - Root fields filter list or None.
    /// * `source_field_name` - Optional metadata key used to store the originating source name.
    /// * `operation` - Logical operation name used in error correlation codes.
    pub async fn execute_query_async(
        &self,
        context: &RequestControlerContext,
        query_parameters: QueryParameters,
        group_name: &str,
        query_name: &str,
        params: &HashMap<String, String>,
        source_params: Option<&ScraperSourceParams>,
        scrapper_list: Option<&Vec<String>>,
        query_media_type_filter: Option<&Vec<String>>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
        source_field_name: Option<&str>,
        operation: &str,
    ) -> ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>> {
        let cache_type = query_parameters.cache_type;
        let client_flow = cache_type.sends_client_etag();
        let server_cache_enabled = cache_type.uses_server_cache();

        if !client_flow && !server_cache_enabled {
            return self
                .execute_query_pass(
                    group_name,
                    query_name,
                    params,
                    source_params,
                    scrapper_list,
                    query_media_type_filter,
                    fields_filters,
                    source_field_name,
                    operation,
                    None,
                    ServerCacheInteraction::None,
                )
                .await;
        }

        // Deterministic ordered service list involved in this request.
        let mut services: Vec<String> = match scrapper_list {
            Some(list) => list.clone(),
            None => self.source_names_in_group(group_name),
        };
        services.retain(|name| !name.trim().is_empty());
        services.sort();
        services.dedup();

        // Phase 1: parallel conditional validation per source.
        //
        // Client fragments only participate in the request-time validation when
        // the global ETag is exposed to the client (`ClientCache` / `FullCache`);
        // in server-cache modes, the stored per-source fragments drive the
        // conditional requests instead.
        let client_etag = if client_flow {
            context
                .get_header(HEADER_IF_NONE_MATCH)
                .map(normalize_client_etag)
                .filter(|etag| !etag.is_empty())
                .map(str::to_string)
        } else {
            None
        };
        let client_fragments = if client_flow {
            client_etag
                .as_deref()
                .map(decode_client_fragments)
                .filter(|fragments| !fragments.is_empty())
        } else {
            None
        };

        let mut result = self
            .execute_query_pass(
                group_name,
                query_name,
                params,
                source_params,
                scrapper_list,
                query_media_type_filter,
                fields_filters,
                source_field_name,
                operation,
                client_fragments.as_ref(),
                if server_cache_enabled {
                    ServerCacheInteraction::Validate
                } else {
                    ServerCacheInteraction::None
                },
            )
            .await;
        let etag_fragments: HashMap<String, String> = result
            .validations
            .iter()
            .map(|(name, validation)| (name.clone(), validation.etag.clone()))
            .collect();
        // Sources without a recorded validation outcome (static queries,
        // failed fetches) carry no fragment and are excluded from the tag:
        // a returned ETag never claims freshness it cannot back.
        let global_etag = build_global_etag(&etag_fragments);

        // Full 304 short-circuit: every validated source is stale and the
        // rebuilt global ETag matches what the client sent — no data needed.
        let all_stale = !result.validations.is_empty()
            && result
                .validations
                .values()
                .all(|validation| validation.status == ScraperSourceStatus::Stale);
        if result.errors.is_empty()
            && all_stale
            && global_etag.is_some()
            && global_etag == client_etag
        {
            result.global_etag = global_etag;
            return result;
        }

        // Phase 2: forced full GETs for stale sources only, merging their
        // freshly parsed rows into the aggregate. The global ETag keeps the
        // phase-1 fragments — they already describe the current remote state.
        let stale_sources: Vec<String> = result
            .validations
            .iter()
            .filter(|(_, validation)| validation.status == ScraperSourceStatus::Stale)
            .map(|(name, _)| name.clone())
            .collect();

        if !stale_sources.is_empty() {
            // Phase 2 resolves the rows of stale sources. With the server cache
            // enabled, "stale" means the remote confirmed a stored fragment via
            // `304`, so cached rows are still current: serve them directly and
            // only re-fetch from the remotes the sources without a usable entry.
            let remote_needed: Vec<String> = if server_cache_enabled {
                let mut remaining: Vec<String> = Vec::new();
                for source in &stale_sources {
                    let Some(key) = self.server_cache_key_for_source(
                        group_name,
                        source,
                        query_name,
                        params,
                        source_params,
                        query_media_type_filter,
                        fields_filters,
                    ) else {
                        remaining.push(source.clone());
                        continue;
                    };
                    match self.server_cache.load_entry(&key).await {
                        Some(entry) => {
                            tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
                                group_name,
                                source = %source,
                                query = query_name,
                                row_count = entry.rows.len(),
                                "source rows served from server cache"
                            );
                            for mut row in entry.rows {
                                if let Some(source_field_name) = source_field_name {
                                    row.insert(
                                        source_field_name.to_string(),
                                        ScraperDataNode::from_values_typed(
                                            vec![source.clone()],
                                            ScraperOutputType::String,
                                        ),
                                    );
                                }
                                result.data.push(row);
                            }
                        }
                        None => {
                            tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
                                group_name,
                                source = %source,
                                query = query_name,
                                "source rows unavailable in server cache; fetching from remote"
                            );
                            remaining.push(source.clone());
                        }
                    }
                }
                remaining
            } else {
                stale_sources.clone()
            };

            if !remote_needed.is_empty() {
                let catch_up = self
                    .execute_query_pass(
                        group_name,
                        query_name,
                        params,
                        source_params,
                        Some(&remote_needed),
                        query_media_type_filter,
                        fields_filters,
                        source_field_name,
                        operation,
                        None,
                        if server_cache_enabled {
                            ServerCacheInteraction::ForceRefresh
                        } else {
                            ServerCacheInteraction::None
                        },
                    )
                    .await;
                result.data.extend(catch_up.data);
                result.errors.extend(catch_up.errors);
            }
        }

        if client_flow && result.errors.is_empty() {
            result.global_etag = global_etag;
        }
        result
    }
}
