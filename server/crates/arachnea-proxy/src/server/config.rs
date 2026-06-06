use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;

use crate::core::ProxyConfig;
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use serde::{Deserialize, Serialize};

use crate::server::{ClientAcl, ProxyAuthConfig};

/// Complete TOML file configuration for the proxy server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileConfig {
    /// Core proxy configuration.
    #[serde(default)]
    pub core: ProxyConfig,
    /// Server listener and ACL configuration.
    #[serde(default)]
    pub server: ServerConfig,
}

impl Default for FileConfig {
    /// Creates the default file configuration.
    fn default() -> Self {
        Self {
            core: ProxyConfig::default(),
            server: ServerConfig::default(),
        }
    }
}

/// Network listener and safety settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// HTTP proxy listener address.
    pub listen_http: Option<SocketAddr>,
    /// SOCKS5 listener address.
    pub listen_socks: Option<SocketAddr>,
    /// HTTPS proxy listener address.
    pub listen_https: Option<SocketAddr>,
    /// DER-encoded certificate chain path for the HTTPS listener.
    pub tls_cert_der_path: Option<PathBuf>,
    /// DER-encoded private key path for the HTTPS listener.
    pub tls_key_der_path: Option<PathBuf>,
    /// PEM-encoded certificate chain path for the HTTPS listener.
    pub tls_cert_pem_path: Option<PathBuf>,
    /// PEM-encoded private key path for the HTTPS listener.
    pub tls_key_pem_path: Option<PathBuf>,
    /// Explicit escape hatch for public binds without ACLs.
    pub allow_unsafe_public_bind_without_acl: bool,
    /// Maximum HTTP header size in bytes.
    pub max_http_header_bytes: usize,
    /// Optional inbound proxy authentication.
    pub authentication: ProxyAuthConfig,
    /// Network ACLs and rate-limiting knobs.
    pub network: NetworkConfig,
}

impl Default for ServerConfig {
    /// Creates loopback-only listener defaults.
    fn default() -> Self {
        Self {
            listen_http: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 8080))),
            listen_socks: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 1080))),
            listen_https: None,
            tls_cert_der_path: None,
            tls_key_der_path: None,
            tls_cert_pem_path: None,
            tls_key_pem_path: None,
            allow_unsafe_public_bind_without_acl: false,
            max_http_header_bytes: 16 * 1024,
            authentication: ProxyAuthConfig::default(),
            network: NetworkConfig::default(),
        }
    }
}

impl ServerConfig {
    /// Validates bind safety rules.
    pub fn validate(&self) -> Result<(), String> {
        let has_acl = !self.network.allow_clients.is_empty();
        if self.listen_https.is_some() {
            let has_der_pair = self.tls_cert_der_path.is_some() && self.tls_key_der_path.is_some();
            let has_pem_pair = self.tls_cert_pem_path.is_some() && self.tls_key_pem_path.is_some();
            let has_partial_der =
                self.tls_cert_der_path.is_some() != self.tls_key_der_path.is_some();
            let has_partial_pem =
                self.tls_cert_pem_path.is_some() != self.tls_key_pem_path.is_some();
            if has_partial_der || has_partial_pem {
                return Err("https listener requires complete certificate/key pairs".to_string());
            }
            if !has_der_pair && !has_pem_pair {
                return Err("https listener requires a DER or PEM certificate/key pair".to_string());
            }
        }

        for addr in [self.listen_http, self.listen_socks, self.listen_https]
            .into_iter()
            .flatten()
        {
            if !is_loopback(addr.ip()) && !has_acl && !self.allow_unsafe_public_bind_without_acl {
                return Err(format!(
                    "refusing non-loopback listener {addr} without an ACL"
                ));
            }
        }
        Ok(())
    }

    /// Builds a client ACL object.
    pub fn acl(&self) -> ClientAcl {
        ClientAcl::new(
            self.network.allow_clients.clone(),
            self.network.deny_clients.clone(),
        )
    }
}

/// Network ACL and coarse limit configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// Allowed client networks.
    pub allow_clients: Vec<IpNet>,
    /// Denied client networks.
    pub deny_clients: Vec<IpNet>,
    /// Advisory per-client request rate.
    pub rate_limit_per_client_per_second: Option<u32>,
    /// Advisory maximum simultaneous connections.
    pub max_connections: Option<u32>,
}

impl Default for NetworkConfig {
    /// Creates loopback ACL and conservative connection defaults.
    fn default() -> Self {
        Self {
            allow_clients: vec![
                IpNet::V4(Ipv4Net::new(Ipv4Addr::LOCALHOST, 8).expect("valid loopback net")),
                IpNet::V6(Ipv6Net::new(Ipv6Addr::LOCALHOST, 128).expect("valid loopback net")),
            ],
            deny_clients: Vec::new(),
            rate_limit_per_client_per_second: Some(50),
            max_connections: Some(1024),
        }
    }
}

/// Returns whether an IP address is loopback.
///
/// # Parameters
///
/// - `ip`: IP address to classify.
///
/// # Returns
///
/// `true` when `ip` belongs to the IPv4 or IPv6 loopback range.
fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback(),
        IpAddr::V6(ip) => ip.is_loopback(),
    }
}
