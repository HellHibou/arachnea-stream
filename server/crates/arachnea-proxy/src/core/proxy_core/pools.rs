//! Pool resolution, candidate selection and compatibility checks.

use crate::core::{
    ApplicationProtocol, ConnectRequest, Destination, ProxyChain, ProxyError, ProxyNode,
    ProxyPoolMemberStatus, Result, TransportKind, PROXY_PARAMETER_AFFINITY,
};
use std::collections::BTreeMap;

use super::dynamic_pools::{dynamic_pool_countries, is_dynamic_country_proxy_unavailable};
use super::{ArachneaProxyCore, ProxyPoolCheckMode, ProxyPoolSelection, ResolvedProxyChain};
use crate::{PROXY_POOL_MEMBER_CHECK_ATTEMPTS, PROXY_POOL_SELECTION_PASSES};

/// Returns the target and check mode used to test a proxy pool marker.
///
/// A pool followed by another concrete proxy must support tunneling to that
/// next proxy endpoint. A final pool serving plain HTTP may select an HTTP
/// proxy that is reachable but does not allow `CONNECT` to port 80, because the
/// actual request will be relayed in absolute-form.
///
/// # Arguments
///
/// - `chain`: Chain containing the proxy pool marker.
/// - `pool_index`: Index of the pool marker inside `chain.nodes`.
/// - `destination`: Original destination requested by the caller.
///
/// # Returns
///
/// Pair containing the probe target and required check mode.
///
/// # Errors
///
/// Returns an error when a later concrete proxy hop has no endpoint or an
/// invalid endpoint.
fn proxy_pool_check_target(
    chain: &ProxyChain,
    pool_index: usize,
    destination: &Destination,
) -> Result<(Destination, ProxyPoolCheckMode)> {
    for node in chain.nodes.iter().skip(pool_index + 1) {
        if node.kind == TransportKind::Direct {
            continue;
        }
        if node.kind == TransportKind::ProxyPool {
            continue;
        }
        let endpoint = node
            .endpoint
            .as_deref()
            .ok_or_else(|| ProxyError::Config(format!("node '{}' has no endpoint", node.name)))?;
        return Ok((
            Destination::from_authority(endpoint, ApplicationProtocol::Tcp)?,
            ProxyPoolCheckMode::Tunnel,
        ));
    }

    let check_mode = if destination.protocol == ApplicationProtocol::Http {
        ProxyPoolCheckMode::HttpForward
    } else {
        ProxyPoolCheckMode::Tunnel
    };
    Ok((destination.clone(), check_mode))
}

impl ArachneaProxyCore {
    /// Replaces proxy pool markers in a chain with selected concrete upstreams.
    ///
    /// # Arguments
    ///
    /// - `chain`: Chain that may contain `ProxyPool` marker nodes.
    /// - `request`: Request whose destination is used for first-use pool checks.
    /// # Returns
    ///
    /// Chain containing only concrete direct or proxy nodes plus selected pool
    /// members.
    ///
    /// # Errors
    ///
    /// Returns an error when a pool is undeclared or no candidate works.
    pub(super) async fn resolve_proxy_pools_in_chain(
        &self,
        chain: &ProxyChain,
        request: &ConnectRequest,
    ) -> Result<ResolvedProxyChain> {
        let fallback_direct = self.fallback_direct_for_request(request)?;
        let mut resolved = chain.clone();
        resolved.nodes = Vec::with_capacity(chain.nodes.len());
        let mut pool_selections = Vec::new();
        for (index, node) in chain.nodes.iter().enumerate() {
            if node.kind == TransportKind::ProxyPool {
                if let Some(countries) = dynamic_pool_countries(&node.name)? {
                    tracing::debug!(
                        destination = %request.destination,
                        requested_proxy_countries = ?countries,
                        "evaluating dynamic country proxy route"
                    );
                    if self.should_bypass_dynamic_country_pools(&countries).await {
                        tracing::debug!(
                            destination = %request.destination,
                            requested_proxy_countries = ?countries,
                            "continuing route without geo proxy because local country is allowed"
                        );
                        continue;
                    }
                    let require_https =
                        self.is_dynamic_country_https_request(chain, index, &request.destination)?;
                    let affinity = request
                        .client_context
                        .get_string(PROXY_PARAMETER_AFFINITY)
                        .map(str::to_string);
                    let domain_group = request
                        .client_context
                        .get_string(crate::core::PROXY_PARAMETER_DOMAIN_GROUP)
                        .filter(|value| !value.is_empty())
                        .map(|value| {
                            serde_json::from_str::<Vec<String>>(value)
                                .map(|domains| crate::core::canonical_proxy_domain_group(&domains))
                                .map_err(|error| {
                                    ProxyError::Config(format!(
                                        "invalid proxy domain group: {error}"
                                    ))
                                })
                        })
                        .transpose()?
                        .unwrap_or_default();
                    let selected = match self
                        .resolve_dynamic_country_pool(
                            &countries,
                            require_https,
                            &request.destination,
                            affinity.as_deref(),
                            &domain_group,
                        )
                        .await
                    {
                        Ok(selected) => selected,
                        Err(error)
                            if is_dynamic_country_proxy_unavailable(&error) && fallback_direct =>
                        {
                            tracing::warn!(
                                destination = %request.destination,
                                requested_proxy_countries = ?countries,
                                %error,
                                "dynamic country proxy unavailable; falling back to a direct route"
                            );
                            crate::core::ProxyFallbackDiagnostics::record(format!(
                                "{error}; using direct fallback for {}",
                                request.destination
                            ));
                            return Ok(ResolvedProxyChain {
                                chain: ProxyChain::direct(),
                                pool_selections: Vec::new(),
                            });
                        }
                        Err(error) => return Err(error),
                    };
                    tracing::debug!(
                        destination = %request.destination,
                        requested_proxy_countries = ?countries,
                        proxy_transport = ?selected.kind,
                        require_https,
                        "selected dynamic country proxy for route"
                    );
                    let endpoint = selected.endpoint.clone();
                    pool_selections.push(ProxyPoolSelection {
                        pool_name: node.name.clone(),
                        upstream: selected.name.clone(),
                        endpoint,
                        affinity,
                    });
                    if let Some(selections) = &self.dynamic_proxy_selections {
                        if let Some(selection) = pool_selections.last().cloned() {
                            if let Ok(mut selections) = selections.write() {
                                selections.insert(request.destination.clone(), selection);
                            }
                        }
                    }
                    resolved.nodes.push(selected);
                } else {
                    let (target, check_mode) =
                        proxy_pool_check_target(chain, index, &request.destination)?;
                    let selected = self
                        .select_proxy_pool_member(&node.name, &target, check_mode)
                        .await?;
                    let endpoint = selected.endpoint.clone();
                    pool_selections.push(ProxyPoolSelection {
                        pool_name: node.name.clone(),
                        upstream: selected.name.clone(),
                        endpoint,
                        affinity: None,
                    });
                    resolved.nodes.push(selected);
                }
            } else {
                resolved.nodes.push(node.clone());
            }
        }
        Ok(ResolvedProxyChain {
            chain: resolved,
            pool_selections,
        })
    }

    /// Selects and caches one working upstream from a proxy pool.
    ///
    /// # Arguments
    ///
    /// - `pool_name`: Egress pool name referenced by the chain.
    /// - `target`: Destination used to test candidates.
    /// - `check_mode`: Compatibility check required at this chain position.
    ///
    /// # Returns
    ///
    /// Concrete proxy node selected from the pool.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool is undeclared, a member is missing, or
    /// all candidates fail their compatibility checks.
    async fn select_proxy_pool_member(
        &self,
        pool_name: &str,
        target: &Destination,
        check_mode: ProxyPoolCheckMode,
    ) -> Result<ProxyNode> {
        let pool = self.config.egress_pool(pool_name).cloned().ok_or_else(|| {
            ProxyError::RouteUnavailable(format!("egress pool '{pool_name}' not found"))
        })?;
        self.mark_proxy_pool_members_untested(&pool);

        if check_mode == ProxyPoolCheckMode::Tunnel {
            return self.select_proxy_pool_member_without_preflight(&pool);
        }

        for pass in 1..=PROXY_POOL_SELECTION_PASSES {
            if let Some(selected) = self.first_ok_proxy_pool_member(&pool) {
                self.store_proxy_pool_selected_member(&pool.name, &selected.name);
                return Ok(selected);
            }

            let candidates = self.next_untested_proxy_pool_batch(&pool)?;
            if candidates.is_empty() {
                if pass < PROXY_POOL_SELECTION_PASSES && self.reset_proxy_pool_ko_members(&pool) {
                    tracing::debug!(
                        proxy_pool = %pool.name,
                        pass = %pass,
                        max_passes = %PROXY_POOL_SELECTION_PASSES,
                        "retrying exhausted proxy pool"
                    );
                    continue;
                }
                return Err(ProxyError::RouteUnavailable(format!(
                    "egress pool '{}' has no working proxy candidates",
                    pool.name
                )));
            }

            let mut handles = Vec::with_capacity(candidates.len());
            for candidate in candidates {
                let core = self.clone();
                let target = target.clone();
                let pool_name = pool.name.clone();
                handles.push(tokio::spawn(async move {
                    let result = core
                        .check_proxy_pool_member(&pool_name, &candidate, &target, check_mode)
                        .await;
                    (candidate, result)
                }));
            }

            let mut statuses = BTreeMap::new();
            for handle in handles {
                let (candidate, result) = handle.await.map_err(|error| {
                    ProxyError::RouteUnavailable(format!("proxy pool worker failed: {error}"))
                })?;
                match result {
                    Ok(()) => {
                        statuses.insert(candidate.name.clone(), ProxyPoolMemberStatus::Ok);
                    }
                    Err(error) => {
                        tracing::debug!(
                            proxy_pool = %pool.name,
                            proxy_node = %candidate.name,
                            %error,
                            "proxy pool candidate failed compatibility check"
                        );
                        statuses.insert(candidate.name.clone(), ProxyPoolMemberStatus::Ko);
                    }
                }
            }
            self.merge_proxy_pool_statuses(&pool.name, statuses);
        }

        Err(ProxyError::RouteUnavailable(format!(
            "egress pool '{}' has no working proxy candidates",
            pool.name
        )))
    }

    /// Selects a tunnel candidate without opening a destructive probe tunnel.
    ///
    /// # Arguments
    /// * `pool` - Configured egress pool whose members and runtime state are inspected.
    ///
    fn select_proxy_pool_member_without_preflight(
        &self,
        pool: &crate::core::EgressPool,
    ) -> Result<ProxyNode> {
        for pass in 1..=PROXY_POOL_SELECTION_PASSES {
            if let Some(selected) = self.first_available_proxy_pool_member(pool) {
                self.store_proxy_pool_selected_member(&pool.name, &selected.name);
                return Ok(selected);
            }

            if pass < PROXY_POOL_SELECTION_PASSES && self.reset_proxy_pool_ko_members(pool) {
                tracing::debug!(
                    proxy_pool = %pool.name,
                    pass = %pass,
                    max_passes = %PROXY_POOL_SELECTION_PASSES,
                    "retrying exhausted proxy pool"
                );
                continue;
            }

            return Err(ProxyError::RouteUnavailable(format!(
                "egress pool '{}' has no working proxy candidates",
                pool.name
            )));
        }

        Err(ProxyError::RouteUnavailable(format!(
            "egress pool '{}' has no working proxy candidates",
            pool.name
        )))
    }

    /// Checks whether one pool member can support this chain position.
    ///
    /// Final HTTP or HTTPS proxy candidates for plain HTTP forwarding are
    /// checked with endpoint reachability. Other candidates use the normal
    /// single-node chain path, including SOCKS DNS fallback behavior. The
    /// successful stream is dropped immediately; later traffic opens its own
    /// stream through the cached member.
    ///
    /// # Arguments
    ///
    /// - `pool_name`: Pool name used for diagnostic metadata.
    /// - `node`: Candidate proxy node to test.
    /// - `target`: Destination used for the compatibility check.
    /// - `check_mode`: Compatibility check required at this chain position.
    ///
    /// # Errors
    ///
    /// Returns an error when the candidate is not a concrete proxy or when the
    /// endpoint, TLS setup, or protocol handshake fails.
    async fn check_proxy_pool_member(
        &self,
        pool_name: &str,
        node: &ProxyNode,
        target: &Destination,
        check_mode: ProxyPoolCheckMode,
    ) -> Result<()> {
        if matches!(node.kind, TransportKind::Direct | TransportKind::ProxyPool) {
            return Err(ProxyError::Config(format!(
                "egress pool '{pool_name}' member '{}' is not a concrete proxy",
                node.name
            )));
        }

        for attempt in 1..=PROXY_POOL_MEMBER_CHECK_ATTEMPTS {
            let result = self
                .check_proxy_pool_member_once(pool_name, node, target, check_mode)
                .await;

            match result {
                Ok(()) => return Ok(()),
                Err(error) if attempt < PROXY_POOL_MEMBER_CHECK_ATTEMPTS => {
                    tracing::debug!(
                        proxy_pool = %pool_name,
                        proxy_node = %node.name,
                        target = %target.authority(),
                        attempt = %attempt,
                        max_attempts = %PROXY_POOL_MEMBER_CHECK_ATTEMPTS,
                        %error,
                        "retrying proxy pool compatibility check"
                    );
                }
                Err(error) => return Err(error),
            }
        }

        Err(ProxyError::RouteUnavailable(format!(
            "proxy pool '{pool_name}' member '{}' compatibility check exhausted",
            node.name
        )))
    }

    /// Runs one compatibility check attempt for a pool member.
    ///
    /// # Arguments
    /// * `pool_name` - Configured pool name or encoded dynamic-country pool marker.
    /// * `node` - Concrete proxy node to test.
    /// * `target` - Optional destination used when the configured mode requires a tunnel check.
    /// * `check_mode` - Whether to test TCP connectivity only or the target tunnel.
    ///
    async fn check_proxy_pool_member_once(
        &self,
        pool_name: &str,
        node: &ProxyNode,
        target: &Destination,
        check_mode: ProxyPoolCheckMode,
    ) -> Result<()> {
        let chain = ProxyChain::single(format!("egress_pool:{pool_name}"), node.clone());
        if check_mode == ProxyPoolCheckMode::HttpForward
            && matches!(
                node.kind,
                TransportKind::HttpProxy | TransportKind::HttpsProxy
            )
        {
            let stream = self.connect_first_proxy(node, &chain).await?;
            if node.kind == TransportKind::HttpsProxy {
                drop(self.wrap_https_proxy_stream(stream, node).await?);
            }
            return Ok(());
        }

        self.connect_chain(&chain, &ConnectRequest::new(target.clone()))
            .await
            .map(|_| ())
    }
}
