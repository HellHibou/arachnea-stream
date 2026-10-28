//! HTTP configuration builder and validation helpers.

use super::*;

/// Builder for `ArachneaHttpConfig`.
#[derive(Debug, Clone)]
pub struct ArachneaHttpConfigBuilder {
    /// Configuration being assembled by the builder.
    config: ArachneaHttpConfig,
}

impl ArachneaHttpConfigBuilder {
    /// Injects a response classifier for correlated Rquest proxy requests.
    ///
    /// # Arguments
    /// - `classifier`: Nonblocking policy evaluated before body materialization.
    #[cfg(feature = "arachnea-proxy")]
    pub fn proxy_response_classifier(
        mut self,
        classifier: arachnea_proxy::core::http::response_classifier::ProxyResponsePolicy,
    ) -> Self {
        self.config.proxy_response_classifier = Some(classifier);
        self
    }
    /// Sets the outbound HTTP engine.
    ///
    /// # Arguments
    ///
    /// - `engine`: Engine kind selected by the caller.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn engine(mut self, engine: impl Into<HttpEngineKind>) -> Self {
        self.config.engine = engine.into();
        self
    }

    /// Sets the default request execution mode.
    ///
    /// # Arguments
    ///
    /// - `mode`: Mode used when an individual request does not override it.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn default_request_mode(mut self, mode: HttpRequestMode) -> Self {
        self.config.default_request_mode = mode;
        self
    }

    /// Sets the outbound HTTP engine from a concrete instance.
    ///
    /// # Arguments
    ///
    /// - `engine`: Concrete engine instance, including any engine-specific
    ///   parameters already applied.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn engine_instance<E>(mut self, engine: E) -> Self
    where
        E: HttpEngine + 'static,
    {
        self.config.engine = HttpEngineKind::from_engine(engine);
        self
    }

    /// Sets Cloudflare orchestration mode.
    ///
    /// This also updates the default request mode:
    /// `Disabled` selects `Direct`, `KnownProtected` selects
    /// `CloudflareSmart`, and `DetectOnForbidden` selects `Auto`. A later
    /// [`default_request_mode`](Self::default_request_mode) call overrides that
    /// choice.
    ///
    /// # Arguments
    ///
    /// - `cloudflare`: Cloudflare mode selected by the caller.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn cloudflare(mut self, cloudflare: CloudflareMode) -> Self {
        self.config.cloudflare = cloudflare;
        self.config.default_request_mode = match cloudflare {
            CloudflareMode::Disabled => HttpRequestMode::Direct,
            CloudflareMode::KnownProtected => HttpRequestMode::CloudflareSmart,
            CloudflareMode::DetectOnForbidden => HttpRequestMode::Auto,
        };
        self
    }

    /// Sets the Cloudflare challenge solver engine.
    ///
    /// # Arguments
    ///
    /// - `solver`: Cloudflare solver selected by the caller.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn cloudflare_solver(mut self, solver: impl Into<CloudflareSolverKind>) -> Self {
        self.config.cloudflare_solver = solver.into();
        self
    }

    /// Sets the Cloudflare challenge solver from a concrete engine instance.
    ///
    /// # Arguments
    ///
    /// - `engine`: Concrete solver engine instance, including any
    ///   engine-specific parameters already applied.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn cloudflare_solver_instance<E>(mut self, engine: E) -> Self
    where
        E: HttpEngine + 'static,
    {
        self.config.cloudflare_solver = CloudflareSolverKind::from_engine(engine);
        self
    }

    /// Sets the browser-backed Cloudflare fallback solver.
    ///
    /// # Arguments
    ///
    /// - `solver`: Browser solver selected by the caller.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn cloudflare_browser_solver(
        mut self,
        solver: impl Into<CloudflareBrowserSolverKind>,
    ) -> Self {
        self.config.cloudflare_browser_solver = solver.into();
        self
    }

    /// Sets the browser-backed Cloudflare fallback solver from a concrete engine.
    ///
    /// # Arguments
    ///
    /// - `engine`: Concrete solver engine instance, including any
    ///   engine-specific parameters already applied.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn cloudflare_browser_solver_instance<E>(mut self, engine: E) -> Self
    where
        E: HttpEngine + 'static,
    {
        self.config.cloudflare_browser_solver = CloudflareBrowserSolverKind::from_engine(engine);
        self
    }

    /// Sets the browser profile.
    ///
    /// # Arguments
    ///
    /// - `profile`: Browser profile used by engines and headers.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn user_agent_profile(mut self, profile: BrowserProfile) -> Self {
        self.config.user_agent_profile = profile;
        self
    }

    /// Sets the outbound proxy or transport override.
    ///
    /// # Arguments
    ///
    /// - `proxy`: Proxy configuration selected by the caller.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn proxy(mut self, proxy: HttpProxyConfig) -> Self {
        self.config.proxy = proxy;
        self
    }

    /// Shares one application-owned loopback listener across HTTP clients.
    ///
    /// # Arguments
    /// - `runtime`: Shared listener and connection-context registry.
    #[cfg(feature = "arachnea-proxy")]
    pub fn proxy_runtime(
        mut self,
        runtime: Arc<arachnea_proxy::server::LoopbackProxyRuntime>,
    ) -> Self {
        self.config.proxy_runtime = Some(runtime);
        self
    }

    /// Shares idle proxy connections across a family of compatible HTTP clients.
    ///
    /// # Arguments
    /// - `pool`: Caller-owned bounded transport pool, independent of cookies and requests.
    #[cfg(feature = "arachnea-proxy")]
    pub fn proxy_transport_pool(mut self, pool: Arc<crate::client::ProxyTransportPool>) -> Self {
        self.config.proxy_transport_pool = Some(pool);
        self
    }

    /// Routes requests through an explicit network proxy URL.
    ///
    /// # Arguments
    ///
    /// - `url`: Proxy URL, optionally without a scheme. Bare authorities default
    ///   to `http://`.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn proxy_url(mut self, url: impl Into<String>) -> Self {
        self.config.proxy = HttpProxyConfig::Network(normalize_proxy_url(url.into()));
        self
    }

    /// Routes requests through an in-process `arachnea-proxy` core.
    ///
    /// Engines that do not expose a direct connector API may fall back to a
    /// loopback proxy compatibility path.
    ///
    /// # Arguments
    ///
    /// - `core`: Proxy core reused for outbound routing decisions.
    ///
    /// # Returns
    ///
    /// The updated builder.
    #[cfg(feature = "arachnea-proxy")]
    pub fn proxy_core(mut self, core: ArachneaProxyCore) -> Self {
        self.config.proxy = HttpProxyConfig::Arachnea(core);
        self
    }

    /// Adds one request routing parameter for proxy transports.
    ///
    /// Parameters are interpreted by the configured `arachnea-proxy` core when
    /// the transport supports proxy-level request hints.
    ///
    /// # Arguments
    ///
    /// - `name`: Proxy parameter name such as `country`.
    /// - `value`: Parameter value such as `FR`.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn proxy_parameter(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.config
            .proxy_parameters
            .push((name.into(), value.into()));
        self
    }

    /// Sets the proactive Cloudflare cookie refresh margin.
    ///
    /// # Arguments
    ///
    /// - `duration`: Margin before expiry where refresh becomes necessary.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn cookie_refresh_margin(mut self, duration: Duration) -> Self {
        self.config.cookie_refresh_margin = duration;
        self
    }

    /// Authorizes proxy retries for operations whose effects and body permit replay.
    ///
    /// # Arguments
    /// - `replayable`: Explicit caller guarantee; false preserves safe defaults.
    pub fn proxy_retry_replayable(mut self, replayable: bool) -> Self {
        self.config.proxy_retry_replayable = replayable;
        self
    }

    /// Sets the outbound request timeout.
    ///
    /// # Arguments
    ///
    /// - `duration`: Timeout for HTTP engine requests.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn request_timeout(mut self, duration: Duration) -> Self {
        self.config.request_timeout = duration;
        self
    }

    /// Sets the maximum number of redirects followed by the HTTP facade.
    ///
    /// # Arguments
    ///
    /// - `redirects`: `None` uses the crate default, `Some(0)` disables
    ///   redirects, and `Some(n)` follows up to `n` redirects.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn max_redirects(mut self, redirects: Option<usize>) -> Self {
        self.config.max_redirects = redirects;
        self
    }

    /// Sets the maximum Cloudflare retry count.
    ///
    /// # Arguments
    ///
    /// - `retries`: Maximum refresh attempts for one request.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn max_cloudflare_retries(mut self, retries: u8) -> Self {
        self.config.max_cloudflare_retries = retries;
        self
    }

    /// Sets the TTL for session cookies.
    ///
    /// # Arguments
    ///
    /// - `duration`: TTL used when a cookie has no explicit expiry.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn default_session_cookie_ttl(mut self, duration: Duration) -> Self {
        self.config.default_session_cookie_ttl = duration;
        self
    }

    /// Sets the browser session management configuration.
    ///
    /// # Arguments
    ///
    /// - `config`: Browser session configuration.
    ///
    /// # Returns
    ///
    /// The updated builder.
    pub fn browser_session(mut self, config: BrowserSessionConfig) -> Self {
        self.config.browser_session = config;
        self
    }

    /// Validates and finalizes the configuration.
    ///
    /// # Returns
    ///
    /// A validated `ArachneaHttpConfig`.
    ///
    /// # Errors
    ///
    /// Returns `InvalidConfiguration` when the request timeout, session-cookie
    /// TTL, browser session count, or browser idle timeout is zero; when
    /// Cloudflare-capable modes have no retries; when a Cloudflare mode has no
    /// compatible solver or required feature; or when the proxy URL is malformed.
    pub fn build(self) -> Result<ArachneaHttpConfig, ArachneaHttpError> {
        if self.config.request_timeout.is_zero() {
            return Err(ArachneaHttpError::InvalidConfiguration(
                "request_timeout must be greater than zero".to_string(),
            ));
        }
        if self.config.default_session_cookie_ttl.is_zero() {
            return Err(ArachneaHttpError::InvalidConfiguration(
                "default_session_cookie_ttl must be greater than zero".to_string(),
            ));
        }
        if self.config.browser_session.max_sessions == 0 {
            return Err(ArachneaHttpError::InvalidConfiguration(
                "browser_session.max_sessions must be greater than zero".to_string(),
            ));
        }
        if self.config.browser_session.idle_timeout.is_zero() {
            return Err(ArachneaHttpError::InvalidConfiguration(
                "browser_session.idle_timeout must be greater than zero".to_string(),
            ));
        }
        if self.config.default_request_mode != HttpRequestMode::Direct
            && self.config.max_cloudflare_retries == 0
        {
            return Err(ArachneaHttpError::InvalidConfiguration(
                "max_cloudflare_retries must be greater than zero when Cloudflare-capable request modes are enabled"
                    .to_string(),
            ));
        }
        if self.config.default_request_mode == HttpRequestMode::CloudflareSmart
            && matches!(self.config.cloudflare_solver, CloudflareSolverKind::Auto)
            && !cfg!(feature = "ghostwire")
        {
            return Err(ArachneaHttpError::InvalidConfiguration(
                "CloudflareSmart with the automatic smart solver requires the ghostwire feature or an injected solver engine".to_string(),
            ));
        }
        if self.config.default_request_mode == HttpRequestMode::CloudflareSmart
            && matches!(
                self.config.cloudflare_solver,
                CloudflareSolverKind::Disabled
            )
        {
            return Err(ArachneaHttpError::InvalidConfiguration(
                "CloudflareSmart requires a smart Cloudflare solver".to_string(),
            ));
        }
        validate_proxy(&self.config.proxy)?;
        validate_browser_solver(&self.config)?;
        Ok(self.config)
    }
}

/// Validates proxy settings that can be checked from the static configuration.
///
/// # Arguments
///
/// - `proxy`: Proxy configuration being finalized.
///
/// # Errors
///
/// Returns `InvalidConfiguration` when the proxy URL is malformed.
fn validate_proxy(proxy: &HttpProxyConfig) -> Result<(), ArachneaHttpError> {
    let Some(url) = proxy.network_url() else {
        return Ok(());
    };
    let parsed = Url::parse(url).map_err(|err| {
        ArachneaHttpError::InvalidConfiguration(format!("invalid proxy URL: {err}"))
    })?;
    if parsed.host_str().is_none() {
        return Err(ArachneaHttpError::InvalidConfiguration(
            "proxy URL must contain a host".to_string(),
        ));
    }
    Ok(())
}

/// Normalizes proxy input so callers may pass `host:port` or a fully qualified URL.
///
/// # Arguments
///
/// - `url`: Proxy URL or bare authority.
///
/// # Returns
///
/// The URL unchanged when it contains `://`, otherwise the same value prefixed
/// with `http://`.
fn normalize_proxy_url(url: String) -> String {
    if url.contains("://") {
        url
    } else {
        format!("http://{url}")
    }
}

/// Validates browser solver settings that can be known at build time.
///
/// # Arguments
///
/// - `config`: Configuration being finalized.
///
/// # Errors
///
/// Returns `InvalidConfiguration` when the selected browser solver cannot be
/// constructed with the enabled feature set.
fn validate_browser_solver(config: &ArachneaHttpConfig) -> Result<(), ArachneaHttpError> {
    match &config.cloudflare_browser_solver {
        CloudflareBrowserSolverKind::ChaserCf if !cfg!(feature = "chaser-cf") => {
            Err(ArachneaHttpError::InvalidConfiguration(
                "chaser-cf browser solver requires the chaser-cf feature".to_string(),
            ))
        }
        CloudflareBrowserSolverKind::TauriCloudflareSolver
            if !cfg!(feature = "tauri-cloudflare-solver") =>
        {
            Err(ArachneaHttpError::InvalidConfiguration(
                "Tauri Cloudflare browser solver requires the tauri-cloudflare-solver feature"
                    .to_string(),
            ))
        }
        CloudflareBrowserSolverKind::Disabled
            if config.default_request_mode == HttpRequestMode::CloudflareBrowser =>
        {
            Err(ArachneaHttpError::InvalidConfiguration(
                "CloudflareBrowser requires a browser Cloudflare solver".to_string(),
            ))
        }
        CloudflareBrowserSolverKind::Auto
            if config.default_request_mode == HttpRequestMode::CloudflareBrowser
                && !cfg!(any(feature = "chaser-cf", feature = "tauri-cloudflare-solver")) =>
        {
            Err(ArachneaHttpError::InvalidConfiguration(
                "CloudflareBrowser with the automatic browser solver requires the chaser-cf or tauri-cloudflare-solver feature, or an injected solver engine".to_string(),
            ))
        }
        _ => Ok(()),
    }
}

impl Default for ArachneaHttpConfigBuilder {
    /// Builds a default configuration builder.
    ///
    /// # Returns
    ///
    /// A builder containing `ArachneaHttpConfig::default()`.
    fn default() -> Self {
        Self {
            config: ArachneaHttpConfig::default(),
        }
    }
}
