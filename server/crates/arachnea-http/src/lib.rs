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
    ResolvedIframeCache, TurnstileTokenState,
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
