//! Shared proxy route configuration and versioned runtime ownership.

#[cfg(feature = "arachnea-proxy")]
use anyhow::{Context, Result};
use arachnea_http::HttpProxyConfig;
#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::core::{ArachneaProxyCore, UsageProfile};
use std::sync::{Arc, RwLock};

/// Mutable proxy transport shared by one scraper instance and its derived HTTP clients.
#[derive(Clone, Default)]
pub struct SharedProxyConfigHandle {
    inner: Arc<RwLock<ProxyConfigState>>,
    #[cfg(feature = "arachnea-proxy")]
    runtime: Arc<arachnea_proxy::server::LoopbackProxyRuntime>,
    #[cfg(feature = "arachnea-proxy")]
    /// Idle transport pool shared with independently configured derived handles.
    pub(super) transports: Arc<arachnea_http::client::ProxyTransportPool>,
}

#[derive(Clone, Default)]
/// Versioned proxy configuration used to invalidate derived transports and manage the default route.
pub(super) struct ProxyConfigState {
    /// Monotonic version used to invalidate cached HTTP transports.
    pub(super) version: u64,
    /// Current optional proxy override for this routing handle.
    pub(super) proxy: Option<HttpProxyConfig>,
    #[cfg(feature = "arachnea-proxy")]
    default_route: Option<Arc<arachnea_proxy::server::LoopbackRoute>>,
}

impl SharedProxyConfigHandle {
    /// Creates a handle without an active proxy override.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an independent routing configuration sharing an application listener.
    ///
    /// # Arguments
    ///
    /// - `runtime`: Shared loopback listener, including provider bootstrap transports.
    #[cfg(feature = "arachnea-proxy")]
    pub fn with_runtime(runtime: Arc<arachnea_proxy::server::LoopbackProxyRuntime>) -> Self {
        Self {
            inner: Arc::default(),
            runtime,
            transports: Arc::default(),
        }
    }

    /// Creates an independent routing handle sharing this family's listener and idle transports.
    #[cfg(feature = "arachnea-proxy")]
    pub fn with_independent_config(&self) -> Self {
        Self {
            inner: Arc::default(),
            runtime: self.runtime.clone(),
            transports: self.transports.clone(),
        }
    }

    /// Returns the listener shared by clients using this handle.
    #[cfg(feature = "arachnea-proxy")]
    pub fn proxy_runtime(&self) -> Arc<arachnea_proxy::server::LoopbackProxyRuntime> {
        self.runtime.clone()
    }

    /// Publishes the active core for external clients, preserving existing connections.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no active core or the registry is unavailable.
    #[cfg(feature = "arachnea-proxy")]
    pub fn publish_default_route(&self) -> Result<()> {
        let mut state = self
            .inner
            .write()
            .map_err(|_| anyhow::anyhow!("proxy configuration lock poisoned"))?;
        let Some(HttpProxyConfig::Arachnea(core)) = &state.proxy else {
            anyhow::bail!("cannot publish loopback server without an Arachnea proxy core");
        };
        let route = self.runtime.register(
            core.as_ref().clone(),
            arachnea_proxy::core::ClientContext::new(),
            true,
        )?;
        self.runtime.set_default_route(&route)?;
        state.default_route = Some(route);
        Ok(())
    }

    /// Publishes the current default route and binds the shared server before external use.
    ///
    /// # Errors
    ///
    /// Returns a contextual error if the route cannot be published or the port cannot bind.
    #[cfg(feature = "arachnea-proxy")]
    pub async fn start_proxy_server(&self) -> Result<std::net::SocketAddr> {
        self.publish_default_route()?;
        self.runtime
            .start()
            .await
            .context("Failed to bind shared loopback proxy server")
    }

    /// Replaces the proxy configuration and increments the version.
    ///
    /// # Arguments
    ///
    /// * `proxy` - Proxy transport configuration replacing the current shared override.
    pub fn set_proxy(&self, proxy: HttpProxyConfig) {
        if let Ok(mut guard) = self.inner.write() {
            guard.version = guard.version.saturating_add(1);
            #[cfg(feature = "arachnea-proxy")]
            if guard.default_route.is_some() {
                if let HttpProxyConfig::Arachnea(core) = &proxy {
                    match self
                        .runtime
                        .register(
                            core.as_ref().clone(),
                            arachnea_proxy::core::ClientContext::new(),
                            true,
                        )
                        .and_then(|route| {
                            self.runtime.set_default_route(&route)?;
                            Ok(route)
                        }) {
                        Ok(route) => guard.default_route = Some(route),
                        Err(error) => {
                            if let Some(route) = guard.default_route.take() {
                                let _ = self.runtime.clear_default_route(&route);
                            }
                            tracing::error!(target: "arachnea_scrapyfy::scrapyfy::http_client", %error, "failed to replace external loopback route");
                        }
                    }
                } else if let Some(route) = guard.default_route.take() {
                    let _ = self.runtime.clear_default_route(&route);
                }
            }
            guard.proxy = Some(proxy);
        }
    }

    /// Removes the proxy override and increments the version.
    pub fn clear_proxy(&self) {
        if let Ok(mut guard) = self.inner.write() {
            guard.version = guard.version.saturating_add(1);
            guard.proxy = None;
            #[cfg(feature = "arachnea-proxy")]
            {
                if let Some(route) = guard.default_route.take() {
                    let _ = self.runtime.clear_default_route(&route);
                }
            }
        }
    }

    /// Installs the default in-process system relay proxy for this handle.
    #[cfg(feature = "arachnea-proxy")]
    pub fn enable_system_proxy(&self) -> Result<()> {
        let proxy_core = ArachneaProxyCore::new(UsageProfile::SystemRelay.config())?;
        self.set_proxy(HttpProxyConfig::Arachnea(Box::new(proxy_core)));
        Ok(())
    }

    /// Returns the current proxy configuration snapshot and version.
    pub(super) fn snapshot(&self) -> ProxyConfigState {
        self.inner
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}
