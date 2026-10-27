//! Direct UDP connections and explicit SOCKS5 UDP associations.

use crate::core::transport::socks;
use crate::core::{
    ConnectRequest, Destination, ProxyError, Result, Socks5UdpAssociation, TransportKind,
};
use tokio::net::UdpSocket;

use super::ArachneaProxyCore;

impl ArachneaProxyCore {
    /// Opens a direct connected UDP socket to a destination.
    ///
    /// Proxied UDP is intentionally kept explicit: non-direct UDP routes return
    /// an unsupported error until a transport-specific association is selected.
    ///
    /// # Parameters
    ///
    /// - `destination`: UDP target destination.
    ///
    /// # Returns
    ///
    /// Connected UDP socket for direct routes.
    ///
    /// # Errors
    ///
    /// Returns an error when policy validation fails, the route is proxied, or
    /// socket setup fails.
    pub async fn connect_udp(&self, destination: Destination) -> Result<UdpSocket> {
        self.connect_udp_request(ConnectRequest::new(destination))
            .await
    }

    /// Opens a direct connected UDP socket using a full connect request.
    ///
    /// # Parameters
    ///
    /// - `request`: UDP destination and client context for route selection.
    ///
    /// # Returns
    ///
    /// Connected UDP socket for direct routes.
    ///
    /// # Errors
    ///
    /// Returns an error when policy validation fails, the route is proxied, or
    /// socket setup fails.
    pub fn connect_udp_request(
        &self,
        request: ConnectRequest,
    ) -> futures::future::BoxFuture<'_, Result<UdpSocket>> {
        Box::pin(self.connect_udp_request_inner(request))
    }

    async fn connect_udp_request_inner(&self, request: ConnectRequest) -> Result<UdpSocket> {
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
        let resolved_chain = self.resolve_proxy_pools_in_chain(&chain, &request).await?;
        if resolved_chain
            .chain
            .nodes
            .iter()
            .any(|node| node.kind != TransportKind::Direct)
        {
            return Err(ProxyError::Unsupported(
                "proxied udp requires a transport-specific association",
            ));
        }
        let destination_addr = self.resolve_destination_addr(&request.destination).await?;
        let bind_addr = if destination_addr.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        let socket = UdpSocket::bind(bind_addr).await?;
        socket.connect(destination_addr).await?;
        Ok(socket)
    }

    /// Opens a SOCKS5 UDP association when the selected UDP route uses one SOCKS5 upstream.
    ///
    /// Multi-hop UDP is deliberately not hidden here: routes with more than one
    /// proxy hop return an unsupported error until transport-specific semantics
    /// are implemented for each chain shape.
    ///
    /// # Parameters
    ///
    /// - `destination`: UDP target used for route selection and policy checks.
    ///
    /// # Returns
    ///
    /// SOCKS5 UDP association for a single SOCKS5 upstream route.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected route is not a single SOCKS5 upstream
    /// or the association handshake fails.
    pub async fn connect_socks5_udp_association(
        &self,
        destination: Destination,
    ) -> Result<Socks5UdpAssociation> {
        self.connect_socks5_udp_association_request(ConnectRequest::new(destination))
            .await
    }

    /// Opens a SOCKS5 UDP association using a full connect request.
    ///
    /// # Parameters
    ///
    /// - `request`: UDP destination and client context for route selection.
    ///
    /// # Returns
    ///
    /// SOCKS5 UDP association for a single SOCKS5 upstream route.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected route is not a single SOCKS5 upstream
    /// or the association handshake fails.
    pub async fn connect_socks5_udp_association_request(
        &self,
        request: ConnectRequest,
    ) -> Result<Socks5UdpAssociation> {
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
        let resolved_chain = self.resolve_proxy_pools_in_chain(&chain, &request).await?;
        let proxy_nodes: Vec<_> = resolved_chain
            .chain
            .nodes
            .iter()
            .filter(|node| node.kind != TransportKind::Direct)
            .cloned()
            .collect();
        if proxy_nodes.len() != 1
            || !matches!(
                proxy_nodes[0].kind,
                TransportKind::Socks5 | TransportKind::TorSocks
            )
        {
            return Err(ProxyError::Unsupported(
                "socks5 udp associate requires a single socks5 upstream route",
            ));
        }

        let node = &proxy_nodes[0];
        let mut control = self
            .connect_first_proxy(node, &resolved_chain.chain)
            .await?;
        let credentials = node.credentials()?;
        let relay_endpoint = socks::udp_associate(
            &mut control,
            self.proxy_timeouts(node).handshake_duration(),
            credentials.as_ref(),
        )
        .await?;
        control.metadata_mut().route_name = Some(decision.selected_chain);
        control.metadata_mut().transport_kind = Some(node.kind.clone());
        control.metadata_mut().hops.push(node.name.clone());
        let relay_addr = self.resolve_udp_relay_addr(node, &relay_endpoint).await?;
        let bind_addr = if relay_addr.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        let socket = UdpSocket::bind(bind_addr).await?;
        self.stats.record_opened();
        Ok(Socks5UdpAssociation::new(socket, relay_addr, control))
    }
}
