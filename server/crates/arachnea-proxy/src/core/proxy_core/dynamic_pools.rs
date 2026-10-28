//! Dynamic country pools and current-country bypass decisions.

use crate::core::{
    ApplicationProtocol, Destination, ProxyChain, ProxyError, ProxyNode, Result, TransportKind,
};

use super::ArachneaProxyCore;

/// Prefix used for dynamic country pool markers emitted by
/// [`crate::core::DynamicCountryRoutingProxyHandler`]. The suffix is a JSON country list.
///
/// Defined in the crate root; re-exported here for path compatibility.
pub(crate) use crate::DYNAMIC_COUNTRY_POOL_PREFIX;
/// Parses the JSON country-list suffix of a dynamic pool marker.
///
/// # Arguments
/// * `value` - JSON array of country-code strings, canonicalized after parsing.
///
/// # Errors
/// Returns a configuration error if the suffix is not a JSON array of strings.
fn parse_dynamic_country_pool(value: &str) -> Result<Vec<String>> {
    let countries: Vec<String> = serde_json::from_str(value)
        .map_err(|_| ProxyError::Config("invalid dynamic country pool marker".to_string()))?;
    Ok(crate::core::canonical_proxy_countries(&countries))
}

/// Decodes a dynamic-country marker, returning `None` for an ordinary pool name.
///
/// # Arguments
/// * `pool_name` - Configured pool name or encoded dynamic-country pool marker.
///
/// # Errors
///
/// Returns a configuration error if a dynamic-country list marker contains invalid JSON.
pub(super) fn dynamic_pool_countries(pool_name: &str) -> Result<Option<Vec<String>>> {
    if let Some(value) = pool_name.strip_prefix(DYNAMIC_COUNTRY_POOL_PREFIX) {
        return parse_dynamic_country_pool(value).map(Some);
    }
    Ok(pool_name
        .strip_prefix("dynamic-country:")
        .map(|country| crate::core::canonical_proxy_countries(&[country.to_string()])))
}

/// Returns whether a dynamic country selection failure should fall back to the
/// direct route when the caller has explicitly enabled that policy.
///
/// # Arguments
/// * `error` - Observed error to classify without changing its cause.
///
pub(super) fn is_dynamic_country_proxy_unavailable(error: &ProxyError) -> bool {
    matches!(error, ProxyError::RouteUnavailable(_))
}

impl ArachneaProxyCore {
    /// Stores the explicitly configured current outbound country on the
    /// configured IP-country resolver, when available.
    ///
    /// # Arguments
    /// * `country` - Current-country code forwarded to the resolver; a blank value clears the explicit override.
    ///
    pub async fn set_current_country(&self, country: impl AsRef<str>) {
        let Some(inventory) = &self.proxy_inventory else {
            return;
        };
        let Some(resolver) = inventory.ip_country_resolver() else {
            return;
        };

        resolver.set_current_country(country).await;
    }

    /// Resolves a dynamic country pool marker through the proxy inventory.
    ///
    /// # Arguments
    ///
    /// - `countries`: Canonical set of allowed ISO country codes extracted from the pool marker.
    /// - `require_https`: Whether the destination requires HTTPS support.
    /// - `destination`: Original destination used for destination-specific
    ///   cooldown filtering.
    /// - `affinity`: Optional opaque request affinity.
    /// - `domain_group`: Optional domain group supplying default proxies.
    /// # Returns
    ///
    /// Concrete proxy node selected from the inventory.
    ///
    /// # Errors
    ///
    /// Returns an error when no inventory is configured or no candidate is
    /// available.
    pub(super) async fn resolve_dynamic_country_pool(
        &self,
        countries: &[String],
        require_https: bool,
        destination: &Destination,
        affinity: Option<&str>,
        domain_group: &[String],
    ) -> Result<ProxyNode> {
        let inventory = self.proxy_inventory.as_ref().ok_or_else(|| {
            ProxyError::RouteUnavailable(format!(
                "dynamic proxy inventory not configured for requested countries"
            ))
        })?;
        let record = inventory
            .select_any_for_destination_with_domain_group(
                countries,
                require_https,
                Some(destination),
                affinity,
                domain_group,
            )
            .await?;
        if let Some(search) = crate::core::ProxySearchContext::current() {
            search.start_origin(&record.authority(), destination)?;
        }
        record.try_to_node().ok_or_else(|| {
            ProxyError::RouteUnavailable(format!(
                "dynamic proxy selected for requested countries has no resolved protocol"
            ))
        })
    }

    /// Returns whether a dynamic country proxy pool marker should be skipped
    /// because the current outbound country matches any requested country.
    ///
    /// # Arguments
    /// * `countries` - Allowed ISO alpha-2 country codes.
    ///
    pub(super) async fn should_bypass_dynamic_country_pools(&self, countries: &[String]) -> bool {
        let Some(inventory) = &self.proxy_inventory else {
            tracing::trace!(
                requested_proxy_countries = ?countries,
                "dynamic country proxy bypass unavailable because no proxy inventory is configured"
            );
            return false;
        };
        let Some(resolver) = inventory.ip_country_resolver() else {
            tracing::trace!(
                requested_proxy_countries = ?countries,
                "dynamic country proxy bypass unavailable because no current-country resolver is configured"
            );
            return false;
        };

        let current_country = resolver.resolve_current_country().await.ok().flatten();
        match current_country.as_deref() {
            Some(local_country) if countries.iter().any(|country| country == local_country) => {
                tracing::debug!(
                    requested_proxy_countries = ?countries,
                    local_country = %local_country,
                    bypass = true,
                    "dynamic country proxy bypass applied"
                );
                true
            }
            Some(local_country) => {
                tracing::debug!(
                    requested_proxy_countries = ?countries,
                    local_country = %local_country,
                    bypass = false,
                    "dynamic country proxy retained because local country differs"
                );
                false
            }
            None => {
                tracing::debug!(
                    requested_proxy_countries = ?countries,
                    bypass = false,
                    "dynamic country proxy retained because current country is unknown"
                );
                false
            }
        }
    }

    /// Determines whether a dynamic country pool requires HTTPS support.
    ///
    /// Returns `true` when the destination following the dynamic pool marker
    /// is a tunnel target (another proxy hop) or the original request uses
    /// HTTPS.
    ///
    /// # Arguments
    ///
    /// - `chain`: Chain containing the dynamic pool marker.
    /// - `pool_index`: Index of the dynamic pool marker inside `chain.nodes`.
    /// - `destination`: Original destination requested by the caller.
    ///
    /// # Returns
    ///
    /// `true` when the selected proxy must support HTTPS tunnelling.
    ///
    /// # Errors
    ///
    /// Returns an error when a later concrete proxy hop has no endpoint.
    pub(super) fn is_dynamic_country_https_request(
        &self,
        chain: &ProxyChain,
        pool_index: usize,
        destination: &Destination,
    ) -> Result<bool> {
        for node in chain.nodes.iter().skip(pool_index + 1) {
            if node.kind == TransportKind::Direct || node.kind == TransportKind::ProxyPool {
                continue;
            }
            let _endpoint = node.endpoint.as_deref().ok_or_else(|| {
                ProxyError::Config(format!("node '{}' has no endpoint", node.name))
            })?;
            return Ok(true);
        }
        Ok(destination.protocol == ApplicationProtocol::Https
            || (destination.protocol == ApplicationProtocol::Tcp && destination.port == 443))
    }
}
