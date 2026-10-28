//! Player discovery, stream resolution and DRM request dispatch.

use super::*;

impl StreamScraper {
    /// Loads players for the provided source and player link.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    /// * `query_source` - Query source name.
    /// * `query_url` - Source-specific player URL or API endpoint.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_players(
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
                "get_players",
                &params,
                None,
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
                "get_players",
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
        populate_stream_resolver_web_links(&mut root);

        Ok((
            ScraperAggregationResult::new(root.children, result.errors)
                .with_validations(result.validations)
                .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }

    /// Resolves a stream through the selected player resolver.
    ///
    /// # Arguments
    /// * `resolver_id` - Player resolver identifier, generic resolver, or scraper query.
    /// * `target` - Source, embed, or stream URL to resolve.
    /// * `source` - Optional scraper source for the scraper-query resolver.
    /// * `proxy_countries` - Ordered country hints forwarded to the proxy.
    /// * `proxy_rewrite_manifest_urls` - Whether proxied HLS manifests rewrite child URLs.
    ///
    /// # Errors
    /// Returns an error for an unsupported resolver or when resolution fails.
    ///
    /// # Returns
    /// The resolved stream wrapped in an aggregation result.
    pub(crate) async fn get_stream(
        &self,
        resolver_id: String,
        target: String,
        source: Option<String>,
        proxy_countries: Vec<String>,
        proxy_rewrite_manifest_urls: Option<bool>,
    ) -> Result<ScraperAggregationResult<ResolvedStream>> {
        let proxy_countries = normalize_proxy_countries(proxy_countries);
        let resolved = if resolver_id.trim() == SCRAPER_QUERY_STREAM_RESOLVER_ID {
            ResolvedStream::Stream(
                resolve_scraper_query_stream(
                    &self.scraper_agregator,
                    &self.player_resolver_endpoints,
                    source.as_deref().unwrap_or_default(),
                    &target,
                    &proxy_countries,
                    proxy_rewrite_manifest_urls.unwrap_or(false),
                )
                .await?,
            )
        } else if resolver_id.trim() == GENERIC_STREAM_RESOLVER_ID {
            StreamResolver::new(&self.scraper_agregator, &self.player_resolver_endpoints)
                .get_stream(&target)
                .await?
        } else {
            let resolver = player_resolver_for_id(&resolver_id)
                .ok_or_else(|| anyhow::anyhow!("Unsupported player resolver `{}`.", resolver_id))?;
            let service_parameters = self
                .scraper_agregator
                .query_collection_parameters(STREAM_SERVICE_GROUP_NAME, resolver.source_id())
                .unwrap_or(&[]);

            ResolvedStream::Stream(
                resolver
                    .get_stream(
                        &self.scraper_agregator,
                        self.credentials_store.as_ref(),
                        &resolver_id,
                        &target,
                        service_parameters,
                        &proxy_countries,
                        &self.player_resolver_endpoints,
                    )
                    .await?,
            )
        };

        Ok(ScraperAggregationResult::ok(resolved))
    }

    /// Forwards a DRM license request to the resolver identified by the stream token.
    ///
    /// # Arguments
    /// * `input` - Controller request whose path carries the stream token and body carries the license challenge.
    ///
    /// # Errors
    /// Returns an error for a missing token, unsupported source, or resolver failure.
    ///
    /// # Returns
    /// The buffered license response with its status, content type and headers.
    pub(crate) async fn get_drm_license(
        &self,
        input: ControlerStreamInput,
    ) -> Result<ControlerStreamOutput> {
        let stream_token = input.path.trim_matches('/').trim();
        if stream_token.is_empty() {
            bail!("Missing stream token.");
        }

        let (source, token) = stream_token
            .split_once('/')
            .unwrap_or(("m6play-fr", stream_token));
        let resolver = player_resolver_for_source(source)
            .ok_or_else(|| anyhow::anyhow!("Unsupported stream source `{}`.", source))?;
        let response = resolver
            .get_drm_license(&self.scraper_agregator, token, &input.body)
            .await?;

        Ok(ControlerStreamOutput {
            status: 200,
            body: ResponseBody::Buffered(response.body),
            content_type: response.content_type,
            headers: response.headers,
        })
    }
}
