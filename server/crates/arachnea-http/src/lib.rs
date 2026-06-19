//! Outbound HTTP client crate for Arachnea.
//!
//! The crate exposes one client facade that can use `rquest` and optional
//! Cloudflare-capable engines such as Ghostwire or chaser-cf. It is intended
//! for authorized traffic only, such as tests, monitoring, and integrations
//! where the caller has permission to access the target service.

pub mod client;
pub mod cloudflare;
pub mod config;
pub mod cookies;
pub mod engine;
pub mod error;

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
