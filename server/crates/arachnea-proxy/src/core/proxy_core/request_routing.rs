//! Request routing, pinned selections and dynamic proxy observations.

use crate::core::{ConnectRequest, Destination, ProxyChain, ProxyError, Result};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::dynamic_pools::dynamic_pool_countries;
use super::ArachneaProxyCore;

/// Returns whether a dynamic proxy failed before a usable tunnel was established.
///
/// Only a directly attributed endpoint connection failure establishes global
/// unavailability. Opaque I/O, TLS and timeout errors are insufficient evidence.
pub(super) fn is_dynamic_proxy_transport_failure(error: &ProxyError) -> bool {
    matches!(
        error,
        ProxyError::Observation {
            phase: crate::core::ProxyObservationPhase::EndpointConnect,
            ..
        }
    )
}

impl ArachneaProxyCore {
    /// Sets the default behavior when no dynamic country proxy is available.
    ///
    /// # Arguments
    /// - `fallback_direct`: Whether to connect without any remote proxy.
    ///
    /// Request-local `fallback_direct` overrides this default. Privacy policies
    /// that forbid direct fallback always take precedence.
    pub fn with_fallback_direct(mut self, fallback_direct: bool) -> Self {
        self.fallback_direct = fallback_direct;
        self
    }

    pub(super) fn fallback_direct_for_request(&self, request: &ConnectRequest) -> Result<bool> {
        use crate::core::{ClientParameter, PROXY_PARAMETER_FALLBACK_DIRECT};
        let enabled = match request.client_context.get(PROXY_PARAMETER_FALLBACK_DIRECT) {
            None => self.fallback_direct,
            Some(ClientParameter::Bool(value)) => *value,
            Some(ClientParameter::String(value)) => value
                .parse::<bool>()
                .map_err(|_| ProxyError::Config("fallback_direct must be true or false".into()))?,
            Some(_) => {
                return Err(ProxyError::Config(
                    "fallback_direct must be a boolean".into(),
                ))
            }
        };
        let config = self.config.as_config();
        Ok(enabled
            && config.routing.allow_direct_fallback
            && !config.privacy.prevent_direct_fallback)
    }

    /// Attaches a request-owned search to adapters executing on another Tokio task.
    ///
    /// # Arguments
    /// - `search`: Shared logical request budget, including redirects and retries.
    pub fn with_proxy_search(&self, search: crate::core::ProxySearchContext) -> Self {
        let mut core = self.clone();
        core.search_context = Some(search);
        core
    }

    /// Remaining establishment time for adapters that execute on a separate task.
    pub fn proxy_search_remaining(&self) -> Option<std::time::Duration> {
        self.search_context
            .as_ref()
            .map(|search| search.origin_remaining())
    }

    /// Returns a clone that tracks dynamic proxy selections for HTTP outcomes.
    ///
    /// The inventory, provider refresh policy, and runtime state remain shared
    /// with the original core. Only the destination-to-selection correlation map
    /// is fresh so an HTTP client can report accepted or rejected origin
    /// responses without owning provider-loading decisions.
    pub fn with_dynamic_proxy_observation_scope(&self) -> Self {
        let mut scoped = self.clone();
        scoped.dynamic_proxy_selections = Some(Arc::new(RwLock::new(HashMap::new())));
        scoped
    }

    /// Resolves a concrete route before a request is sent and pins every reconnect.
    ///
    /// # Arguments
    /// - `request`: Exact destination and validated caller routing parameters.
    ///
    /// # Errors
    /// Returns policy, routing or dynamic selection errors without sending a business request.
    pub fn select_for_request(
        &self,
        request: ConnectRequest,
    ) -> futures::future::BoxFuture<'_, Result<crate::core::SelectedProxy>> {
        Box::pin(async move {
            let search = crate::core::ProxySearchContext::current().or(self.search_context.clone());
            if let Some(search) = search {
                search
                    .scope(search.origin(self.select_for_request_inner(request)))
                    .await?
            } else {
                self.select_for_request_inner(request).await
            }
        })
    }

    fn select_for_request_inner(
        &self,
        request: ConnectRequest,
    ) -> futures::future::BoxFuture<'_, Result<crate::core::SelectedProxy>> {
        Box::pin(async move {
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
            let resolved = Box::pin(self.resolve_proxy_pools_in_chain(&chain, &request)).await?;
            let token = resolved.pool_selections.iter().rev().find_map(|selection| {
                dynamic_pool_countries(&selection.pool_name)
                    .ok()
                    .flatten()?;
                Some(
                    crate::core::ProxySelectionToken::new(
                        self.proxy_inventory.clone()?,
                        selection.endpoint.clone()?,
                        request.destination.clone(),
                    )
                    .with_protocol(
                        resolved
                            .chain
                            .nodes
                            .iter()
                            .find(|node| node.endpoint.as_deref() == selection.endpoint.as_deref())
                            .and_then(|node| match node.kind {
                                crate::core::TransportKind::HttpProxy => {
                                    Some(crate::core::ProxyProtocol::Http)
                                }
                                crate::core::TransportKind::HttpsProxy => {
                                    Some(crate::core::ProxyProtocol::Https)
                                }
                                crate::core::TransportKind::Socks5
                                | crate::core::TransportKind::TorSocks => {
                                    Some(crate::core::ProxyProtocol::Socks5)
                                }
                                _ => None,
                            }),
                    ),
                )
            });
            use std::hash::{Hash, Hasher};
            let mut route_hash = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", resolved.chain).hash(&mut route_hash);
            Arc::as_ptr(&self.config).hash(&mut route_hash);
            Arc::as_ptr(&self.parameter_handlers).hash(&mut route_hash);
            self.proxy_inventory
                .as_ref()
                .map(Arc::as_ptr)
                .hash(&mut route_hash);
            self.dns_resolver.transport_identity().hash(&mut route_hash);
            for hosts in [&self.insecure_tls_hosts, &self.socks5_local_dns_nodes] {
                let hosts = hosts.read().map_err(|_| {
                    crate::core::ProxyError::Config("transport policy lock poisoned".into())
                })?;
                let mut hosts = hosts.iter().collect::<Vec<_>>();
                hosts.sort();
                hosts.hash(&mut route_hash);
            }
            let route_generation = route_hash.finish();
            let mut scoped = self.clone();
            scoped.dynamic_proxy_selections = None;
            scoped.search_context =
                crate::core::ProxySearchContext::current().or(self.search_context.clone());
            scoped.pinned_route = Some(Arc::new((request.destination, resolved, token.clone())));
            Ok(crate::core::SelectedProxy {
                core: scoped,
                selection_token: token,
                route_generation,
            })
        })
    }

    pub(super) fn validate_pinned_destination(&self, destination: &Destination) -> Result<()> {
        if self.pinned_route.as_ref().is_some_and(|route| {
            route.0.address != destination.address || route.0.port != destination.port
        }) {
            return Err(ProxyError::AccessDenied(
                "pinned proxy route cannot be reused for another destination".into(),
            ));
        }
        Ok(())
    }

    /// Records the dynamically selected proxy as blocked by one destination.
    ///
    /// This is intended for HTTP clients that can observe an origin response
    /// after the proxy tunnel has been established. The recorded failure remains
    /// scoped to the destination and clears the current selection so the next
    /// connection can choose another cached candidate.
    ///
    /// # Arguments
    ///
    /// - `destination`: Origin that rejected the selected proxy.
    ///
    /// # Returns
    ///
    /// `true` when a dynamic selection was found and invalidated.
    pub async fn mark_dynamic_proxy_blocked_by_origin(&self, destination: &Destination) -> bool {
        let selection = self
            .dynamic_proxy_selections
            .as_ref()
            .and_then(|selections| selections.write().ok()?.remove(destination));
        let Some(selection) = selection else {
            return false;
        };

        if let Some(inventory) = &self.proxy_inventory {
            let authority = selection
                .endpoint
                .as_deref()
                .or_else(|| selection.upstream.rsplit_once('-').map(|(_, addr)| addr))
                .unwrap_or(&selection.upstream);
            inventory
                .record_destination_failure(
                    authority,
                    destination,
                    crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
                )
                .await;
            if let Some(affinity) = selection.affinity.as_deref() {
                inventory.clear_affinity(affinity, authority).await;
            }
        }

        true
    }

    /// Records a transport failure observed after a dynamic proxy stream was opened.
    ///
    /// HTTP engines can report failures that happen after the proxy core has
    /// successfully established the route, such as a TLS handshake or response
    /// transfer failure inside an HTTP CONNECT tunnel. The failure remains scoped
    /// to the destination because the observer cannot reliably determine whether
    /// the proxy is globally unavailable. The current selection and optional
    /// affinity are cleared, and the provider refresh cooldown is released so an
    /// exhausted cache can discover replacement endpoints on the next selection.
    ///
    /// # Arguments
    ///
    /// - `destination`: Origin reached through the failed dynamic proxy stream.
    /// - `reason`: Best available classification of the observed failure.
    ///
    /// # Returns
    ///
    /// `true` when a dynamic selection was found and invalidated.
    pub async fn mark_dynamic_proxy_transport_failed(
        &self,
        destination: &Destination,
        reason: crate::core::ProxyDestinationFailureReason,
    ) -> bool {
        let selection = self
            .dynamic_proxy_selections
            .as_ref()
            .and_then(|selections| selections.write().ok()?.remove(destination));
        let Some(selection) = selection else {
            return false;
        };

        if let Some(inventory) = &self.proxy_inventory {
            let authority = selection
                .endpoint
                .as_deref()
                .or_else(|| selection.upstream.rsplit_once('-').map(|(_, addr)| addr))
                .unwrap_or(&selection.upstream);
            inventory
                .record_destination_failure(authority, destination, reason)
                .await;
            if let Some(affinity) = selection.affinity.as_deref() {
                inventory.clear_affinity(affinity, authority).await;
            }
            if let Ok(Some(countries)) = dynamic_pool_countries(&selection.pool_name) {
                inventory
                    .allow_provider_refresh_after_observed_failure(&countries)
                    .await;
            }
        }

        true
    }

    /// Renews the validation age of the dynamic proxy selected for a destination.
    ///
    /// HTTP clients call this after receiving any origin response that is not
    /// classified as a proxy rejection. The selection remains tracked so a later
    /// rejected response on the same pooled connection can still invalidate it.
    pub async fn mark_dynamic_proxy_validated(&self, destination: &Destination) -> bool {
        let selection = self
            .dynamic_proxy_selections
            .as_ref()
            .and_then(|selections| selections.read().ok()?.get(destination).cloned());
        let Some(selection) = selection else {
            return false;
        };

        let Some(inventory) = &self.proxy_inventory else {
            return false;
        };
        let authority = selection
            .endpoint
            .as_deref()
            .or_else(|| selection.upstream.rsplit_once('-').map(|(_, addr)| addr))
            .unwrap_or(&selection.upstream);
        inventory
            .record_validated_response(authority, destination)
            .await;
        if let Some(affinity) = selection.affinity.as_deref() {
            inventory.renew_affinity(affinity, authority).await;
        }
        true
    }

    /// Applies configured request parameter handlers to a selected chain.
    ///
    /// # Parameters
    ///
    /// - `chain`: Chain selected by the route policy.
    /// - `request`: Request whose client context feeds parameter handlers.
    ///
    /// # Returns
    ///
    /// Chain with any request-local extra proxies appended.
    ///
    /// # Errors
    ///
    /// Returns an error when a handler fails.
    pub(super) fn apply_parameter_handlers(
        &self,
        chain: &ProxyChain,
        request: &ConnectRequest,
    ) -> Result<ProxyChain> {
        let mut effective_chain = chain.clone();
        for handler in self.parameter_handlers.iter() {
            let decision = handler.proxy_from_parameters(&request.client_context)?;
            if let Some(proxy) = decision.extra_proxy {
                effective_chain.nodes.push(proxy);
            }
            if decision.stop {
                break;
            }
        }
        Ok(effective_chain)
    }
}
