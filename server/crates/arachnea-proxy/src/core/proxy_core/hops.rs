//! Per-hop proxy handshakes and upstream TLS setup.

use crate::core::transport::{http, socks, tls};
use crate::core::{
    ApplicationProtocol, ConnectMetadata, Destination, ProxyChain, ProxyError, ProxyNode,
    ProxyStream, Result, TransportKind,
};
use tokio::net::TcpStream;
use tokio::time;

use super::ArachneaProxyCore;

/// Extracts the host portion from a `host:port` endpoint.
///
/// # Parameters
///
/// - `endpoint`: Endpoint string that may contain a bracketed IPv6 address.
///
/// # Returns
///
/// Host value without IPv6 brackets.
///
/// # Errors
///
/// Returns an error when the endpoint is not a valid authority.
pub(super) fn endpoint_host(endpoint: &str) -> Result<&str> {
    if let Some(rest) = endpoint.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| ProxyError::InvalidDestination(endpoint.to_string()))?;
        return Ok(&rest[..end]);
    }
    let (host, _) = endpoint
        .rsplit_once(':')
        .ok_or_else(|| ProxyError::InvalidDestination(endpoint.to_string()))?;
    Ok(host)
}

impl ArachneaProxyCore {
    /// Returns configured timeouts capped only for a selected localized endpoint.
    ///
    /// # Arguments
    /// - `node`: Proxy hop whose authority must belong to the active search.
    pub(super) fn proxy_timeouts(&self, node: &ProxyNode) -> crate::core::TimeoutConfig {
        let configured = &self.config.as_config().timeouts;
        if let Some(token) = self
            .pinned_route
            .as_ref()
            .and_then(|route| route.2.as_ref())
            .filter(|_| {
                self.search_context.is_some()
                    || crate::core::ProxySearchContext::current().is_some()
            })
        {
            if node.endpoint.as_deref() == Some(token.endpoint()) {
                let mut timeouts = configured.clone();
                timeouts.connect_ms = timeouts.connect_ms.min(if token.reuses_destination() {
                    30_000
                } else {
                    3_000
                });
                timeouts.handshake_ms = timeouts.handshake_ms.min(if token.reuses_destination() {
                    30_000
                } else {
                    5_000
                });
                return timeouts;
            }
        }
        match (
            crate::core::ProxySearchContext::current(),
            node.endpoint.as_deref(),
        ) {
            (Some(search), Some(endpoint)) => search.connection_timeouts(endpoint, configured),
            _ => configured.clone(),
        }
    }

    /// Opens the TCP connection to the first proxy node in a chain.
    ///
    /// # Parameters
    ///
    /// - `first`: First non-direct proxy node.
    /// - `chain`: Chain used for route metadata.
    ///
    /// # Returns
    ///
    /// A proxy stream connected to `first`.
    ///
    /// # Errors
    ///
    /// Returns an error when the node has no endpoint, the endpoint is invalid,
    /// or the TCP connection fails.
    pub(super) async fn connect_first_proxy(
        &self,
        first: &ProxyNode,
        chain: &ProxyChain,
    ) -> Result<ProxyStream> {
        let endpoint = first
            .endpoint
            .as_deref()
            .ok_or_else(|| ProxyError::Config(format!("node '{}' has no endpoint", first.name)))?;
        let first_destination = Destination::from_authority(endpoint, ApplicationProtocol::Tcp)?;
        tracing::debug!(
            node = %first.name,
            endpoint = %endpoint,
            chain = %chain.name,
            transport = ?first.kind,
            "connecting to first proxy hop"
        );
        let first_addr = self
            .resolve_destination_addr(&first_destination)
            .await
            .map_err(|error| error.at_phase(crate::core::ProxyObservationPhase::EndpointConnect))?;
        let tcp = time::timeout(
            self.proxy_timeouts(first).connect_duration(),
            TcpStream::connect(first_addr),
        )
        .await
        .map_err(|_| {
            ProxyError::Timeout("connect to first proxy")
                .at_phase(crate::core::ProxyObservationPhase::EndpointConnect)
        })?
        .map_err(|error| {
            ProxyError::Io(error).at_phase(crate::core::ProxyObservationPhase::EndpointConnect)
        })?;

        Ok(ProxyStream::new(
            tcp,
            ConnectMetadata {
                route_name: Some(chain.name.clone()),
                transport_kind: Some(first.kind.clone()),
                hops: Vec::new(),
                selection_token: None,
            },
        ))
    }

    /// Wraps a proxy stream in client TLS for an HTTPS proxy hop.
    ///
    /// # Parameters
    ///
    /// - `stream`: Existing stream connected to the HTTPS proxy endpoint.
    /// - `node`: HTTPS proxy node whose endpoint supplies the TLS server name.
    ///
    /// # Returns
    ///
    /// A `ProxyStream` containing the TLS-protected stream.
    ///
    /// # Errors
    ///
    /// Returns an error when the endpoint is invalid or TLS setup fails.
    pub(super) async fn wrap_https_proxy_stream(
        &self,
        stream: ProxyStream,
        node: &ProxyNode,
    ) -> Result<ProxyStream> {
        let endpoint = node
            .endpoint
            .as_deref()
            .ok_or_else(|| ProxyError::Config(format!("node '{}' has no endpoint", node.name)))?;
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
        Ok(ProxyStream::new(tls_stream, metadata))
    }

    /// Performs the protocol handshake for one proxy hop toward a target.
    ///
    /// # Parameters
    ///
    /// - `stream`: Stream already connected to `node`.
    /// - `node`: Proxy node whose protocol should be used for the handshake.
    /// - `target`: Destination requested through `node`.
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy protocol rejects the target or the
    /// transport kind is unsupported.
    pub(super) async fn connect_proxy_hop(
        &self,
        stream: &mut ProxyStream,
        node: &ProxyNode,
        target: &Destination,
    ) -> Result<()> {
        let target = self.target_for_proxy_node(node, target).await?;
        let result = match node.kind {
            TransportKind::HttpProxy | TransportKind::HttpsProxy => {
                let credentials = node.credentials()?;
                http::connect_tunnel(
                    stream,
                    &target,
                    self.proxy_timeouts(node).handshake_duration(),
                    credentials.as_ref(),
                )
                .await
            }
            TransportKind::Socks5 | TransportKind::TorSocks => {
                let credentials = node.credentials()?;
                socks::connect_tunnel(
                    stream,
                    &target,
                    self.proxy_timeouts(node).handshake_duration(),
                    credentials.as_ref(),
                )
                .await
            }
            TransportKind::Socks4 => {
                socks::connect_tunnel_v4(
                    stream,
                    &target,
                    self.proxy_timeouts(node).handshake_duration(),
                    node.username.as_deref(),
                )
                .await
            }
            TransportKind::ExternalTunnel => {
                Err(ProxyError::Unsupported("external tunnel transport"))
            }
            TransportKind::ProxyPool => Err(ProxyError::Unsupported(
                "unresolved proxy pool marker in transport chain",
            )),
            TransportKind::MasqueUdp => {
                Err(ProxyError::Unsupported("masque connect-udp transport"))
            }
            TransportKind::Direct => Ok(()),
        };

        match &result {
            Ok(()) => {
                tracing::debug!(
                    node = %node.name,
                    target = %target.authority(),
                    transport = ?node.kind,
                    "proxy hop connected"
                );
            }
            Err(error) => {
                tracing::debug!(
                    node = %node.name,
                    target = %target.authority(),
                    transport = ?node.kind,
                    %error,
                    "proxy hop connection failed"
                );
            }
        }

        result
    }
}
