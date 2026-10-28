//! Validation of one destination through a known proxy protocol.

use crate::core::{Destination, ProxyError, ProxyProtocol, ProxyRecord, Result};
use tokio::net::TcpStream;
use tokio::time;
use tokio::time::Instant;

use super::capabilities::AsyncProbeStream;
use super::http::send_http_connect;
use super::socks::{
    connect_socks5_with_local_dns_fallback, prepare_socks4_destination, send_socks4_connect,
};
use super::tls::wrap_tls;
use super::ProxyProbe;

impl ProxyProbe {
    /// Validates whether a proxy record can reach a specific destination.
    ///
    /// Unlike `probe`, this does not update the record's global status. It
    /// returns `Ok(())` when the destination is reachable through the proxy,
    /// or an error describing the failure so the caller can record a
    /// per-destination cooldown.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    /// * `destination` - Final origin used for capability checks and destination-specific exclusions.
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy is unreachable, the protocol handshake
    /// fails, or the destination rejects the tunnel.
    pub async fn validate_destination(
        &self,
        record: &ProxyRecord,
        destination: &Destination,
    ) -> Result<()> {
        time::timeout(
            self.config.timeouts.attempt,
            self.validate_destination_inner(record, destination),
        )
        .await
        .map_err(|_| ProxyError::Timeout("destination validation attempt"))?
    }

    /// Checks destination reachability through the record's known proxy protocol.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    /// * `destination` - Final origin used for capability checks and destination-specific exclusions.
    async fn validate_destination_inner(
        &self,
        record: &ProxyRecord,
        destination: &Destination,
    ) -> Result<()> {
        let protocol = record.protocol.as_ref().ok_or_else(|| {
            ProxyError::Config(
                "cannot validate destination without a known protocol on the record".to_string(),
            )
        })?;

        let authority = record.authority();
        let endpoint =
            Destination::from_authority(&authority, crate::core::ApplicationProtocol::Tcp)?;

        let _start = Instant::now();
        let stream = time::timeout(
            self.config.timeouts.proxy_connect,
            TcpStream::connect((&*endpoint.host_for_protocol(), endpoint.port)),
        )
        .await
        .map_err(|_| ProxyError::Timeout("destination validation tcp connect"))?
        .map_err(|_| {
            ProxyError::RouteUnavailable(format!(
                "cannot connect to proxy for destination validation: {}",
                destination.authority()
            ))
        })?;

        let mut stream: Box<dyn AsyncProbeStream> = if matches!(protocol, ProxyProtocol::Https) {
            Box::new(
                wrap_tls(
                    stream,
                    &endpoint.host_for_protocol(),
                    self.config.timeouts.tls_handshake,
                    true,
                )
                .await?,
            )
        } else {
            Box::new(stream)
        };

        match protocol {
            ProxyProtocol::Http | ProxyProtocol::Https => {
                send_http_connect(
                    &mut *stream,
                    destination,
                    self.config.timeouts.target_connect,
                    None,
                )
                .await?;
            }
            ProxyProtocol::Socks5 => {
                connect_socks5_with_local_dns_fallback(
                    &mut stream,
                    &record.host,
                    record.port,
                    destination,
                    &self.config.timeouts,
                    &self.dns_resolver,
                )
                .await?;
            }
            ProxyProtocol::Socks4 | ProxyProtocol::Socks4a => {
                let target = prepare_socks4_destination(
                    protocol,
                    destination,
                    self.config.timeouts.target_connect,
                    &self.dns_resolver,
                )
                .await?;
                send_socks4_connect(
                    &mut *stream,
                    &target,
                    self.config.timeouts.target_connect,
                    None,
                )
                .await?;
            }
        }

        Ok(())
    }
}
