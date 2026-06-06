use serde::{Deserialize, Serialize};

use crate::core::{ClientContext, ProxyError, Result};

/// Kind of outbound transport used by a proxy node.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    /// Connect directly to the destination.
    Direct,
    /// Use a cleartext HTTP proxy with absolute requests or CONNECT.
    HttpProxy,
    /// Use an HTTP proxy over TLS.
    HttpsProxy,
    /// Use SOCKS5.
    Socks5,
    /// Use SOCKS4 or SOCKS4a for TCP CONNECT.
    Socks4,
    /// Use a local Tor SOCKS endpoint.
    TorSocks,
    /// Select one working upstream from a named egress pool.
    ProxyPool,
    /// Use an externally managed tunnel endpoint.
    ExternalTunnel,
    /// Reserved transport kind for future MASQUE CONNECT-UDP support.
    MasqueUdp,
}

/// DNS resolution behavior used when a proxy node receives a hostname target.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyNameResolutionMode {
    /// Resolve hostnames through `arachnea-dns` or the system resolver before the handshake.
    Local,
    /// Try remote proxy resolution first, then remember this node as local-DNS-only on failure.
    ProxyThenLocalFallback,
    /// Send hostnames to the proxy and never fall back to local resolution for this node.
    ProxyOnly,
}

impl Default for ProxyNameResolutionMode {
    /// Returns proxy resolution with local fallback as the default.
    fn default() -> Self {
        Self::ProxyThenLocalFallback
    }
}

/// One outbound proxy or direct hop.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyNode {
    /// Stable node name used by chains and diagnostics.
    pub name: String,
    /// Transport kind for this node.
    pub kind: TransportKind,
    /// Endpoint for proxy-like nodes, formatted as `host:port`.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Whether hostnames should be sent to this node when supported.
    #[serde(default = "default_true")]
    pub send_hostname: bool,
    /// Optional DNS resolution behavior for this node.
    ///
    /// When absent, `send_hostname` is used for backward compatibility. SOCKS5
    /// nodes with `send_hostname = true` default to `ProxyThenLocalFallback`;
    /// other nodes with `send_hostname = true` default to `ProxyOnly`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_resolution: Option<ProxyNameResolutionMode>,
    /// Optional username for future authenticated upstreams.
    #[serde(default)]
    pub username: Option<String>,
    /// Environment variable containing an upstream password.
    #[serde(default)]
    pub password_env: Option<String>,
    /// Whether TLS validation must be enabled for TLS-based upstreams.
    #[serde(default = "default_true")]
    pub verify_tls: bool,
}

impl ProxyNode {
    /// Creates a direct node.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable node name.
    ///
    /// # Returns
    ///
    /// Direct proxy node.
    pub fn direct(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: TransportKind::Direct,
            endpoint: None,
            send_hostname: true,
            dns_resolution: None,
            username: None,
            password_env: None,
            verify_tls: true,
        }
    }

    /// Creates a chain marker that selects one upstream from an egress pool.
    ///
    /// The marker name must match an `EgressPool` declared on `ProxyConfig`.
    /// Runtime connection code replaces this marker with the pool's cached
    /// working upstream before executing the chain.
    ///
    /// # Parameters
    ///
    /// - `name`: Egress pool name.
    ///
    /// # Returns
    ///
    /// Proxy pool marker node.
    pub fn pool(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: TransportKind::ProxyPool,
            endpoint: None,
            send_hostname: true,
            dns_resolution: None,
            username: None,
            password_env: None,
            verify_tls: true,
        }
    }

    /// Creates a cleartext HTTP proxy node.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable node name.
    /// - `endpoint`: Upstream endpoint formatted as `host:port`.
    ///
    /// # Returns
    ///
    /// HTTP proxy node.
    pub fn http(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self::proxy(name, TransportKind::HttpProxy, endpoint)
    }

    /// Creates an HTTPS proxy node.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable node name.
    /// - `endpoint`: Upstream endpoint formatted as `host:port`.
    ///
    /// # Returns
    ///
    /// HTTPS proxy node.
    pub fn https(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self::proxy(name, TransportKind::HttpsProxy, endpoint)
    }

    /// Creates a SOCKS5 node.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable node name.
    /// - `endpoint`: Upstream endpoint formatted as `host:port`.
    ///
    /// # Returns
    ///
    /// SOCKS5 proxy node.
    pub fn socks5(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self::proxy(name, TransportKind::Socks5, endpoint)
    }

    /// Creates a SOCKS4 node.
    ///
    /// SOCKS4 nodes use local DNS by default because plain SOCKS4 only carries
    /// IPv4 addresses. Set `ProxyNameResolutionMode::ProxyOnly` to use the
    /// SOCKS4a hostname extension.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable node name.
    /// - `endpoint`: Upstream endpoint formatted as `host:port`.
    ///
    /// # Returns
    ///
    /// SOCKS4 proxy node.
    pub fn socks4(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self::proxy(name, TransportKind::Socks4, endpoint)
    }

    /// Creates a Tor-over-SOCKS node.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable node name.
    /// - `endpoint`: Tor SOCKS endpoint formatted as `host:port`.
    ///
    /// # Returns
    ///
    /// Tor SOCKS proxy node.
    pub fn tor_socks(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        let mut node = Self::proxy(name, TransportKind::TorSocks, endpoint);
        node.send_hostname = true;
        node
    }

    /// Builds a single-forwarder configuration from this node.
    ///
    /// This is the shortest path for code that wants to start a core from one
    /// proxy and optionally append more hops at runtime.
    ///
    /// # Returns
    ///
    /// Proxy configuration that uses this node as the default chain.
    pub fn config(self) -> crate::core::ProxyConfig {
        crate::core::UsageProfile::SingleForwarder(self).config()
    }

    /// Creates a proxy node from a proxy URL.
    ///
    /// Supported schemes are `http`, `https`, `socks`, `socks4`, `socks4a`,
    /// `socks5`, and `socks5h`. URLs without an explicit port receive the usual scheme
    /// default: 80 for HTTP, 443 for HTTPS, and 1080 for SOCKS.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable node name.
    /// - `proxy_url`: Proxy URL such as `socks5://127.0.0.1:9050`.
    ///
    /// # Returns
    ///
    /// Proxy node configured from the supplied URL.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL has an unsupported scheme, contains inline
    /// credentials, or does not contain a usable host.
    pub fn from_url(name: impl Into<String>, proxy_url: impl AsRef<str>) -> Result<Self> {
        let name = name.into();
        let proxy_url = proxy_url.as_ref().trim();
        let (scheme, rest) = split_proxy_scheme(proxy_url);
        let kind = match scheme.as_str() {
            "http" => TransportKind::HttpProxy,
            "https" => TransportKind::HttpsProxy,
            "socks4" | "socks4a" => TransportKind::Socks4,
            "socks" | "socks5" | "socks5h" => TransportKind::Socks5,
            "tor" | "torsocks" => TransportKind::TorSocks,
            other => {
                return Err(ProxyError::Config(format!(
                    "unsupported proxy URL scheme '{other}'"
                )));
            }
        };
        let endpoint = proxy_endpoint_from_url_rest(rest, &scheme)?;
        crate::core::Destination::from_authority(&endpoint, crate::core::ApplicationProtocol::Tcp)?;
        let mut node = Self::proxy(name, kind, endpoint);
        if scheme == "socks4" {
            node = node.with_dns_resolution(ProxyNameResolutionMode::Local);
        } else if scheme == "socks4a" {
            node = node.with_dns_resolution(ProxyNameResolutionMode::ProxyOnly);
        } else {
            node.send_hostname = true;
        }
        Ok(node)
    }

    /// Creates a SOCKS5 proxy node from a URL and an explicit DNS mode.
    ///
    /// Supported schemes are `socks`, `socks5`, and `socks5h`. URLs without an
    /// explicit port receive the SOCKS default port `1080`.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable node name.
    /// - `proxy_url`: SOCKS5 proxy URL such as `socks5://127.0.0.1:9050`.
    /// - `dns_resolution`: DNS resolution behavior used for hostname targets.
    ///
    /// # Returns
    ///
    /// SOCKS5 proxy node configured from the supplied URL and DNS mode.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL is not a SOCKS5 URL or cannot be parsed.
    pub fn from_socks5_url(
        name: impl Into<String>,
        proxy_url: impl AsRef<str>,
        dns_resolution: ProxyNameResolutionMode,
    ) -> Result<Self> {
        let node = Self::from_url(name, proxy_url)?;
        if node.kind != TransportKind::Socks5 {
            return Err(ProxyError::Config(format!(
                "proxy URL scheme for node '{}' must be socks5-compatible",
                node.name
            )));
        }
        Ok(node.with_dns_resolution(dns_resolution))
    }

    /// Creates an externally managed tunnel node.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable node name.
    /// - `endpoint`: Local or remote tunnel endpoint formatted as `host:port`.
    ///
    /// # Returns
    ///
    /// External tunnel node.
    pub fn external_tunnel(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self::proxy(name, TransportKind::ExternalTunnel, endpoint)
    }

    /// Configures upstream credentials through an environment variable.
    ///
    /// # Parameters
    ///
    /// - `username`: Username sent to the upstream proxy.
    /// - `password_env`: Environment variable containing the password.
    ///
    /// # Returns
    ///
    /// Updated proxy node.
    pub fn with_credentials_env(
        mut self,
        username: impl Into<String>,
        password_env: impl Into<String>,
    ) -> Self {
        self.username = Some(username.into());
        self.password_env = Some(password_env.into());
        self
    }

    /// Sets whether hostnames are sent to capable upstream transports.
    ///
    /// # Parameters
    ///
    /// - `send_hostname`: Whether the proxy should receive hostnames instead
    ///   of locally resolved IP addresses when the protocol supports it.
    ///
    /// # Returns
    ///
    /// Updated proxy node.
    pub fn with_hostname_forwarding(mut self, send_hostname: bool) -> Self {
        self.send_hostname = send_hostname;
        self.dns_resolution = None;
        self
    }

    /// Sets the DNS resolution behavior for this proxy node.
    ///
    /// # Parameters
    ///
    /// - `mode`: DNS resolution behavior used for hostname targets.
    ///
    /// # Returns
    ///
    /// Updated proxy node.
    pub fn with_dns_resolution(mut self, mode: ProxyNameResolutionMode) -> Self {
        self.send_hostname = !matches!(mode, ProxyNameResolutionMode::Local);
        self.dns_resolution = Some(mode);
        self
    }

    /// Returns the effective DNS resolution behavior for this proxy node.
    ///
    /// # Returns
    ///
    /// Configured DNS resolution behavior, including `send_hostname`
    /// compatibility when the explicit mode is absent.
    pub fn effective_name_resolution(&self) -> ProxyNameResolutionMode {
        self.dns_resolution.clone().unwrap_or_else(|| {
            if !self.send_hostname {
                ProxyNameResolutionMode::Local
            } else if matches!(self.kind, TransportKind::Socks5 | TransportKind::TorSocks) {
                ProxyNameResolutionMode::ProxyThenLocalFallback
            } else if matches!(self.kind, TransportKind::Socks4 | TransportKind::ProxyPool) {
                ProxyNameResolutionMode::Local
            } else {
                ProxyNameResolutionMode::ProxyOnly
            }
        })
    }

    /// Sets TLS verification behavior for TLS-based upstream transports.
    ///
    /// # Parameters
    ///
    /// - `verify_tls`: Whether TLS certificates must be verified.
    ///
    /// # Returns
    ///
    /// Updated proxy node.
    pub fn with_tls_verification(mut self, verify_tls: bool) -> Self {
        self.verify_tls = verify_tls;
        self
    }

    /// Resolves a proxy node from request parameters through caller-provided code.
    ///
    /// This helper is useful for custom parameter handlers that need to select
    /// an extra request-local proxy without defining a new serializable
    /// configuration shape.
    ///
    /// # Parameters
    ///
    /// - `parameters`: Client context extracted from the inbound proxy request.
    /// - `resolver`: Custom resolver that may return an extra proxy node.
    ///
    /// # Returns
    ///
    /// Extra proxy selected by `resolver`, when any.
    ///
    /// # Errors
    ///
    /// Returns an error when `resolver` fails.
    pub fn custom_from_parameters<F>(
        parameters: &ClientContext,
        resolver: F,
    ) -> Result<Option<Self>>
    where
        F: FnOnce(&ClientContext) -> Result<Option<Self>>,
    {
        resolver(parameters)
    }

    /// Creates a proxy node for transports that require an endpoint.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable node name.
    /// - `kind`: Proxy transport kind.
    /// - `endpoint`: Upstream endpoint formatted as `host:port`.
    ///
    /// # Returns
    ///
    /// A proxy node with hostname forwarding and TLS verification enabled.
    fn proxy(name: impl Into<String>, kind: TransportKind, endpoint: impl Into<String>) -> Self {
        let dns_resolution = default_dns_resolution_for_transport(&kind);
        Self {
            name: name.into(),
            kind,
            endpoint: Some(endpoint.into()),
            send_hostname: true,
            dns_resolution,
            username: None,
            password_env: None,
            verify_tls: true,
        }
    }

    /// Loads upstream proxy credentials when both credential fields are configured.
    ///
    /// Upstream authentication remains optional: a node with no `password_env`
    /// uses no-auth handshakes and sends no proxy authorization headers.
    ///
    /// # Returns
    ///
    /// Loaded upstream credentials when authentication is enabled for this node.
    ///
    /// # Errors
    ///
    /// Returns an error when `password_env` is configured but the environment
    /// variable cannot be read.
    pub fn credentials(&self) -> Result<Option<ProxyNodeCredentials>> {
        let (Some(username), Some(password_env)) = (&self.username, &self.password_env) else {
            return Ok(None);
        };
        let password = std::env::var(password_env).map_err(|_| {
            ProxyError::Config(format!(
                "upstream node '{}' password env '{}' is not set",
                self.name, password_env
            ))
        })?;
        Ok(Some(ProxyNodeCredentials {
            username: username.clone(),
            password,
        }))
    }
}

/// Splits a proxy URL into a lower-case scheme and the remaining URL text.
///
/// # Parameters
///
/// - `proxy_url`: Proxy URL supplied by an API caller or environment variable.
///
/// # Returns
///
/// Tuple containing the scheme and URL text after `://`.
fn split_proxy_scheme(proxy_url: &str) -> (String, &str) {
    proxy_url
        .split_once("://")
        .map_or(("http".to_string(), proxy_url), |(scheme, rest)| {
            (scheme.trim().to_ascii_lowercase(), rest)
        })
}

/// Extracts an endpoint authority from a proxy URL remainder.
///
/// # Parameters
///
/// - `rest`: URL text after the scheme separator.
/// - `scheme`: Lower-case proxy URL scheme.
///
/// # Returns
///
/// Endpoint formatted as `host:port` or `[ipv6]:port`.
///
/// # Errors
///
/// Returns an error when the authority is empty, contains inline credentials,
/// or the scheme does not have a known default port.
fn proxy_endpoint_from_url_rest(rest: &str, scheme: &str) -> Result<String> {
    let authority_end = rest
        .char_indices()
        .find(|(_, character)| matches!(*character, '/' | '?' | '#'))
        .map_or(rest.len(), |(index, _)| index);
    let authority = rest[..authority_end].trim();
    if authority.is_empty() {
        return Err(ProxyError::Config(
            "proxy URL must include a host".to_string(),
        ));
    }
    if authority.contains('@') {
        return Err(ProxyError::Config(
            "proxy URL inline credentials are not supported; use with_credentials_env".to_string(),
        ));
    }
    if authority_has_port(authority)? {
        return Ok(authority.to_string());
    }
    let port = default_proxy_port(scheme)?;
    Ok(format!("{authority}:{port}"))
}

/// Returns whether an authority already contains an explicit port.
///
/// # Parameters
///
/// - `authority`: Authority string from a proxy URL.
///
/// # Returns
///
/// `true` when a port is present.
///
/// # Errors
///
/// Returns an error when a bracketed IPv6 authority is malformed or when an
/// unbracketed authority contains an invalid port.
fn authority_has_port(authority: &str) -> Result<bool> {
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| ProxyError::Config(format!("invalid proxy authority '{authority}'")))?;
        let suffix = &rest[end + 1..];
        if suffix.is_empty() {
            return Ok(false);
        }
        let port = suffix
            .strip_prefix(':')
            .ok_or_else(|| ProxyError::Config(format!("invalid proxy authority '{authority}'")))?;
        port.parse::<u16>().map_err(|_| {
            ProxyError::Config(format!("invalid proxy authority port in '{authority}'"))
        })?;
        return Ok(true);
    }
    let Some((host, port)) = authority.rsplit_once(':') else {
        return Ok(false);
    };
    if host.contains(':') {
        return Err(ProxyError::Config(format!(
            "IPv6 proxy authority '{authority}' must be bracketed"
        )));
    }
    port.parse::<u16>().map_err(|_| {
        ProxyError::Config(format!("invalid proxy authority port in '{authority}'"))
    })?;
    Ok(true)
}

/// Returns the default port for a proxy URL scheme.
///
/// # Parameters
///
/// - `scheme`: Lower-case proxy URL scheme.
///
/// # Returns
///
/// Default port for the scheme.
///
/// # Errors
///
/// Returns an error when the scheme has no supported default port.
fn default_proxy_port(scheme: &str) -> Result<u16> {
    match scheme {
        "http" => Ok(80),
        "https" => Ok(443),
        "socks" | "socks4" | "socks4a" | "socks5" | "socks5h" | "tor" | "torsocks" => Ok(1080),
        other => Err(ProxyError::Config(format!(
            "unsupported proxy URL scheme '{other}'"
        ))),
    }
}

/// Loaded credentials for an upstream proxy node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyNodeCredentials {
    /// Username sent to the upstream proxy.
    pub username: String,
    /// Password sent to the upstream proxy.
    pub password: String,
}

/// Returns `true` for serde defaults.
///
/// # Returns
///
/// Always returns `true`.
fn default_true() -> bool {
    true
}

/// Returns the default DNS resolution mode for a transport kind.
///
/// # Parameters
///
/// - `kind`: Proxy transport kind being initialized.
///
/// # Returns
///
/// Default DNS resolution mode, when the transport has a proxy DNS policy.
fn default_dns_resolution_for_transport(kind: &TransportKind) -> Option<ProxyNameResolutionMode> {
    if matches!(kind, TransportKind::Socks5 | TransportKind::TorSocks) {
        Some(ProxyNameResolutionMode::default())
    } else if matches!(kind, TransportKind::Socks4 | TransportKind::ProxyPool) {
        Some(ProxyNameResolutionMode::Local)
    } else {
        None
    }
}

/// Strategy used to consume nodes inside a chain.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainStrategy {
    /// Use nodes in their declared order.
    FixedOrder,
    /// Try chains or nodes in order until one succeeds.
    Fallback,
}

impl Default for ChainStrategy {
    /// Returns fixed-order chain execution.
    fn default() -> Self {
        Self::FixedOrder
    }
}

/// Ordered list of proxy nodes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyChain {
    /// Stable chain name.
    pub name: String,
    /// Ordered nodes in the chain.
    #[serde(default)]
    pub nodes: Vec<ProxyNode>,
    /// Execution strategy.
    #[serde(default)]
    pub strategy: ChainStrategy,
}

impl ProxyChain {
    /// Creates a direct chain.
    ///
    /// # Returns
    ///
    /// Direct proxy chain.
    pub fn direct() -> Self {
        Self {
            name: "direct".to_string(),
            nodes: vec![ProxyNode::direct("direct")],
            strategy: ChainStrategy::FixedOrder,
        }
    }

    /// Creates a fixed-order chain.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable chain name.
    /// - `nodes`: Ordered nodes in the chain.
    ///
    /// # Returns
    ///
    /// Fixed-order proxy chain.
    pub fn fixed(name: impl Into<String>, nodes: Vec<ProxyNode>) -> Self {
        Self {
            name: name.into(),
            nodes,
            strategy: ChainStrategy::FixedOrder,
        }
    }

    /// Creates a fixed-order chain containing one proxy node.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable chain name.
    /// - `node`: Proxy node used by the chain.
    ///
    /// # Returns
    ///
    /// Fixed-order chain containing `node`.
    pub fn single(name: impl Into<String>, node: ProxyNode) -> Self {
        Self::fixed(name, vec![node])
    }

    /// Creates a fixed-order chain that selects from one proxy pool.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable chain name.
    /// - `pool`: Egress pool name selected by the chain.
    ///
    /// # Returns
    ///
    /// Fixed-order chain containing a proxy pool marker.
    pub fn pool(name: impl Into<String>, pool: impl Into<String>) -> Self {
        Self::fixed(name, vec![ProxyNode::pool(pool)])
    }

    /// Appends one proxy node to this chain.
    ///
    /// # Parameters
    ///
    /// - `node`: Additional proxy hop appended after the existing nodes.
    ///
    /// # Returns
    ///
    /// Updated chain.
    pub fn with_extra_proxy(mut self, node: ProxyNode) -> Self {
        self.nodes.push(node);
        self
    }

    /// Appends an egress pool marker to this chain.
    ///
    /// # Parameters
    ///
    /// - `pool`: Egress pool name appended as the next hop.
    ///
    /// # Returns
    ///
    /// Updated chain.
    pub fn with_proxy_pool(mut self, pool: impl Into<String>) -> Self {
        self.nodes.push(ProxyNode::pool(pool));
        self
    }
}
