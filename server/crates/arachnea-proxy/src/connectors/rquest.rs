/// Rquest integration mode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RquestIntegrationMode {
    /// Use a loopback proxy URL configured through `rquest::Proxy`.
    LoopbackProxy,
}

/// Notes the current rquest support status.
pub fn support_status() -> &'static str {
    "rquest in-process transport replacement is not stable here; use a loopback proxy helper"
}

#[cfg(feature = "rquest")]
use std::net::{Ipv4Addr, SocketAddr};
#[cfg(feature = "rquest")]
use std::sync::Arc;

#[cfg(feature = "rquest")]
use crate::core::{ArachneaProxyCore, ParameterDefinition};
#[cfg(feature = "rquest")]
use crate::server::{handlers, NetworkConfig, ProxyAuthConfig, ServerConfig};
#[cfg(feature = "rquest")]
use rquest::header::{HeaderMap, HeaderName, HeaderValue};
#[cfg(feature = "rquest")]
use tokio::net::TcpListener;
#[cfg(feature = "rquest")]
use tokio::task::JoinHandle;

#[cfg(feature = "rquest")]
use crate::connectors::{ConnectorError, Result};

/// Loopback proxy helper for rquest clients.
#[cfg(feature = "rquest")]
pub struct ArachneaRquestLoopback {
    proxy_url: String,
    parameter_definitions: Vec<ParameterDefinition>,
    task: JoinHandle<()>,
}

#[cfg(feature = "rquest")]
impl ArachneaRquestLoopback {
    /// Starts a loopback HTTP proxy listener backed by the supplied core.
    ///
    /// # Parameters
    ///
    /// - `core`: Proxy core used by the loopback HTTP listener.
    ///
    /// # Returns
    ///
    /// Loopback helper containing the selected proxy URL and listener task.
    ///
    /// # Errors
    ///
    /// Returns an error when the loopback listener cannot bind.
    pub async fn start(core: ArachneaProxyCore) -> Result<Self> {
        let parameter_definitions = core.parameter_definitions();
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
        let addr = listener.local_addr()?;
        let config = Arc::new(ServerConfig {
            listen_http: Some(addr),
            listen_socks: None,
            listen_https: None,
            tls_cert_der_path: None,
            tls_key_der_path: None,
            tls_cert_pem_path: None,
            tls_key_pem_path: None,
            allow_unsafe_public_bind_without_acl: false,
            max_http_header_bytes: 16 * 1024,
            authentication: ProxyAuthConfig::default(),
            network: NetworkConfig::default(),
        });
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, peer)) = listener.accept().await else {
                    break;
                };
                let conn_config = Arc::clone(&config);
                let conn_core = core.clone();
                tokio::spawn(async move {
                    if let Err(error) =
                        handlers::http::handle(stream, peer, conn_config, conn_core).await
                    {
                        tracing::warn!(
                            peer = %peer,
                            %error,
                            "rquest loopback proxy connection failed"
                        );
                    }
                });
            }
        });

        Ok(Self {
            proxy_url: format!("http://{addr}"),
            parameter_definitions,
            task,
        })
    }

    /// Returns the proxy URL usable with `rquest::Proxy`.
    ///
    /// # Returns
    ///
    /// Loopback proxy URL selected when the helper started.
    pub fn proxy_url(&self) -> &str {
        &self.proxy_url
    }

    /// Returns parameter definitions captured from the proxy core.
    ///
    /// # Returns
    ///
    /// Parameter definitions accepted by the loopback proxy.
    pub fn parameter_definitions(&self) -> &[ParameterDefinition] {
        &self.parameter_definitions
    }

    /// Builds a rquest client configured to use the loopback proxy.
    ///
    /// # Returns
    ///
    /// Rquest client configured for this loopback proxy.
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy cannot be configured or the rquest
    /// client cannot be built.
    pub fn client(&self) -> Result<rquest::Client> {
        let proxy = rquest::Proxy::all(&self.proxy_url)
            .map_err(|error| ConnectorError::Rquest(error.to_string()))?;
        rquest::Client::builder()
            .proxy(proxy)
            .build()
            .map_err(|error| ConnectorError::Rquest(error.to_string()))
    }

    /// Builds a rquest client that sends routing parameters to the loopback proxy.
    ///
    /// Parameters are encoded as proxy-level HTTP headers so they are visible
    /// on both absolute-form HTTP requests and HTTPS `CONNECT` requests. The
    /// headers are selected from the core parameter definitions captured when
    /// the loopback listener started.
    ///
    /// # Parameters
    ///
    /// - `parameters`: Request-parameter pairs encoded as `name=value`.
    ///
    /// # Returns
    ///
    /// Rquest client configured for this loopback proxy.
    ///
    /// # Errors
    ///
    /// Returns an error when a parameter is unknown, a header is invalid, the
    /// proxy cannot be configured, or the rquest client cannot be built.
    pub fn client_with_parameters<'a>(
        &self,
        parameters: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<rquest::Client> {
        let mut headers = HeaderMap::new();
        for (name, value) in parameters {
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                ConnectorError::Rquest(format!(
                    "invalid proxy parameter header '{}': {error}",
                    name
                ))
            })?;

            let header_value = HeaderValue::from_str(value).map_err(|error| {
                ConnectorError::Rquest(format!(
                    "invalid proxy parameter value for '{}': {error}",
                    name
                ))
            })?;
            headers.insert(header_name, header_value);
        }

        let proxy = rquest::Proxy::all(&self.proxy_url)
            .map_err(|error| ConnectorError::Rquest(error.to_string()))?
            .custom_http_headers(headers);
        rquest::Client::builder()
            .proxy(proxy)
            .build()
            .map_err(|error| ConnectorError::Rquest(error.to_string()))
    }
}

#[cfg(feature = "rquest")]
impl Drop for ArachneaRquestLoopback {
    /// Stops the background loopback listener task.
    ///
    /// # Parameters
    ///
    /// - `self`: Loopback helper being dropped.
    fn drop(&mut self) {
        self.task.abort();
    }
}
