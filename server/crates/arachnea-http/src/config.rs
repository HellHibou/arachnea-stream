mod builder;
pub use builder::ArachneaHttpConfigBuilder;

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
    ///
    /// The box owns a core value; cloning this variant allocates a new box and
    /// clones the core while retaining its existing shared internal handles.
    #[cfg(feature = "arachnea-proxy")]
    Arachnea(Box<ArachneaProxyCore>),
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
        crate::HTTP_DEFAULT_PROXY
    }
}

impl fmt::Debug for HttpProxyConfig {
    /// Formats the proxy selection without exposing connector internals.
    ///
    /// # Arguments
    ///
    /// - `f`: Destination formatter.
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
    /// Use the built-in `rquest` path.
    ///
    /// The per-request mode still controls Cloudflare refresh behavior. This
    /// variant and [`Auto`](Self::Auto) currently select the same built-in path.
    Rquest,
    /// Prefer the built-in `rquest` path and use configured solvers through the
    /// request mode.
    ///
    /// This variant and [`Rquest`](Self::Rquest) currently select the same
    /// built-in path.
    Auto,
    /// Use a caller-provided engine instance.
    Engine(DynHttpEngine),
}

impl HttpEngineKind {
    /// Selects an HTTP engine instance.
    ///
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
    /// Pick the default compiled browser solver.
    ///
    /// The engine layer selects Tauri/Wry when
    /// `tauri-cloudflare-solver` is enabled, otherwise chaser-cf when
    /// `chaser-cf` is enabled. It returns no solver when neither is enabled.
    Auto,
    /// Do not configure a browser-backed Cloudflare solver.
    Disabled,
    /// Use chaser-cf. Requires the `chaser-cf` feature.
    ChaserCf,
    /// Use the interactive Tauri/Wry solver. Requires the
    /// `tauri-cloudflare-solver` feature.
    TauriCloudflareSolver,
    /// Use a caller-provided engine instance.
    Engine(DynHttpEngine),
}

impl CloudflareBrowserSolverKind {
    /// Selects a browser-backed Cloudflare solver instance.
    ///
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
    /// # Arguments
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
        crate::HTTP_DEFAULT_BROWSER_PROFILE
    }
}

/// Runtime configuration for `ArachneaHttpClient`.
#[derive(Debug, Clone)]
pub struct ArachneaHttpConfig {
    /// Explicit engine selection.
    ///
    /// Only [`HttpEngineKind::Engine`] changes runtime dispatch by injecting a
    /// direct engine; the default execution policy still comes from
    /// [`default_request_mode`](Self::default_request_mode).
    pub engine: HttpEngineKind,
    /// Default execution policy used when a request does not specify one.
    pub default_request_mode: HttpRequestMode,
    /// Recorded Cloudflare handling mode.
    ///
    /// The builder uses this value to choose the initial
    /// [`default_request_mode`](Self::default_request_mode). Runtime dispatch
    /// reads that request mode; mutating this field directly after construction
    /// does not reconfigure an existing client.
    pub cloudflare: CloudflareMode,
    /// Engine used by the smart Cloudflare cookie refresh path.
    pub cloudflare_solver: CloudflareSolverKind,
    /// Browser-backed engine used as a stronger Cloudflare fallback.
    pub cloudflare_browser_solver: CloudflareBrowserSolverKind,
    /// Browser profile used for outbound headers and engine construction.
    pub user_agent_profile: BrowserProfile,
    /// Transport override applied to engines that support proxy configuration.
    pub proxy: HttpProxyConfig,
    /// Shared loopback server for application-owned proxy transports.
    #[cfg(feature = "arachnea-proxy")]
    pub proxy_runtime: Option<Arc<arachnea_proxy::server::LoopbackProxyRuntime>>,
    /// Optional caller-owned pool of idle, request-rebound proxy transports.
    #[cfg(feature = "arachnea-proxy")]
    pub proxy_transport_pool: Option<Arc<crate::client::ProxyTransportPool>>,
    /// Request routing parameters sent to proxy transports that support them.
    pub proxy_parameters: Vec<(String, String)>,
    /// Optional code-injected response policy; not part of serialized configuration.
    #[cfg(feature = "arachnea-proxy")]
    pub proxy_response_classifier:
        Option<arachnea_proxy::core::http::response_classifier::ProxyResponsePolicy>,
    /// Margin before cookie expiry where Cloudflare cookies are proactively refreshed.
    pub cookie_refresh_margin: Duration,
    /// Ordinary total timeout. Localized searches use it separately for origin
    /// response headers and accepted-body transfer within their shared search budget.
    /// Reused proxy evidence caps the header allowance at 60 seconds without
    /// increasing a shorter configured timeout.
    pub request_timeout: Duration,
    /// Explicit operation-level authorization to replay a reproducible modifying request.
    pub proxy_retry_replayable: bool,
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
            engine: crate::HTTP_DEFAULT_ENGINE,
            default_request_mode: crate::HTTP_DEFAULT_REQUEST_MODE,
            cloudflare: crate::HTTP_DEFAULT_CLOUDFLARE_MODE,
            cloudflare_solver: crate::HTTP_DEFAULT_CLOUDFLARE_SOLVER,
            cloudflare_browser_solver: crate::HTTP_DEFAULT_BROWSER_SOLVER,
            user_agent_profile: BrowserProfile::default(),
            proxy: crate::HTTP_DEFAULT_PROXY,
            #[cfg(feature = "arachnea-proxy")]
            proxy_runtime: None,
            #[cfg(feature = "arachnea-proxy")]
            proxy_transport_pool: None,
            proxy_parameters: Vec::new(),
            #[cfg(feature = "arachnea-proxy")]
            proxy_response_classifier: None,
            cookie_refresh_margin: crate::HTTP_DEFAULT_COOKIE_REFRESH_MARGIN,
            request_timeout: crate::HTTP_DEFAULT_REQUEST_TIMEOUT,
            proxy_retry_replayable: crate::HTTP_DEFAULT_PROXY_RETRY_REPLAYABLE,
            max_redirects: None,
            max_cloudflare_retries: crate::HTTP_DEFAULT_MAX_CLOUDFLARE_RETRIES,
            default_session_cookie_ttl: crate::HTTP_DEFAULT_SESSION_COOKIE_TTL,
            browser_session: BrowserSessionConfig::default(),
        }
    }
}
