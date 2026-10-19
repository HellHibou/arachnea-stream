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

#[path = "core/chain.rs"]
mod chain;
#[path = "core/config.rs"]
mod config;
#[path = "core/connections.rs"]
mod connections;
#[path = "core/pools.rs"]
mod pools;
#[path = "core/profiles.rs"]
mod profiles;
#[path = "core/request_routing.rs"]
mod request_routing;
#[cfg(feature = "arachnea-dns")]
#[path = "core/resolution.rs"]
mod resolution;
#[path = "core/support.rs"]
mod support;
