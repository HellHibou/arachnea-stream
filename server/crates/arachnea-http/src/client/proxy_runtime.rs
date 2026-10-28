//! Prepared runtime, exclusive transport retention and bound browser pages.

use super::*;

/// Prepared proxy runtime shared by engines that need live helper state.
///
/// This struct holds runtime state for proxy transports that need to be initialized
/// asynchronously, such as the `arachnea-proxy` loopback helper.
#[derive(Clone)]
pub(super) struct PreparedProxyRuntime {
    /// Request-selected transports isolated by exact destination and endpoint.
    #[cfg(feature = "arachnea-proxy")]
    pub(super) pinned_clients: Arc<ProxyTransportPool>,
    #[cfg(feature = "arachnea-proxy")]
    pub(super) loopback_proxy: Option<Arc<ArachneaRquestLoopback>>,
}

/// Bounded idle transports shared by a caller-owned HTTP client family.
/// Up to two idle transports per route and 32 globally are retained for 60 seconds.
/// Entries are checked out exclusively, reselected and rebound per request.
/// Cookies and request headers remain outside this pool; every use has a fresh selection.
#[cfg(feature = "arachnea-proxy")]
#[derive(Default)]
pub struct ProxyTransportPool {
    /// Idle transports grouped by their exact routing and destination key.
    pub(super) idle: tokio::sync::Mutex<HashMap<String, Vec<IdleProxyTransport>>>,
}

#[cfg(feature = "arachnea-proxy")]
impl std::fmt::Debug for ProxyTransportPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProxyTransportPool")
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "arachnea-proxy")]
/// One pooled proxy transport together with its idle timestamp.
pub(super) struct IdleProxyTransport {
    /// Loopback connector that owns the selected route.
    pub(super) loopback: ArachneaRquestLoopback,
    /// Engine configured for the loopback connector.
    pub(super) engine: RquestEngine,
    /// Time of the last completed use, used for TTL and oldest-entry eviction.
    pub(super) idle_since: std::time::Instant,
}

/// Owns a complete redirect transport until the redirect loop confirms following.
#[cfg(feature = "arachnea-proxy")]
pub(super) struct PendingRedirectTransport {
    pub(super) pool: Arc<ProxyTransportPool>,
    pub(super) key: String,
    pub(super) transport: IdleProxyTransport,
}

/// Keeps transport ownership out of the cloneable public response.
pub(super) struct StepResponse {
    pub(super) response: ArachneaResponse,
    #[cfg(feature = "arachnea-proxy")]
    pub(super) redirect_transport: Option<Box<PendingRedirectTransport>>,
}

impl StepResponse {
    /// Wraps one response step without a retained proxy transport.
    ///
    /// # Arguments
    ///
    /// - `response`: Materialized response for this hop.
    ///
    /// # Returns
    ///
    /// A step response with no redirect transport.
    pub(super) fn new(response: ArachneaResponse) -> Self {
        Self {
            response,
            #[cfg(feature = "arachnea-proxy")]
            redirect_transport: None,
        }
    }
}

#[cfg(feature = "arachnea-proxy")]
impl ProxyTransportPool {
    /// Returns one transport to the idle pool.
    ///
    /// Expired transports are dropped. The pool keeps at most the configured
    /// per-route and global limits, evicting the oldest transport when a limit
    /// is exceeded.
    ///
    /// # Arguments
    ///
    /// - `key`: Exact routing and destination key.
    /// - `transport`: Transport that completed its request successfully.
    pub(super) async fn retain(&self, key: String, transport: IdleProxyTransport) {
        if transport.idle_since.elapsed() >= crate::HTTP_PROXY_TRANSPORT_IDLE_TTL {
            return;
        }
        let mut cache = self.idle.lock().await;
        let route = cache.entry(key).or_default();
        route.push(transport);
        if route.len() > crate::HTTP_PROXY_IDLE_PER_ROUTE {
            let oldest = route
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.idle_since)
                .unwrap()
                .0;
            route.swap_remove(oldest);
        }
        if cache.values().map(Vec::len).sum::<usize>() > crate::HTTP_PROXY_IDLE_GLOBAL {
            let (key, index) = cache
                .iter()
                .flat_map(|(key, entries)| {
                    entries
                        .iter()
                        .enumerate()
                        .map(move |(index, entry)| (key, index, entry.idle_since))
                })
                .min_by_key(|(_, _, since)| *since)
                .map(|(key, index, _)| (key.clone(), index))
                .unwrap();
            let entries = cache.get_mut(&key).unwrap();
            entries.swap_remove(index);
            if entries.is_empty() {
                cache.remove(&key);
            }
        }
    }

    /// Checks out the newest compatible transport without waiting for active borrowers.
    ///
    /// Expired entries are removed before lookup. Checkout removes the selected
    /// transport from the pool, so it cannot be borrowed concurrently.
    ///
    /// # Arguments
    ///
    /// - `key`: Exact routing and destination key.
    ///
    /// # Returns
    ///
    /// The selected transport, the number of expired entries removed, and the
    /// number of idle entries remaining in the pool.
    pub(super) async fn checkout(&self, key: &str) -> (Option<IdleProxyTransport>, usize, usize) {
        let mut cache = self.idle.lock().await;
        let before = cache.values().map(Vec::len).sum::<usize>();
        cache.retain(|_, entries| {
            entries
                .retain(|entry| entry.idle_since.elapsed() < crate::HTTP_PROXY_TRANSPORT_IDLE_TTL);
            !entries.is_empty()
        });
        let expired = before - cache.values().map(Vec::len).sum::<usize>();
        let cached = cache.get_mut(key).and_then(|entries| {
            let newest = entries
                .iter()
                .enumerate()
                .max_by_key(|(_, entry)| entry.idle_since)?
                .0;
            Some(entries.swap_remove(newest))
        });
        if cache.get(key).is_some_and(Vec::is_empty) {
            cache.remove(key);
        }
        let remaining = cache.values().map(Vec::len).sum();
        (cached, expired, remaining)
    }
}

impl PreparedProxyRuntime {
    /// Prepares runtime state required by the configured proxy transport.
    ///
    /// # Arguments
    ///
    /// - `config`: Client configuration containing proxy settings
    ///
    /// # Returns
    ///
    /// A prepared proxy runtime with any necessary helper state
    ///
    /// # Errors
    ///
    /// Returns proxy initialization errors when the `arachnea-proxy` feature is enabled
    /// and the loopback helper cannot be started
    pub(super) async fn new(config: &ArachneaHttpConfig) -> Result<Self, ArachneaHttpError> {
        #[cfg(feature = "arachnea-proxy")]
        {
            let loopback_proxy = match &config.proxy {
                HttpProxyConfig::Arachnea(core) => {
                    let runtime = config.proxy_runtime.clone().unwrap_or_default();
                    let definitions = core.parameter_definitions();
                    let context = arachnea_proxy::core::context_from_parameter_pairs(
                        config
                            .proxy_parameters
                            .iter()
                            .map(|(name, value)| (name.as_str(), value.as_str())),
                        &definitions,
                    );
                    Some(Arc::new(
                        ArachneaRquestLoopback::start_on_runtime(
                            runtime,
                            core.clone(),
                            definitions,
                            context,
                            true,
                        )
                        .await
                        .map_err(|error| {
                            ArachneaHttpError::Proxy(format!(
                                "failed to prepare shared loopback proxy: {error}"
                            ))
                        })?,
                    ))
                }
                _ => None,
            };
            return Ok(Self {
                pinned_clients: config.proxy_transport_pool.clone().unwrap_or_default(),
                loopback_proxy,
            });
        }

        #[cfg(not(feature = "arachnea-proxy"))]
        {
            let _ = config;
            Ok(Self {})
        }
    }

    /// Returns the proxy URL that built-in engines should use, when one is available.
    ///
    /// # Arguments
    ///
    /// - `config`: Client configuration to inspect.
    ///
    /// # Returns
    ///
    /// The proxy URL string when a network proxy is configured, or the loopback
    /// proxy URL when `arachnea-proxy` is enabled and initialized. Returns
    /// `None` when proxying is disabled or the loopback is unavailable.
    pub(super) fn proxy_url<'a>(&'a self, config: &'a ArachneaHttpConfig) -> Option<&'a str> {
        match &config.proxy {
            HttpProxyConfig::Network(url) => Some(url.as_str()),
            #[cfg(feature = "arachnea-proxy")]
            HttpProxyConfig::Arachnea(_) => self
                .loopback_proxy
                .as_deref()
                .map(|loopback| loopback.proxy_url()),
            HttpProxyConfig::Disabled => None,
        }
    }

    /// Returns the proxy URL for chaser-cf, binding configured proxy parameters
    /// into its routing lease when Chrome cannot send them.
    ///
    /// # Arguments
    ///
    /// - `config`: Client configuration to inspect.
    ///
    /// # Returns
    ///
    /// The explicit network proxy URL, or the prepared loopback URL when one is
    /// available. Returns `None` when no proxy is configured.
    pub(super) fn chaser_cf_proxy_url<'a>(
        &'a self,
        config: &'a ArachneaHttpConfig,
    ) -> Option<&'a str> {
        match &config.proxy {
            HttpProxyConfig::Network(url) => Some(url.as_str()),
            #[cfg(feature = "arachnea-proxy")]
            HttpProxyConfig::Arachnea(_) => self
                .loopback_proxy
                .as_deref()
                .map(|loopback| loopback.proxy_url()),
            HttpProxyConfig::Disabled => None,
        }
    }

    /// Builds a rquest client configured with proxy transport and parameters.
    ///
    /// # Arguments
    ///
    /// - `config`: Client configuration containing proxy settings
    ///
    /// # Returns
    ///
    /// An optional rquest client configured with the arachnea-proxy loopback
    /// transport when available, or `None` when no loopback proxy is initialized
    ///
    /// # Errors
    ///
    /// Returns proxy errors when the loopback client cannot be created
    #[cfg(feature = "arachnea-proxy")]
    pub(super) fn rquest_client(
        &self,
        config: &ArachneaHttpConfig,
    ) -> Result<Option<rquest::Client>, ArachneaHttpError> {
        let Some(loopback) = self.loopback_proxy.as_deref() else {
            return Ok(None);
        };
        if config.proxy_parameters.is_empty() {
            return loopback
                .client()
                .map(Some)
                .map_err(|err| ArachneaHttpError::Proxy(err.to_string()));
        }
        loopback
            .client_with_parameters(
                config
                    .proxy_parameters
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
            )
            .map(Some)
            .map_err(|err| ArachneaHttpError::Proxy(err.to_string()))
    }

    /// Builds a rquest client configured with proxy transport and parameters.
    ///
    /// Without the `arachnea-proxy` feature, no loopback transport exists and
    /// this method always returns `None`.
    ///
    /// # Arguments
    ///
    /// - `_config`: Client configuration; unused in this feature set.
    ///
    /// # Returns
    ///
    /// Always `None` without the `arachnea-proxy` feature.
    ///
    /// # Errors
    ///
    /// Never returns an error in this feature set.
    #[cfg(not(feature = "arachnea-proxy"))]
    pub(super) fn rquest_client(
        &self,
        _config: &ArachneaHttpConfig,
    ) -> Result<Option<rquest::Client>, ArachneaHttpError> {
        Ok(None)
    }
}

/// Keeps the routing lease alive while a cached browser page can open subresources.
pub(super) struct RuntimeBoundBrowserPage {
    pub(super) page: Box<dyn crate::browser::BrowserPageSession>,
    pub(super) _runtime: Arc<PreparedProxyRuntime>,
}

#[async_trait::async_trait]
impl crate::browser::BrowserPageSession for RuntimeBoundBrowserPage {
    async fn navigate(
        &mut self,
        request: PageNavigationRequest,
    ) -> Result<PageNavigationResponse, ArachneaHttpError> {
        self.page.navigate(request).await
    }

    async fn fetch(
        &mut self,
        request: PageFetchRequest,
    ) -> Result<PageFetchResponse, ArachneaHttpError> {
        self.page.fetch(request).await
    }

    async fn click_and_wait(
        &mut self,
        request: PageClickRequest,
    ) -> Result<PageClickResponse, ArachneaHttpError> {
        self.page.click_and_wait(request).await
    }

    async fn metadata(&mut self) -> Result<BrowserSessionMetadata, ArachneaHttpError> {
        self.page.metadata().await
    }

    async fn read_turnstile_token(&mut self) -> Result<Option<String>, ArachneaHttpError> {
        self.page.read_turnstile_token().await
    }

    async fn clear_turnstile_token(&mut self) -> Result<(), ArachneaHttpError> {
        self.page.clear_turnstile_token().await
    }

    async fn close(self: Box<Self>) {
        self.page.close().await;
    }
}
