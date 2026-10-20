//! Destination resolution, profile DNS setup and remembered SOCKS5 DNS fallbacks.

#[cfg(feature = "arachnea-dns")]
use crate::core::ProxyProfile;
use crate::core::{
    ApplicationProtocol, Destination, DestinationAddress, ProxyChain, ProxyError,
    ProxyNameResolutionMode, ProxyNode, Result, TransportKind,
};
use std::net::{IpAddr, SocketAddr};

use super::hops::endpoint_host;
use super::ArachneaProxyCore;

/// Returns whether a SOCKS5 error likely means remote hostname resolution failed.
///
/// # Parameters
///
/// - `error`: Proxy error returned by a SOCKS5 handshake.
///
/// # Returns
///
/// `true` for SOCKS5 host-unreachable or general-failure replies.
fn is_socks5_remote_dns_failure(error: &ProxyError) -> bool {
    if let ProxyError::Observation { source, .. } = error {
        return is_socks5_remote_dns_failure(source);
    }
    let ProxyError::UpstreamRejected(message) = error else {
        return false;
    };
    message.contains("socks5 reply code 4") || message.contains("socks5 reply code 1")
}

impl ArachneaProxyCore {
    /// Builds the default DNS configuration that corresponds to a proxy profile.
    ///
    /// The mapping intentionally avoids hard-coded public resolvers. Profiles
    /// that need a concrete forwarder source use the system resolver as the
    /// default upstream so callers can replace it with a private resolver by
    /// installing their own `ArachneaDnsCore`.
    ///
    /// # Parameters
    ///
    /// - `profile`: Proxy profile whose DNS counterpart should be built.
    ///
    /// # Returns
    ///
    /// DNS core configuration initialized from the equivalent DNS profile.
    #[cfg(feature = "arachnea-dns")]
    pub fn arachnea_dns_config_for_proxy_profile(
        profile: &ProxyProfile,
    ) -> arachnea_dns::core::CoreConfig {
        use arachnea_dns::core::{Upstream, UsageProfile as DnsUsageProfile};

        match profile {
            ProxyProfile::Direct => DnsUsageProfile::SystemRelay.config(),
            ProxyProfile::SingleProxy => {
                DnsUsageProfile::SingleForwarder(Upstream::system(None)).config()
            }
            ProxyProfile::MultiForwarder => {
                DnsUsageProfile::Resilience(vec![Upstream::system(None)]).config()
            }
            ProxyProfile::Privacy => {
                DnsUsageProfile::Privacy(vec![Upstream::system(None)]).config()
            }
            ProxyProfile::CensorshipResistance => DnsUsageProfile::Resilience(Vec::new()).config(),
            ProxyProfile::Advanced => DnsUsageProfile::Advanced.config(),
        }
    }

    /// Builds the default DNS core that corresponds to a proxy profile.
    ///
    /// # Parameters
    ///
    /// - `profile`: Proxy profile whose DNS counterpart should be built.
    ///
    /// # Returns
    ///
    /// DNS core initialized from the equivalent DNS profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the DNS profile cannot be resolved.
    #[cfg(feature = "arachnea-dns")]
    pub(super) fn default_arachnea_dns_core_for_proxy_profile(
        profile: &ProxyProfile,
    ) -> Result<arachnea_dns::core::ArachneaDnsCore> {
        arachnea_dns::core::ArachneaDnsCore::new(Self::arachnea_dns_config_for_proxy_profile(
            profile,
        ))
        .map_err(|error| ProxyError::Dns(error.to_string()))
    }

    /// Resolves the relay endpoint returned by a SOCKS5 UDP association.
    ///
    /// # Parameters
    ///
    /// - `node`: SOCKS5 node that returned the relay endpoint.
    /// - `relay_endpoint`: Relay endpoint returned by the upstream handshake.
    ///
    /// # Returns
    ///
    /// A concrete socket address usable for UDP relay packets.
    ///
    /// # Errors
    ///
    /// Returns an error when the endpoint is invalid or DNS resolution fails.
    pub(super) async fn resolve_udp_relay_addr(
        &self,
        node: &ProxyNode,
        relay_endpoint: &str,
    ) -> Result<SocketAddr> {
        let mut relay = Destination::from_authority(relay_endpoint, ApplicationProtocol::Udp)?;
        let relay_is_unspecified = match relay.address {
            crate::core::DestinationAddress::Ip(ip) => ip.is_unspecified(),
            crate::core::DestinationAddress::Host(_) => false,
        };
        if relay_is_unspecified {
            let endpoint = node.endpoint.as_deref().ok_or_else(|| {
                ProxyError::Config(format!("node '{}' has no endpoint", node.name))
            })?;
            let proxy_host = endpoint_host(endpoint)?;
            relay.address = proxy_host
                .parse::<IpAddr>()
                .map(crate::core::DestinationAddress::Ip)
                .unwrap_or_else(|_| crate::core::DestinationAddress::Host(proxy_host.to_string()));
        }

        match relay.address {
            crate::core::DestinationAddress::Ip(ip) => Ok(SocketAddr::new(ip, relay.port)),
            crate::core::DestinationAddress::Host(_) => self.resolve_destination_addr(&relay).await,
        }
    }

    /// Resolves a destination to a concrete socket address.
    ///
    /// # Parameters
    ///
    /// - `destination`: Destination whose hostname may need DNS resolution.
    ///
    /// # Returns
    ///
    /// Socket address with the destination port attached.
    ///
    /// # Errors
    ///
    /// Returns an error when DNS resolution fails or produces no usable address.
    pub(super) async fn resolve_destination_addr(
        &self,
        destination: &Destination,
    ) -> Result<SocketAddr> {
        match &destination.address {
            DestinationAddress::Ip(ip) => Ok(SocketAddr::new(*ip, destination.port)),
            DestinationAddress::Host(host) => {
                tracing::debug!(host = %host, port = %destination.port, "resolving destination address");
                let ip = self.dns_resolver.resolve_ip(host).await?;
                Ok(SocketAddr::new(ip, destination.port))
            }
        }
    }

    /// Resolves a hostname destination into an IP destination.
    ///
    /// # Parameters
    ///
    /// - `destination`: Destination that may contain a hostname.
    ///
    /// # Returns
    ///
    /// Destination with a literal IP address when resolution was required.
    ///
    /// # Errors
    ///
    /// Returns an error when DNS resolution fails.
    pub(super) async fn resolve_destination_to_ip(
        &self,
        destination: &Destination,
    ) -> Result<Destination> {
        let socket_addr = self.resolve_destination_addr(destination).await?;
        Ok(Destination {
            address: DestinationAddress::Ip(socket_addr.ip()),
            port: socket_addr.port(),
            protocol: destination.protocol.clone(),
        })
    }

    /// Resolves a hostname destination into an IPv4 destination.
    ///
    /// SOCKS4 cannot carry IPv6 addresses, so local DNS mode must select an
    /// IPv4 address before the protocol handshake.
    ///
    /// # Parameters
    ///
    /// - `destination`: Destination that may contain a hostname.
    ///
    /// # Returns
    ///
    /// Destination with a literal IPv4 address when resolution was required.
    ///
    /// # Errors
    ///
    /// Returns an error when DNS resolution fails or no IPv4 address is
    /// available.
    async fn resolve_destination_to_ipv4(&self, destination: &Destination) -> Result<Destination> {
        let ip = match &destination.address {
            DestinationAddress::Ip(IpAddr::V4(ip)) => IpAddr::V4(*ip),
            DestinationAddress::Ip(IpAddr::V6(_)) => {
                return Err(ProxyError::InvalidDestination(
                    "socks4 cannot encode ipv6 destinations".to_string(),
                ));
            }
            DestinationAddress::Host(host) => self.dns_resolver.resolve_ipv4(host).await?,
        };
        Ok(Destination {
            address: DestinationAddress::Ip(ip),
            port: destination.port,
            protocol: destination.protocol.clone(),
        })
    }

    /// Resolves a proxy handshake target when a node cannot receive hostnames.
    ///
    /// # Parameters
    ///
    /// - `node`: Proxy node that will receive the target.
    /// - `target`: Original destination for the proxy handshake.
    ///
    /// # Returns
    ///
    /// Original target when hostnames can be forwarded, otherwise an IP target.
    ///
    /// # Errors
    ///
    /// Returns an error when hostname resolution is required and fails.
    pub(super) async fn target_for_proxy_node(
        &self,
        node: &ProxyNode,
        target: &Destination,
    ) -> Result<Destination> {
        match node.effective_name_resolution() {
            ProxyNameResolutionMode::Local if node.kind == TransportKind::Socks4 => {
                self.resolve_destination_to_ipv4(target).await
            }
            ProxyNameResolutionMode::Local => self.resolve_destination_to_ip(target).await,
            ProxyNameResolutionMode::ProxyOnly => Ok(target.clone()),
            ProxyNameResolutionMode::ProxyThenLocalFallback => {
                if self.socks5_node_uses_remembered_local_dns(node) {
                    self.resolve_destination_to_ip(target).await
                } else {
                    Ok(target.clone())
                }
            }
        }
    }

    /// Flags one SOCKS5 fallback node for local DNS resolution after a remote DNS failure.
    ///
    /// # Parameters
    ///
    /// - `chain`: Chain whose SOCKS5 nodes may be flagged.
    /// - `target`: Destination that failed through remote proxy resolution.
    /// - `error`: Error returned by the previous chain attempt.
    ///
    /// # Returns
    ///
    /// `true` when a node was flagged and the caller should retry the chain.
    pub(super) async fn flag_socks5_local_dns_fallback(
        &self,
        chain: &ProxyChain,
        target: &Destination,
        error: &ProxyError,
    ) -> bool {
        if !is_socks5_remote_dns_failure(error)
            || !matches!(&target.address, DestinationAddress::Host(_))
            || self.resolve_destination_to_ip(target).await.is_err()
        {
            return false;
        }

        let Some(node) = chain.nodes.iter().find(|node| {
            matches!(node.kind, TransportKind::Socks5 | TransportKind::TorSocks)
                && node.effective_name_resolution()
                    == ProxyNameResolutionMode::ProxyThenLocalFallback
                && !self.socks5_node_uses_remembered_local_dns(node)
        }) else {
            return false;
        };

        self.remember_socks5_local_dns_node(node);
        tracing::debug!(
            proxy_node = %node.name,
            "remembering socks5 node for local dns resolution after remote dns failure"
        );
        true
    }

    /// Returns whether a SOCKS5 fallback node is remembered as local-DNS-only.
    ///
    /// # Parameters
    ///
    /// - `node`: Proxy node to inspect.
    ///
    /// # Returns
    ///
    /// `true` when previous fallback handling marked this node.
    fn socks5_node_uses_remembered_local_dns(&self, node: &ProxyNode) -> bool {
        self.socks5_local_dns_nodes
            .read()
            .map(|nodes| nodes.contains(&node.name))
            .unwrap_or(false)
    }

    /// Remembers a SOCKS5 fallback node as requiring local DNS resolution.
    ///
    /// # Parameters
    ///
    /// - `node`: Proxy node to flag by stable name.
    fn remember_socks5_local_dns_node(&self, node: &ProxyNode) {
        if let Ok(mut nodes) = self.socks5_local_dns_nodes.write() {
            nodes.insert(node.name.clone());
        }
    }
}
