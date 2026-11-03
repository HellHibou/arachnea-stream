//! HTML fallback fetching and source recognition.

use super::*;

impl StreamResolver<'_> {
    /// Runs the HTML-content fallback phase after every direct resolver attempt fails.
    ///
    /// # Arguments
    /// * `url` - Source URL to fetch as HTML.
    /// * `source_names` - Resolver service names tried in order.
    ///
    /// # Errors
    /// Returns an error when fetching fails or a recognized service reports a
    /// terminal error or yields no stream.
    ///
    /// # Returns
    /// The existing stream box forwarded from the successful attempt, or `None`
    /// when no service recognizes the document.
    pub(super) async fn resolve_from_fetched_html(
        &self,
        url: &str,
        source_names: &[String],
    ) -> Result<Option<Box<ResolvedPlayerStream>>> {
        let fetched = self.fetch_embed_html(url).await?;

        for name in source_names {
            match self
                .can_resolve_html(name, &fetched.url, &fetched.html)
                .await
            {
                Ok(true) => {
                    tracing::debug!(target: "arachnea_stream::stream_resolver",
                        service = %name,
                        url = %fetched.url,
                        "HTML content recognized by YAML resolver"
                    );

                    return match self
                        .try_resolve_stream(name, &fetched.url, Some(&fetched.html))
                        .await
                    {
                        Ok(ResolveAttemptResult::Stream(stream)) => Ok(Some(stream)),
                        Ok(ResolveAttemptResult::NoStream) => bail!(
                            "HTML resolver `{}` recognized URL `{}` but produced no stream",
                            name, fetched.url
                        ),
                        Ok(ResolveAttemptResult::TerminalError(message)) => {
                            tracing::debug!(target: "arachnea_stream::stream_resolver", 
                                service = %name,
                                url = %fetched.url,
                                "Hoster reported a terminal error message"
                            );
                            bail!("{}", message)
                        }
                        Err(error) => Err(error).with_context(|| {
                            format!(
                                "HTML resolver `{}` recognized URL `{}` but failed to extract stream",
                                name, fetched.url
                            )
                        }),
                    };
                }
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(target: "arachnea_stream::stream_resolver",
                        service = %name,
                        url = %url,
                        error = ?error,
                        "Error while checking HTML resolver compatibility, trying next service"
                    );
                }
            }
        }

        Ok(None)
    }

    /// Fetches HTML for resolver recognition using Stream's existing limits.
    ///
    /// HTTP and literal JavaScript redirect budgets are independent. The size
    /// limit checks the materialized body, not the number of downloaded bytes.
    ///
    /// # Arguments
    /// * `url` - Initial embed URL.
    ///
    /// # Errors
    /// Returns document retrieval, acceptance or redirect-limit errors.
    ///
    /// # Returns
    /// The final document URL and decoded HTML body.
    pub(super) async fn fetch_embed_html(&self, url: &str) -> Result<FetchedHtmlDocument> {
        let http_config = ScraperHttpConfig {
            max_redirects: Some(DEFAULT_MAX_REDIRECTS),
            ..ScraperHttpConfig::default()
        };
        let client = self.scraper_agregator.create_http_client(http_config);
        fetch_html_document(
            &client,
            url,
            &HtmlFetchPolicy {
                max_html_bytes: MAX_EMBED_HTML_BYTES,
                max_js_redirects: Some(DEFAULT_MAX_REDIRECTS),
            },
        )
        .await
    }

    /// Runs optional `can_resolve_html` on one service with `{url, html}` params.
    ///
    /// # Arguments
    /// * `service_name` - Resolver service to query.
    /// * `url` - Current document URL.
    /// * `html` - Fetched document body.
    ///
    /// # Errors
    /// Returns an error when the scraper query fails.
    ///
    /// # Returns
    /// `true` when the service reports that it recognizes the document.
    pub(super) async fn can_resolve_html(
        &self,
        service_name: &str,
        url: &str,
        html: &str,
    ) -> Result<bool> {
        let params = self.build_resolver_params(url, Some(html));
        let results = self
            .scraper_agregator
            .execute_query_async(
                &RequestControlerContext::default(),
                QueryParameters {
                    cache_type: CacheType::NoCache,
                },
                STREAM_RESOLVER_GROUP_NAME,
                CAN_RESOLVE_HTML_QUERY_NAME,
                &params,
                arachnea_scrapyfy::ScraperAggregationOptions {
                    source_params: None,
                    scrapper_list: Some(&vec![service_name.to_string()]),
                    query_media_type_filter: None,
                    fields_filters: None,
                    source_field_name: None,
                    operation: "can_resolve_html",
                },
            )
            .await
            .data;

        Ok(results.into_iter().any(|entry| {
            entry
                .get("resolver")
                .and_then(|node| node.value_as_string())
                .is_some_and(|value| !value.trim().is_empty())
        }))
    }

    /// Runs optional `can_resolve_url` on one service as a cheap direct-resolution prefilter.
    ///
    /// # Arguments
    /// * `service_name` - Resolver service to query.
    /// * `url` - Source URL to test.
    ///
    /// # Errors
    /// Returns an error when the scraper query fails.
    ///
    /// # Returns
    /// `true` when the service reports that it can resolve the URL.
    pub(super) async fn can_resolve_url(&self, service_name: &str, url: &str) -> Result<bool> {
        let params = self.build_resolver_params(url, None);
        let results = self
            .scraper_agregator
            .execute_query_async(
                &RequestControlerContext::default(),
                QueryParameters {
                    cache_type: CacheType::NoCache,
                },
                STREAM_RESOLVER_GROUP_NAME,
                CAN_RESOLVE_URL_QUERY_NAME,
                &params,
                arachnea_scrapyfy::ScraperAggregationOptions {
                    source_params: None,
                    scrapper_list: Some(&vec![service_name.to_string()]),
                    query_media_type_filter: None,
                    fields_filters: None,
                    source_field_name: None,
                    operation: "can_resolve_url",
                },
            )
            .await
            .data;

        Ok(results.into_iter().any(|entry| {
            entry
                .get("resolver")
                .and_then(|node| node.value_as_string())
                .is_some_and(|value| !value.trim().is_empty())
        }))
    }
}
