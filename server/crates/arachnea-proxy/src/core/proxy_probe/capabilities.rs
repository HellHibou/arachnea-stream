//! Endpoint connection and protocol capability attempts.

use crate::core::{ProxyCapabilityStatus, ProxyError, ProxyProtocol, Result};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time;
use tokio::time::Instant;

use super::http::{
    parse_http_probe_url, parse_https_probe_url, probe_origin_target, send_http_connect,
};
use super::results::{
    capability_status, failed_capability_status, is_auth_required, CapabilityProbeAttempt,
    CapabilityProbeTarget,
};
use super::socks::{
    connect_socks5_with_local_dns_fallback, is_socks5_auth_required, prepare_socks4_destination,
    send_socks4_connect,
};
use super::tls::wrap_tls;
use super::ProxyProbe;

/// Trait alias for a boxed async read/write stream used during probing.
pub(super) trait AsyncProbeStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncProbeStream for T {}

/// Opens a TCP connection to the proxy and measures its latency.
pub(super) async fn tcp_connect(
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<(u64, TcpStream)> {
    let start = Instant::now();
    let stream = time::timeout(timeout, TcpStream::connect((host, port)))
        .await
        .map_err(|_| ProxyError::Timeout("tcp connect to proxy"))?
        .map_err(|e| ProxyError::Io(e))?;
    let latency_ms = start.elapsed().as_millis() as u64;
    Ok((latency_ms, stream))
}

impl ProxyProbe {
    pub(super) async fn probe_endpoint(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
    ) -> CapabilityProbeAttempt {
        match time::timeout(
            self.config.timeouts.attempt,
            self.probe_endpoint_inner(host, port, protocol),
        )
        .await
        {
            Ok(attempt) => attempt,
            Err(_) => CapabilityProbeAttempt::failed(ProxyError::Timeout("proxy probe attempt")),
        }
    }

    async fn probe_endpoint_inner(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
    ) -> CapabilityProbeAttempt {
        let start = Instant::now();
        let stream = match tcp_connect(host, port, self.config.timeouts.proxy_connect).await {
            Ok((_, stream)) => stream,
            Err(error) => {
                self.observe(
                    crate::core::ProxyObservationPhase::EndpointConnect,
                    protocol,
                    None,
                    false,
                    None,
                );
                return CapabilityProbeAttempt::endpoint_connection_failed(error);
            }
        };
        self.observe(
            crate::core::ProxyObservationPhase::EndpointConnect,
            protocol,
            None,
            true,
            None,
        );
        if matches!(protocol, ProxyProtocol::Https) {
            if let Err(error) =
                wrap_tls(stream, host, self.config.timeouts.tls_handshake, true).await
            {
                self.observe(
                    crate::core::ProxyObservationPhase::ProxyTls,
                    protocol,
                    None,
                    false,
                    None,
                );
                return CapabilityProbeAttempt {
                    endpoint_reachable: true,
                    endpoint_connection_failed: false,
                    latency_ms: Some(start.elapsed().as_millis() as u64),
                    proxy_tls_certificate: ProxyCapabilityStatus::Unavailable,
                    capability_status: ProxyCapabilityStatus::Unknown,
                    destination_tls: ProxyCapabilityStatus::Unknown,
                    usable: false,
                    authentication_required: false,
                    error: Some(error),
                };
            }
            self.observe(
                crate::core::ProxyObservationPhase::ProxyTls,
                protocol,
                None,
                true,
                None,
            );
        }
        CapabilityProbeAttempt {
            endpoint_reachable: true,
            endpoint_connection_failed: false,
            latency_ms: Some(start.elapsed().as_millis() as u64),
            proxy_tls_certificate: if matches!(protocol, ProxyProtocol::Https) {
                ProxyCapabilityStatus::Available
            } else {
                ProxyCapabilityStatus::Unknown
            },
            capability_status: ProxyCapabilityStatus::Unknown,
            destination_tls: ProxyCapabilityStatus::Unknown,
            usable: false,
            authentication_required: false,
            error: None,
        }
    }

    pub(super) async fn probe_capability(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
        target: CapabilityProbeTarget<'_>,
    ) -> CapabilityProbeAttempt {
        let (url, policy) = match &target {
            CapabilityProbeTarget::HttpForwarding(_) => (
                self.config.http_probe_url.as_deref(),
                &self.config.http_policy,
            ),
            CapabilityProbeTarget::Https(_) => (
                self.config.https_probe_url.as_deref(),
                &self.config.https_policy,
            ),
        };
        let Some(url) = url else {
            return self.probe_endpoint(host, port, protocol).await;
        };
        let first = self
            .probe_target_once(host, port, protocol, target, url)
            .await;
        if first.authentication_required
            || first.proxy_tls_certificate == ProxyCapabilityStatus::Unavailable
        {
            return first;
        }
        let accepted = self.target_accepted(url);
        if accepted || first.endpoint_connection_failed {
            return first;
        }
        if let Some(fallback) = &policy.fallback_url {
            if self
                .fallback_used
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                let mut alternate = self.clone();
                alternate.configuration_identity = Some(self.configuration_id());
                let mut second = if url.starts_with("https:") {
                    alternate.config.https_probe_url = Some(fallback.clone());
                    match parse_https_probe_url(fallback) {
                        Ok(probe) => {
                            alternate
                                .probe_target_once(
                                    host,
                                    port,
                                    protocol,
                                    CapabilityProbeTarget::Https(&probe),
                                    fallback,
                                )
                                .await
                        }
                        Err(error) => CapabilityProbeAttempt::failed(error),
                    }
                } else {
                    alternate.config.http_probe_url = Some(fallback.clone());
                    match parse_http_probe_url(fallback, 80) {
                        Ok(probe) => {
                            alternate
                                .probe_target_once(
                                    host,
                                    port,
                                    protocol,
                                    CapabilityProbeTarget::HttpForwarding(&probe),
                                    fallback,
                                )
                                .await
                        }
                        Err(error) => CapabilityProbeAttempt::failed(error),
                    }
                };
                if first.capability_status == ProxyCapabilityStatus::Available {
                    second.capability_status = first.capability_status;
                }
                if first.destination_tls == ProxyCapabilityStatus::Available {
                    second.destination_tls = first.destination_tls;
                }
                if first.proxy_tls_certificate == ProxyCapabilityStatus::Available
                    && second.proxy_tls_certificate == ProxyCapabilityStatus::Unknown
                {
                    second.proxy_tls_certificate = first.proxy_tls_certificate;
                }
                if first.endpoint_reachable {
                    second.endpoint_reachable = true;
                    second.endpoint_connection_failed = false;
                }
                return second;
            }
        }
        first
    }

    pub(super) async fn probe_capability_inner(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
        target: CapabilityProbeTarget<'_>,
    ) -> CapabilityProbeAttempt {
        let start = Instant::now();
        let stream = match tcp_connect(host, port, self.config.timeouts.proxy_connect).await {
            Ok((_, stream)) => stream,
            Err(error) => {
                self.observe(
                    crate::core::ProxyObservationPhase::EndpointConnect,
                    protocol,
                    None,
                    false,
                    None,
                );
                return CapabilityProbeAttempt::endpoint_connection_failed(error);
            }
        };
        self.observe(
            crate::core::ProxyObservationPhase::EndpointConnect,
            protocol,
            None,
            true,
            None,
        );
        let mut proxy_tls_certificate = ProxyCapabilityStatus::Unknown;
        let mut stream: Box<dyn AsyncProbeStream> = if matches!(protocol, ProxyProtocol::Https) {
            match wrap_tls(stream, host, self.config.timeouts.tls_handshake, true).await {
                Ok(stream) => {
                    proxy_tls_certificate = ProxyCapabilityStatus::Available;
                    self.observe(
                        crate::core::ProxyObservationPhase::ProxyTls,
                        protocol,
                        None,
                        true,
                        None,
                    );
                    Box::new(stream)
                }
                Err(error) => {
                    self.observe(
                        crate::core::ProxyObservationPhase::ProxyTls,
                        protocol,
                        None,
                        false,
                        None,
                    );
                    return CapabilityProbeAttempt {
                        endpoint_reachable: true,
                        endpoint_connection_failed: false,
                        latency_ms: Some(start.elapsed().as_millis() as u64),
                        proxy_tls_certificate: ProxyCapabilityStatus::Unavailable,
                        capability_status: ProxyCapabilityStatus::Unknown,
                        destination_tls: ProxyCapabilityStatus::Unknown,
                        usable: false,
                        authentication_required: false,
                        error: Some(error),
                    };
                }
            }
        } else {
            Box::new(stream)
        };

        let (capability_status, destination_tls, authentication_required, capability_result) =
            match (protocol, target) {
                (
                    ProxyProtocol::Http | ProxyProtocol::Https,
                    CapabilityProbeTarget::HttpForwarding(probe),
                ) => {
                    let result = self
                        .exchange_probe(
                            &mut *stream,
                            &probe.request_target,
                            &probe.host_header,
                            self.config.http_probe_url.as_deref().unwrap(),
                            &self.config.http_policy,
                            protocol,
                        )
                        .await
                        .map_err(|error| {
                            if is_auth_required(&error) {
                                error
                            } else {
                                error.at_phase(crate::core::ProxyObservationPhase::DestinationHttp)
                            }
                        });
                    let authentication_required =
                        result.as_ref().is_err_and(|error| is_auth_required(error));
                    let result = result.map(|_| ());
                    self.observe(
                        crate::core::ProxyObservationPhase::ProxyProtocol,
                        protocol,
                        None,
                        result.is_ok(),
                        None,
                    );
                    (
                        capability_status(&result),
                        ProxyCapabilityStatus::Unknown,
                        authentication_required,
                        result,
                    )
                }
                (ProxyProtocol::Socks5, CapabilityProbeTarget::HttpForwarding(probe)) => {
                    let result = connect_socks5_with_local_dns_fallback(
                        &mut stream,
                        host,
                        port,
                        &probe.destination,
                        &self.config.timeouts,
                        &self.dns_resolver,
                    )
                    .await;
                    self.observe(
                        crate::core::ProxyObservationPhase::Tunnel,
                        protocol,
                        self.config.http_probe_url.as_deref(),
                        result.is_ok(),
                        None,
                    );
                    self.observe(
                        crate::core::ProxyObservationPhase::ProxyProtocol,
                        protocol,
                        None,
                        result.is_ok(),
                        None,
                    );
                    let authentication_required = result
                        .as_ref()
                        .is_err_and(|error| is_socks5_auth_required(error));
                    let result = if result.is_ok() {
                        let url = self.config.http_probe_url.as_deref().unwrap();
                        self.exchange_probe(
                            &mut *stream,
                            &probe_origin_target(url),
                            &probe.host_header,
                            url,
                            &self.config.http_policy,
                            protocol,
                        )
                        .await
                        .map_err(|error| {
                            error.at_phase(crate::core::ProxyObservationPhase::DestinationHttp)
                        })
                        .map(|_| ())
                    } else {
                        result
                    };
                    (
                        capability_status(&result),
                        ProxyCapabilityStatus::Unknown,
                        authentication_required,
                        result,
                    )
                }
                (
                    ProxyProtocol::Socks4 | ProxyProtocol::Socks4a,
                    CapabilityProbeTarget::HttpForwarding(probe),
                ) => {
                    let result = match prepare_socks4_destination(
                        protocol,
                        &probe.destination,
                        self.config.timeouts.target_connect,
                        &self.dns_resolver,
                    )
                    .await
                    {
                        Ok(destination) => {
                            send_socks4_connect(
                                &mut *stream,
                                &destination,
                                self.config.timeouts.proxy_handshake,
                                None,
                            )
                            .await
                        }
                        Err(error) => Err(error),
                    };
                    let result = if result.is_ok() {
                        let url = self.config.http_probe_url.as_deref().unwrap();
                        self.exchange_probe(
                            &mut *stream,
                            &probe_origin_target(url),
                            &probe.host_header,
                            url,
                            &self.config.http_policy,
                            protocol,
                        )
                        .await
                        .map_err(|error| {
                            error.at_phase(crate::core::ProxyObservationPhase::DestinationHttp)
                        })
                        .map(|_| ())
                    } else {
                        result
                    };
                    (
                        capability_status(&result),
                        ProxyCapabilityStatus::Unknown,
                        false,
                        result,
                    )
                }
                (protocol, CapabilityProbeTarget::Https(probe)) => {
                    let tunnel_result = match protocol {
                        ProxyProtocol::Http | ProxyProtocol::Https => {
                            send_http_connect(
                                &mut *stream,
                                &probe.destination,
                                self.config.timeouts.target_connect,
                                None,
                            )
                            .await
                        }
                        ProxyProtocol::Socks5 => {
                            connect_socks5_with_local_dns_fallback(
                                &mut stream,
                                host,
                                port,
                                &probe.destination,
                                &self.config.timeouts,
                                &self.dns_resolver,
                            )
                            .await
                        }
                        ProxyProtocol::Socks4 | ProxyProtocol::Socks4a => {
                            match prepare_socks4_destination(
                                protocol,
                                &probe.destination,
                                self.config.timeouts.target_connect,
                                &self.dns_resolver,
                            )
                            .await
                            {
                                Ok(destination) => {
                                    send_socks4_connect(
                                        &mut *stream,
                                        &destination,
                                        self.config.timeouts.proxy_handshake,
                                        None,
                                    )
                                    .await
                                }
                                Err(error) => Err(error),
                            }
                        }
                    };

                    self.observe(
                        crate::core::ProxyObservationPhase::Tunnel,
                        protocol,
                        self.config.https_probe_url.as_deref(),
                        tunnel_result.is_ok(),
                        None,
                    );
                    if tunnel_result.is_ok() {
                        self.observe(
                            crate::core::ProxyObservationPhase::ProxyProtocol,
                            protocol,
                            None,
                            true,
                            None,
                        );
                    }
                    if let Err(error) = tunnel_result {
                        let authentication_required =
                            is_auth_required(&error) || is_socks5_auth_required(&error);
                        (
                            failed_capability_status(&error),
                            ProxyCapabilityStatus::Unknown,
                            authentication_required,
                            Err(error),
                        )
                    } else {
                        match wrap_tls(
                            stream,
                            &probe.server_name,
                            self.config.timeouts.tls_handshake,
                            true,
                        )
                        .await
                        {
                            Err(error) => {
                                self.observe(
                                    crate::core::ProxyObservationPhase::DestinationTls,
                                    protocol,
                                    self.config.https_probe_url.as_deref(),
                                    false,
                                    None,
                                );
                                let destination_tls = ProxyCapabilityStatus::Unknown;
                                (
                                    ProxyCapabilityStatus::Available,
                                    destination_tls,
                                    false,
                                    Err(error),
                                )
                            }
                            Ok(mut destination_stream) => {
                                self.observe(
                                    crate::core::ProxyObservationPhase::DestinationTls,
                                    protocol,
                                    self.config.https_probe_url.as_deref(),
                                    true,
                                    None,
                                );
                                let response = self
                                    .exchange_probe(
                                        &mut destination_stream,
                                        &probe.request_target,
                                        &probe.host_header,
                                        self.config.https_probe_url.as_deref().unwrap(),
                                        &self.config.https_policy,
                                        protocol,
                                    )
                                    .await;
                                let result = response.map(|_| ());
                                let destination_tls = ProxyCapabilityStatus::Available;
                                (
                                    ProxyCapabilityStatus::Available,
                                    destination_tls,
                                    false,
                                    result,
                                )
                            }
                        }
                    }
                }
            };
        CapabilityProbeAttempt {
            endpoint_reachable: true,
            endpoint_connection_failed: false,
            latency_ms: Some(start.elapsed().as_millis() as u64),
            proxy_tls_certificate,
            capability_status,
            destination_tls,
            usable: capability_result.is_ok(),
            authentication_required,
            error: capability_result.err(),
        }
    }
}
