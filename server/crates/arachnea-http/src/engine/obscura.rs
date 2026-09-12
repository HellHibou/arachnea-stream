//! Obscura embedded headless-browser engine.
//!
//! Phase 1 of `docs/dev-tracking/obscura-embedded-engine-implementation-plan.md`:
//! this module only carries the Cargo integration, the stable Arachnea-side
//! configuration, and the engine construction path. Cloudflare solving and
//! persistent page sessions land in Phases 2 and 3, so every normalized
//! operation currently fails with `ObscuraFailure` instead of pretending to
//! solve anything. The `Auto` browser-solver selection is intentionally
//! unchanged; Obscura must be selected explicitly through
//! `CloudflareBrowserSolverKind::Obscura`.

use std::{sync::Arc, time::Duration};

use arachnea_core::persistence::TypedEntityStore;
use async_trait::async_trait;
use http::Method;
use tracing::debug;

use crate::{
    chaser_session::{memory_session_store, CachedChaserSession},
    config::{ArachneaHttpConfig, HttpProxyConfig},
    engine::{EngineRequest, EngineResponse, HttpEngine},
    error::ArachneaHttpError,
};

/// Registry name for the Obscura embedded engine.
pub const ENGINE_NAME: &str = "obscura";

/// Obscura Git revision pinned in `Cargo.toml`.
///
/// Kept manually in sync with the `rev` entry of the dependency so runtime
/// logs can report the exact embedded browser revision. The pin is re-checked
/// after the Phase 2 PoC, as required by the integration plan.
pub const OBSCURA_PINNED_REVISION: &str = "eec047a188cc75b7a1a257397ad84493ee59c091";

/// Dedicated timeout for the Obscura Cloudflare clearance protocol, kept
/// independent from the HTTP request timeout because an embedded browser solve
/// can far outlast a regular request. Starting bound copied from the chaser-cf
/// solve timeout until Obscura measurements (Phase 5) provide better values.
const OBSCURA_CLEARANCE_TIMEOUT: Duration = Duration::from_secs(180);

/// Stable Arachnea-side settings for the embedded Obscura browser.
///
/// This type intentionally keeps Obscura types out of the Arachnea
/// configuration surface; only the embedded engine consumes it.
///
/// Phase 1 carries configuration and construction only; the settings are
/// consumed by the Phase 2 browser build and solve paths, so the dead-code
/// allowance is scoped to this skeleton type.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ObscuraEngineConfig {
    /// Enable the Obscura stealth profile. The flag reaches
    /// `obscura::BrowserConfig::stealth` for fingerprint alignment, but the
    /// impersonated TLS/HTTP stack requires the dependency `stealth` feature,
    /// currently excluded because its BoringSSL linking conflicts with
    /// `newwreq` (see the integration plan).
    stealth: bool,
    /// Enable the Obscura render layer used by real click flows and element
    /// geometry.
    render: bool,
    /// User-Agent forced into the embedded browser context.
    user_agent: Option<String>,
    /// Timeout applied to normalized requests executed by this engine.
    request_timeout: Duration,
    /// Timeout of the initial Cloudflare clearance protocol.
    clearance_timeout: Duration,
    /// Margin before `cf_clearance` expiry where cached sessions refresh.
    cookie_refresh_margin: Duration,
    /// Proxy transport selection carried from `ArachneaHttpConfig`.
    proxy: HttpProxyConfig,
}

impl ObscuraEngineConfig {
    /// Derives the embedded engine settings from the client configuration.
    fn from_arachnea(config: &ArachneaHttpConfig) -> Self {
        Self {
            stealth: true,
            render: true,
            user_agent: Some(config.user_agent_profile.user_agent().to_string()),
            request_timeout: config.request_timeout,
            clearance_timeout: OBSCURA_CLEARANCE_TIMEOUT,
            cookie_refresh_margin: config.cookie_refresh_margin,
            proxy: config.proxy.clone(),
        }
    }
}

/// Transport mode selected for the embedded browser network stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObscuraTransportMode {
    /// The embedded browser's own network stack, optionally behind a network
    /// proxy URL.
    NetworkProxy(Option<String>),
    /// Every intercepted request is fulfilled through the in-process Arachnea
    /// proxy core. Selected for `HttpProxyConfig::Arachnea`; the interceptor
    /// adapter itself lands in Phase 4.
    #[allow(dead_code)]
    InterceptorFulfill,
}

impl ObscuraTransportMode {
    /// Selects the transport from the proxy configuration.
    fn select(proxy: &HttpProxyConfig) -> Self {
        match proxy {
            HttpProxyConfig::Disabled => Self::NetworkProxy(None),
            HttpProxyConfig::Network(url) => Self::NetworkProxy(Some(url.clone())),
            #[cfg(feature = "arachnea-proxy")]
            HttpProxyConfig::Arachnea(_) => Self::InterceptorFulfill,
        }
    }

    /// Returns the stable transport label used in logs; never carries secrets.
    fn label(&self) -> &'static str {
        match self {
            Self::NetworkProxy(_) => "network-proxy",
            Self::InterceptorFulfill => "interceptor-fulfill",
        }
    }
}

/// Persistent Cloudflare session cache reusing the existing
/// `CachedChaserSession` storage shape, per the integration plan. The cache is
/// read and written from Phase 2 onward.
#[allow(dead_code)]
struct ObscuraSessionCache {
    /// Shared typed session store.
    store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    /// Proactive refresh margin before `cf_clearance` expiry.
    refresh_margin: Duration,
}

/// Obscura adapter that resolves Cloudflare sessions with the embedded
/// headless browser: no Chrome/Chromium, no separate Obscura/CDP process.
///
/// Phase 1 only constructs the engine; the Phase 2 solve paths consume its
/// fields, so the dead-code allowance is scoped to this skeleton struct.
#[allow(dead_code)]
pub struct ObscuraEngine {
    /// Stable Arachnea-side engine settings.
    config: ObscuraEngineConfig,
    /// Transport mode selected from the effective proxy configuration.
    transport: ObscuraTransportMode,
    /// Optional persistent session cache reused across refreshes.
    session_cache: Option<ObscuraSessionCache>,
}

impl ObscuraEngine {
    /// Creates an engine from the client configuration with an in-memory
    /// session store.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration used to derive the engine settings.
    ///
    /// # Returns
    ///
    /// A configured Obscura engine.
    ///
    /// # Errors
    ///
    /// Returns `ObscuraFailure` when the in-memory session store cannot be
    /// created.
    pub fn new(config: &ArachneaHttpConfig) -> Result<Self, ArachneaHttpError> {
        let session_store = memory_session_store().map_err(|error| {
            ArachneaHttpError::ObscuraFailure(format!("session cache store unavailable: {error}"))
        })?;
        Self::new_with_proxy_url(config, config.proxy.network_url(), session_store)
    }

    /// Creates an engine using the effective proxy URL selected by the HTTP
    /// client runtime and a shared typed session store.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration used to derive the engine settings.
    /// - `proxy_url`: Effective network proxy URL, when the runtime selected
    ///   one.
    /// - `session_store`: Shared typed Cloudflare session store.
    ///
    /// # Returns
    ///
    /// A configured Obscura engine.
    ///
    /// # Errors
    ///
    /// Returns `ObscuraFailure` when the engine settings cannot be derived.
    pub(crate) fn new_with_proxy_url(
        config: &ArachneaHttpConfig,
        proxy_url: Option<&str>,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) -> Result<Self, ArachneaHttpError> {
        let engine_config = ObscuraEngineConfig::from_arachnea(config);
        // The runtime-provided proxy URL wins so the solver shares the rquest
        // proxy route, exactly like the chaser-cf adapter.
        let transport = match proxy_url {
            Some(url) => ObscuraTransportMode::NetworkProxy(Some(url.to_string())),
            None => ObscuraTransportMode::select(&engine_config.proxy),
        };
        debug!(
            engine = ENGINE_NAME,
            revision = OBSCURA_PINNED_REVISION,
            platform = std::env::consts::OS,
            transport = transport.label(),
            stealth = engine_config.stealth,
            render = engine_config.render,
            has_user_agent = engine_config.user_agent.is_some(),
            "obscura engine constructed"
        );
        Ok(Self {
            config: engine_config,
            transport,
            session_cache: Some(ObscuraSessionCache {
                store: session_store,
                refresh_margin: config.cookie_refresh_margin,
            }),
        })
    }

    /// Rejects the methods the browser-solver contract does not accept.
    fn reject_unsupported_methods(
        request: &EngineRequest,
        operation: &'static str,
    ) -> Result<(), ArachneaHttpError> {
        if request.method != Method::GET && request.method != Method::HEAD {
            return Err(ArachneaHttpError::UnsupportedEngineOperation {
                engine: ENGINE_NAME,
                operation,
            });
        }
        Ok(())
    }
}

#[async_trait]
impl HttpEngine for ObscuraEngine {
    fn name(&self) -> &'static str {
        ENGINE_NAME
    }

    /// Executes a normalized GET/HEAD request through the embedded browser.
    ///
    /// Phase 2 wires the clearance protocol and HTML extraction; until then
    /// the engine fails explicitly instead of returning an unsolved response.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for non-GET/HEAD methods, and
    /// `ObscuraFailure` while the solve protocol is not implemented yet.
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        Self::reject_unsupported_methods(&request, "non-GET/HEAD HTTP requests")?;
        Err(ArachneaHttpError::ObscuraFailure(
            "Cloudflare solving is not implemented yet (Phase 2 of the Obscura integration plan)"
                .to_string(),
        ))
    }

    /// Refreshes Cloudflare state, reusing the persistent session cache when
    /// it holds a still-valid clearance.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for non-GET/HEAD methods, and
    /// `ObscuraFailure` while the solve protocol is not implemented yet.
    async fn refresh_cloudflare(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        Self::reject_unsupported_methods(&request, "non-GET Cloudflare refresh requests")?;
        Err(ArachneaHttpError::ObscuraFailure(
            "Cloudflare refresh is not implemented yet (Phase 2 of the Obscura integration plan)"
                .to_string(),
        ))
    }

    /// Refreshes Cloudflare state while bypassing the persistent session
    /// cache.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for non-GET/HEAD methods, and
    /// `ObscuraFailure` while the solve protocol is not implemented yet.
    async fn refresh_cloudflare_fresh(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        Self::reject_unsupported_methods(&request, "non-GET Cloudflare refresh requests")?;
        Err(ArachneaHttpError::ObscuraFailure(
            "Cloudflare refresh is not implemented yet (Phase 2 of the Obscura integration plan)"
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Disabled` proxy config selects the browser stack without proxy URL.
    #[test]
    fn disabled_proxy_selects_direct_network_transport() {
        assert_eq!(
            ObscuraTransportMode::select(&HttpProxyConfig::Disabled),
            ObscuraTransportMode::NetworkProxy(None)
        );
    }

    /// Explicit network proxy URLs flow into the transport selection as-is.
    #[test]
    fn network_proxy_url_is_carried_into_the_transport() {
        assert_eq!(
            ObscuraTransportMode::select(&HttpProxyConfig::Network(
                "socks5h://127.0.0.1:9050".to_string()
            )),
            ObscuraTransportMode::NetworkProxy(Some("socks5h://127.0.0.1:9050".to_string()))
        );
    }

    /// Transport labels stay generic so logs never embed proxy URLs or
    /// credentials.
    #[test]
    fn transport_labels_stay_generic() {
        assert_eq!(
            ObscuraTransportMode::NetworkProxy(Some("http://user:pass@host:1".to_string()))
                .label(),
            "network-proxy"
        );
        assert_eq!(
            ObscuraTransportMode::InterceptorFulfill.label(),
            "interceptor-fulfill"
        );
    }
}