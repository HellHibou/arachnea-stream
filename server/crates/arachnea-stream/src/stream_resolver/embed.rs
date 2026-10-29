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

    /// Fetches an embed page once for HTML-content resolver detection.
    ///
    /// The returned URL is the final HTTP redirect target, or the final simple JavaScript
    /// redirect target when one was followed.
    ///
    /// Follows HTTP redirects (handled by the HTTP client, bounded by
    /// `DEFAULT_MAX_REDIRECTS`) and additionally follows a **simple** JavaScript
    /// redirect (`window.location.href = "…"` string-literal assignment) so content-based
    /// detection operates on the final document instead of a redirecting shim. JavaScript is
    /// never executed: only the target of the first plain literal assignment is extracted and
    /// re-fetched via HTTP. The number of followed JS hops is bounded and each hop is validated
    /// like the initial fetch (success status, `text/html`, size limit).
    ///
    /// # Arguments
    /// * `url` - Initial embed URL.
    ///
    /// # Errors
    /// Returns an error on HTTP failure, unsupported content type, oversized body
    /// or excessive JavaScript redirects.
    ///
    /// # Returns
    /// The final document URL and its HTML body.
    pub(super) async fn fetch_embed_html(&self, url: &str) -> Result<FetchedEmbedHtml> {
        let http_config = ScraperHttpConfig {
            max_redirects: Some(DEFAULT_MAX_REDIRECTS),
            ..ScraperHttpConfig::default()
        };
        let client = self.scraper_agregator.create_http_client(http_config);

        let mut current_url = url.to_string();
        let mut visited: Vec<String> = Vec::new();

        for _ in 0..=DEFAULT_MAX_REDIRECTS {
            if visited.iter().any(|value| value == &current_url) {
                break;
            }
            visited.push(current_url.clone());

            let response = client
                .send_for_request(http::Method::GET, &current_url, &HashMap::new(), None)
                .await
                .with_context(|| format!("Failed to fetch embed HTML `{}`", current_url))?;
            let response_url = response.url().to_string();

            if !response.status().is_success() {
                bail!(
                    "Failed to fetch embed HTML `{}`: HTTP status {}",
                    current_url,
                    response.status()
                );
            }

            let content_type = response
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            let mime_type = content_type
                .split(';')
                .next()
                .map(str::trim)
                .unwrap_or_default();

            if !mime_type.eq_ignore_ascii_case("text/html") {
                bail!(
                    "Failed to inspect embed HTML `{}`: unsupported Content-Type `{}`",
                    current_url,
                    content_type
                );
            }

            let bytes = response.bytes().await?;
            if bytes.len() > MAX_EMBED_HTML_BYTES {
                bail!(
                    "Failed to inspect embed HTML `{}`: body is {} bytes, limit is {} bytes",
                    current_url,
                    bytes.len(),
                    MAX_EMBED_HTML_BYTES
                );
            }

            let body = response
                .text()
                .await
                .with_context(|| format!("Failed to read embed HTML `{}`", current_url))?;

            if let Some(target) = extract_simple_js_redirect_target(&body) {
                if let Some(resolved) = resolve_redirect_target(&response_url, &target) {
                    if resolved != response_url {
                        tracing::debug!(target: "arachnea_stream::stream_resolver",
                            current = %response_url,
                            redirect = %resolved,
                            "Following simple JS window.location.href redirect while fetching embed HTML"
                        );
                        current_url = resolved;
                        continue;
                    }
                }
            }

            return Ok(FetchedEmbedHtml {
                url: response_url,
                html: body,
            });
        }

        bail!(
            "Failed to fetch embed HTML `{}`: too many JS redirects",
            url
        );
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
