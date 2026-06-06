use crate::core::{
    protocol::record_from_hickory, Answer, AnswerMetadata, CacheState, DnsError, DnssecState,
    QueryRequest, Result, Transport, Upstream, UpstreamEndpoint,
};
use hickory_resolver::proto::{rr::Name, xfer::Protocol};
use hickory_resolver::{
    config::{NameServerConfig, NameServerConfigGroup, ResolveHosts, ResolverConfig},
    name_server::TokioConnectionProvider,
    ResolveError, TokioResolver,
};
#[cfg(feature = "doq")]
use std::net::IpAddr;
#[cfg(feature = "dot")]
use std::net::SocketAddr;
#[cfg(any(feature = "dot", feature = "doq"))]
use std::net::ToSocketAddrs;
use std::time::Duration;

/// Resolves through Hickory Resolver, optionally enabling DNSSEC validation.
pub(crate) async fn resolve_with_hickory(
    query: &QueryRequest,
    upstream: &Upstream,
    validate_dnssec: bool,
    default_timeout_ms: u64,
) -> Result<Answer> {
    let resolver_config =
        ResolverConfig::from_parts(None, Vec::new(), name_server_config_group(upstream)?);
    let mut builder =
        TokioResolver::builder_with_config(resolver_config, TokioConnectionProvider::default());
    let opts = builder.options_mut();
    opts.timeout = Duration::from_millis(upstream.timeout_ms.unwrap_or(default_timeout_ms));
    opts.attempts = 1;
    opts.validate = validate_dnssec;
    opts.preserve_intermediates = true;
    opts.use_hosts_file = ResolveHosts::Never;
    opts.try_tcp_on_error = true;
    opts.negative_min_ttl = Some(Duration::from_secs(1));
    opts.negative_max_ttl = Some(Duration::from_secs(300));

    let resolver = builder.build();
    let name = Name::from_ascii(format!("{}.", query.name))
        .map_err(|error| DnsError::Protocol(error.to_string()))?;
    let lookup = resolver
        .lookup(name, query.record_type.to_hickory())
        .await
        .map_err(|error| map_hickory_error(error, query))?;

    let mut records = Vec::new();
    let mut cname_chain = Vec::new();
    for record in lookup.record_iter() {
        if let Some(record) = record_from_hickory(record)? {
            if let crate::core::RecordData::CNAME { name } = &record.data {
                cname_chain.push(name.clone());
            }
            if record.data.record_type() == query.record_type
                || record.data.record_type() == crate::core::RecordType::CNAME
            {
                records.push(record);
            }
        }
    }

    if records.is_empty() {
        return Err(DnsError::NoData {
            name: query.name.clone(),
            record_type: query.record_type,
        });
    }

    Ok(Answer {
        query: query.clone(),
        metadata: AnswerMetadata {
            ttl: records.iter().map(|record| record.ttl).min(),
            cname_chain,
            upstream: Some(upstream.name.clone()),
            cache: CacheState::Miss,
            dnssec: if validate_dnssec {
                DnssecState::Strict
            } else {
                DnssecState::Off
            },
            policy: None,
        },
        records,
    })
}

/// Builds Hickory name server configuration for an upstream.
///
/// # Parameters
///
/// - `upstream`: DNS upstream declaration.
///
/// # Returns
///
/// Hickory resolver name server configuration group.
///
/// # Errors
///
/// Returns an error when the upstream endpoint cannot be represented by the
/// selected Hickory transport.
fn name_server_config_group(upstream: &Upstream) -> Result<NameServerConfigGroup> {
    match (&upstream.transport, &upstream.endpoint) {
        (Transport::Udp, UpstreamEndpoint::Socket(addr)) => Ok(single_name_server(
            NameServerConfig::new(*addr, Protocol::Udp),
        )),
        (Transport::Tcp, UpstreamEndpoint::Socket(addr)) => Ok(single_name_server(
            NameServerConfig::new(*addr, Protocol::Tcp),
        )),
        #[cfg(feature = "dot")]
        (Transport::Dot, UpstreamEndpoint::HostPort { host, port }) => {
            let addr = resolve_host_port(host, *port)?;
            let mut config = NameServerConfig::new(addr, Protocol::Tls);
            config.tls_dns_name = Some(host.clone());
            Ok(single_name_server(config))
        }
        #[cfg(feature = "doq")]
        (Transport::Doq, UpstreamEndpoint::HostPort { host, port }) => {
            let ips = resolve_host_ips(host)?;
            Ok(NameServerConfigGroup::from_ips_quic(
                &ips,
                *port,
                host.clone(),
                true,
            ))
        }
        (transport, _) => Err(DnsError::UnsupportedFeature(format!(
            "Hickory resolver path does not support upstream transport {transport:?}"
        ))),
    }
}

/// Wraps one name server configuration in a Hickory group.
///
/// # Parameters
///
/// - `config`: Name server configuration to wrap.
///
/// # Returns
///
/// Name server configuration group containing `config`.
fn single_name_server(config: NameServerConfig) -> NameServerConfigGroup {
    NameServerConfigGroup::from(vec![config])
}

/// Resolves a host and port into one socket address.
///
/// # Parameters
///
/// - `host`: Hostname or IP address to resolve.
/// - `port`: Port to attach to the resolved address.
///
/// # Returns
///
/// First socket address produced by the system resolver.
///
/// # Errors
///
/// Returns an error when no address can be resolved.
#[cfg(feature = "dot")]
fn resolve_host_port(host: &str, port: u16) -> Result<SocketAddr> {
    (host, port)
        .to_socket_addrs()
        .map_err(|error| DnsError::UpstreamUnavailable(error.to_string()))?
        .next()
        .ok_or_else(|| DnsError::UpstreamUnavailable(format!("cannot resolve {host}:{port}")))
}

/// Resolves a host into all socket IPs Hickory can try for DoQ.
///
/// # Parameters
///
/// - `host`: Hostname or IP address to resolve.
///
/// # Returns
///
/// IP addresses returned by the system resolver.
///
/// # Errors
///
/// Returns an error when no address can be resolved.
#[cfg(feature = "doq")]
fn resolve_host_ips(host: &str) -> Result<Vec<IpAddr>> {
    let ips = (host, 0)
        .to_socket_addrs()
        .map_err(|error| DnsError::UpstreamUnavailable(error.to_string()))?
        .map(|addr| addr.ip())
        .collect::<Vec<_>>();
    if ips.is_empty() {
        return Err(DnsError::UpstreamUnavailable(format!(
            "cannot resolve DoQ host {host}"
        )));
    }
    Ok(ips)
}

/// Maps a Hickory resolver error into the crate error type.
///
/// # Parameters
///
/// - `error`: Hickory resolver error.
/// - `query`: Query associated with the error.
///
/// # Returns
///
/// DNS error used by this crate.
fn map_hickory_error(error: ResolveError, query: &QueryRequest) -> DnsError {
    let message = error.to_string();
    if message.contains("no record found") || message.contains("No records found") {
        DnsError::NoData {
            name: query.name.clone(),
            record_type: query.record_type,
        }
    } else if message.contains("NXDomain") || message.contains("NXDOMAIN") {
        DnsError::Nxdomain(query.name.clone())
    } else if message.contains("DNSSEC") || message.contains("validation") {
        DnsError::DnssecValidationFailed(message)
    } else {
        DnsError::UpstreamUnavailable(message)
    }
}
