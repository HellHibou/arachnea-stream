//! TCP and HTTP connection orchestration across proxy chains.

use crate::core::transport::{http, tls};
use crate::core::{
    ApplicationProtocol, ConnectRequest, Destination, HttpRequestStream, HttpRequestTargetForm,
    ProxyChain, ProxyError, ProxyNameResolutionMode, ProxyNode, ProxyStream, Result, TransportKind,
};

use super::hops::endpoint_host;
use super::request_routing::is_dynamic_proxy_transport_failure;
use super::ArachneaProxyCore;

/// Logs a connect_chain iteration at DEBUG level.
///
/// # Arguments
///
/// - `chain`: Chain being traversed.
/// - `destination`: Final destination.
/// - `attempt`: Current attempt number (0-based).
fn log_connect_chain_iteration(chain: &ProxyChain, destination: &Destination, attempt: usize) {
    let hops: Vec<&str> = chain
        .nodes
        .iter()
        .filter(|n| n.kind != TransportKind::Direct)
        .map(|n| n.name.as_str())
        .collect();
    tracing::debug!(
        chain = %chain.name,
        destination = %destination.authority(),
        attempt = %attempt,
        hops = %hops.join(" -> "),
        "connecting through proxy chain"
    );
}

/// Loads credentials for the final HTTP proxy in a chain.
///
/// # Arguments
///
/// - `chain`: Chain selected for an HTTP request.
///
/// # Returns
///
/// Optional credentials for the last HTTP or HTTPS proxy hop.
///
/// # Errors
///
/// Returns an error when credentials are configured but cannot be loaded.
fn selected_http_proxy_credentials(
    chain: &ProxyChain,
) -> Result<Option<crate::core::ProxyNodeCredentials>> {
    chain
        .nodes
        .iter()
        .rev()
        .find(|node| {
            matches!(
                node.kind,
                TransportKind::HttpProxy | TransportKind::HttpsProxy
            )
        })
        .map(ProxyNode::credentials)
        .unwrap_or(Ok(None))
}

impl ArachneaProxyCore {
    /// Opens a stream to a destination using route policy.
    ///
    /// # Arguments
    ///
    /// - `destination`: Target destination requested by the caller.
    ///
    /// # Returns
    ///
    /// Connected proxy stream.
    ///
    /// # Errors
    ///
    /// Returns an error when policy validation, route selection, or connection
    /// setup fails.
    pub async fn connect(&self, destination: Destination) -> Result<ProxyStream> {
        self.connect_request(ConnectRequest::new(destination)).await
    }

    /// Opens a stream using a full connect request.
    ///
    /// # Arguments
    ///
    /// - `request`: Destination and client context for route selection.
    ///
    /// # Returns
    ///
    /// Connected proxy stream.
    ///
    /// # Errors
    ///
    /// Returns an error when policy validation, routing, or transport setup
    /// fails.
    pub fn connect_request(
        &self,
        request: ConnectRequest,
    ) -> futures::future::BoxFuture<'_, Result<ProxyStream>> {
        Box::pin(async move {
            let search = crate::core::ProxySearchContext::current()
                .or(self.search_context.clone())
                .or_else(|| {
                    self.proxy_inventory.as_ref().map(|_| {
                        let mut limits = crate::core::ProxySearchLimits::default();
                        limits.extension = std::time::Duration::ZERO;
                        crate::core::ProxySearchContext::new(limits)
                    })
                });
            if let Some(search) = search {
                search
                    .scope(search.origin(self.connect_request_inner(request)))
                    .await?
            } else {
                self.connect_request_inner(request).await
            }
        })
    }

    /// Applies security and routing policy before opening the requested TCP transport.
    ///
    /// # Arguments
    /// * `request` - Outbound request, including its destination and routing options.
    fn connect_request_inner(
        &self,
        request: ConnectRequest,
    ) -> futures::future::BoxFuture<'_, Result<ProxyStream>> {
        Box::pin(async move {
            self.validate_pinned_destination(&request.destination)?;
            self.config
                .as_config()
                .security
                .validate_destination(&request.destination)?;
            let decision = self
                .config
                .as_config()
                .routing
                .decide(&self.config, &request)?;
            let chain = self.apply_parameter_handlers(&decision.chain, &request)?;
            let retry_budget = if self.pinned_route.is_some() {
                Some(0)
            } else {
                self.proxy_pool_retry_budget(&chain)
            };

            let mut attempt = 0usize;
            loop {
                let resolved_chain = if let Some(route) = &self.pinned_route {
                    route.1.clone()
                } else {
                    self.resolve_proxy_pools_in_chain(&chain, &request).await?
                };
                match self.connect_chain(&resolved_chain.chain, &request).await {
                    Ok(mut stream) => {
                        stream.metadata_mut().route_name = Some(decision.selected_chain);
                        stream.metadata_mut().selection_token =
                            self.pinned_route.as_ref().and_then(|route| route.2.clone());
                        self.stats.record_opened();
                        return Ok(stream);
                    }
                    Err(error)
                        if retry_budget.map_or(true, |limit| attempt < limit)
                            && !resolved_chain.pool_selections.is_empty() =>
                    {
                        let proxy_names: Vec<&str> = resolved_chain
                            .pool_selections
                            .iter()
                            .map(|s| s.upstream.as_str())
                            .collect();
                        tracing::debug!(
                            attempt = %attempt,
                            max_attempts = ?retry_budget,
                            failed_proxies = %proxy_names.join(", "),
                            "dynamic proxy selection failed, marking failed and retrying with another candidate"
                        );
                        self.mark_proxy_pool_selections_ko(
                            &resolved_chain.pool_selections,
                            &request.destination,
                            &error,
                        )
                        .await;
                        attempt += 1;
                        continue;
                    }
                    Err(error) => {
                        if let Some(token) = self
                            .pinned_route
                            .as_ref()
                            .and_then(|route| route.2.as_ref())
                        {
                            token.record_transport_failure(&error).await;
                        }
                        self.stats.record_failed();
                        return Err(error);
                    }
                }
            }
        })
    }

    /// Opens a stream for relaying one explicit HTTP request.
    ///
    /// When the selected route ends with an HTTP or HTTPS proxy and the target
    /// protocol is plain HTTP, this method connects to that proxy and asks the
    /// caller to send an absolute-form request target. Other routes connect to
    /// the final destination and expect origin-form request targets.
    ///
    /// # Arguments
    ///
    /// - `request`: HTTP destination and client context for route selection.
    ///
    /// # Returns
    ///
    /// Connected stream plus the HTTP request-target form expected by the next
    /// peer.
    ///
    /// # Errors
    ///
    /// Returns an error when policy validation, routing, or connection setup
    /// fails.
    pub fn connect_http_request(
        &self,
        request: ConnectRequest,
    ) -> futures::future::BoxFuture<'_, Result<HttpRequestStream>> {
        Box::pin(async move {
            let search = crate::core::ProxySearchContext::current()
                .or(self.search_context.clone())
                .or_else(|| {
                    self.proxy_inventory.as_ref().map(|_| {
                        let mut limits = crate::core::ProxySearchLimits::default();
                        limits.extension = std::time::Duration::ZERO;
                        crate::core::ProxySearchContext::new(limits)
                    })
                });
            if let Some(search) = search {
                search
                    .scope(search.origin(self.connect_http_request_inner(request)))
                    .await?
            } else {
                self.connect_http_request_inner(request).await
            }
        })
    }

    /// Applies security and routing policy before opening an HTTP-capable transport.
    ///
    /// # Arguments
    /// * `request` - Outbound request, including its destination and routing options.
    fn connect_http_request_inner(
        &self,
        request: ConnectRequest,
    ) -> futures::future::BoxFuture<'_, Result<HttpRequestStream>> {
        Box::pin(async move {
            self.validate_pinned_destination(&request.destination)?;
            self.config
                .as_config()
                .security
                .validate_destination(&request.destination)?;
            let decision = self
                .config
                .as_config()
                .routing
                .decide(&self.config, &request)?;
            let chain = self.apply_parameter_handlers(&decision.chain, &request)?;
            let retry_budget = if self.pinned_route.is_some() {
                Some(0)
            } else {
                self.proxy_pool_retry_budget(&chain)
            };

            let mut attempt = 0usize;
            loop {
                let resolved_chain = if let Some(route) = &self.pinned_route {
                    route.1.clone()
                } else {
                    self.resolve_proxy_pools_in_chain(&chain, &request).await?
                };
                let use_absolute_form = request.destination.protocol == ApplicationProtocol::Http
                    && resolved_chain
                        .chain
                        .nodes
                        .iter()
                        .rev()
                        .find(|node| node.kind != TransportKind::Direct)
                        .is_some_and(|node| {
                            matches!(
                                node.kind,
                                TransportKind::HttpProxy | TransportKind::HttpsProxy
                            )
                        });

                let result = if use_absolute_form {
                    self.connect_chain_to_http_proxy(&resolved_chain.chain)
                        .await
                } else {
                    self.connect_chain(&resolved_chain.chain, &request).await
                };

                let proxy_authorization = if use_absolute_form {
                    selected_http_proxy_credentials(&resolved_chain.chain)?.map(|credentials| {
                        format!("Basic {}", http::basic_authorization_value(&credentials))
                    })
                } else {
                    None
                };

                match result {
                    Ok(mut stream) => {
                        stream.metadata_mut().route_name = Some(decision.selected_chain);
                        stream.metadata_mut().selection_token =
                            self.pinned_route.as_ref().and_then(|route| route.2.clone());
                        self.stats.record_opened();
                        return Ok(HttpRequestStream {
                            stream,
                            target_form: if use_absolute_form {
                                HttpRequestTargetForm::AbsoluteForm
                            } else {
                                HttpRequestTargetForm::OriginForm
                            },
                            proxy_authorization,
                        });
                    }
                    Err(error)
                        if retry_budget.map_or(true, |limit| attempt < limit)
                            && !resolved_chain.pool_selections.is_empty() =>
                    {
                        let proxy_names: Vec<&str> = resolved_chain
                            .pool_selections
                            .iter()
                            .map(|s| s.upstream.as_str())
                            .collect();
                        tracing::debug!(
                            attempt = %attempt,
                            max_attempts = ?retry_budget,
                            failed_proxies = %proxy_names.join(", "),
                            "dynamic proxy selection failed, marking failed and retrying with another candidate"
                        );
                        self.mark_proxy_pool_selections_ko(
                            &resolved_chain.pool_selections,
                            &request.destination,
                            &error,
                        )
                        .await;
                        attempt += 1;
                        continue;
                    }
                    Err(error) => {
                        if let Some(token) = self
                            .pinned_route
                            .as_ref()
                            .and_then(|route| route.2.as_ref())
                        {
                            token.record_transport_failure(&error).await;
                        }
                        self.stats.record_failed();
                        return Err(error);
                    }
                }
            }
        })
    }

    /// Connects a request through a selected proxy chain.
    ///
    /// # Arguments
    ///
    /// - `chain`: Resolved proxy chain selected by routing.
    /// - `request`: Original connect request and client context.
    ///
    /// # Returns
    ///
    /// A connected proxy stream for the destination.
    ///
    /// # Errors
    ///
    /// Returns an error when any hop is invalid, unsupported, times out, or
    /// rejects the tunnel.
    pub(super) async fn connect_chain(
        &self,
        chain: &ProxyChain,
        request: &ConnectRequest,
    ) -> Result<ProxyStream> {
        log_connect_chain_iteration(chain, &request.destination, 0);
        if crate::core::ProxySearchContext::current().is_some()
            && (request.client_context.get_string("countries").is_some()
                || self
                    .pinned_route
                    .as_ref()
                    .is_some_and(|route| route.2.is_some()))
        {
            return self.connect_chain_once(chain, request).await;
        }
        let retry_budget = chain
            .nodes
            .iter()
            .filter(|node| {
                matches!(node.kind, TransportKind::Socks5 | TransportKind::TorSocks)
                    && node.effective_name_resolution()
                        == ProxyNameResolutionMode::ProxyThenLocalFallback
            })
            .count();

        for attempt in 0..=retry_budget {
            match self.connect_chain_once(chain, request).await {
                Ok(stream) => return Ok(stream),
                Err(error) if attempt < retry_budget => {
                    if self
                        .flag_socks5_local_dns_fallback(chain, &request.destination, &error)
                        .await
                    {
                        continue;
                    }
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }

        Err(ProxyError::RouteUnavailable(
            "socks5 dns fallback retry budget exhausted".to_string(),
        ))
    }

    /// Opens a stream through a selected proxy chain once.
    ///
    /// # Arguments
    ///
    /// - `chain`: Resolved proxy chain selected by routing.
    /// - `request`: Original connect request and client context.
    ///
    /// # Returns
    ///
    /// A connected proxy stream for the destination.
    ///
    /// # Errors
    ///
    /// Returns an error from the first failed hop without applying retry logic.
    async fn connect_chain_once(
        &self,
        chain: &ProxyChain,
        request: &ConnectRequest,
    ) -> Result<ProxyStream> {
        let proxy_nodes: Vec<_> = chain
            .nodes
            .iter()
            .filter(|node| node.kind != TransportKind::Direct)
            .cloned()
            .collect();

        if proxy_nodes.is_empty() {
            let destination = self.resolve_destination_to_ip(&request.destination).await?;
            return crate::core::transport::direct::connect_tcp(
                &destination,
                self.config.as_config().timeouts.connect_duration(),
            )
            .await;
        }

        let first = &proxy_nodes[0];
        let mut stream = match self.connect_first_proxy(first, chain).await {
            Ok(stream) => stream,
            Err(error) => {
                if let Some(token) = self
                    .pinned_route
                    .as_ref()
                    .and_then(|route| route.2.as_ref())
                {
                    if first.endpoint.as_deref() == Some(token.endpoint())
                        && is_dynamic_proxy_transport_failure(&error)
                    {
                        if let Some(inventory) = &self.proxy_inventory {
                            inventory.record_global_failure(token.endpoint()).await;
                        }
                    }
                }
                return Err(error);
            }
        };
        if let Some(token) = self
            .pinned_route
            .as_ref()
            .and_then(|route| route.2.as_ref())
        {
            if first.endpoint.as_deref() == Some(token.endpoint()) {
                token
                    .record_phase(
                        crate::core::ProxyObservationPhase::EndpointConnect,
                        true,
                        None,
                    )
                    .await;
            }
        }

        for (index, node) in proxy_nodes.iter().enumerate() {
            if node.kind == TransportKind::HttpsProxy {
                let endpoint = node.endpoint.as_deref().ok_or_else(|| {
                    ProxyError::Config(format!("node '{}' has no endpoint", node.name))
                })?;
                let server_name = endpoint_host(endpoint)?;
                let metadata = stream.metadata().clone();
                let tls_stream = tls::client_tls(
                    stream,
                    server_name,
                    self.proxy_timeouts(node).handshake_duration(),
                    node.verify_tls,
                )
                .await
                .map_err(|error| error.at_phase(crate::core::ProxyObservationPhase::ProxyTls))?;
                stream = ProxyStream::new(tls_stream, metadata);
                if let Some(token) = self
                    .pinned_route
                    .as_ref()
                    .and_then(|route| route.2.as_ref())
                {
                    if node.endpoint.as_deref() == Some(token.endpoint()) {
                        token
                            .record_phase(crate::core::ProxyObservationPhase::ProxyTls, true, None)
                            .await;
                    }
                }
            }

            let target = if let Some(next) = proxy_nodes.get(index + 1) {
                let endpoint = next.endpoint.as_deref().ok_or_else(|| {
                    ProxyError::Config(format!("node '{}' has no endpoint", next.name))
                })?;
                Destination::from_authority(endpoint, crate::core::ApplicationProtocol::Tcp)?
            } else {
                request.destination.clone()
            };

            self.connect_proxy_hop(&mut stream, node, &target)
                .await
                .map_err(|error| error.at_phase(crate::core::ProxyObservationPhase::Tunnel))?;
            stream.metadata_mut().hops.push(node.name.clone());
            if let Some(token) = self
                .pinned_route
                .as_ref()
                .and_then(|route| route.2.as_ref())
            {
                if node.endpoint.as_deref() == Some(token.endpoint()) {
                    token
                        .record_phase(
                            crate::core::ProxyObservationPhase::ProxyProtocol,
                            true,
                            None,
                        )
                        .await;
                    token
                        .record_phase(crate::core::ProxyObservationPhase::Tunnel, true, None)
                        .await;
                }
            }
            stream.metadata_mut().transport_kind = Some(node.kind.clone());
        }

        Ok(stream)
    }

    /// Connects through a chain up to its final HTTP proxy hop.
    ///
    /// # Arguments
    ///
    /// - `chain`: Chain whose last non-direct hop must be an HTTP or HTTPS proxy.
    ///
    /// # Returns
    ///
    /// A stream connected to the final HTTP proxy, ready for absolute-form
    /// request forwarding.
    ///
    /// # Errors
    ///
    /// Returns an error when the chain has no HTTP proxy endpoint, contains
    /// unsupported hops, or a hop connection fails.
    async fn connect_chain_to_http_proxy(&self, chain: &ProxyChain) -> Result<ProxyStream> {
        let proxy_nodes: Vec<_> = chain
            .nodes
            .iter()
            .filter(|node| node.kind != TransportKind::Direct)
            .cloned()
            .collect();
        let final_index = proxy_nodes
            .iter()
            .rposition(|node| {
                matches!(
                    node.kind,
                    TransportKind::HttpProxy | TransportKind::HttpsProxy
                )
            })
            .ok_or_else(|| {
                ProxyError::RouteUnavailable(format!(
                    "chain '{}' does not end at an http proxy",
                    chain.name
                ))
            })?;
        if final_index + 1 != proxy_nodes.len() {
            return Err(ProxyError::RouteUnavailable(format!(
                "chain '{}' has non-http nodes after its http proxy",
                chain.name
            )));
        }

        let first = &proxy_nodes[0];
        let mut stream = self.connect_first_proxy(first, chain).await?;

        for (index, node) in proxy_nodes.iter().enumerate() {
            if index == final_index {
                if node.kind == TransportKind::HttpsProxy {
                    stream = self.wrap_https_proxy_stream(stream, node).await?;
                }
                stream.metadata_mut().hops.push(node.name.clone());
                stream.metadata_mut().transport_kind = Some(node.kind.clone());
                return Ok(stream);
            }

            if node.kind == TransportKind::HttpsProxy {
                stream = self.wrap_https_proxy_stream(stream, node).await?;
            }

            let next = &proxy_nodes[index + 1];
            let endpoint = next.endpoint.as_deref().ok_or_else(|| {
                ProxyError::Config(format!("node '{}' has no endpoint", next.name))
            })?;
            let target = Destination::from_authority(endpoint, ApplicationProtocol::Tcp)?;
            self.connect_proxy_hop(&mut stream, node, &target)
                .await
                .map_err(|error| error.at_phase(crate::core::ProxyObservationPhase::Tunnel))?;
            stream.metadata_mut().hops.push(node.name.clone());
            stream.metadata_mut().transport_kind = Some(node.kind.clone());
        }

        Err(ProxyError::RouteUnavailable(format!(
            "chain '{}' did not produce an http proxy stream",
            chain.name
        )))
    }
}
