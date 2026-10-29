//! Request execution through explicit HTTP and proxy engines.

use super::*;

impl ArachneaHttpClient {
    /// Executes a prepared request with the configured engine policy.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns URL, engine, cookie, or Cloudflare detection failures.
    pub(super) async fn execute(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        #[cfg(feature = "arachnea-proxy")]
        if arachnea_proxy::core::ProxySearchContext::current().is_none()
            && matches!(self.config.proxy, HttpProxyConfig::Arachnea(_))
            && self
                .config
                .proxy_parameters
                .iter()
                .any(|(name, value)| name == "countries" && !value.is_empty() && value != "[]")
        {
            let search = arachnea_proxy::core::ProxySearchContext::default()
                .with_replayable(self.config.proxy_retry_replayable);
            return search.scope(Box::pin(self.execute_scoped(options))).await;
        }
        Box::pin(self.execute_scoped(options)).await
    }

    /// Executes one request inside an existing proxy-search scope.
    ///
    /// The per-request mode overrides the configured default mode.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns engine, URL, cookie, or Cloudflare failures from the selected
    /// execution path.
    pub(super) async fn execute_scoped(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        let mode = options.mode.unwrap_or(self.config.default_request_mode);
        match mode {
            HttpRequestMode::Auto => self.execute_auto_request(options).await,
            HttpRequestMode::Direct => self.execute_with_rquest(options).await,
            HttpRequestMode::CloudflareSmart => {
                self.execute_cloudflare_cookie_mode(options, CloudflareRefreshStrategy::Smart)
                    .await
            }
            HttpRequestMode::CloudflareBrowser => {
                self.execute_cloudflare_cookie_mode(options, CloudflareRefreshStrategy::Browser)
                    .await
            }
        }
    }

    /// Executes one request with the automatic fallback chain.
    ///
    /// When a direct engine is configured, this method delegates to it without
    /// Cloudflare fallback. Otherwise the automatic chain starts with
    /// `rquest`, refreshes with the smart solver when available, then falls
    /// back to the browser solver when it is configured and the Cloudflare
    /// block remains.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns URL, engine, cookie, or Cloudflare detection failures.
    pub(super) async fn execute_auto_request(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        let origin = origin_url(&options.url)?;
        if self.direct_engine.is_some() {
            return self.execute_with_direct_engine(options).await;
        }
        let mut response = self.execute_with_rquest(options.clone()).await?;
        let detection =
            detect_cloudflare_block(response.status, &response.headers, response.preview_body());
        if !detection.detected {
            return Ok(response);
        }

        let mut attempted_refresh = false;
        if self.smart_cloudflare_engine.is_some() {
            attempted_refresh = true;
            info!(target: "arachnea_http::client",
                origin = %origin,
                reason = %detection.reason,
                "Cloudflare block detected; refreshing with smart solver"
            );
            match self
                .refresh_cloudflare_for_url_with_strategy(
                    &options.url,
                    CloudflareRefreshStrategy::Smart,
                    true,
                    Some(&options.headers),
                )
                .await
            {
                Ok(()) => {
                    response = self.execute_with_rquest(options.clone()).await?;
                    let retry_detection = detect_cloudflare_block(
                        response.status,
                        &response.headers,
                        response.preview_body(),
                    );
                    if !retry_detection.detected {
                        return Ok(response);
                    }
                    info!(target: "arachnea_http::client",
                        origin = %origin,
                        reason = %retry_detection.reason,
                        "Cloudflare block still present after smart solver"
                    );
                }
                Err(err) if self.browser_cloudflare_engine.is_some() => {
                    warn!(target: "arachnea_http::client",
                        origin = %origin,
                        error = %err,
                        "smart Cloudflare refresh failed; trying browser solver"
                    );
                }
                Err(err) => return Err(err),
            }
        }

        if self.browser_cloudflare_engine.is_some() {
            info!(target: "arachnea_http::client",
                origin = %origin,
                reason = %detection.reason,
                "Cloudflare block detected; refreshing with browser solver"
            );
            self.refresh_cloudflare_for_url_with_strategy(
                &options.url,
                CloudflareRefreshStrategy::Browser,
                false,
                Some(&options.headers),
            )
            .await?;
            response = self.execute_with_rquest(options.clone()).await?;
            let retry_detection = detect_cloudflare_block(
                response.status,
                &response.headers,
                response.preview_body(),
            );
            if !retry_detection.detected {
                return Ok(response);
            }
            self.refresh_cloudflare_for_url_with_strategy(
                &options.url,
                CloudflareRefreshStrategy::Browser,
                true,
                Some(&options.headers),
            )
            .await?;
            response = self.execute_with_rquest(options.clone()).await?;
            let retry_detection = detect_cloudflare_block(
                response.status,
                &response.headers,
                response.preview_body(),
            );
            if !retry_detection.detected {
                return Ok(response);
            }
            return Err(ArachneaHttpError::CloudflareBlocked {
                origin,
                reason: retry_detection.reason,
            });
        }

        if attempted_refresh {
            Err(ArachneaHttpError::CloudflareBlocked {
                origin,
                reason: detection.reason,
            })
        } else {
            Err(ArachneaHttpError::CloudflareSolverUnavailable)
        }
    }

    /// Executes one request with cookie-aware Cloudflare refresh behavior.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    /// - `strategy`: Cloudflare refresh strategy used when cookies are stale or
    ///   a block is detected.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns URL, engine, cookie, or Cloudflare detection failures.
    pub(super) async fn execute_cloudflare_cookie_mode(
        &self,
        options: RequestOptions,
        strategy: CloudflareRefreshStrategy,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        let origin = origin_url(&options.url)?;
        let mut refresh_attempts_used = 0;
        if self
            .cookies
            .write()
            .await
            .needs_refresh(&origin, self.config.cookie_refresh_margin)?
        {
            // The first browser-strategy refresh may consume a persisted
            // session from the store. Retries after a block stay fresh so a
            // rejected session is never reused.
            let fresh = false;
            self.refresh_cloudflare_for_url_with_strategy(
                &options.url,
                strategy,
                fresh,
                Some(&options.headers),
            )
            .await?;
            refresh_attempts_used = 1;
        }
        self.execute_rquest_with_retry(options, &origin, refresh_attempts_used, strategy)
            .await
    }

    /// Executes one request through rquest and optionally retries after Cloudflare refresh.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    /// - `origin`: Origin used for Cloudflare refresh and errors.
    /// - `refresh_attempts_used`: Cloudflare refresh attempts already spent by
    ///   the caller before the first fast-path request.
    /// - `strategy`: Cloudflare refresh strategy used for retries.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns Cloudflare block errors after bounded retry exhaustion.
    pub(super) async fn execute_rquest_with_retry(
        &self,
        options: RequestOptions,
        origin: &str,
        refresh_attempts_used: u8,
        strategy: CloudflareRefreshStrategy,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        let mut response = self.execute_with_rquest(options.clone()).await?;
        let detection =
            detect_cloudflare_block(response.status, &response.headers, response.preview_body());
        if !detection.detected {
            return Ok(response);
        }

        let mut block_reason = detection.reason;
        let first_retry = refresh_attempts_used.min(self.config.max_cloudflare_retries);
        for retry in first_retry..self.config.max_cloudflare_retries {
            warn!(target: "arachnea_http::client",
                origin = %origin,
                retry = retry + 1,
                strategy = strategy.name(),
                reason = %block_reason,
                "Cloudflare block detected; refreshing cookies"
            );
            self.refresh_cloudflare_for_url_with_strategy(
                &options.url,
                strategy,
                true,
                Some(&options.headers),
            )
            .await?;
            response = self.execute_with_rquest(options.clone()).await?;
            let retry_detection = detect_cloudflare_block(
                response.status,
                &response.headers,
                response.preview_body(),
            );
            if !retry_detection.detected {
                return Ok(response);
            }
            warn!(target: "arachnea_http::client",
                origin = %origin,
                retry = retry + 1,
                strategy = strategy.name(),
                reason = %retry_detection.reason,
                "Cloudflare block still present after refresh"
            );
            block_reason = retry_detection.reason;
        }

        Err(ArachneaHttpError::CloudflareBlocked {
            origin: origin.to_string(),
            reason: block_reason,
        })
    }

    /// Executes one request through rquest.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns request execution or cookie storage errors.
    pub(super) async fn execute_with_rquest(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        #[cfg(feature = "arachnea-proxy")]
        if let Some(search) = arachnea_proxy::core::ProxySearchContext::current() {
            use arachnea_proxy::core::{proxy_retry_allowed, ProxyRequestDelivery};
            loop {
                let result = Box::pin(self.execute_with_rquest_once(options.clone())).await;
                match result {
                    Ok(response) => {
                        if let (Some(token), Some(decision)) = (
                            response.proxy_selection(),
                            response.proxy_response_decision(),
                        ) {
                            token.record_response_decision(decision).await;
                            let rejected = matches!(decision, arachnea_proxy::core::http::response_classifier::ProxyResponseDecision::RejectedForDestination { .. });
                            if rejected
                                && search.can_start()
                                && proxy_retry_allowed(
                                    options.method.as_str(),
                                    search.replayable(),
                                    ProxyRequestDelivery::ResponseReceived,
                                    true,
                                )
                            {
                                search.end_attempt();
                                continue;
                            }
                        }
                        #[cfg(feature = "performance-diagnostics")]
                        search.log_progress("response received");
                        return Ok(response);
                    }
                    Err(error) => {
                        if let ArachneaHttpError::ProxyAttempt {
                            selection: Some(token),
                            delivery,
                            ..
                        } = &error
                        {
                            tracing::debug!(target: "arachnea_http::client", search_id = search.id(), proxy = token.endpoint(), delivery = ?delivery,
                                retry_allowed = search.can_start() && proxy_retry_allowed(options.method.as_str(), search.replayable(), *delivery, false),
                                "localized proxy origin attempt failed");
                            if *delivery != ProxyRequestDelivery::ResponseReceived {
                                token
                                    .record_failure(
                                        arachnea_proxy::core::ProxyDestinationFailureReason::Other,
                                    )
                                    .await;
                            }
                            if search.can_start()
                                && proxy_retry_allowed(
                                    options.method.as_str(),
                                    search.replayable(),
                                    *delivery,
                                    false,
                                )
                            {
                                search.end_attempt();
                                #[cfg(feature = "performance-diagnostics")]
                                search.log_progress("retry after origin failure");
                                continue;
                            }
                        }
                        #[cfg(feature = "performance-diagnostics")]
                        search.log_progress("request failed");
                        tracing::warn!(target: "arachnea_http::client",
                            search_id = search.id(),
                            origin = ?url::Url::parse(&options.url).ok().map(|url| url.origin().ascii_serialization()),
                            method = %options.method,
                            delivery = ?match &error {
                                ArachneaHttpError::ProxyAttempt { delivery, .. } => Some(delivery),
                                _ => None,
                            },
                            "localized proxy request failed after retries or replay refusal"
                        );
                        return Err(error);
                    }
                }
            }
        }
        let response = Box::pin(self.execute_with_rquest_once(options)).await?;
        #[cfg(feature = "arachnea-proxy")]
        if let (Some(token), Some(decision)) = (
            response.proxy_selection(),
            response.proxy_response_decision(),
        ) {
            token.record_response_decision(decision).await;
        }
        Ok(response)
    }

    /// Executes one logical request through `rquest`, following configured redirects.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// The final materialized response.
    ///
    /// # Errors
    ///
    /// Returns request, redirect, cookie, or proxy failures.
    pub(super) async fn execute_with_rquest_once(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        self.execute_with_redirects(
            options,
            "rquest",
            |options, requested_url, redirect_urls| async move {
                self.execute_one_with_rquest(options, requested_url, redirect_urls)
                    .await
            },
        )
        .await
    }

    /// Executes one request through rquest without following redirects.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    /// - `requested_url`: Original URL before the redirect chain.
    /// - `redirect_urls`: Redirect targets already followed.
    ///
    /// # Returns
    ///
    /// One materialized response.
    ///
    /// # Errors
    ///
    /// Returns request execution or cookie storage errors.
    pub(super) async fn execute_one_with_rquest(
        &self,
        options: RequestOptions,
        requested_url: String,
        redirect_urls: Vec<String>,
    ) -> Result<StepResponse, ArachneaHttpError> {
        #[cfg(feature = "arachnea-proxy")]
        if let Some(search) = arachnea_proxy::core::ProxySearchContext::current() {
            return search
                .for_url(&options.url)
                .scope(Box::pin(self.execute_one_with_rquest_scoped(
                    options,
                    requested_url,
                    redirect_urls,
                )))
                .await;
        }
        self.execute_one_with_rquest_scoped(options, requested_url, redirect_urls)
            .await
    }

    /// Executes one `rquest` hop with an already established proxy-search scope.
    ///
    /// When the `arachnea-proxy` feature is disabled, the redirect context is
    /// ignored. This method does not follow redirects.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared options for this hop.
    /// - `requested_url`: Original URL before the redirect chain.
    /// - `redirect_urls`: Redirect targets already followed.
    ///
    /// # Returns
    ///
    /// One response step, including any transport retained for a followed redirect.
    ///
    /// # Errors
    ///
    /// Returns request, cookie, or proxy failures.
    pub(super) async fn execute_one_with_rquest_scoped(
        &self,
        options: RequestOptions,
        requested_url: String,
        redirect_urls: Vec<String>,
    ) -> Result<StepResponse, ArachneaHttpError> {
        #[cfg(not(feature = "arachnea-proxy"))]
        let _ = (&requested_url, &redirect_urls);
        debug!(target: "arachnea_http::client", engine = "rquest", method = %options.method, url = %options.url, "sending request");
        let request = self.engine_request(options).await?;
        #[cfg(feature = "arachnea-proxy")]
        if let HttpProxyConfig::Arachnea(core) = &self.config.proxy {
            use arachnea_proxy::core::{ApplicationProtocol, ConnectRequest, Destination};
            let url = url::Url::parse(&request.url)
                .map_err(|error| ArachneaHttpError::InvalidUrl(error.to_string()))?;
            let destination = Destination::host_port(
                url.host_str()
                    .ok_or_else(|| ArachneaHttpError::InvalidUrl("missing host".into()))?,
                url.port_or_known_default()
                    .ok_or_else(|| ArachneaHttpError::InvalidUrl("missing port".into()))?,
            )
            .with_protocol(if url.scheme() == "https" {
                ApplicationProtocol::Https
            } else {
                ApplicationProtocol::Http
            });
            let context = arachnea_proxy::core::context_from_parameter_pairs(
                self.config
                    .proxy_parameters
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
                &core.parameter_definitions(),
            );
            let selected =
                Box::pin(core.select_for_request(
                    ConnectRequest::new(destination).with_client_context(context),
                ))
                .await
                .map_err(|error| ArachneaHttpError::ProxyAttempt {
                    selection: None,
                    delivery: arachnea_proxy::core::ProxyRequestDelivery::NotSent,
                    source: Box::new(ArachneaHttpError::Proxy(error.to_string())),
                })?;
            let connect_timeout = arachnea_proxy::core::ProxySearchContext::current().map(|_| {
                if selected
                    .selection_token
                    .as_ref()
                    .is_some_and(|token| token.reuses_destination())
                {
                    crate::HTTP_CLIENT_REUSED_ROUTE_CONNECT_TIMEOUT
                } else {
                    self.config
                        .request_timeout
                        .min(crate::HTTP_CLIENT_NEW_ROUTE_CONNECT_TIMEOUT)
                }
            });
            let key = format!(
                "{}|{}|{}|{:?}|{:?}|{:?}|{:?}|{:p}",
                url.origin().ascii_serialization(),
                selected
                    .selection_token
                    .as_ref()
                    .map(|token| token.endpoint())
                    .unwrap_or("static"),
                selected.route_generation,
                self.config.user_agent_profile,
                self.config.proxy_parameters,
                self.config.request_timeout,
                connect_timeout,
                Arc::as_ptr(
                    &self
                        ._proxy_runtime
                        .loopback_proxy
                        .as_ref()
                        .expect("prepared proxy runtime")
                        .runtime()
                ),
            );
            #[cfg(feature = "performance-diagnostics")]
            let checkout_started = std::time::Instant::now();
            let (cached, _expired, _idle_remaining) =
                self._proxy_runtime.pinned_clients.checkout(&key).await;
            #[cfg(feature = "performance-diagnostics")]
            tracing::debug!(target: "arachnea_http::client",
                search_id = arachnea_proxy::core::ProxySearchContext::current().map(|search| search.id()),
                destination = %url.origin().ascii_serialization(),
                reused = cached.is_some(),
                idle_ms = cached.as_ref().map(|transport| transport.idle_since.elapsed().as_millis()),
                expired = _expired,
                idle_remaining = _idle_remaining,
                checkout_us = checkout_started.elapsed().as_micros(),
                "checked localized proxy HTTP transport pool"
            );
            #[cfg(feature = "performance-diagnostics")]
            let preparation_started = std::time::Instant::now();
            let mut transport = if let Some(mut transport) = cached {
                transport
                    .loopback
                    .rebind(selected.core.clone())
                    .map_err(|error| ArachneaHttpError::ProxyAttempt {
                        selection: selected.selection_token.clone().map(Box::new),
                        delivery: arachnea_proxy::core::ProxyRequestDelivery::NotSent,
                        source: Box::new(ArachneaHttpError::Proxy(error.to_string())),
                    })?;
                tracing::debug!(target: "arachnea_http::client", search_id = arachnea_proxy::core::ProxySearchContext::current().map(|search| search.id()),
                    destination = %url.origin().ascii_serialization(), "reusing selected proxy HTTP transport");
                transport
            } else {
                let loopback = ArachneaRquestLoopback::start_on_runtime(
                    self._proxy_runtime
                        .loopback_proxy
                        .as_ref()
                        .expect("prepared proxy runtime")
                        .runtime(),
                    selected.core.clone(),
                    selected.core.parameter_definitions(),
                    arachnea_proxy::core::ClientContext::new(),
                    false,
                )
                .await
                .map_err(|error| ArachneaHttpError::ProxyAttempt {
                    selection: selected.selection_token.clone().map(Box::new),
                    delivery: arachnea_proxy::core::ProxyRequestDelivery::NotSent,
                    source: Box::new(ArachneaHttpError::Proxy(error.to_string())),
                })?;
                let engine = RquestEngine::new(
                    &self.config,
                    Some(loopback.proxy_url()),
                    Some(
                        loopback
                            .client_with_connect_timeout(connect_timeout)
                            .map_err(|error| ArachneaHttpError::Proxy(error.to_string()))?,
                    ),
                )?;
                IdleProxyTransport {
                    loopback,
                    engine,
                    idle_since: std::time::Instant::now(),
                }
            };
            #[cfg(feature = "performance-diagnostics")]
            tracing::debug!(target: "arachnea_http::client",
                search_id = arachnea_proxy::core::ProxySearchContext::current().map(|search| search.id()),
                destination = %url.origin().ascii_serialization(),
                duration_us = preparation_started.elapsed().as_micros(),
                "prepared localized proxy HTTP transport"
            );
            if let (Some(search), Some(token)) = (
                arachnea_proxy::core::ProxySearchContext::current(),
                selected.selection_token.as_ref(),
            ) {
                tracing::debug!(target: "arachnea_http::client", search_id = search.id(), proxy = token.endpoint(),
                    destination = %token.destination(), method = %request.method,
                    "trying localized proxy origin request");
                #[cfg(feature = "performance-diagnostics")]
                search.log_progress("origin attempt started");
            }
            let (response, decision, allow_redirect_transport) = transport
                .engine
                .send_for_proxy(
                    request,
                    selected.selection_token.clone(),
                    self.config.proxy_response_classifier.as_ref(),
                    &requested_url,
                    &redirect_urls,
                )
                .await?;
            transport.idle_since = std::time::Instant::now();
            let pending = if matches!(
                decision,
                arachnea_proxy::core::http::response_classifier::ProxyResponseDecision::Accepted
            ) {
                self._proxy_runtime
                    .pinned_clients
                    .retain(key, transport)
                    .await;
                None
            } else if allow_redirect_transport {
                Some(Box::new(PendingRedirectTransport {
                    pool: self._proxy_runtime.pinned_clients.clone(),
                    key,
                    transport,
                }))
            } else {
                None
            };
            self.store_response_cookies(&response.url, &response.headers)
                .await?;
            let mut response = ArachneaResponse::from_engine(response);
            response.proxy_selection = selected.selection_token;
            response.proxy_response_decision = Some(decision);
            return Ok(StepResponse {
                response,
                redirect_transport: pending,
            });
        }
        let response = if let Some(rquest) = self.rquest.as_ref() {
            rquest.send(request).await?
        } else {
            RquestEngine::new(
                &self.config,
                self._proxy_runtime.proxy_url(&self.config),
                self._proxy_runtime.rquest_client(&self.config)?,
            )?
            .send(request)
            .await?
        };
        self.store_response_cookies(&response.url, &response.headers)
            .await?;
        Ok(StepResponse::new(ArachneaResponse::from_engine(response)))
    }

    /// Executes one request through the configured direct engine.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns request execution or cookie storage errors.
    pub(super) async fn execute_with_direct_engine(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        self.execute_with_redirects(options, "direct", |options, _, _| async move {
            self.execute_one_with_direct_engine(options)
                .await
                .map(StepResponse::new)
        })
        .await
    }

    /// Executes one request through the configured direct engine without following redirects.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// One materialized response.
    ///
    /// # Errors
    ///
    /// Returns request execution or cookie storage errors.
    pub(super) async fn execute_one_with_direct_engine(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        #[cfg(feature = "arachnea-proxy")]
        if self.config.proxy_response_classifier.is_some() {
            tracing::warn!(target: "arachnea_http::client", "selected HTTP engine does not support correlated early proxy response classification; no preference or exclusion will be inferred");
        }
        let engine = self
            .direct_engine
            .as_ref()
            .ok_or(ArachneaHttpError::CloudflareSolverUnavailable)?;
        debug!(target: "arachnea_http::client", engine = engine.name(), method = %options.method, url = %options.url, "sending request");
        let request = self.engine_request(options).await?;
        #[cfg(feature = "arachnea-proxy")]
        if arachnea_proxy::core::ProxySearchContext::current().is_some() {
            return Err(ArachneaHttpError::UnsupportedEngineOperation {
                engine: engine.name(),
                operation: "correlated localized proxy search",
            });
        }
        let response = engine.send(request).await?;
        self.store_response_cookies(&response.url, &response.headers)
            .await?;
        Ok(ArachneaResponse::from_engine(response))
    }
}
