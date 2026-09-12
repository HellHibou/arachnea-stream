//! Internal HTTP engine adapters.

use std::sync::Arc;

use crate::chaser_session::CachedChaserSession;
use arachnea_core::persistence::TypedEntityStore;
use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode};

use crate::{
    browser::BrowserPageSession,
    config::{ArachneaHttpConfig, CloudflareBrowserSolverKind},
    error::ArachneaHttpError,
};

#[cfg(feature = "chaser-cf")]
/// chaser-cf browser-based Cloudflare solver engine.
pub mod chaser_cf;
#[cfg(feature = "ghostwire")]
/// Ghostwire smart Cloudflare challenge solver engine.
pub mod ghostwire;
#[cfg(feature = "obscura")]
/// Obscura embedded headless-browser Cloudflare solver engine.
pub mod obscura;
/// Fast `rquest` engine adapter.
pub mod rquest;
#[cfg(feature = "tauri-cloudflare-solver")]
/// Interactive Tauri/Wry browser-based Cloudflare solver engine.
pub mod tauri_cloudflare;

/// Shared dynamic HTTP engine handle.
pub type DynHttpEngine = Arc<dyn HttpEngine>;

/// Internal header used to return a solver-observed browser user-agent.
pub(crate) const SOLVER_USER_AGENT_HEADER: &str = "x-arachnea-solver-user-agent";

/// Engine-normalized HTTP request.
#[derive(Debug, Clone)]
pub struct EngineRequest {
    /// HTTP method.
    pub method: Method,
    /// Absolute request URL.
    pub url: String,
    /// Request headers.
    pub headers: HeaderMap,
    /// Optional request body.
    pub body: Option<Bytes>,
}

/// Engine-normalized HTTP response.
#[derive(Debug, Clone)]
pub struct EngineResponse {
    /// Final URL reported by the engine.
    pub url: String,
    /// HTTP status code.
    pub status: StatusCode,
    /// Response headers.
    pub headers: HeaderMap,
    /// Complete response body.
    pub body: Bytes,
}

/// Common interface implemented by all HTTP engine adapters.
#[async_trait]
pub trait HttpEngine: Send + Sync {
    /// Returns a stable engine name for logs and errors.
    ///
    /// # Returns
    ///
    /// A short engine identifier.
    fn name(&self) -> &'static str;

    /// Sends one normalized request.
    ///
    /// # Parameters
    ///
    /// - `request`: Request to execute.
    ///
    /// # Returns
    ///
    /// A normalized response with headers and body materialized.
    ///
    /// # Errors
    ///
    /// Returns engine-specific failures mapped to `ArachneaHttpError`.
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError>;

    /// Opens a persistent browser page session for page-scoped JavaScript work.
    ///
    /// Implementations that only support normalized HTTP transport return
    /// `UnsupportedEngineOperation`. Browser-backed engines override this to
    /// retain one page across a source navigation and subsequent `fetch()`.
    async fn open_browser_page_session(
        &self,
    ) -> Result<Box<dyn BrowserPageSession>, ArachneaHttpError> {
        Err(ArachneaHttpError::UnsupportedEngineOperation {
            engine: self.name(),
            operation: "persistent browser page sessions",
        })
    }

    /// Refreshes Cloudflare state for one normalized request.
    ///
    /// Solver engines can override this method to avoid collecting a page body
    /// when only cookies and browser metadata are needed. The default behavior
    /// sends the request normally.
    ///
    /// # Parameters
    ///
    /// - `request`: Request used to refresh the Cloudflare session.
    ///
    /// # Returns
    ///
    /// A normalized response containing any cookies or solver metadata.
    ///
    /// # Errors
    ///
    /// Returns engine-specific failures mapped to `ArachneaHttpError`.
    async fn refresh_cloudflare(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.send(request).await
    }

    /// Refreshes Cloudflare state while bypassing engine-specific caches.
    ///
    /// Browser-backed solvers can override this method to force a fresh browser
    /// solve after an actively blocked response proves cached cookies are not
    /// accepted by the target. The default behavior uses the normal refresh
    /// path.
    ///
    /// # Parameters
    ///
    /// - `request`: Request used to refresh the Cloudflare session.
    ///
    /// # Returns
    ///
    /// A normalized response containing fresh cookies or solver metadata.
    ///
    /// # Errors
    ///
    /// Returns engine-specific failures mapped to `ArachneaHttpError`.
    async fn refresh_cloudflare_fresh(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.refresh_cloudflare(request).await
    }

    /// Builds an HTTP client configuration that uses this solver engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP configuration builder rejects the provided
    /// engine instance or any default configuration value.
    fn config(self) -> Result<ArachneaHttpConfig, ArachneaHttpError>
    where
        Self: Sized + 'static,
    {
        return ArachneaHttpConfig::builder().engine_instance(self).build();
    }
}

/// Builds the automatic smart Cloudflare solver.
///
/// # Parameters
///
/// - `config`: Client configuration used to configure the engine.
///
/// # Returns
///
/// Ghostwire when the `ghostwire` feature is enabled, otherwise `None`.
///
/// # Errors
///
/// Returns construction failures from the selected engine.
pub(crate) async fn build_auto_smart_cloudflare_solver(
    config: &ArachneaHttpConfig,
    ghostwire_proxy_url: Option<&str>,
) -> Result<Option<DynHttpEngine>, ArachneaHttpError> {
    #[cfg(feature = "ghostwire")]
    {
        return build_ghostwire_engine(config, ghostwire_proxy_url)
            .await
            .map(Some);
    }

    #[cfg(not(feature = "ghostwire"))]
    {
        let _ = config;
        let _ = ghostwire_proxy_url;
        Ok(None)
    }
}

/// Builds the configured browser-backed Cloudflare solver.
///
/// # Parameters
///
/// - `config`: Client configuration used to configure the engine.
/// - `solver`: Browser solver selection.
///
/// # Returns
///
/// A dynamic browser solver when one is enabled and available.
///
/// # Errors
///
/// Returns `CloudflareSolverUnavailable` for unavailable explicit selections,
/// or construction failures from the selected engine.
pub(crate) async fn build_browser_cloudflare_solver(
    config: &ArachneaHttpConfig,
    solver: &CloudflareBrowserSolverKind,
    proxy_url: Option<&str>,
    session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
) -> Result<Option<DynHttpEngine>, ArachneaHttpError> {
    match solver {
        CloudflareBrowserSolverKind::Disabled => Ok(None),
        CloudflareBrowserSolverKind::Engine(engine) => Ok(Some(engine.clone())),
        CloudflareBrowserSolverKind::Auto => {
            get_default_cloudflare_solver(config, proxy_url, session_store).await
        }
        CloudflareBrowserSolverKind::ChaserCf => {
            build_explicit_chaser_cf_engine(config, proxy_url, session_store).await
        }
        CloudflareBrowserSolverKind::TauriCloudflareSolver => {
            build_explicit_tauri_cloudflare_engine(config).await
        }
        CloudflareBrowserSolverKind::Obscura => {
            build_explicit_obscura_engine(config, proxy_url, session_store).await
        }
    }
}

/// Builds the default browser-backed Cloudflare solver.
///
/// # Parameters
///
/// - `config`: Client configuration used to configure the engine.
///
/// # Returns
///
/// The selected default browser-backed Cloudflare solver when one is available.
///
/// # Errors
///
/// Returns construction failures from the selected engine.
pub(crate) async fn get_default_cloudflare_solver(
    config: &ArachneaHttpConfig,
    proxy_url: Option<&str>,
    session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
) -> Result<Option<DynHttpEngine>, ArachneaHttpError> {
    #[cfg(feature = "tauri-cloudflare-solver")]
    {
        let _ = proxy_url;
        let _ = session_store;
        return build_tauri_cloudflare_engine(config).await.map(Some);
    }

    #[cfg(all(not(feature = "tauri-cloudflare-solver"), feature = "chaser-cf"))]
    {
        return build_chaser_cf_engine(config, proxy_url, session_store)
            .await
            .map(Some);
    }

    #[cfg(not(any(feature = "chaser-cf", feature = "tauri-cloudflare-solver")))]
    {
        let _ = config;
        let _ = proxy_url;
        let _ = session_store;
        Ok(None)
    }
}

/// Builds chaser-cf for an explicit browser solver selection.
///
/// # Parameters
///
/// - `config`: Client configuration used to configure the engine.
///
/// # Returns
///
/// A dynamic chaser-cf engine.
///
/// # Errors
///
/// Returns `CloudflareSolverUnavailable` when the `chaser-cf` feature is not
/// enabled, or construction failures from chaser-cf.
async fn build_explicit_chaser_cf_engine(
    config: &ArachneaHttpConfig,
    proxy_url: Option<&str>,
    session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
) -> Result<Option<DynHttpEngine>, ArachneaHttpError> {
    #[cfg(feature = "chaser-cf")]
    {
        return build_chaser_cf_engine(config, proxy_url, session_store)
            .await
            .map(Some);
    }

    #[cfg(not(feature = "chaser-cf"))]
    {
        let _ = config;
        let _ = proxy_url;
        let _ = session_store;
        Err(ArachneaHttpError::CloudflareSolverUnavailable)
    }
}

/// Builds the Tauri/Wry solver for an explicit browser solver selection.
///
/// # Parameters
///
/// - `config`: Client configuration used to configure the engine.
///
/// # Returns
///
/// A dynamic Tauri/Wry engine.
///
/// # Errors
///
/// Returns `CloudflareSolverUnavailable` when the
/// `tauri-cloudflare-solver` feature is not enabled.
async fn build_explicit_tauri_cloudflare_engine(
    config: &ArachneaHttpConfig,
) -> Result<Option<DynHttpEngine>, ArachneaHttpError> {
    #[cfg(feature = "tauri-cloudflare-solver")]
    {
        return build_tauri_cloudflare_engine(config).await.map(Some);
    }

    #[cfg(not(feature = "tauri-cloudflare-solver"))]
    {
        let _ = config;
        Err(ArachneaHttpError::CloudflareSolverUnavailable)
    }
}

/// Builds Obscura for an explicit browser solver selection.
///
/// # Parameters
///
/// - `config`: Client configuration used to configure the engine.
/// - `proxy_url`: Effective proxy URL selected by the HTTP client runtime.
/// - `session_store`: Shared typed Cloudflare session store.
///
/// # Returns
///
/// A dynamic Obscura engine.
///
/// # Errors
///
/// Returns `CloudflareSolverUnavailable` when the `obscura` feature is not
/// enabled, or construction failures from the Obscura adapter.
async fn build_explicit_obscura_engine(
    config: &ArachneaHttpConfig,
    proxy_url: Option<&str>,
    session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
) -> Result<Option<DynHttpEngine>, ArachneaHttpError> {
    #[cfg(feature = "obscura")]
    {
        return build_obscura_engine(config, proxy_url, session_store)
            .await
            .map(Some);
    }

    #[cfg(not(feature = "obscura"))]
    {
        let _ = config;
        let _ = proxy_url;
        let _ = session_store;
        Err(ArachneaHttpError::CloudflareSolverUnavailable)
    }
}

/// Builds an Obscura engine.
///
/// # Parameters
///
/// - `config`: Client configuration used to configure the engine.
/// - `proxy_url`: Effective proxy URL selected by the HTTP client runtime.
/// - `session_store`: Shared typed Cloudflare session store.
///
/// # Returns
///
/// A dynamic Obscura engine.
///
/// # Errors
///
/// Returns a construction failure from the Obscura adapter.
#[cfg(feature = "obscura")]
pub(crate) async fn build_obscura_engine(
    config: &ArachneaHttpConfig,
    proxy_url: Option<&str>,
    session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
) -> Result<DynHttpEngine, ArachneaHttpError> {
    Ok(Arc::new(obscura::ObscuraEngine::new_with_proxy_url(
        config,
        proxy_url,
        session_store,
    )?))
}

/// Builds a Ghostwire engine.
///
/// # Parameters
///
/// - `config`: Client configuration used to configure the engine.
///
/// # Returns
///
/// A dynamic Ghostwire engine.
///
/// # Errors
///
/// Returns a construction failure from the Ghostwire adapter.
#[cfg(feature = "ghostwire")]
pub(crate) async fn build_ghostwire_engine(
    config: &ArachneaHttpConfig,
    ghostwire_proxy_url: Option<&str>,
) -> Result<DynHttpEngine, ArachneaHttpError> {
    Ok(Arc::new(ghostwire::GhostwireEngine::new(
        config,
        ghostwire_proxy_url,
    )?))
}

/// Builds a chaser-cf engine.
///
/// # Parameters
///
/// - `config`: Client configuration used to configure the engine.
///
/// # Returns
///
/// A dynamic chaser-cf engine.
///
/// # Errors
///
/// Returns a construction failure from the chaser-cf adapter.
#[cfg(feature = "chaser-cf")]
pub(crate) async fn build_chaser_cf_engine(
    config: &ArachneaHttpConfig,
    proxy_url: Option<&str>,
    session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
) -> Result<DynHttpEngine, ArachneaHttpError> {
    Ok(Arc::new(chaser_cf::ChaserCfEngine::new_with_proxy_url(
        config,
        proxy_url,
        session_store,
    )?))
}

/// Builds an interactive Tauri/Wry Cloudflare solver engine.
///
/// # Parameters
///
/// - `config`: Client configuration used to configure the solver.
///
/// # Returns
///
/// A dynamic Tauri/Wry solver engine.
///
/// # Errors
///
/// Returns a construction failure from the Tauri/Wry adapter.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) async fn build_tauri_cloudflare_engine(
    config: &ArachneaHttpConfig,
) -> Result<DynHttpEngine, ArachneaHttpError> {
    Ok(Arc::new(
        tauri_cloudflare::TauriCloudflareSolverEngine::new(config),
    ))
}
