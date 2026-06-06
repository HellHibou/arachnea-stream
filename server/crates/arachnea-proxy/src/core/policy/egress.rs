use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::{
    ConnectRequest, ProxyChain, ProxyConfig, ProxyError, ProxyNode, Result, RouteDecision,
    RoutePolicy, TimeoutConfig,
};

/// Strategy used to select a working proxy from an egress pool.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressPoolStrategy {
    /// Test candidates in parallel and keep the first declared candidate that works.
    FirstAvailable,
}

impl Default for EgressPoolStrategy {
    /// Returns first-available pool selection.
    fn default() -> Self {
        Self::FirstAvailable
    }
}

/// Runtime health state for one proxy pool member.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyPoolMemberStatus {
    /// The member has not been tested by this core instance yet.
    Untested,
    /// The member passed the pool compatibility check.
    Ok,
    /// The member failed the pool compatibility check.
    Ko,
}

/// Inspectable runtime status for one proxy pool member.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyPoolMemberState {
    /// Upstream node name.
    pub upstream: String,
    /// Last known runtime status.
    pub status: ProxyPoolMemberStatus,
    /// Whether this member is the cached selection for its pool.
    pub selected: bool,
}

/// Named pool of candidate egress nodes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EgressPool {
    /// Pool name referenced by routing rules or extensions.
    pub name: String,
    /// Optional ISO country code or provider-specific region label.
    pub country: Option<String>,
    /// Proxy nodes in this pool.
    pub proxy_nodes: Vec<ProxyNode>,
    /// Selection strategy used by the core.
    pub strategy: EgressPoolStrategy,
    /// Maximum number of untested members probed concurrently.
    #[serde(default = "default_proxy_pool_test_batch_size")]
    pub test_batch_size: usize,
}

impl EgressPool {
    /// Creates a first-available egress pool.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable pool name.
    /// - `proxy_nodes`: Proxy nodes that belong to the pool.
    ///
    /// # Returns
    ///
    /// Egress pool using first-available selection.
    pub fn new(
        name: impl Into<String>,
        proxy_nodes: Vec<ProxyNode>,
        test_batch_size: Option<usize>,
    ) -> Self {
        Self {
            name: name.into(),
            country: None,
            proxy_nodes,
            strategy: EgressPoolStrategy::FirstAvailable,
            test_batch_size: test_batch_size.unwrap_or_else(default_proxy_pool_test_batch_size),
        }
    }

    /// Assigns a country or region label to this pool.
    ///
    /// # Parameters
    ///
    /// - `country`: Country code or provider-specific region label.
    ///
    /// # Returns
    ///
    /// Updated egress pool.
    pub fn with_country(mut self, country: impl Into<String>) -> Self {
        self.country = Some(country.into());
        self
    }

    /// Builds a proxy configuration that routes through this pool by default.
    ///
    /// # Parameters
    ///
    /// - `chain_name`: Name assigned to the generated default chain.
    ///
    /// # Returns
    ///
    /// Proxy configuration whose default route uses this pool.
    pub fn config(self, chain_name: impl Into<String>) -> ProxyConfig {
        let chain_name = chain_name.into();
        let pool_name = self.name.clone();
        let upstreams = self.proxy_nodes.clone();
        ProxyConfig {
            upstreams,
            egress_pools: vec![self],
            chains: vec![ProxyChain::pool(chain_name.clone(), pool_name)],
            routing: RoutePolicy {
                default_chain: Some(chain_name),
                allow_direct_fallback: false,
                rules: Vec::new(),
            },
            timeouts: TimeoutConfig::default(),
            ..ProxyConfig::default()
        }
    }
}

impl Default for EgressPool {
    /// Creates an empty pool value for deserialization defaults.
    fn default() -> Self {
        Self {
            name: String::new(),
            country: None,
            proxy_nodes: Vec::new(),
            strategy: EgressPoolStrategy::default(),
            test_batch_size: default_proxy_pool_test_batch_size(),
        }
    }
}

/// Returns the default proxy-pool test batch size.
///
/// # Returns
///
/// Default number of untested pool members probed concurrently.
fn default_proxy_pool_test_batch_size() -> usize {
    8
}

/// Extension point for selecting an egress route from client context.
#[async_trait]
pub trait EgressSelector: Send + Sync {
    /// Selects an egress decision from a request, context and candidate nodes.
    async fn select_egress(
        &self,
        request: &ConnectRequest,
        candidates: &[ProxyNode],
    ) -> Result<RouteDecision>;
}

/// Egress selector that always fails when no plugin is configured.
pub struct NoopEgressSelector;

#[async_trait]
impl EgressSelector for NoopEgressSelector {
    /// Declines to select an egress route.
    ///
    /// # Parameters
    ///
    /// - `_request`: Connect request that would need egress selection.
    /// - `_candidates`: Candidate egress nodes.
    ///
    /// # Returns
    ///
    /// Always returns a route-unavailable error.
    async fn select_egress(
        &self,
        _request: &ConnectRequest,
        _candidates: &[ProxyNode],
    ) -> Result<RouteDecision> {
        Err(ProxyError::RouteUnavailable(
            "no egress selector configured".to_string(),
        ))
    }
}
