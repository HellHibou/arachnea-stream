use std::{fmt, sync::Arc, time::Duration};

#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::core::ArachneaProxyCore;
use url::Url;

use crate::{
    browser::BrowserSessionConfig,
    engine::{DynHttpEngine, HttpEngine},
    error::ArachneaHttpError,
};

/// Configures how outbound HTTP engines reach the network.
#[derive(Clone)]
pub enum HttpProxyConfig {
    /// Do not use any proxy or custom transport override.
    Disabled,
    /// Route requests through an explicit proxy URL such as `http://127.0.0.1:8080`
    /// or `socks5h://127.0.0.1:9050`.
    Network(String),
    /// Route requests through an in-process `arachnea-proxy` core when the
    /// selected engine supports direct connector integration.
    #[cfg(feature = "arachnea-proxy")]
    Arachnea(ArachneaProxyCore),
}

impl HttpProxyConfig {
    /// Returns a normalized proxy URL when this configuration uses a network proxy.
    ///
    /// # Returns
    ///
    /// The normalized proxy URL, or `None` when another transport mode is selected.
    pub fn network_url(&self) -> Option<&str> {
        match self {
            Self::Network(url) => Some(url.as_str()),
            #[cfg(feature = "arachnea-proxy")]
            Self::Arachnea(_) | Self::Disabled => None,
            #[cfg(not(feature = "arachnea-proxy"))]
            Self::Disabled => None,
        }
    }
}

impl Default for HttpProxyConfig {
    /// Builds the default proxy configuration.
    ///
    /// # Returns
    ///
    /// A configuration with proxy support disabled.
    fn default() -> Self {
        Self::Disabled
    }
}

impl fmt::Debug for HttpProxyConfig {
    /// Formats the proxy selection without exposing connector internals.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => f.write_str("Disabled"),
            Self::Network(url) => f.debug_tuple("Network").field(url).finish(),
            #[cfg(feature = "arachnea-proxy")]
            Self::Arachnea(_) => f.write_str("Arachnea"),
        }
    }
}

/// Selects which HTTP engine executes outbound requests.
#[derive(Clone)]
pub enum HttpEngineKind {
    /// Always use `rquest`, except for configured Cloudflare refreshes.
    Rquest,
    /// Prefer `rquest` and use the configured solver only when Cloudflare handling requires it.
    Auto,
    /// Use a caller-provided engine instance.
    Engine(DynHttpEngine),
}

impl HttpEngineKind {
    /// Selects an HTTP engine instance.
    ///
    /// # Parameters
    ///
    /// - `engine`: Engine instance with its own implementation-specific
    ///   configuration.
    ///
    /// # Returns
    ///
    /// An engine selection that reuses the provided instance.
    pub fn engine(engine: DynHttpEngine) -> Self {
        Self::Engine(engine)
    }

    /// Selects an HTTP engine from a concrete implementation.
    ///
    /// # Parameters
    ///
    /// - `engine`: Concrete engine instance.
    ///
    /// # Returns
    ///
    /// An engine selection that owns the provided instance.
    pub fn from_engine<E>(engine: E) -> Self
    where
        E: HttpEngine + 'static,
    {
        Self::Engine(Arc::new(engine))
    }

    /// Returns the configured direct engine instance.
    ///
    /// # Returns
    ///
    /// The shared engine when this kind selects a direct engine.
    pub(crate) fn direct_engine(&self) -> Option<DynHttpEngine> {
        match self {
            Self::Engine(engine) => Some(engine.clone()),
            Self::Rquest | Self::Auto => None,
        }
    }
}

impl fmt::Debug for HttpEngineKind {
    /// Formats the engine selection without exposing implementation internals.
    ///
    /// # Parameters
    ///
    /// - `f`: Formatter receiving the debug representation.
    ///
    /// # Errors
    ///
    /// Returns formatting errors from the formatter.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rquest => f.write_str("Rquest"),
            Self::Auto => f.write_str("Auto"),
            Self::Engine(engine) => f.debug_tuple("Engine").field(&engine.name()).finish(),
        }
    }
}

impl<E> From<Arc<E>> for HttpEngineKind
where
    E: HttpEngine + 'static,
{
    /// Converts a shared engine into an HTTP engine selection.
    ///
    /// # Parameters
    ///
    /// - `engine`: Shared concrete engine instance.
    ///
    /// # Returns
    ///
    /// An engine selection backed by the shared instance.
    fn from(engine: Arc<E>) -> Self {
        Self::Engine(engine)
    }
}

impl From<DynHttpEngine> for HttpEngineKind {
    /// Converts a shared dynamic engine into an HTTP engine selection.
    ///
    /// # Parameters
    ///
    /// - `engine`: Shared dynamic engine instance.
    ///
    /// # Returns
    ///
    /// An engine selection backed by the shared instance.
    fn from(engine: DynHttpEngine) -> Self {
        Self::Engine(engine)
    }
}

/// Selects which Cloudflare-capable engine refreshes challenge cookies.
#[derive(Clone)]
pub enum CloudflareSolverKind {
    /// Pick Ghostwire when the `ghostwire` feature is enabled.
    Auto,
    /// Do not configure a smart Cloudflare solver.
    Disabled,
    /// Use a caller-provided engine instance.
    Engine(DynHttpEngine),
}

impl CloudflareSolverKind {
    /// Selects a Cloudflare solver instance.
    ///
    /// # Parameters
    ///
    /// - `engine`: Engine instance with its own implementation-specific
    ///   configuration.
    ///
    /// # Returns
    ///
    /// A solver selection that reuses the provided instance.
    pub fn engine(engine: DynHttpEngine) -> Self {
        Self::Engine(engine)
    }

    /// Selects a Cloudflare solver from a concrete implementation.
    ///
    /// # Parameters
    ///
    /// - `engine`: Concrete engine instance.
    ///
    /// # Returns
    ///
    /// A solver selection that owns the provided instance.
    pub fn from_engine<E>(engine: E) -> Self
    where
        E: HttpEngine + 'static,
    {
        Self::Engine(Arc::new(engine))
    }
}

impl fmt::Debug for CloudflareSolverKind {
    /// Formats the solver selection without exposing implementation internals.
    ///
    /// # Parameters
    ///
    /// - `f`: Formatter receiving the debug representation.
    ///
    /// # Errors
    ///
    /// Returns formatting errors from the formatter.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("Auto"),
            Self::Disabled => f.write_str("Disabled"),
            Self::Engine(engine) => f.debug_tuple("Engine").field(&engine.name()).finish(),
        }
    }
}

/// Selects the browser-backed Cloudflare solver used as a stronger fallback.
#[derive(Clone)]
pub enum CloudflareBrowserSolverKind {
    /// Pick Obscura when enabled, otherwise the next available browser solver.
    Auto,
    /// Do not configure a browser-backed Cloudflare solver.
    Disabled,
    /// Use chaser-cf. Requires the `chaser-cf` feature.
    ChaserCf,
    /// Use the interactive Tauri/Wry solver. Requires the
    /// `tauri-cloudflare-solver` feature.
    TauriCloudflareSolver,
    /// Use the embedded Obscura headless browser (no Chromium, no separate
    /// Obscura process). Requires the `obscura` feature.
    Obscura,
    /// Use a caller-provided engine instance.
    Engine(DynHttpEngine),
}

impl CloudflareBrowserSolverKind {
    /// Selects a browser-backed Cloudflare solver instance.
    ///
    /// # Parameters
    ///
    /// - `engine`: Engine instance with its own implementation-specific
    ///   configuration.
    ///
    /// # Returns
    ///
    /// A solver selection that reuses the provided instance.
    pub fn engine(engine: DynHttpEngine) -> Self {
        Self::Engine(engine)
    }

    /// Selects a browser-backed Cloudflare solver from a concrete engine.
    ///
    /// # Parameters
    ///
    /// - `engine`: Concrete engine instance.
    ///
    /// # Returns
    ///
    /// A solver selection that owns the provided instance.
    pub fn from_engine<E>(engine: E) -> Self
    where
        E: HttpEngine + 'static,
    {
        Self::Engine(Arc::new(engine))
    }
}

impl fmt::Debug for CloudflareBrowserSolverKind {
    /// Formats the browser solver selection without exposing implementation internals.
    ///
    /// # Parameters
    ///
    /// - `f`: Formatter receiving the debug representation.
    ///
    /// # Errors
    ///
    /// Returns formatting errors from the formatter.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("Auto"),
            Self::Disabled => f.write_str("Disabled"),
            Self::ChaserCf => f.write_str("ChaserCf"),
            Self::TauriCloudflareSolver => f.write_str("TauriCloudflareSolver"),
            Self::Obscura => f.write_str("Obscura"),
            Self::Engine(engine) => f.debug_tuple("Engine").field(&engine.name()).finish(),
        }
    }
}

impl<E> From<Arc<E>> for CloudflareBrowserSolverKind
where
    E: HttpEngine + 'static,
{
    /// Converts a shared engine into a browser solver selection.
    ///
    /// # Parameters
    ///
    /// - `engine`: Shared concrete engine instance.
    ///
    /// # Returns
    ///
    /// A solver selection backed by the shared instance.
    fn from(engine: Arc<E>) -> Self {
        Self::Engine(engine)
    }
}

impl From<DynHttpEngine> for CloudflareBrowserSolverKind {
    /// Converts a shared dynamic engine into a browser solver selection.
    ///
    /// # Parameters
    ///
    /// - `engine`: Shared dynamic engine instance.
    ///
    /// # Returns
    ///
    /// A solver selection backed by the shared instance.
    fn from(engine: DynHttpEngine) -> Self {
        Self::Engine(engine)
    }
}

impl<E> From<Arc<E>> for CloudflareSolverKind
where
    E: HttpEngine + 'static,
{
    /// Converts a shared engine into a Cloudflare solver selection.
    ///
    /// # Parameters
    ///
    /// - `engine`: Shared concrete engine instance.
    ///
    /// # Returns
    ///
    /// A solver selection backed by the shared instance.
    fn from(engine: Arc<E>) -> Self {
        Self::Engine(engine)
    }
}

impl From<DynHttpEngine> for CloudflareSolverKind {
    /// Converts a shared dynamic engine into a Cloudflare solver selection.
    ///
    /// # Parameters
    ///
    /// - `engine`: Shared dynamic engine instance.
    ///
    /// # Returns
    ///
    /// A solver selection backed by the shared instance.
    fn from(engine: DynHttpEngine) -> Self {
        Self::Engine(engine)
    }
}

/// Controls whether Cloudflare-specific orchestration is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudflareMode {
    /// Disable Cloudflare refresh and fallback behavior.
    Disabled,
    /// Treat requested origins as protected and refresh cookies before use.
    KnownProtected,
    /// Detect Cloudflare blocks and retry after refresh.
    DetectOnForbidden,
}

/// Predefined execution policy selected per outbound HTTP request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HttpRequestMode {
    /// Use `rquest` first, then Ghostwire when enabled, then the configured
    /// browser solver when enabled.
    #[default]
    Auto,
    /// Use `rquest` only.
    Direct,
    /// Reuse valid Cloudflare cookies and refresh missing or stale cookies with
    /// Ghostwire when the `ghostwire` feature or an injected smart solver exists.
    CloudflareSmart,
    /// Reuse valid Cloudflare cookies and refresh missing or stale cookies with
    /// the configured browser solver.
    CloudflareBrowser,
}

/// Browser profile used to keep user-agent and fingerprint intent coherent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserProfile {
    /// A recent stable Chrome desktop profile.
    ChromeStable,
    /// A recent stable Firefox desktop profile.
    FirefoxStable,
    /// A caller-provided user-agent string.
    Custom(String),
}

impl BrowserProfile {
    /// Returns the user-agent header value associated with the profile.
    ///
    /// # Returns
    ///
    /// A borrowed user-agent string suitable for outbound requests.
    pub fn user_agent(&self) -> &str {
        match self {
            Self::ChromeStable => {
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36"
            }
            Self::FirefoxStable => {
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:126.0) Gecko/20100101 Firefox/126.0"
            }
            Self::Custom(value) => value,
        }
    }
}

impl Default for BrowserProfile {
    /// Builds the default browser profile.
    ///
    /// # Returns
    ///
    /// The default Chrome stable profile.
    fn default() -> Self {
        Self::ChromeStable
    }
}

/// Runtime configuration for `ArachneaHttpClient`.
#[derive(Debug, Clone)]
pub struct ArachneaHttpConfig {
    /// Explicit engine selection.
    pub engine: HttpEngineKind,
    /// Default execution policy used when a request does not specify one.
    pub default_request_mode: HttpRequestMode,
    /// Cloudflare handling mode.
    pub cloudflare: CloudflareMode,
    /// Engine used by the smart Cloudflare cookie refresh path.
    pub cloudflare_solver: CloudflareSolverKind,
    /// Browser-backed engine used as a stronger Cloudflare fallback.
    pub cloudflare_browser_solver: CloudflareBrowserSolverKind,
    /// Browser profile used for outbound headers and engine construction.
    pub user_agent_profile: BrowserProfile,
    /// Transport override applied to engines that support proxy configuration.
    pub proxy: HttpProxyConfig,
    /// Request routing parameters sent to proxy transports that support them.
    pub proxy_parameters: Vec<(String, String)>,
    /// Margin before cookie expiry where Cloudflare cookies are proactively refreshed.
    pub cookie_refresh_margin: Duration,
    /// Timeout applied to outbound HTTP requests.
    pub request_timeout: Duration,
    /// Maximum number of HTTP redirects followed by the facade.
    ///
    /// `None` uses the crate default, currently three redirects. `Some(0)`
    /// disables redirect following.
    pub max_redirects: Option<usize>,
    /// Maximum Cloudflare refresh retries per request.
    pub max_cloudflare_retries: u8,
    /// TTL applied to session cookies that do not carry an explicit expiration.
    pub default_session_cookie_ttl: Duration,
    /// Configuration for the browser session manager when browser page sessions
    /// are needed.
    pub browser_session: BrowserSessionConfig,
}

impl ArachneaHttpConfig {
    /// Starts a fluent configuration builder.
    ///
    /// # Returns
    ///
    /// A builder initialized with conservative defaults.
    pub fn builder() -> ArachneaHttpConfigBuilder {
        ArachneaHttpConfigBuilder::default()
    }
}

impl Default for ArachneaHttpConfig {
    /// Builds the default client configuration.
    ///
    /// # Returns
    ///
    /// A default configuration that starts with `rquest` and uses enabled
    /// Cloudflare solver features only when Cloudflare protection is detected.
    fn default() -> Self {
        Self {
            engine: HttpEngineKind::Rquest,
            default_request_mode: HttpRequestMode::Auto,
            cloudflare: CloudflareMode::Disabled,
            cloudflare_solver: CloudflareSolverKind::Auto,
            cloudflare_browser_solver: CloudflareBrowserSolverKind::Auto,
            user_agent_profile: BrowserProfile::default(),
            proxy: HttpProxyConfig::Disabled,
            proxy_parameters: Vec::new(),
            cookie_refresh_margin: Duration::from_secs(300),
            request_timeout: Duration::from_secs(30),
            max_redirects: None,
            max_cloudflare_retries: 1,
            default_session_cookie_ttl: Duration::from_secs(60 * 60),
            browser_session: BrowserSessionConfig::default(),
        }
    }
}

/// Builder for `ArachneaHttpConfig`.
#[derive(Debug, Clone)]
pub struct ArachneaHttpConfigBuilder {
    /// Configuration being assembled by the builder.
    config: ArachneaHttpConfig,
}

impl ArachneaHttpConfigBuilder {
    /// Sets the outbound HTTP engine.
    ///
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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

    /// Routes requests through an explicit network proxy URL.
    ///
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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

    /// Sets the outbound request timeout.
    ///
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// Returns `InvalidConfiguration` when durations are zero or retry limits are
    /// inconsistent with the selected request mode.
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
/// # Parameters
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
fn normalize_proxy_url(url: String) -> String {
    if url.contains("://") {
        url
    } else {
        format!("http://{url}")
    }
}

/// Validates browser solver settings that can be known at build time.
///
/// # Parameters
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
        CloudflareBrowserSolverKind::Obscura if !cfg!(feature = "obscura") => {
            Err(ArachneaHttpError::InvalidConfiguration(
                "Obscura browser solver requires the obscura feature".to_string(),
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
                && !cfg!(any(
                    feature = "obscura",
                    feature = "chaser-cf",
                    feature = "tauri-cloudflare-solver"
                )) =>
        {
            Err(ArachneaHttpError::InvalidConfiguration(
                "CloudflareBrowser with the automatic browser solver requires the obscura, chaser-cf, or tauri-cloudflare-solver feature, or an injected solver engine".to_string(),
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
