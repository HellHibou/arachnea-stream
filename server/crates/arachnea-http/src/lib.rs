#![warn(missing_docs)]
//! Outbound HTTP client crate for Arachnea.
//!
//! `arachnea-http` provides a unified HTTP client facade that can use `rquest` for
//! fast, direct requests and optional Cloudflare-capable engines (Ghostwire, chaser-cf,
//! or Tauri/Wry-based solvers) for handling Cloudflare-protected origins.
//!
//! # Design Contract
//!
//! - **Single facade**: All HTTP operations go through `ArachneaHttpClient`.
//! - **Explicit engine selection**: Cloudflare behavior is explicit through configuration
//!   and request modes.
//! - **Shared cookie cache**: Cookies are shared across all client instances in the process.
//! - **Engine isolation**: Engine-specific APIs are isolated in `src/engine/`.
//! - **Security**: Sensitive headers are redacted in logs and errors by default.
//!
//! # Main Features
//!
//! - `ArachneaHttpClient`: Main request facade with builder pattern
//! - Multiple request modes: `Auto`, `Direct`, `CloudflareSmart`, `CloudflareBrowser`
//! - Shared in-memory cookie cache with Cloudflare state helpers
//! - Configurable proxy support (network proxy or `arachnea-proxy` integration)
//! - Automatic Cloudflare detection and solver fallback chain
//! - JSON and text response parsing with charset detection
//! - Redirect handling with configurable limits
//!
//! # Authorization Note
//!
//! This crate is intended for **authorized traffic only**. Callers must respect target
//! service terms, rate limits, `robots.txt`, and applicable law. The crate does not
//! implement rate limiting or legal compliance checks.
//!
//! # Example
//!
//! ```rust,no_run
//! use arachnea_http::ArachneaHttpClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), arachnea_http::ArachneaHttpError> {
//!     // Create client with default configuration
//!     let client = ArachneaHttpClient::default().await?;
//!
//!     // Send a GET request
//!     let response = client.get("https://example.com").send().await?;
//!
//!     // Get response as text
//!     let text = response.text().await?;
//!     println!("Response: {}", text);
//!
//!     Ok(())
//! }
//! ```
//!
//! # Crate Modules
//!
//! - `browser`: Reusable browser session management and in-page fetch execution
//! - `client`: Main client facade and request builder
//! - `config`: Configuration structures and builders
//! - `cookies`: Shared in-memory cookie cache
//! - `cloudflare`: Cloudflare detection and cookie state helpers
//! - `engine`: HTTP engine abstraction and concrete implementations
//! - `error`: Error types for HTTP operations

/// Reusable browser session management and in-page HTTP fetch execution.
pub mod browser;
/// Typed persistent Cloudflare session entity shared across engines.
pub mod chaser_session;
/// Main client facade, request builder, and response types.
pub mod client;
/// Cloudflare block detection and cookie state helpers.
pub mod cloudflare;
/// Configuration structures and builders.
pub mod config;
/// Shared in-memory cookie cache.
pub mod cookies;
/// HTTP engine abstraction and implementations.
pub mod engine;
/// Error types for HTTP operations.
pub mod error;

pub use browser::{
    BrowserPageSession, BrowserSessionConfig, BrowserSessionHandle, BrowserSessionKey,
    BrowserSessionManager, BrowserSessionMetadata, PageClickRequest, PageClickResponse,
    PageFetchRequest, PageFetchResponse, PageNavigationRequest, PageNavigationResponse,
    TurnstileTokenState,
};

pub use client::{
    header_map_from_strings, ArachneaHttpClient, ArachneaRequestBuilder, ArachneaResponse,
    RequestOptions,
};
pub use cloudflare::CloudflareCookieState;
pub use config::{
    ArachneaHttpConfig, ArachneaHttpConfigBuilder, BrowserProfile, CloudflareBrowserSolverKind,
    CloudflareMode, CloudflareSolverKind, HttpEngineKind, HttpProxyConfig, HttpRequestMode,
};
pub use cookies::{global_cookie_cache, CookieEntry, SharedCookieCache};
pub use engine::{DynHttpEngine, EngineRequest, EngineResponse, HttpEngine};
pub use error::ArachneaHttpError;

// Crate-owned defaults, contracts and resource references.

// --- Engine identities ---

/// Stable registry name of the chaser HTTP engine.
#[cfg(feature = "chaser-cf")]
pub const HTTP_CHASER_ENGINE_NAME: &str = "chaser-cf";

/// Stable registry name of the tauri HTTP engine.
#[cfg(feature = "tauri-cloudflare-solver")]
pub const HTTP_TAURI_ENGINE_NAME: &str = "tauri-cloudflare-solver";

/// Stable registry name of the ghostwire HTTP engine.
#[cfg(feature = "ghostwire")]
pub const HTTP_GHOSTWIRE_ENGINE_NAME: &str = "ghostwire";

// --- Session contracts ---

/// Internal response header carrying the solver user-agent, without cookie values.
pub(crate) const SOLVER_USER_AGENT_HEADER: &str = "x-arachnea-solver-user-agent";

/// Persistence store name for cached Cloudflare sessions; changing it changes store identity.
pub const CLOUDFLARE_SESSION_STORE_NAME: &str = "cloudflare-session";

/// Fallback lifetime for session-cache entries without an explicit expiry.
pub(crate) const HTTP_SESSION_CACHE_TTL_NO_EXPIRY: std::time::Duration =
    std::time::Duration::from_secs(1_800);

// --- Chaser solver budgets ---

/// Margin before cached chaser clearance expires.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_SESSION_CACHE_REFRESH_MARGIN: std::time::Duration =
    std::time::Duration::from_secs(300);

/// Browser challenge budget, independent of the ordinary HTTP request timeout.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_SOLVE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(180);

/// Maximum wait for the target selector after a browser click.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_CLICK_WAIT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);

/// Maximum challenge-solving time after a page click.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_CLICK_SOLVE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(90);

/// Maximum wait for an injected captcha token.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_TURNSTILE_WAIT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(300);

/// Passive challenge wait in milliseconds before proof-of-work interaction.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_CLEARANCE_PASSIVE_WAIT_MS: u64 = 2_000;

/// Minimum interval in milliseconds between automated challenge clicks.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_CLEARANCE_CLICK_INTERVAL_MS: u64 = 1_000;

// --- Native solver budgets ---

/// Interval between native solver cookie inspections.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_COOKIE_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(500);

/// Standalone native solver clearance stabilization period.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_CLEARANCE_STABILIZATION: std::time::Duration =
    std::time::Duration::from_millis(500);

/// Standalone solver delay after the post-clearance reload.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_POST_CLEARANCE_RELOAD_DELAY: std::time::Duration =
    std::time::Duration::from_secs(10);

/// Application-owned solver clearance stabilization period.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_APP_CLEARANCE_STABILIZATION: std::time::Duration =
    std::time::Duration::from_secs(3);

/// Application-owned solver delay after the post-clearance reload.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_APP_POST_CLEARANCE_RELOAD_DELAY: std::time::Duration =
    std::time::Duration::from_secs(12);

/// Initial native solver window width in logical pixels.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_WINDOW_WIDTH: f64 = 1_280.0;

/// Initial native solver window height in logical pixels.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_WINDOW_HEIGHT: f64 = 900.0;

/// Debounce lifetime of a successful native interactive refresh.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_REFRESH_CACHE_TTL: std::time::Duration =
    std::time::Duration::from_secs(30);

/// Grace period before destroying a closed standalone solver window.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_WINDOW_DESTROY_GRACE: std::time::Duration =
    std::time::Duration::from_secs(1);

/// Maximum wait for native solver page-state capture.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_PAGE_STATE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(2000);

/// Polling interval while waiting for native solver page-state capture.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_PAGE_STATE_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(50);

/// Standalone solver event-loop wakeup interval.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_EVENT_LOOP_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(50);

// --- Embedded chaser scripts ---

/// Embedded browser script for open browser page session.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_SCRIPT_OPEN_BROWSER_PAGE_SESSION: &str =
    include_str!("../resources/chaser_cf/open_browser_page_session.js");

/// Embedded browser script for clear turnstile token.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_SCRIPT_CLEAR_TURNSTILE_TOKEN: &str =
    include_str!("../resources/chaser_cf/clear_turnstile_token.js");

/// Embedded browser script for read turnstile token.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_SCRIPT_READ_TURNSTILE_TOKEN: &str =
    include_str!("../resources/chaser_cf/read_turnstile_token.js");

/// Embedded browser script for challenge checkbox point.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_SCRIPT_CHALLENGE_CHECKBOX_POINT: &str =
    include_str!("../resources/chaser_cf/challenge_checkbox_point.js");

/// Embedded browser script for has turnstile token.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_SCRIPT_HAS_TURNSTILE_TOKEN: &str =
    include_str!("../resources/chaser_cf/has_turnstile_token.js");

/// Embedded browser script for challenge is present.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_SCRIPT_CHALLENGE_IS_PRESENT: &str =
    include_str!("../resources/chaser_cf/challenge_is_present.js");

/// Embedded browser script for debug dump.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_SCRIPT_DEBUG_DUMP: &str =
    include_str!("../resources/chaser_cf/debug_dump.js");

// --- HTTP configuration defaults ---

/// Default fast HTTP engine.
pub(crate) const HTTP_DEFAULT_ENGINE: crate::config::HttpEngineKind =
    crate::config::HttpEngineKind::Rquest;

/// Default request execution mode.
pub(crate) const HTTP_DEFAULT_REQUEST_MODE: crate::config::HttpRequestMode =
    crate::config::HttpRequestMode::Auto;

/// Default Cloudflare handling policy, disabled unless configured.
pub(crate) const HTTP_DEFAULT_CLOUDFLARE_MODE: crate::config::CloudflareMode =
    crate::config::CloudflareMode::Disabled;

/// Default explicitly requested Cloudflare solver selection.
pub(crate) const HTTP_DEFAULT_CLOUDFLARE_SOLVER: crate::config::CloudflareSolverKind =
    crate::config::CloudflareSolverKind::Auto;

/// Default browser-solver selection when browser handling is requested.
pub(crate) const HTTP_DEFAULT_BROWSER_SOLVER: crate::config::CloudflareBrowserSolverKind =
    crate::config::CloudflareBrowserSolverKind::Auto;

/// Default outbound proxy policy, without a proxy override.
pub(crate) const HTTP_DEFAULT_PROXY: crate::config::HttpProxyConfig =
    crate::config::HttpProxyConfig::Disabled;

/// Default proactive cookie refresh margin.
pub(crate) const HTTP_DEFAULT_COOKIE_REFRESH_MARGIN: std::time::Duration =
    std::time::Duration::from_secs(300);

/// Default ordinary HTTP request timeout.
pub(crate) const HTTP_DEFAULT_REQUEST_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);

/// Whether modifying requests explicitly guarantee replay safety by default.
pub(crate) const HTTP_DEFAULT_PROXY_RETRY_REPLAYABLE: bool = false;

/// Default maximum Cloudflare refresh retries.
pub(crate) const HTTP_DEFAULT_MAX_CLOUDFLARE_RETRIES: u8 = 1;

/// Default lifetime of a session cookie without an explicit expiry.
pub(crate) const HTTP_DEFAULT_SESSION_COOKIE_TTL: std::time::Duration =
    std::time::Duration::from_secs(60 * 60);

/// Default user-agent browser profile.
pub(crate) const HTTP_DEFAULT_BROWSER_PROFILE: crate::config::BrowserProfile =
    crate::config::BrowserProfile::ChromeStable;

// --- Reusable browser sessions ---

/// Default maximum number of retained browser page sessions.
pub(crate) const HTTP_BROWSER_MAX_SESSIONS: usize = 8;

/// Default idle eviction threshold for retained browser page sessions.
pub(crate) const HTTP_BROWSER_IDLE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(300);

// --- Proxy transport budgets ---

/// Maximum idle age of an exclusively checked-out proxy transport.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const HTTP_PROXY_TRANSPORT_IDLE_TTL: std::time::Duration =
    std::time::Duration::from_secs(60);

/// Maximum idle transports retained per route.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const HTTP_PROXY_IDLE_PER_ROUTE: usize = 2;

/// Maximum idle transports retained across routes.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const HTTP_PROXY_IDLE_GLOBAL: usize = 32;

/// Connection budget for an HTTP request reusing destination evidence.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const HTTP_CLIENT_REUSED_ROUTE_CONNECT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(60);

/// Connection cap for an HTTP request without reused destination evidence.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const HTTP_CLIENT_NEW_ROUTE_CONNECT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(15);

/// Rquest header cap for correlated proxy requests.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const HTTP_RQUEST_PROXY_HEADER_TIMEOUT_MAX: std::time::Duration =
    std::time::Duration::from_secs(60);

/// Existing reused-route connection-envelope marker interpreted by Rquest.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const HTTP_RQUEST_REUSED_ROUTE_CONNECT_ENVELOPE: std::time::Duration =
    std::time::Duration::from_secs(60);

// --- Native solver defaults ---

/// Default native solver devtools setting.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_DEFAULT_DEVTOOLS: bool = false;

/// Default native solver auto close after clearance setting.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_DEFAULT_AUTO_CLOSE_AFTER_CLEARANCE: bool = true;

/// Default native solver incognito setting.
#[cfg(feature = "tauri-cloudflare-solver")]
pub(crate) const HTTP_TAURI_DEFAULT_INCOGNITO: bool = true;

// --- Chaser observation intervals ---

/// Initial elapsed-click offset allowing the first challenge interaction immediately.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_INITIAL_CLICK_OFFSET: std::time::Duration =
    std::time::Duration::from_secs(30);

/// Initial foreground-clock offset for a newly observed browser page.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_INITIAL_FRONT_OFFSET: std::time::Duration =
    std::time::Duration::from_secs(1);

/// Minimum interval between browser foreground requests.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_FOREGROUND_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(500);

/// Retry delay after an unsuccessful clearance inspection.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_CLEARANCE_ERROR_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(500);

/// Regular polling interval while waiting for clearance.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_CLEARANCE_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(400);

/// Regular polling interval while waiting for an injected challenge.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_INJECTED_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(400);

/// Required challenge absence before returning from injected-captcha observation.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_CHALLENGE_ABSENCE_GRACE: std::time::Duration =
    std::time::Duration::from_millis(1500);

/// Polling interval for the target selector after a retained-page click.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_PAGE_SELECTOR_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(200);

/// Polling interval while waiting for a retained-page captcha token.
#[cfg(feature = "chaser-cf")]
pub(crate) const HTTP_CHASER_TOKEN_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(500);
