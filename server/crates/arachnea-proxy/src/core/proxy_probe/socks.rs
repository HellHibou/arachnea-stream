//! SOCKS probe handshakes and protocol-specific DNS fallbacks.

use crate::core::dns_resolver::ProxyDnsResolver;
use crate::core::{Destination, DestinationAddress, ProxyError, ProxyProtocol, Result};
use std::net::IpAddr;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time;

use super::capabilities::{tcp_connect, AsyncProbeStream};
use super::config::ProbeTimeoutConfig;
use super::ProxyProbe;

/// Connects through SOCKS5, retrying with local DNS only after a likely remote
/// hostname-resolution failure.
///
/// # Arguments
/// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
/// * `proxy_host` - Proxy endpoint hostname used when reconnecting after a remote DNS failure.
/// * `proxy_port` - Proxy endpoint TCP port used for the fallback connection.
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
/// * `timeouts` - Per-phase and overall limits applied to connection and protocol exchanges.
/// * `dns_resolver` - Resolver used only when the proxy protocol requires local address resolution.
///
pub(super) async fn connect_socks5_with_local_dns_fallback(
    stream: &mut Box<dyn AsyncProbeStream>,
    proxy_host: &str,
    proxy_port: u16,
    destination: &Destination,
    timeouts: &ProbeTimeoutConfig,
    dns_resolver: &ProxyDnsResolver,
) -> Result<()> {
    let first_result =
        send_socks5_connect(&mut **stream, destination, timeouts.proxy_handshake, None).await;
    let Err(error) = first_result else {
        return Ok(());
    };
    if !is_socks5_remote_dns_failure(&error)
        || !matches!(destination.address, DestinationAddress::Host(_))
    {
        return Err(error);
    }

    let local_destination =
        resolve_destination_to_ip(destination, timeouts.target_connect, dns_resolver).await?;
    let (_, fallback_stream) = tcp_connect(proxy_host, proxy_port, timeouts.proxy_connect).await?;
    let mut fallback_stream: Box<dyn AsyncProbeStream> = Box::new(fallback_stream);
    send_socks5_connect(
        &mut *fallback_stream,
        &local_destination,
        timeouts.proxy_handshake,
        None,
    )
    .await?;
    *stream = fallback_stream;
    Ok(())
}

/// Prepares the target according to the declared SOCKS4 protocol variant.
///
/// # Arguments
/// * `protocol` - SOCKS4 or SOCKS4a variant deciding whether local IPv4 resolution is required.
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
/// * `timeout` - Maximum duration allowed for this network phase.
/// * `dns_resolver` - Resolver used only when the proxy protocol requires local address resolution.
///
pub(super) async fn prepare_socks4_destination(
    protocol: &ProxyProtocol,
    destination: &Destination,
    timeout: Duration,
    dns_resolver: &ProxyDnsResolver,
) -> Result<Destination> {
    match protocol {
        ProxyProtocol::Socks4 => {
            resolve_destination_to_ipv4(destination, timeout, dns_resolver).await
        }
        ProxyProtocol::Socks4a => Ok(destination.clone()),
        _ => Err(ProxyError::Config(
            "SOCKS4 destination preparation requires SOCKS4 or SOCKS4a".to_string(),
        )),
    }
}

/// Resolves a hostname destination into the first available IP address.
///
/// # Arguments
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
/// * `timeout` - Maximum duration allowed for this network phase.
/// * `dns_resolver` - Resolver used only when the proxy protocol requires local address resolution.
///
async fn resolve_destination_to_ip(
    destination: &Destination,
    timeout: Duration,
    dns_resolver: &ProxyDnsResolver,
) -> Result<Destination> {
    let ip = match &destination.address {
        DestinationAddress::Ip(ip) => *ip,
        DestinationAddress::Host(host) => time::timeout(timeout, dns_resolver.resolve_ip(host))
            .await
            .map_err(|_| ProxyError::Timeout("local target dns resolution"))??,
    };
    Ok(Destination {
        address: DestinationAddress::Ip(ip),
        port: destination.port,
        protocol: destination.protocol.clone(),
    })
}

/// Resolves a hostname destination into the first available IPv4 address.
///
/// # Arguments
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
/// * `timeout` - Maximum duration allowed for this network phase.
/// * `dns_resolver` - Resolver used only when the proxy protocol requires local address resolution.
///
async fn resolve_destination_to_ipv4(
    destination: &Destination,
    timeout: Duration,
    dns_resolver: &ProxyDnsResolver,
) -> Result<Destination> {
    let ip = match &destination.address {
        DestinationAddress::Ip(std::net::IpAddr::V4(ip)) => *ip,
        DestinationAddress::Ip(std::net::IpAddr::V6(_)) => {
            return Err(ProxyError::InvalidDestination(
                "socks4 cannot encode ipv6 destinations".to_string(),
            ));
        }
        DestinationAddress::Host(host) => {
            match time::timeout(timeout, dns_resolver.resolve_ipv4(host))
                .await
                .map_err(|_| ProxyError::Timeout("local target ipv4 dns resolution"))??
            {
                std::net::IpAddr::V4(ip) => ip,
                std::net::IpAddr::V6(_) => unreachable!("resolve_ipv4 returned an IPv6 address"),
            }
        }
    };
    Ok(Destination {
        address: DestinationAddress::Ip(std::net::IpAddr::V4(ip)),
        port: destination.port,
        protocol: destination.protocol.clone(),
    })
}

/// Performs a SOCKS5 CONNECT handshake over an existing stream.
///
/// # Arguments
/// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
/// * `timeout` - Maximum duration allowed for this network phase.
/// * `_credentials` - Reserved credentials; authentication is not sent by this probe implementation.
///
async fn send_socks5_connect<S>(
    stream: &mut S,
    destination: &Destination,
    timeout: Duration,
    _credentials: Option<&str>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    time::timeout(timeout, async {
        // Negotiate: propose no-auth only (0x00)
        stream.write_all(&[0x05, 0x01, 0x00]).await?;
        let mut response = [0u8; 2];
        stream.read_exact(&mut response).await?;
        if response != [0x05, 0x00] {
            let method = response[1];
            if method == 0xFF {
                return Err(ProxyError::UpstreamRejected(
                    "socks5: no acceptable authentication method (0xFF)".to_string(),
                ));
            }
            return Err(ProxyError::UpstreamRejected(format!(
                "socks5: server selected unexpected method 0x{method:02x}"
            )));
        }

        // CONNECT request
        let request = encode_socks5_request(destination)?;
        stream.write_all(&request).await?;

        // Read reply
        let mut reply = [0u8; 4];
        stream.read_exact(&mut reply).await?;
        if reply[0] != 0x05 {
            return Err(ProxyError::Protocol(
                "socks5: invalid reply version".to_string(),
            ));
        }
        if reply[1] != 0x00 {
            return Err(ProxyError::UpstreamRejected(format!(
                "socks5: connect rejected with code 0x{:02x}",
                reply[1]
            )));
        }
        // Read remaining address/port bytes of the SOCKS5 reply
        let addr_len = match reply[3] {
            0x01 => 4,
            0x03 => {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await?;
                len[0] as usize
            }
            0x04 => 16,
            _ => {
                return Err(ProxyError::Protocol(
                    "socks5: unknown address type".to_string(),
                ));
            }
        };
        let mut rest = vec![0u8; addr_len + 2];
        stream.read_exact(&mut rest).await?;

        Ok(())
    })
    .await
    .map_err(|_| ProxyError::Timeout("socks5 connect handshake"))?
}

/// Performs a SOCKS4 or SOCKS4a CONNECT handshake over an existing stream.
///
/// # Arguments
/// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
/// * `timeout` - Maximum duration allowed for this network phase.
/// * `_user_id` - Reserved SOCKS4 user ID; the probe sends an empty user ID.
///
pub(super) async fn send_socks4_connect<S>(
    stream: &mut S,
    destination: &Destination,
    timeout: Duration,
    _user_id: Option<&str>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    time::timeout(timeout, async {
        let mut request = Vec::with_capacity(9);
        request.push(0x04);
        request.push(0x01);
        request.extend_from_slice(&destination.port.to_be_bytes());
        match &destination.address {
            DestinationAddress::Ip(IpAddr::V4(ip)) => {
                request.extend_from_slice(&ip.octets());
                request.push(0x00);
            }
            DestinationAddress::Ip(IpAddr::V6(_)) => {
                return Err(ProxyError::InvalidDestination(
                    "socks4 cannot encode ipv6 destinations".to_string(),
                ));
            }
            DestinationAddress::Host(host) => {
                if host.is_empty() || host.as_bytes().contains(&0) {
                    return Err(ProxyError::InvalidDestination(
                        "socks4a hostnames must be non-empty and contain no nul bytes".to_string(),
                    ));
                }
                request.extend_from_slice(&[0, 0, 0, 1]);
                request.push(0x00);
                request.extend_from_slice(host.as_bytes());
                request.push(0x00);
            }
        }

        stream.write_all(&request).await?;

        let mut reply = [0u8; 8];
        stream.read_exact(&mut reply).await?;
        if reply[0] != 0x00 {
            return Err(ProxyError::Protocol(
                "socks4: invalid reply null byte".to_string(),
            ));
        }
        if reply[1] != 0x5A {
            return Err(ProxyError::UpstreamRejected(format!(
                "socks4: connect rejected with code 0x{:02x}",
                reply[1]
            )));
        }

        Ok(())
    })
    .await
    .map_err(|_| ProxyError::Timeout("socks4 connect handshake"))?
}

/// Encodes a SOCKS5 CONNECT request.
///
/// # Arguments
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
///
fn encode_socks5_request(destination: &Destination) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(22);
    buf.push(0x05); // version
    buf.push(0x01); // CONNECT
    buf.push(0x00); // reserved

    match &destination.address {
        DestinationAddress::Ip(std::net::IpAddr::V4(ip)) => {
            buf.push(0x01);
            buf.extend_from_slice(&ip.octets());
        }
        DestinationAddress::Ip(std::net::IpAddr::V6(ip)) => {
            buf.push(0x04);
            buf.extend_from_slice(&ip.octets());
        }
        DestinationAddress::Host(host) => {
            buf.push(0x03);
            let host_bytes = host.as_bytes();
            if host_bytes.len() > 255 {
                return Err(ProxyError::Config(
                    "socks5 hostname too long for handshake encoding".to_string(),
                ));
            }
            buf.push(host_bytes.len() as u8);
            buf.extend_from_slice(host_bytes);
        }
    }

    buf.extend_from_slice(&destination.port.to_be_bytes());
    Ok(buf)
}

/// Returns `true` when the SOCKS5 error indicates auth is required.
///
/// # Arguments
/// * `error` - Observed error to classify without changing its cause.
///
pub(super) fn is_socks5_auth_required(error: &ProxyError) -> bool {
    if let ProxyError::UpstreamRejected(msg) = error {
        msg.contains("0xFF")
    } else {
        false
    }
}

/// Returns true when a SOCKS5 rejection likely represents remote DNS failure.
///
/// # Arguments
/// * `error` - Observed error to classify without changing its cause.
///
fn is_socks5_remote_dns_failure(error: &ProxyError) -> bool {
    let ProxyError::UpstreamRejected(message) = error else {
        return false;
    };
    message.contains("code 0x01") || message.contains("code 0x04")
}

impl ProxyProbe {}
