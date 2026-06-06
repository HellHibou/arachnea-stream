use std::sync::Arc;

use crate::core::{ArachneaProxyCore, ProxyConfig};
use anyhow::Context;

use crate::server::listeners::{http, https, socks};
use crate::server::ServerConfig;

/// Running proxy server configuration and core handle.
#[derive(Clone)]
pub struct ProxyServer {
    /// Server-side listener configuration.
    pub config: ServerConfig,
    /// Shared proxy core.
    pub core: ArachneaProxyCore,
}

impl ProxyServer {
    /// Creates a server from server and core configuration.
    ///
    /// # Parameters
    ///
    /// - `config`: Server listener and safety configuration.
    /// - `core_config`: Proxy core routing and transport configuration.
    ///
    /// # Returns
    ///
    /// Proxy server ready to run.
    ///
    /// # Errors
    ///
    /// Returns an error when server validation or core configuration resolution
    /// fails.
    pub fn new(config: ServerConfig, core_config: ProxyConfig) -> anyhow::Result<Self> {
        config.validate().map_err(anyhow::Error::msg)?;
        let core = ArachneaProxyCore::new(core_config)?;
        Ok(Self { config, core })
    }

    /// Runs configured listeners until all of them stop.
    ///
    /// # Errors
    ///
    /// Returns an error when validation fails, a listener fails, or a listener
    /// task panics.
    pub async fn run(self) -> anyhow::Result<()> {
        self.config.validate().map_err(anyhow::Error::msg)?;
        let config = Arc::new(self.config);
        let core = self.core;
        let mut handles = Vec::new();

        if let Some(addr) = config.listen_http {
            let listener_config = Arc::clone(&config);
            let listener_core = core.clone();
            handles.push(tokio::spawn(async move {
                http::run(addr, listener_config, listener_core).await
            }));
        }

        if let Some(addr) = config.listen_socks {
            let listener_config = Arc::clone(&config);
            let listener_core = core.clone();
            handles.push(tokio::spawn(async move {
                socks::run(addr, listener_config, listener_core).await
            }));
        }

        if let Some(addr) = config.listen_https {
            let listener_config = Arc::clone(&config);
            let listener_core = core.clone();
            handles.push(tokio::spawn(async move {
                https::run(addr, listener_config, listener_core).await
            }));
        }

        for handle in handles {
            handle.await.context("proxy listener task panicked")??;
        }

        Ok(())
    }
}
