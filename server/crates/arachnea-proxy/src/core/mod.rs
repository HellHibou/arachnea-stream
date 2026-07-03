//! Core proxy routing module for Arachnea.
//!
//! This crate owns proxy configuration, destination modeling, route selection,
//! outbound transports and the in-process API. Server listeners and HTTP client
//! adapters live behind sibling crate features.

mod chain;
mod client_context;
mod config;
mod connect;
mod core;
mod destination;
mod dns_integration;
mod error;
mod extension;
pub mod http;
mod observability;
mod parameters;
pub mod policy;
pub mod proxy_inventory;
pub mod proxy_probe;
pub mod proxy_record;
pub mod proxy_store;
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
pub use core::ArachneaProxyCore;
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
pub use observability::{LogSensitivity, ObservabilityConfig};
pub use parameters::{
    build_parameter_handler, context_from_parameter_pairs, normalize_parameter_value,
    CountryRoutingProxyHandler, ParameterDefinition, ParameterHandlerConfig,
    ParameterHandlerDecision, ParameterHandlerKind, ParameterProxyRoute, ParameterRegistry,
    ProxyParameterHandler, SmartDnsProxyHandler, PROXY_HEADER_PARAMETER_COUNTRY,
    PROXY_PARAMETER_COUNTRY,
};
pub use policy::{
    EgressPool, EgressPoolStrategy, EgressSelector, NoopEgressSelector, PrivacyPolicy,
    ProxyPoolMemberState, ProxyPoolMemberStatus, SecurityPolicy,
};
pub use proxy_inventory::{CoexistencePolicy, InventoryConfig, ProxyInventory};
pub use proxy_probe::{ProbeConfig, ProbeMode, ProxyProbe};
pub use proxy_record::{
    ProxyAvailabilityHint, ProxyDataProvider, ProxyDestinationFailure,
    ProxyDestinationFailureReason, ProxyLoadRequest, ProxyProtocol, ProxyRecord,
    ProxyRuntimeStatus,
};
pub use proxy_store::{JsonProxyCodec, ProxySerdeCodec, ProxySerdeStore, ProxyStore};
pub use routing::{RouteDecision, RoutePolicy, RouteRule};
pub use stats::{ProxyStats, ProxyStatsSnapshot};
pub use transport::{socks::Socks5UdpAssociation, OutboundTransport, UdpAssociation};

#[cfg(test)]
#[path = "tests/core_tests.rs"]
mod core_tests;
