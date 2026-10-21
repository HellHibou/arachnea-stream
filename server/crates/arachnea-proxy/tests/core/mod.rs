use crate::core::{
    ApplicationProtocol, ArachneaProxyCore, ClientContext, ClientParameter, ConnectRequest,
    CountryRoutingProxyHandler, Destination, EgressPool, HttpRequestTargetForm,
    ParameterHandlerConfig, ParameterHandlerKind, ParameterProxyRoute, ProxyChain, ProxyConfig,
    ProxyError, ProxyNameResolutionMode, ProxyNode, ProxyParameterHandler, ProxyPoolMemberStatus,
    ProxyProfile, RoutePolicy, RouteRule, SecurityPolicy, SmartDnsRouteHint, TimeoutConfig,
    TransportKind, UsageProfile, PROXY_PARAMETER_COUNTRIES,
};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use support::*;

#[path = "chain.rs"]
mod chain;
#[path = "config/builder.rs"]
mod config_builder;
#[path = "config/proxy_config.rs"]
mod config_proxy_config;
#[path = "config/proxy_profile.rs"]
mod config_proxy_profile;
#[path = "config/usage_profile.rs"]
mod config_usage_profile;
#[path = "config/validation.rs"]
mod config_validation;
#[path = "destination.rs"]
mod destination;
#[path = "parameters.rs"]
mod parameters;
#[path = "proxy_core/connections.rs"]
mod proxy_core_connections;
#[path = "proxy_core/mod.rs"]
mod proxy_core_mod;
#[path = "proxy_core/pools.rs"]
mod proxy_core_pools;
#[path = "proxy_core/request_routing.rs"]
mod proxy_core_request_routing;
#[cfg(feature = "arachnea-dns")]
#[path = "proxy_core/resolution.rs"]
mod proxy_core_resolution;
#[path = "../support/core.rs"]
mod support;
