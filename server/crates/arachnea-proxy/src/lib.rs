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

/// Default target deadline for [`ProxySearchLimits::target`] (120 seconds).
/// New technical work and business attempts cannot begin after this budget.
#[cfg(feature = "core")]
pub const PROXY_SEARCH_DEFAULT_TARGET: Duration = Duration::from_secs(120);

/// Default grace period for [`ProxySearchLimits::extension`] (15 seconds).
/// Only an already engaged business attempt may use this grace, giving a
/// 135-second hard deadline with [`PROXY_SEARCH_DEFAULT_TARGET`].
#[cfg(feature = "core")]
pub const PROXY_SEARCH_DEFAULT_EXTENSION: Duration = Duration::from_secs(15);

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

/// Exact-destination suspension after a timeout proven during tunnel establishment.
#[cfg(feature = "core")]
pub const PROXY_TUNNEL_TIMEOUT_COOLDOWN: Duration = Duration::from_secs(120);

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

/// Optional JSON domain group used as a default dynamic proxy preference.
#[cfg(feature = "core")]
pub const PROXY_HEADER_PARAMETER_DOMAIN_GROUP: &str = "Arachnea-Proxy-Domain-Group";

/// Context parameter carrying an optional JSON array of related domains.
#[cfg(feature = "core")]
pub const PROXY_PARAMETER_DOMAIN_GROUP: &str = "proxy_domain_group";

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

/// HTTP header carrying a Smart DNS route selector.
#[cfg(feature = "core")]
pub const PROXY_HEADER_PARAMETER_SMART_DNS: &str = "Arachnea-Proxy-Smart-Dns";

/// Context parameter carrying a Smart DNS route selector.
#[cfg(feature = "core")]
pub const PROXY_PARAMETER_SMART_DNS: &str = "smart_dns";

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

/// Default for [`InventoryConfig::probe_ttl`].
///
/// Freshness window for completed proxy probes (600 seconds).
#[cfg(feature = "core")]
pub const PROXY_INVENTORY_DEFAULT_PROBE_TTL: Duration = Duration::from_secs(600);

/// Default for [`InventoryConfig::ko_cooldown`].
///
/// Cooldown before retrying a globally failed proxy (900 seconds).
#[cfg(feature = "core")]
pub const PROXY_INVENTORY_DEFAULT_KO_COOLDOWN: Duration = Duration::from_secs(900);

/// Default for [`InventoryConfig::max_destination_failures_before_ko`].
///
/// Legacy destination-failure threshold retained for configuration compatibility.
/// Destination observations no longer promote a proxy to global failure.
#[cfg(feature = "core")]
pub const PROXY_INVENTORY_DEFAULT_MAX_DESTINATION_FAILURES_BEFORE_KO: usize = 10;

/// Default for [`InventoryConfig::probe_batch_size`].
///
/// Concurrent candidate pipelines per inventory selection pass (eight).
/// Full validations retain their separate per-search concurrency limit.
#[cfg(feature = "core")]
pub const PROXY_INVENTORY_DEFAULT_PROBE_BATCH_SIZE: usize = 8;

/// Default for [`InventoryConfig::reserve_size`].
///
/// Fresh fallback endpoints prepared in addition to an accepted proxy (two).
/// The configurable reserve size is capped at two; zero disables preparation.
#[cfg(feature = "core")]
pub const PROXY_INVENTORY_DEFAULT_RESERVE_SIZE: usize = 2;

/// Default for [`InventoryConfig::reserve_timeout`].
///
/// Independent preparation deadline after business acceptance (15 seconds).
/// This does not extend the business search budget or delay the accepted response.
#[cfg(feature = "core")]
pub const PROXY_INVENTORY_DEFAULT_RESERVE_TIMEOUT: Duration = Duration::from_secs(15);

/// Default for [`InventoryConfig::provider_refresh_cooldown`].
///
/// Minimum interval between ordinary provider refresh attempts per country (120 seconds).
#[cfg(feature = "core")]
pub const PROXY_INVENTORY_DEFAULT_PROVIDER_REFRESH_COOLDOWN: Duration = Duration::from_secs(120);

/// Default for [`InventoryConfig::provider_min_refresh_interval`].
///
/// Refresh interval floor after an observed failure permits early refresh (10 seconds).
#[cfg(feature = "core")]
pub const PROXY_INVENTORY_DEFAULT_PROVIDER_MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

/// Default for [`InventoryConfig::exploration_before_refresh`].
///
/// Cache exploration time before provider discovery overlaps selection (10 seconds).
#[cfg(feature = "core")]
pub const PROXY_INVENTORY_DEFAULT_EXPLORATION_BEFORE_REFRESH: Duration = Duration::from_secs(10);

/// Default for [`InventoryConfig::failures_before_refresh`].
///
/// Unsuccessful validations before provider discovery overlaps cache exploration (eight).
#[cfg(feature = "core")]
pub const PROXY_INVENTORY_DEFAULT_FAILURES_BEFORE_REFRESH: usize = 8;

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

// --- Default values for `Default` implementations --------------------------

/// Default TCP or upstream connect timeout in milliseconds for [`TimeoutConfig::connect_ms`].
#[cfg(feature = "core")]
pub const PROXY_DEFAULT_CONNECT_TIMEOUT_MS: u64 = 30_000;

/// Default proxy handshake timeout in milliseconds for [`TimeoutConfig::handshake_ms`].
#[cfg(feature = "core")]
pub const PROXY_DEFAULT_HANDSHAKE_TIMEOUT_MS: u64 = 30_000;

/// Default idle timeout in milliseconds for adapters that enforce [`TimeoutConfig::idle_ms`].
#[cfg(feature = "core")]
pub const PROXY_DEFAULT_IDLE_TIMEOUT_MS: u64 = 60_000;

/// Default TCP connection timeout for [`ProbeTimeoutConfig::proxy_connect`].
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Default proxy exchange timeout for [`ProbeTimeoutConfig::proxy_handshake`].
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_PROXY_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Default target resolution or tunnel setup timeout for [`ProbeTimeoutConfig::target_connect`].
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_TARGET_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default TLS handshake timeout for [`ProbeTimeoutConfig::tls_handshake`].
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Default overall probe attempt timeout for [`ProbeTimeoutConfig::attempt`].
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default HTTP reachability URL for [`ProbeConfig::http_probe_url`].
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_HTTP_URL: &str = "http://example.com/";

/// Default number of independent endpoint indications before suspending a complementary probe target.
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_SUSPENSION_DISTINCT_ENDPOINTS: usize = 3;

/// Default validity window for complementary-target service-unavailability indications.
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_SUSPENSION_OBSERVATION_WINDOW: Duration = Duration::from_secs(60);

/// Default complementary-target suspension and recovery backoffs; the last delay is the cap.
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_SUSPENSION_BACKOFF: [Duration; 3] = [
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
];

/// Default HTTP statuses indicating complementary-target service unavailability, without excluding a proxy.
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_UNAVAILABLE_STATUSES: &[u16] = &[502, 503, 504];

/// Stable identity of the default complementary-target probe policy.
#[cfg(feature = "core")]
pub const PROXY_PROBE_DEFAULT_POLICY_IDENTITY: &str = "conservative-v1";

/// Default timeout for a single IP-country provider resolution.
#[cfg(feature = "core")]
pub const PROXY_IP_COUNTRY_DEFAULT_RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// Default number of untested egress-pool members probed concurrently.
#[cfg(feature = "core")]
pub const PROXY_EGRESS_POOL_DEFAULT_TEST_BATCH_SIZE: usize = 8;

/// Default route chain name for [`RoutePolicy::default_chain`] ("direct").
#[cfg(feature = "core")]
pub const PROXY_ROUTE_DEFAULT_CHAIN: &str = "direct";

/// Whether direct fallback is allowed by default in [`RoutePolicy`].
#[cfg(feature = "core")]
pub const PROXY_ROUTE_DEFAULT_ALLOW_DIRECT_FALLBACK: bool = true;

/// Default HTTP listener port for [`ServerConfig::listen_http`] (8080).
#[cfg(feature = "server")]
pub const PROXY_SERVER_DEFAULT_HTTP_PORT: u16 = 8080;

/// Default SOCKS5 listener port for [`ServerConfig::listen_socks`] (1080).
#[cfg(feature = "server")]
pub const PROXY_SERVER_DEFAULT_SOCKS_PORT: u16 = 1080;

/// Whether unsafe public binds without ACL are allowed by default in [`ServerConfig`].
#[cfg(feature = "server")]
pub const PROXY_SERVER_DEFAULT_ALLOW_UNSAFE_PUBLIC_BIND: bool = false;

/// Default maximum HTTP header size in bytes for [`ServerConfig::max_http_header_bytes`].
#[cfg(feature = "server")]
pub const PROXY_SERVER_DEFAULT_MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;

/// Default authentication realm for [`ProxyAuthConfig::realm`].
#[cfg(feature = "server")]
pub const PROXY_AUTH_DEFAULT_REALM: &str = "Arachnea Proxy";

/// Default advisory per-client request rate for [`NetworkConfig::rate_limit_per_client_per_second`].
#[cfg(feature = "server")]
pub const PROXY_NETWORK_DEFAULT_RATE_LIMIT: u32 = 50;

/// Default maximum simultaneous connections for [`NetworkConfig::max_connections`].
#[cfg(feature = "server")]
pub const PROXY_NETWORK_DEFAULT_MAX_CONNECTIONS: u32 = 1024;

/// Default port for [`LoopbackProxyRuntime`] (0 means OS-allocated).
#[cfg(feature = "server")]
pub const PROXY_LOOPBACK_DEFAULT_PORT: u16 = 0;
