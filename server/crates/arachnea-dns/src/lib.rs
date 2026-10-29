#![warn(missing_docs)]
//! Unified Arachnea DNS crate.
//!
//! The `core` and `server` features expose the former DNS crates as module
//! groups within one package.

#[cfg(feature = "core")]
pub mod core;
#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "core")]
pub use core::*;
#[cfg(feature = "server")]
pub use server::{
    handle_packet, run_tcp, run_udp, FileBlocklist, FileConfig, FileForwarder, FileLocalRecord,
    FileProfile, NetworkConfig, Server, ServerConfig,
};

// Crate-owned defaults and operational limits, grouped by responsibility.

// --- Resolver defaults ---

/// Default resolver mode, delegating to the system resolver.
#[cfg(feature = "core")]
pub(crate) const DNS_RESOLVER_DEFAULT_MODE: crate::core::ResolutionMode =
    crate::core::ResolutionMode::Stub;

/// Default per-operation resolver timeout in milliseconds.
#[cfg(feature = "core")]
pub(crate) const DNS_RESOLVER_DEFAULT_TIMEOUT_MS: u64 = 2_000;

/// Default number of resolver retry attempts.
#[cfg(feature = "core")]
pub(crate) const DNS_RESOLVER_DEFAULT_RETRIES: u8 = 2;

/// Whether the default resolver may query upstreams in parallel.
#[cfg(feature = "core")]
pub(crate) const DNS_RESOLVER_DEFAULT_PARALLEL_UPSTREAMS: bool = false;

/// Whether default resolution allows fallback to insecure transports.
#[cfg(feature = "core")]
pub(crate) const DNS_RESOLVER_DEFAULT_ALLOW_INSECURE_FALLBACK: bool = false;

/// Default order for selecting configured upstreams.
#[cfg(feature = "core")]
pub(crate) const DNS_RESOLVER_DEFAULT_UPSTREAM_STRATEGY: crate::core::UpstreamStrategy =
    crate::core::UpstreamStrategy::FixedOrder;

/// Default handling of the first upstream NXDOMAIN response.
#[cfg(feature = "core")]
pub(crate) const DNS_RESOLVER_DEFAULT_NXDOMAIN_STRATEGY: crate::core::NxdomainStrategy =
    crate::core::NxdomainStrategy::TrustFirst;

// --- Per-core cache defaults ---

/// Whether the per-core memory cache is enabled by default.
#[cfg(feature = "core")]
pub(crate) const DNS_CACHE_DEFAULT_ENABLED: bool = false;

/// Default maximum number of entries retained by a core cache.
#[cfg(feature = "core")]
pub(crate) const DNS_CACHE_DEFAULT_MAX_ENTRIES: usize = 100_000;

/// Default lower bound for cached TTLs in seconds.
#[cfg(feature = "core")]
pub(crate) const DNS_CACHE_DEFAULT_MIN_TTL_SECONDS: u32 = 0;

/// Default upper bound for cached TTLs in seconds.
#[cfg(feature = "core")]
pub(crate) const DNS_CACHE_DEFAULT_MAX_TTL_SECONDS: u32 = 86_400;

/// Whether negative answers may be cached when the cache is enabled.
#[cfg(feature = "core")]
pub(crate) const DNS_CACHE_DEFAULT_NEGATIVE_CACHE: bool = true;

/// Whether aggressive DNSSEC negative synthesis is enabled by default.
#[cfg(feature = "core")]
pub(crate) const DNS_CACHE_DEFAULT_AGGRESSIVE_NEGATIVE_CACHE: bool = false;

/// Whether stale cached answers may be served by default.
#[cfg(feature = "core")]
pub(crate) const DNS_CACHE_DEFAULT_SERVE_STALE: bool = false;

/// Default stale retention window in seconds, independent of the fresh TTL cap.
#[cfg(feature = "core")]
pub(crate) const DNS_CACHE_DEFAULT_MAX_STALE_SECONDS: u32 = 86_400;

/// Default TTL in seconds assigned to answers served stale.
#[cfg(feature = "core")]
pub(crate) const DNS_CACHE_DEFAULT_STALE_RESPONSE_TTL_SECONDS: u32 = 30;

// --- Privacy and security defaults ---

/// Whether the default policy minimizes query names.
#[cfg(feature = "core")]
pub(crate) const DNS_PRIVACY_DEFAULT_QNAME_MINIMISATION: bool = false;

/// Default EDNS client-subnet policy, without client subnet forwarding.
#[cfg(feature = "core")]
pub(crate) const DNS_PRIVACY_DEFAULT_ECS: crate::core::EcsPolicy = crate::core::EcsPolicy::Disabled;

/// Whether the default privacy policy requests Oblivious DoH.
#[cfg(feature = "core")]
pub(crate) const DNS_PRIVACY_DEFAULT_ODOH: bool = false;

/// Whether the default privacy policy requests DNS query padding.
#[cfg(feature = "core")]
pub(crate) const DNS_PRIVACY_DEFAULT_QUERY_PADDING: bool = false;

/// Whether query names may be logged by the default privacy policy.
#[cfg(feature = "core")]
pub(crate) const DNS_PRIVACY_DEFAULT_LOG_QNAMES: bool = false;

/// Default DNSSEC policy; explicit secure profiles may override it.
#[cfg(feature = "core")]
pub(crate) const DNS_SECURITY_DEFAULT_DNSSEC: crate::core::DnssecState =
    crate::core::DnssecState::Off;

/// Whether the default security policy enables DNS cookies.
#[cfg(feature = "core")]
pub(crate) const DNS_SECURITY_DEFAULT_COOKIES: bool = false;

/// Default optional server rate limit, in requests per client per second.
#[cfg(feature = "core")]
pub(crate) const DNS_SECURITY_DEFAULT_RATE_LIMIT: Option<u32> = Some(50);

// --- Upstream defaults ---

/// Whether upstream constructors and file forwarders validate TLS by default.
#[cfg(feature = "core")]
pub(crate) const DNS_UPSTREAM_DEFAULT_VERIFY_TLS: bool = true;

/// Stable name assigned by the system-relay upstream constructor.
#[cfg(feature = "core")]
pub(crate) const DNS_SYSTEM_UPSTREAM_NAME: &str = "system";

// --- Recursive root hints ---

/// Embedded root-server address list used when no custom hints are configured.
#[cfg(feature = "core")]
pub(crate) const DNS_EMBEDDED_ROOT_HINTS: &str = include_str!("../resources/root_hints.txt");

// --- DNS server and file defaults ---

/// Default enabled flag used by server configuration and file blocklist entries.
#[cfg(feature = "server")]
pub(crate) const DNS_FILE_DEFAULT_ENABLED: bool = true;

/// Default loopback address used by both DNS server listeners.
#[cfg(feature = "server")]
pub(crate) const DNS_SERVER_DEFAULT_LISTEN_ADDR: std::net::SocketAddr =
    std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 5353);

/// Default profile selected when a TOML file omits its profile.
#[cfg(feature = "server")]
pub(crate) const DNS_FILE_DEFAULT_PROFILE: crate::server::FileProfile =
    crate::server::FileProfile::SystemRelay;

/// DNSSEC preset for secure file profiles without an explicit security override; distinct from core defaults.
#[cfg(feature = "server")]
pub(crate) const DNS_FILE_DEFAULT_DNSSEC: crate::core::DnssecState =
    crate::core::DnssecState::ReportOnly;

/// Name assigned to a compact file forwarder without an explicit name.
#[cfg(feature = "server")]
pub(crate) const DNS_FILE_FORWARDER_DEFAULT_NAME: &str = "single-forwarder";

/// Existing default port assigned to a compact DoT forwarder without a port.
#[cfg(feature = "server")]
pub(crate) const DNS_FILE_DOT_DEFAULT_PORT: u16 = 853;

/// Existing fallback port for compact DoQ and ODoH forwarders; preserved independently of DoT.
#[cfg(feature = "server")]
pub(crate) const DNS_FILE_OTHER_ENCRYPTED_DEFAULT_PORT: u16 = 443;

/// Default action for file blocklist entries.
#[cfg(feature = "server")]
pub(crate) const DNS_FILE_DEFAULT_BLOCK_ACTION: crate::core::BlockAction =
    crate::core::BlockAction::Nxdomain;

// --- Hickory lookup limits ---

/// Attempts performed by one Hickory lookup; outer resolver retries stay independent.
#[cfg(all(feature = "core", any(feature = "dnssec", feature = "doq")))]
pub(crate) const DNS_HICKORY_ATTEMPTS: usize = 1;

/// Minimum negative TTL used by the Hickory upstream resolver.
#[cfg(all(feature = "core", any(feature = "dnssec", feature = "doq")))]
pub(crate) const DNS_HICKORY_NEGATIVE_MIN_TTL: std::time::Duration =
    std::time::Duration::from_secs(1);

/// Maximum negative TTL used by the Hickory upstream resolver.
#[cfg(all(feature = "core", any(feature = "dnssec", feature = "doq")))]
pub(crate) const DNS_HICKORY_NEGATIVE_MAX_TTL: std::time::Duration =
    std::time::Duration::from_secs(300);

// --- Aggressive negative cache limits ---

/// Minimum retention of synthesized negative evidence in seconds.
#[cfg(feature = "core")]
pub(crate) const DNS_AGGRESSIVE_NEGATIVE_MIN_TTL_SECONDS: u32 = 1;

/// Maximum retention of synthesized negative evidence in seconds; independent of upstream caches.
#[cfg(feature = "core")]
pub(crate) const DNS_AGGRESSIVE_NEGATIVE_MAX_TTL_SECONDS: u32 = 300;

// --- DNS runtime limits ---

/// Maximum admitted recursive lookup depth; the existing comparison remains strictly greater than this bound.
#[cfg(all(feature = "core", feature = "recursive"))]
pub(crate) const DNS_RECURSIVE_MAX_DEPTH: u8 = 32;

/// Receive buffer size in bytes for iterative recursive UDP queries.
#[cfg(all(feature = "core", feature = "recursive"))]
pub(crate) const DNS_RECURSIVE_UDP_BUFFER_BYTES: usize = 4096;

/// Port used by the existing recursive DNS upstream exchanges.
#[cfg(all(feature = "core", feature = "recursive"))]
pub(crate) const DNS_RECURSIVE_UPSTREAM_PORT: u16 = 53;

/// Receive buffer size in bytes for configured UDP upstream queries.
#[cfg(feature = "core")]
pub(crate) const DNS_FORWARDER_UDP_BUFFER_BYTES: usize = 4096;

/// Stable tracing target for DNS listener events, preserving existing log filters.
#[cfg(feature = "server")]
pub(crate) const DNS_SERVER_LOG_TARGET: &str = "arachnea_dns::server::server";

/// Receive buffer size in bytes for incoming DNS UDP requests.
#[cfg(feature = "server")]
pub(crate) const DNS_SERVER_UDP_BUFFER_BYTES: usize = 4096;

/// Accounting window used by the per-client server request limit.
#[cfg(feature = "server")]
pub(crate) const DNS_SERVER_RATE_LIMIT_WINDOW: std::time::Duration =
    std::time::Duration::from_secs(1);
