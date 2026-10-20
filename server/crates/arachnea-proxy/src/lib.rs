#![warn(missing_docs)]
//! Unified Arachnea proxy crate.
//!
//! The `core`, `server`, and `connectors` features expose the former proxy
//! crates as module groups within one package.
//!
//! ## Features
//!
//! - **core**: In-process proxy routing core with multi-hop chains, proxy pools,
//!   route selection, and outbound transports (HTTP, HTTPS, SOCKS4/4a, SOCKS5, Tor).
//! - **server**: Proxy server listeners (HTTP, HTTPS, SOCKS4/4a, SOCKS5) with
//!   configuration, ACLs, authentication, and safety checks.
//! - **connectors**: Client-side adapters for `tower`, `hyper`, and `rquest` ecosystems.
//!
//! ## Usage
//!
//! Add to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! arachnea-proxy = { version = "0.1", features = ["core", "server"] }
//! ```
//!
//! Then use the core for in-process routing:
//!
//! ```no_run
//! use arachnea_proxy::core::{ArachneaProxyCore, Destination, ApplicationProtocol};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), arachnea_proxy::ProxyError> {
//!     let core = ArachneaProxyCore::new(Default::default())?;
//!     let _stream = core.connect(Destination::from_authority(
//!         "example.com:443",
//!         ApplicationProtocol::Https,
//!     )?).await?;
//!     Ok(())
//! }
//! ```

#[cfg(feature = "connectors")]
pub mod connectors;
#[cfg(feature = "core")]
pub mod core;
#[cfg(feature = "server")]
pub mod server;

#[cfg(all(feature = "connectors", feature = "rquest"))]
pub use connectors::ArachneaRquestLoopback;
#[cfg(feature = "connectors")]
pub use connectors::{
    ArachneaHyperConnector, ArachneaProxyLayer, ArachneaTowerService, ConnectorError,
    ConnectorStream,
};
#[cfg(feature = "core")]
pub use core::*;
#[cfg(feature = "server")]
pub use server::{
    validate_http_basic_header, ClientAcl, FileConfig, NetworkConfig, ProxyAuthConfig,
    ProxyCredentials, ProxyServer, ServerConfig,
};

// ---------------------------------------------------------------------------
// Shared constants
//
// Crate-wide tuning values, wire-contract identifiers and store names live
// here so they are discoverable in one place. Feature-gated constants are
// only compiled with the feature owning the related subsystem. Modules that
// historically owned a constant re-export it so existing import paths keep
// working.
// ---------------------------------------------------------------------------

#[cfg(feature = "core")]
use std::time::Duration;

// --- Connection budgets and timeouts ----------------------------------------

/// Overall HTTP CONNECT establishment budget, including route selection,
/// provider loading, capability probes, and upstream connection attempts.
/// Established tunnel relays are not subject to this budget.
#[cfg(feature = "core")]
pub const PROXY_CONNECT_ESTABLISHMENT_TIMEOUT: Duration = Duration::from_secs(2 * 60);

/// Maximum age of a proxy validation before a subsequent failure removes it.
///
/// Stale proxies remain selectable: this is a conditional eviction threshold,
/// not a hard expiration deadline.
#[cfg(feature = "core")]
pub const PROXY_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Default duration for excluding a proxy from one exact destination after a
/// destination-scoped failure such as an HTTP 403 origin rejection.
#[cfg(feature = "core")]
pub const PROXY_DESTINATION_FAILURE_COOLDOWN: Duration = Duration::from_secs(24 * 60 * 60);

/// Maximum suspension after an ambiguous transport failure for one destination.
#[cfg(feature = "core")]
pub const PROXY_TRANSIENT_FAILURE_COOLDOWN: Duration = Duration::from_secs(30);

/// Lifetime of one recorded affinity binding between a request key and an
/// eligible dynamic proxy in the inventory preference cache.
#[cfg(feature = "core")]
pub(crate) const PROXY_AFFINITY_TTL: Duration = Duration::from_secs(15 * 60);

/// Default maximum wait for optional prefix inspection, not a transfer timeout.
#[cfg(feature = "core")]
pub const PROXY_RESPONSE_INSPECTION_TIMEOUT: Duration = Duration::from_secs(3);

/// Maximum prefix exposed to response classifiers.
#[cfg(feature = "core")]
pub const MAX_PROXY_RESPONSE_PREFIX: usize = 64 * 1024;

// --- Request parameters and headers (public wire contract) ------------------

/// Proxy countries parameter header name. Its value is a JSON array of ISO
/// alpha-2 country codes so comma-like values cannot be ambiguous.
#[cfg(feature = "core")]
pub const PROXY_HEADER_PARAMETER_COUNTRIES: &str = "Arachnea-Proxy-Countries";

/// Proxy affinity parameter header name. Its opaque value pins related
/// requests to the same eligible dynamic proxy.
#[cfg(feature = "core")]
pub const PROXY_HEADER_PARAMETER_AFFINITY: &str = "Arachnea-Proxy-Affinity";

/// Proxy countries parameter name.
#[cfg(feature = "core")]
pub const PROXY_PARAMETER_COUNTRIES: &str = "countries";

/// Request override for direct fallback when no country proxy is available.
#[cfg(feature = "core")]
pub const PROXY_PARAMETER_FALLBACK_DIRECT: &str = "fallback_direct";

/// HTTP header carrying the boolean direct-fallback override.
#[cfg(feature = "core")]
pub const PROXY_HEADER_PARAMETER_FALLBACK_DIRECT: &str = "Arachnea-Proxy-Fallback-Direct";

/// Proxy affinity parameter name.
#[cfg(feature = "core")]
pub const PROXY_PARAMETER_AFFINITY: &str = "proxy_affinity";

/// Parameter naming a smart-DNS chain to install on the selected route.
#[cfg(feature = "core")]
pub const PROXY_PARAMETER_SMART_DNS_CHAIN: &str = "smart_dns_chain";

/// Parameter naming the egress pool used by smart-DNS resolution.
#[cfg(feature = "core")]
pub const PROXY_PARAMETER_SMART_DNS_EGRESS_POOL: &str = "smart_dns_egress_pool";

// --- Proxy HTTP service limits and actions ----------------------------------

/// Maximum byte length accepted for a fully decoded local proxy URL.
#[cfg(feature = "controller-service")]
pub(crate) const MAX_LOCAL_URL_BYTES: usize = 8192;

/// Known option fields allowed in the `opts` JSON.
#[cfg(feature = "controller-service")]
pub(crate) const ALLOWED_OPTS_FIELDS: &[&str] = &[
    "headers",
    "cookies",
    "proxy",
    "insecure_tls",
    "fallback_direct",
    "follow_redirects",
    "proxy_rejection_statuses",
    "proxy_response_classifier",
    "proxy_retry_replayable",
];

/// Proxy action header that carries a JSON ReplaceAll rule.
#[cfg(feature = "core")]
pub const REPLACE_ALL_ACTION_HEADER: &str = "Arachnea-Proxy-ReplaceAll";

/// Lowercase form of [`REPLACE_ALL_ACTION_HEADER`] used for case-insensitive
/// header lookups.
#[cfg(feature = "controller-service")]
pub(crate) const REPLACE_ALL_ACTION_HEADER_LOWER: &str = "arachnea-proxy-replaceall";

/// Proxy action header that carries a JSON RemoveHeader rule.
#[cfg(feature = "core")]
pub const REMOVE_HEADER_ACTION_HEADER: &str = "Arachnea-Proxy-RemoveHeader";

/// Lowercase form of [`REMOVE_HEADER_ACTION_HEADER`] used for case-insensitive
/// header lookups.
#[cfg(feature = "controller-service")]
pub(crate) const REMOVE_HEADER_ACTION_HEADER_LOWER: &str = "arachnea-proxy-removeheader";

/// Redirect trigger name matching an HTTP 302 response for RemoveHeader rules.
#[cfg(feature = "core")]
pub(crate) const REMOVE_HEADER_TRIGGER_ON_HTTP_302: &str = "onHttp302";

// --- Proxy pools and dynamic pool routing -----------------------------------

/// Compatibility checks performed per pool member before it is selected.
#[cfg(feature = "core")]
pub(crate) const PROXY_POOL_MEMBER_CHECK_ATTEMPTS: usize = 2;

/// Selection passes over one pool, allowing a KO-state reset between passes.
#[cfg(feature = "core")]
pub(crate) const PROXY_POOL_SELECTION_PASSES: usize = 2;

/// Name prefix marking dynamically generated country proxy pools.
#[cfg(feature = "core")]
pub(crate) const DYNAMIC_COUNTRY_POOL_PREFIX: &str = "dynamic-countries:";

// --- Inventory, preferences and persistence ---------------------------------

/// Number of accepted exact-destination preferences kept per key.
#[cfg(feature = "core")]
pub const PROXY_PREFERENCE_HISTORY_LIMIT: usize = 3;

/// SQLite store name for the shared proxy inventory tables.
#[cfg(feature = "persistence")]
pub const PROXY_STORE_NAME: &str = "proxy-inventory";

/// SQLite store name for the proxy failure tables.
#[cfg(feature = "persistence")]
pub const PROXY_FAILURE_STORE_NAME: &str = "proxy-failures";

/// SQLite store name for accepted-destination preference histories.
#[cfg(feature = "persistence")]
pub const PROXY_PREFERENCE_STORE_NAME: &str = "proxy-preferences";

// --- Protocol selection ------------------------------------------------------

/// Preferred protocol order for source selection and runtime detection.
///
/// HTTP is intentionally last because it provides the weakest transport
/// capabilities among the supported proxy protocols.
#[cfg(feature = "core")]
pub const PROXY_PROTOCOL_PRIORITY: [crate::core::proxy_record::ProxyProtocol; 5] = [
    crate::core::proxy_record::ProxyProtocol::Socks5,
    crate::core::proxy_record::ProxyProtocol::Socks4,
    crate::core::proxy_record::ProxyProtocol::Https,
    crate::core::proxy_record::ProxyProtocol::Socks4a,
    crate::core::proxy_record::ProxyProtocol::Http,
];
