//! Read-only access to validated proxy configuration.

use crate::core::{EgressPool, ProxyChain, ProxyNode};

use super::proxy_config::ProxyConfig;

/// Effective validated proxy configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedProxyConfig {
    /// Validated configuration retained for runtime routing and transport decisions.
    pub(super) inner: ProxyConfig,
}

impl ResolvedProxyConfig {
    /// Returns the original resolved configuration.
    ///
    /// # Returns
    ///
    /// Underlying validated proxy configuration.
    pub fn as_config(&self) -> &ProxyConfig {
        &self.inner
    }

    /// Returns a chain by name.
    ///
    /// # Arguments
    ///
    /// - `name`: Chain name to look up.
    ///
    /// # Returns
    ///
    /// Matching chain when declared.
    pub fn chain(&self, name: &str) -> Option<&ProxyChain> {
        self.inner.chains.iter().find(|chain| chain.name == name)
    }

    /// Returns an egress pool by name.
    ///
    /// # Arguments
    ///
    /// - `name`: Egress pool name to look up.
    ///
    /// # Returns
    ///
    /// Matching egress pool when declared.
    pub fn egress_pool(&self, name: &str) -> Option<&EgressPool> {
        self.inner
            .egress_pools
            .iter()
            .find(|pool| pool.name == name)
    }

    /// Returns an upstream node by name.
    ///
    /// # Arguments
    ///
    /// - `name`: Upstream node name to look up.
    ///
    /// # Returns
    ///
    /// Matching upstream node when declared.
    pub fn upstream(&self, name: &str) -> Option<&ProxyNode> {
        self.inner
            .upstreams
            .iter()
            .find(|upstream| upstream.name == name)
    }
}
