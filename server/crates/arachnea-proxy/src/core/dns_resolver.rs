use std::net::IpAddr;

#[cfg(feature = "arachnea-dns")]
use std::sync::Arc;

use tokio::net::lookup_host;

use crate::core::{ProxyError, Result};

/// Shared local hostname resolver used by proxy runtime and probe paths.
#[derive(Clone, Default)]
pub(crate) struct ProxyDnsResolver {
    #[cfg(feature = "arachnea-dns")]
    dns_core: Option<Arc<arachnea_dns::core::ArachneaDnsCore>>,
}

impl ProxyDnsResolver {
    /// Creates a resolver that falls back to the system resolver.
    pub(crate) fn system() -> Self {
        Self::default()
    }

    /// Creates a resolver backed by the supplied Arachnea DNS core.
    #[cfg(feature = "arachnea-dns")]
    pub(crate) fn with_arachnea_dns(dns_core: Arc<arachnea_dns::core::ArachneaDnsCore>) -> Self {
        Self {
            dns_core: Some(dns_core),
        }
    }

    /// Returns whether local resolution uses an Arachnea DNS core.
    #[cfg(feature = "arachnea-dns")]
    pub(crate) fn uses_arachnea_dns(&self) -> bool {
        self.dns_core.is_some()
    }

    /// Resolves a hostname to the first available IP address.
    pub(crate) async fn resolve_ip(&self, host: &str) -> Result<IpAddr> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            tracing::debug!(host = %host, ip = %ip, "destination is a literal ip address");
            return Ok(ip);
        }
        tracing::debug!(host = %host, "resolving hostname via dns");

        #[cfg(feature = "arachnea-dns")]
        if let Some(dns_core) = &self.dns_core {
            return dns_core
                .resolve_ip(host)
                .await
                .map_err(|error| ProxyError::Dns(error.to_string()))?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    ProxyError::Dns(format!("hostname '{host}' resolved to no address"))
                });
        }

        lookup_host((host, 0))
            .await
            .map_err(|error| ProxyError::Dns(error.to_string()))?
            .next()
            .map(|address| address.ip())
            .ok_or_else(|| ProxyError::Dns(format!("hostname '{host}' resolved to no address")))
    }

    /// Resolves a hostname to the first available IPv4 address.
    pub(crate) async fn resolve_ipv4(&self, host: &str) -> Result<IpAddr> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return match ip {
                IpAddr::V4(_) => Ok(ip),
                IpAddr::V6(_) => Err(no_ipv4_error(host)),
            };
        }

        #[cfg(feature = "arachnea-dns")]
        if let Some(dns_core) = &self.dns_core {
            return dns_core
                .resolve_ip(host)
                .await
                .map_err(|error| ProxyError::Dns(error.to_string()))?
                .into_iter()
                .find(IpAddr::is_ipv4)
                .ok_or_else(|| no_ipv4_error(host));
        }

        lookup_host((host, 0))
            .await
            .map_err(|error| ProxyError::Dns(error.to_string()))?
            .find(|address| address.ip().is_ipv4())
            .map(|address| address.ip())
            .ok_or_else(|| no_ipv4_error(host))
    }
}

fn no_ipv4_error(host: &str) -> ProxyError {
    ProxyError::Dns(format!("hostname '{host}' resolved to no ipv4 address"))
}
