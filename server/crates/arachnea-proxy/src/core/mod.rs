//! Core proxy routing module for Arachnea.
//!
//! This crate owns proxy configuration, destination modeling, route selection,
//! outbound transports and the in-process API. Server listeners and HTTP client
//! adapters live behind sibling crate features.

/// Overall HTTP CONNECT establishment budget, including route selection,
/// provider loading, capability probes, and upstream connection attempts.
/// Established tunnel relays are not subject to this budget.
///
/// Defined in the crate root; re-exported here for path compatibility.
pub use crate::PROXY_CONNECT_ESTABLISHMENT_TIMEOUT;

mod chain;
mod client_context;
mod config;
mod connect;
mod destination;
mod dns_integration;
mod dns_resolver;
mod error;
mod extension;
pub mod http;
/// Country-level IP geolocation provider.
pub mod ip_country;
mod observability;
mod parameters;
pub mod policy;
mod proxy_core;
mod proxy_fallback;
/// Dynamic proxy inventory with health tracking.
pub mod proxy_inventory;
/// Contextual proxy observations and positive health evidence.
pub mod proxy_observation;
/// Bounded accepted histories for exact destinations.
pub mod proxy_preference;
/// Typed persistence repository for accepted histories.
#[cfg(feature = "persistence")]
pub mod proxy_preference_repository;
/// Proxy liveness probing engine.
pub mod proxy_probe;
mod proxy_probe_target;
pub use proxy_fallback::ProxyFallbackDiagnostics;
pub use proxy_probe_target::{
    ProbeHttpMethod, ProbeTargetDiagnostics, ProbeTargetPolicy, ProbeTargetSuspension,
};
/// Proxy record types used by inventory and stores.
pub mod proxy_record;
/// Typed persistence repository for proxy records.
#[cfg(feature = "persistence")]
pub mod proxy_repository;
/// Shared request-owned proxy search budgets and replay policy.
pub mod proxy_search;
/// Explicit request selection and pinned route correlation.
pub mod proxy_selection;
pub use proxy_search::{proxy_retry_allowed, ProxySearchContext, ProxySearchLimits};
mod routing;
mod stats;
pub mod transport;

pub use chain::{
    ChainStrategy, ProxyChain, ProxyNameResolutionMode, ProxyNode, ProxyNodeCredentials,
    TransportKind,
};
pub use client_context::{ClientContext, ClientParameter};
pub use config::{
    NameResolutionMode, ProxyConfig, ProxyConfigBuilder, ProxyProfile, ResolvedProxyConfig,
    TimeoutConfig, UsageProfile,
};
pub use connect::{ConnectMetadata, ConnectRequest, ProxyStream};
pub use destination::{ApplicationProtocol, Destination, DestinationAddress};
pub use dns_integration::{
    SmartDnsRouteHint, PROXY_PARAMETER_SMART_DNS_CHAIN, PROXY_PARAMETER_SMART_DNS_EGRESS_POOL,
};
pub use error::{ProxyError, Result};
pub use extension::{
    ClientParameterExtractor, ClientParameterValidator, ExtensionProvider, RejectAllParameters,
    RoutePlugin,
};
pub use http::{HttpRequestStream, HttpRequestTargetForm};
pub use ip_country::{
    IpCountryDataProvider, IpCountryRecord, IpCountryResolver, IpCountryResolverConfig,
    IpCountrySerdeCodec, IpCountrySerdeStore, IpCountryStore, JsonIpCountryCodec,
};
pub use observability::{LogSensitivity, ObservabilityConfig};
pub use parameters::{
    build_parameter_handler, canonical_proxy_countries, context_from_parameter_pairs,
    normalize_parameter_value, CountryRoutingProxyHandler, DynamicCountryRoutingProxyHandler,
    ParameterDefinition, ParameterHandlerConfig, ParameterHandlerDecision, ParameterHandlerKind,
    ParameterProxyRoute, ParameterRegistry, ProxyParameterHandler, SmartDnsProxyHandler,
    PROXY_HEADER_PARAMETER_AFFINITY, PROXY_HEADER_PARAMETER_COUNTRIES,
    PROXY_HEADER_PARAMETER_FALLBACK_DIRECT, PROXY_PARAMETER_AFFINITY, PROXY_PARAMETER_COUNTRIES,
    PROXY_PARAMETER_FALLBACK_DIRECT,
};
pub use policy::{
    EgressPool, EgressPoolStrategy, EgressSelector, NoopEgressSelector, PrivacyPolicy,
    ProxyPoolMemberState, ProxyPoolMemberStatus, SecurityPolicy,
};
pub use proxy_core::ArachneaProxyCore;
pub use proxy_inventory::{
    CoexistencePolicy, InventoryConfig, ProxyInventory, PROXY_DESTINATION_FAILURE_COOLDOWN,
    PROXY_TRANSIENT_FAILURE_COOLDOWN,
};
pub use proxy_observation::{
    ProxyFailureScope, ProxyHealthEvidence, ProxyObservation, ProxyObservationOutcome,
    ProxyObservationPhase, ProxyProbeContext, ProxyRequestDelivery,
};
pub use proxy_preference::{
    ProxyAcceptedEndpoint, ProxyDestinationPreference, PROXY_PREFERENCE_HISTORY_LIMIT,
};
#[cfg(feature = "persistence")]
pub use proxy_preference_repository::{
    ProxyPreferenceRepository, TypedProxyPreferenceRepository, PROXY_PREFERENCE_STORE_NAME,
};
pub use proxy_probe::{ProbeConfig, ProbeMode, ProbeTimeoutConfig, ProxyProbe};
pub use proxy_record::{
    ProxyAvailabilityHint, ProxyBatchSink, ProxyCapabilityStatus, ProxyDataProvider,
    ProxyDeclaration, ProxyDestinationFailure, ProxyDestinationFailureReason, ProxyKey,
    ProxyLoadRequest, ProxyProtocol, ProxyRecord, ProxyRuntimeStatus, PROXY_CACHE_TTL,
    PROXY_PROTOCOL_PRIORITY,
};
#[cfg(feature = "persistence")]
pub use proxy_repository::{ProxyRepository, TypedProxyRepository, PROXY_STORE_NAME};
pub use proxy_selection::{ProxySelectionProvenance, ProxySelectionToken, SelectedProxy};
pub use routing::{RouteDecision, RoutePolicy, RouteRule};
pub use stats::{ProxyStats, ProxyStatsSnapshot};
pub use transport::{socks::Socks5UdpAssociation, OutboundTransport, UdpAssociation};

#[cfg(test)]
#[path = "../../tests/core/mod.rs"]
mod core_tests;
