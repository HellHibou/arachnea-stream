//! Cached pool member state, failure bookkeeping and retry budgets.

use crate::core::{
    Destination, ProxyChain, ProxyError, ProxyNode, ProxyPoolMemberState, ProxyPoolMemberStatus,
    Result, TransportKind,
};
use std::collections::BTreeMap;

use super::dynamic_pools::{dynamic_pool_countries, DYNAMIC_COUNTRY_POOL_PREFIX};
use super::request_routing::is_dynamic_proxy_transport_failure;
use super::{ArachneaProxyCore, ProxyPoolSelection};

impl ArachneaProxyCore {
    /// Returns runtime status for members of a proxy pool.
    ///
    /// Members remain `Untested` until the pool is first used by this core
    /// instance. Once a pool has selected a working member, that selection is
    /// cached and reported through the `selected` flag.
    ///
    /// # Arguments
    ///
    /// - `pool_name`: Egress pool to inspect.
    ///
    /// # Returns
    ///
    /// Status entries in configured upstream order.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool is not declared.
    pub fn proxy_pool_statuses(&self, pool_name: &str) -> Result<Vec<ProxyPoolMemberState>> {
        let pool = self.config.egress_pool(pool_name).ok_or_else(|| {
            ProxyError::RouteUnavailable(format!("egress pool '{pool_name}' not found"))
        })?;
        let state = self
            .proxy_pool_states
            .read()
            .ok()
            .and_then(|states| states.get(pool_name).cloned());
        Ok(pool
            .proxy_nodes
            .iter()
            .map(|node| ProxyPoolMemberState {
                upstream: node.name.clone(),
                status: state
                    .as_ref()
                    .and_then(|state| state.statuses.get(&node.name).cloned())
                    .unwrap_or(ProxyPoolMemberStatus::Untested),
                selected: state
                    .as_ref()
                    .and_then(|state| state.selected.as_deref())
                    .is_some_and(|selected| selected == node.name),
            })
            .collect())
    }

    /// Returns the first working pool member in configured order.
    ///
    /// # Arguments
    ///
    /// - `pool`: Egress pool to inspect.
    ///
    /// # Returns
    ///
    /// First `Ok` member when any.
    pub(super) fn first_ok_proxy_pool_member(
        &self,
        pool: &crate::core::EgressPool,
    ) -> Option<ProxyNode> {
        let states = self.proxy_pool_states.read().ok()?;
        let state = states.get(&pool.name)?;
        if let Some(selected) = &state.selected {
            if state.statuses.get(selected) == Some(&ProxyPoolMemberStatus::Ok) {
                return pool
                    .proxy_nodes
                    .iter()
                    .find(|node| node.name == *selected)
                    .cloned();
            }
        }
        pool.proxy_nodes
            .iter()
            .find(|node| state.statuses.get(&node.name) == Some(&ProxyPoolMemberStatus::Ok))
            .cloned()
    }

    /// Returns the selected or first non-failed pool member in configured order.
    ///
    /// # Arguments
    /// * `pool` - Configured egress pool whose members and runtime state are inspected.
    ///
    pub(super) fn first_available_proxy_pool_member(
        &self,
        pool: &crate::core::EgressPool,
    ) -> Option<ProxyNode> {
        let states = self.proxy_pool_states.read().ok()?;
        let state = states.get(&pool.name)?;
        if let Some(selected) = &state.selected {
            if state.statuses.get(selected) != Some(&ProxyPoolMemberStatus::Ko) {
                return pool
                    .proxy_nodes
                    .iter()
                    .find(|node| node.name == *selected)
                    .cloned();
            }
        }
        pool.proxy_nodes
            .iter()
            .find(|node| state.statuses.get(&node.name) != Some(&ProxyPoolMemberStatus::Ko))
            .cloned()
    }

    /// Returns the next untested pool member batch.
    ///
    /// # Arguments
    ///
    /// - `pool`: Egress pool whose untested members should be selected.
    ///
    /// # Returns
    ///
    /// Concrete upstream nodes for at most `pool.test_batch_size` untested members.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool references a missing upstream.
    pub(super) fn next_untested_proxy_pool_batch(
        &self,
        pool: &crate::core::EgressPool,
    ) -> Result<Vec<ProxyNode>> {
        let statuses = self
            .proxy_pool_states
            .read()
            .ok()
            .and_then(|states| states.get(&pool.name).map(|state| state.statuses.clone()))
            .unwrap_or_default();
        Ok(pool
            .proxy_nodes
            .iter()
            .filter(|node| {
                statuses
                    .get(&node.name)
                    .is_none_or(|status| *status == ProxyPoolMemberStatus::Untested)
            })
            .take(pool.test_batch_size)
            .cloned()
            .collect())
    }

    /// Initializes untested status entries for a proxy pool.
    ///
    /// # Arguments
    ///
    /// - `pool`: Egress pool being prepared.
    pub(super) fn mark_proxy_pool_members_untested(&self, pool: &crate::core::EgressPool) {
        if let Ok(mut states) = self.proxy_pool_states.write() {
            let state = states.entry(pool.name.clone()).or_default();
            for node in &pool.proxy_nodes {
                state
                    .statuses
                    .entry(node.name.clone())
                    .or_insert(ProxyPoolMemberStatus::Untested);
            }
        }
    }

    /// Resets failed pool members so transient upstream failures can be retried.
    ///
    /// # Arguments
    /// * `pool` - Configured egress pool whose members and runtime state are inspected.
    ///
    pub(super) fn reset_proxy_pool_ko_members(&self, pool: &crate::core::EgressPool) -> bool {
        let Ok(mut states) = self.proxy_pool_states.write() else {
            return false;
        };
        let state = states.entry(pool.name.clone()).or_default();
        let mut changed = false;
        for node in &pool.proxy_nodes {
            if state.statuses.get(&node.name) == Some(&ProxyPoolMemberStatus::Ko) {
                state
                    .statuses
                    .insert(node.name.clone(), ProxyPoolMemberStatus::Untested);
                changed = true;
            }
        }
        if changed {
            state.selected = None;
        }
        changed
    }

    /// Stores the selected proxy pool member.
    ///
    /// # Arguments
    ///
    /// - `pool_name`: Egress pool name that selected a member.
    /// - `selected`: Upstream node selected for future requests from this pool.
    pub(super) fn store_proxy_pool_selected_member(&self, pool_name: &str, selected: &str) {
        if let Ok(mut states) = self.proxy_pool_states.write() {
            states.entry(pool_name.to_string()).or_default().selected = Some(selected.to_string());
        }
    }

    /// Merges status updates into one proxy pool runtime state.
    ///
    /// # Arguments
    ///
    /// - `pool_name`: Egress pool name that received updates.
    /// - `statuses`: Status updates keyed by upstream node name.
    pub(super) fn merge_proxy_pool_statuses(
        &self,
        pool_name: &str,
        statuses: BTreeMap<String, ProxyPoolMemberStatus>,
    ) {
        if let Ok(mut states) = self.proxy_pool_states.write() {
            let state = states.entry(pool_name.to_string()).or_default();
            for (upstream, status) in statuses {
                state.statuses.insert(upstream, status);
            }
            if state
                .selected
                .as_ref()
                .and_then(|selected| state.statuses.get(selected))
                == Some(&ProxyPoolMemberStatus::Ko)
            {
                state.selected = None;
            }
        }
    }

    /// Marks selected pool members as failed after a connection setup failure.
    ///
    /// Only explicitly attributed endpoint connection failures affect global
    /// inventory health. Tunnel and opaque transport failures remain contextual.
    ///
    /// # Arguments
    ///
    /// - `selections`: Pool selections used by the failed connection attempt.
    /// - `destination`: Destination associated with the failed attempt.
    /// - `error`: Failure evidence distinguishing global endpoint failure from destination-specific errors.
    pub(super) async fn mark_proxy_pool_selections_ko(
        &self,
        selections: &[ProxyPoolSelection],
        destination: &Destination,
        error: &ProxyError,
    ) {
        if let Ok(mut states) = self.proxy_pool_states.write() {
            for selection in selections {
                if dynamic_pool_countries(&selection.pool_name)
                    .ok()
                    .flatten()
                    .is_some()
                    && !is_dynamic_proxy_transport_failure(error)
                {
                    continue;
                }
                let state = states.entry(selection.pool_name.clone()).or_default();
                state
                    .statuses
                    .insert(selection.upstream.clone(), ProxyPoolMemberStatus::Ko);
                if state.selected.as_deref() == Some(selection.upstream.as_str()) {
                    state.selected = None;
                }
            }
        }
        // Propagate failures from dynamic country pools to the proxy inventory
        // synchronously before retry so the same proxy is not selected again.
        for selection in selections {
            if let Ok(Some(countries)) = dynamic_pool_countries(&selection.pool_name) {
                if let Some(inventory) = &self.proxy_inventory {
                    let authority = selection
                        .endpoint
                        .as_deref()
                        .or_else(|| selection.upstream.rsplit_once('-').map(|(_, addr)| addr))
                        .unwrap_or(&selection.upstream);
                    if is_dynamic_proxy_transport_failure(error) {
                        tracing::debug!(
                            pool = %selection.pool_name,
                            countries = ?countries,
                            proxy = %selection.upstream,
                            authority = %authority,
                            %error,
                            "recording global dynamic proxy transport failure in inventory for retry"
                        );
                        inventory.record_global_failure(authority).await;
                    } else {
                        tracing::debug!(
                            pool = %selection.pool_name,
                            countries = ?countries,
                            proxy = %selection.upstream,
                            authority = %authority,
                            %error,
                            "recording destination-scoped dynamic proxy failure in inventory for retry"
                        );
                        inventory
                            .record_destination_failure(
                                authority,
                                destination,
                                crate::core::ProxyDestinationFailureReason::Other,
                            )
                            .await;
                    }
                    inventory
                        .allow_provider_refresh_after_observed_failure(&countries)
                        .await;
                }
            }
        }
    }

    /// Returns the number of retry attempts allowed for pool-backed chains.
    ///
    /// For static pools, the budget is the number of pool members. For dynamic
    /// country pools, a fixed budget is used since the exact eligible count
    /// requires an async call to the inventory.
    ///
    /// # Arguments
    ///
    /// - `chain`: Chain that may contain proxy pool markers.
    ///
    /// # Returns
    ///
    /// Static retry count, or `None` for dynamic discovery bounded by the search deadline.
    pub(super) fn proxy_pool_retry_budget(&self, chain: &ProxyChain) -> Option<usize> {
        let mut budget = 0usize;
        for node in &chain.nodes {
            if node.kind != TransportKind::ProxyPool {
                continue;
            }
            // Static pools declared in configuration.
            if let Some(pool) = self.config.egress_pool(&node.name) {
                budget += pool.proxy_nodes.len();
                continue;
            }
            // Dynamic discovery stops at list exhaustion or the shared deadline.
            if node.name.starts_with(DYNAMIC_COUNTRY_POOL_PREFIX) {
                return None;
            }
        }
        Some(budget)
    }
}
