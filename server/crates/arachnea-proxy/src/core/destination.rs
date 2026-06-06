use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::core::{ProxyError, Result};

/// Network address component of a proxy destination.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DestinationAddress {
    /// A literal IP address supplied by the caller or client protocol.
    Ip(IpAddr),
    /// A hostname that should be preserved until a transport needs an IP.
    Host(String),
}

/// Application-level intent associated with a destination.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationProtocol {
    /// Raw TCP traffic.
    Tcp,
    /// Raw UDP traffic.
    Udp,
    /// Cleartext HTTP traffic.
    Http,
    /// HTTPS or other TLS-over-TCP traffic.
    Https,
    /// WebSocket traffic.
    WebSocket,
    /// A caller-defined protocol label.
    Other(String),
}

impl Default for ApplicationProtocol {
    /// Returns raw TCP as the default protocol hint.
    fn default() -> Self {
        Self::Tcp
    }
}

/// Destination requested by a client or library caller.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Destination {
    /// Address to reach, preserving hostnames where possible.
    pub address: DestinationAddress,
    /// Explicit target port.
    pub port: u16,
    /// Application-level protocol hint for policies and routing.
    pub protocol: ApplicationProtocol,
}

impl Destination {
    /// Creates a TCP destination from a hostname and port.
    ///
    /// # Parameters
    ///
    /// - `host`: Hostname to preserve in the destination.
    /// - `port`: Target port.
    ///
    /// # Returns
    ///
    /// TCP destination using a hostname address.
    pub fn host_port(host: impl Into<String>, port: u16) -> Self {
        Self {
            address: DestinationAddress::Host(host.into()),
            port,
            protocol: ApplicationProtocol::Tcp,
        }
    }

    /// Creates a TCP destination from an IP address and port.
    ///
    /// # Parameters
    ///
    /// - `ip`: Target IP address.
    /// - `port`: Target port.
    ///
    /// # Returns
    ///
    /// TCP destination using a literal IP address.
    pub fn ip_port(ip: IpAddr, port: u16) -> Self {
        Self {
            address: DestinationAddress::Ip(ip),
            port,
            protocol: ApplicationProtocol::Tcp,
        }
    }

    /// Returns a copy of this destination with a different protocol hint.
    ///
    /// # Parameters
    ///
    /// - `protocol`: Application protocol hint to assign.
    ///
    /// # Returns
    ///
    /// Updated destination.
    pub fn with_protocol(mut self, protocol: ApplicationProtocol) -> Self {
        self.protocol = protocol;
        self
    }

    /// Parses an authority value such as `example.com:443` or `[::1]:443`.
    ///
    /// # Parameters
    ///
    /// - `authority`: Authority string to parse.
    /// - `protocol`: Application protocol hint to attach.
    ///
    /// # Returns
    ///
    /// Parsed destination.
    ///
    /// # Errors
    ///
    /// Returns an error when the authority is invalid.
    pub fn from_authority(authority: &str, protocol: ApplicationProtocol) -> Result<Self> {
        let (host, port) = split_authority(authority)?;
        let address = match IpAddr::from_str(host) {
            Ok(ip) => DestinationAddress::Ip(ip),
            Err(_) => DestinationAddress::Host(host.to_string()),
        };
        Ok(Self {
            address,
            port,
            protocol,
        })
    }

    /// Returns the host component without brackets.
    pub fn host(&self) -> &str {
        match &self.address {
            DestinationAddress::Ip(ip) => match ip {
                IpAddr::V4(_) => "",
                IpAddr::V6(_) => "",
            },
            DestinationAddress::Host(host) => host.as_str(),
        }
    }

    /// Returns the address formatted as a host value for proxy protocols.
    pub fn host_for_protocol(&self) -> String {
        match &self.address {
            DestinationAddress::Ip(ip) => ip.to_string(),
            DestinationAddress::Host(host) => host.clone(),
        }
    }

    /// Returns `host:port`, with IPv6 addresses bracketed.
    pub fn authority(&self) -> String {
        match &self.address {
            DestinationAddress::Ip(IpAddr::V6(ip)) => format!("[{ip}]:{}", self.port),
            DestinationAddress::Ip(ip) => format!("{ip}:{}", self.port),
            DestinationAddress::Host(host) => format!("{host}:{}", self.port),
        }
    }

    /// Returns true when the destination is a literal private or loopback address.
    pub fn is_private_or_loopback_ip(&self) -> bool {
        match self.address {
            DestinationAddress::Ip(IpAddr::V4(ip)) => {
                ip.is_private() || ip.is_loopback() || ip.is_link_local()
            }
            DestinationAddress::Ip(IpAddr::V6(ip)) => {
                ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
            }
            DestinationAddress::Host(_) => false,
        }
    }
}

impl fmt::Display for Destination {
    /// Formats the destination as an authority string.
    ///
    /// # Parameters
    ///
    /// - `f`: Formatter receiving the destination authority.
    ///
    /// # Errors
    ///
    /// Returns an error when writing to the formatter fails.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.authority())
    }
}

/// Splits an authority string into host and port components.
///
/// # Parameters
///
/// - `authority`: Authority formatted as `host:port` or `[ipv6]:port`.
///
/// # Returns
///
/// Borrowed host component and parsed port.
///
/// # Errors
///
/// Returns an error when the authority is missing a host, port, or valid IPv6
/// bracket syntax.
fn split_authority(authority: &str) -> Result<(&str, u16)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| ProxyError::InvalidDestination(authority.to_string()))?;
        let host = &rest[..end];
        let port = rest[end + 1..]
            .strip_prefix(':')
            .ok_or_else(|| ProxyError::InvalidDestination(authority.to_string()))?
            .parse::<u16>()
            .map_err(|_| ProxyError::InvalidDestination(authority.to_string()))?;
        return Ok((host, port));
    }

    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| ProxyError::InvalidDestination(authority.to_string()))?;
    if host.is_empty() {
        return Err(ProxyError::InvalidDestination(authority.to_string()));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| ProxyError::InvalidDestination(authority.to_string()))?;
    Ok((host, port))
}
