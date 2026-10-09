use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{self, Instant};
use tokio_rustls::TlsConnector;

use crate::core::dns_resolver::ProxyDnsResolver;
use crate::core::{
    Destination, DestinationAddress, ProxyCapabilityStatus, ProxyError, ProxyProtocol, ProxyRecord,
    ProxyRuntimeStatus, Result, PROXY_PROTOCOL_PRIORITY,
};

/// Probe mode that controls how strictly capabilities are verified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeMode {
    /// Use only capabilities that were validated successfully at runtime.
    Strict,
    /// Allow compatible source claims when runtime validation is absent or
    /// inconclusive, without changing runtime capability statuses.
    Relaxed,
}

/// Timeout values used by one proxy probe attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeTimeoutConfig {
    /// Maximum time allowed to establish a TCP connection to the proxy.
    pub proxy_connect: Duration,
    /// Maximum time allowed for a proxy protocol exchange or probe request.
    pub proxy_handshake: Duration,
    /// Maximum time allowed for local target resolution or tunnel setup.
    pub target_connect: Duration,
    /// Maximum time allowed for a proxy or destination TLS handshake.
    pub tls_handshake: Duration,
    /// Overall limit for one capability probe or destination validation attempt.
    pub attempt: Duration,
}

impl Default for ProbeTimeoutConfig {
    fn default() -> Self {
        Self {
            proxy_connect: Duration::from_secs(3),
            proxy_handshake: Duration::from_secs(5),
            target_connect: Duration::from_secs(5),
            tls_handshake: Duration::from_secs(5),
            attempt: Duration::from_secs(10),
        }
    }
}

/// Configuration for proxy probing.
#[derive(Clone, Debug)]
pub struct ProbeConfig {
    /// URL for HTTP reachability probe.
    ///
    /// When absent, TCP connectivity is verified but HTTP forwarding capability
    /// is not tested.
    pub http_probe_url: Option<String>,
    /// URL for the complete HTTPS probe.
    ///
    /// The probe opens a tunnel, validates destination TLS, then sends a light
    /// HTTP request. When absent, [`ProxyRecord::https_tunnel`] and
    /// [`ProxyRecord::destination_tls`] remain unknown.
    pub https_probe_url: Option<String>,
    /// Time limits for the individual probe phases and complete attempts.
    pub timeouts: ProbeTimeoutConfig,
    /// Probe mode.
    pub mode: ProbeMode,
    /// Protocol detection order for records without a known protocol.
    pub protocol_detection_order: Vec<ProxyProtocol>,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            http_probe_url: Some("http://example.com/".to_string()),
            https_probe_url: None,
            timeouts: ProbeTimeoutConfig::default(),
            mode: ProbeMode::Relaxed,
            protocol_detection_order: PROXY_PROTOCOL_PRIORITY.to_vec(),
        }
    }
}

/// Probes proxy records by connecting to the proxy endpoint and testing
/// protocol handshakes, HTTP forwarding, and HTTPS tunnelling.
///
/// Each probe updates the supplied [`ProxyRecord`] in place so that callers
/// never have to reconcile a separate report structure.
#[derive(Clone)]
pub struct ProxyProbe {
    config: ProbeConfig,
    dns_resolver: ProxyDnsResolver,
}

impl ProxyProbe {
    /// Creates a new proxy prober.
    pub fn new(config: ProbeConfig) -> Self {
        Self {
            config,
            dns_resolver: ProxyDnsResolver::system(),
        }
    }

    /// Configures local probe resolution with an Arachnea DNS core.
    #[cfg(feature = "arachnea-dns")]
    pub fn with_arachnea_dns(mut self, dns_core: Arc<arachnea_dns::core::ArachneaDnsCore>) -> Self {
        self.dns_resolver = ProxyDnsResolver::with_arachnea_dns(dns_core);
        self
    }

    /// Returns a reference to the probe configuration.
    pub fn config(&self) -> &ProbeConfig {
        &self.config
    }

    /// Checks endpoint TCP reachability without asking the proxy to connect to
    /// a destination. Success does not establish protocol or application support.
    pub(crate) async fn precheck_endpoint(&self, record: &ProxyRecord) -> Result<()> {
        tcp_connect(
            &record.host,
            record.port,
            self.config.timeouts.proxy_connect,
        )
        .await
        .map(|_| ())
    }

    /// Measures only the capability required by automatic selection, retaining
    /// the unrelated capability and the configured DNS resolver.
    pub(crate) async fn probe_required(
        &self,
        record: &mut ProxyRecord,
        require_https: bool,
    ) -> Result<()> {
        let mut targeted = self.clone();
        let previous_protocol = record.protocol.clone();
        let http_forwarding = record.http_forwarding;
        let https_tunnel = record.https_tunnel;
        let destination_tls = record.destination_tls;
        if require_https {
            targeted.config.http_probe_url = None;
        } else {
            targeted.config.https_probe_url = None;
        }
        let result = targeted.probe(record).await;
        let same_protocol = previous_protocol == record.protocol;
        if require_https {
            record.http_forwarding = if same_protocol {
                http_forwarding
            } else {
                ProxyCapabilityStatus::Unknown
            };
        } else {
            record.https_tunnel = if same_protocol {
                https_tunnel
            } else {
                ProxyCapabilityStatus::Unknown
            };
            record.destination_tls = if same_protocol {
                destination_tls
            } else {
                ProxyCapabilityStatus::Unknown
            };
        }
        result
    }

    /// Probes a single proxy record, updating every measurable field in place.
    ///
    /// Always sets `last_checked`. Updates the global status, detected protocol,
    /// runtime capabilities, latency, authentication state and failure count.
    ///
    /// Source-declared protocols are tested first in configured priority order.
    /// Relaxed mode then tries undeclared protocols as a controlled correction
    /// path. Records without declarations retain full protocol detection.
    ///
    /// # Errors
    ///
    /// Returns the concrete connection, handshake, protocol, TLS or
    /// configuration error when no configured capability can be used. The
    /// record is updated before the error is returned.
    pub async fn probe(&self, record: &mut ProxyRecord) -> Result<()> {
        record.last_checked = Some(SystemTime::now());

        let protocols = self.protocol_candidates(record);
        let result = self.detect_protocol(record, &protocols).await;

        match result {
            Ok(probe_result) => {
                if let Some(error) = self.apply_result(record, probe_result) {
                    return Err(error);
                }
            }
            Err(error) => {
                record.status = ProxyRuntimeStatus::Ko;
                record.failure_count += 1;
                return Err(error);
            }
        }

        Ok(())
    }

    /// Validates whether a proxy record can reach a specific destination.
    ///
    /// Unlike `probe`, this does not update the record's global status. It
    /// returns `Ok(())` when the destination is reachable through the proxy,
    /// or an error describing the failure so the caller can record a
    /// per-destination cooldown.
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy is unreachable, the protocol handshake
    /// fails, or the destination rejects the tunnel.
    pub async fn validate_destination(
        &self,
        record: &ProxyRecord,
        destination: &Destination,
    ) -> Result<()> {
        time::timeout(
            self.config.timeouts.attempt,
            self.validate_destination_inner(record, destination),
        )
        .await
        .map_err(|_| ProxyError::Timeout("destination validation attempt"))?
    }

    async fn validate_destination_inner(
        &self,
        record: &ProxyRecord,
        destination: &Destination,
    ) -> Result<()> {
        let protocol = record.protocol.as_ref().ok_or_else(|| {
            ProxyError::Config(
                "cannot validate destination without a known protocol on the record".to_string(),
            )
        })?;

        let authority = record.authority();
        let endpoint =
            Destination::from_authority(&authority, crate::core::ApplicationProtocol::Tcp)?;

        let _start = Instant::now();
        let stream = time::timeout(
            self.config.timeouts.proxy_connect,
            TcpStream::connect((&*endpoint.host_for_protocol(), endpoint.port)),
        )
        .await
        .map_err(|_| ProxyError::Timeout("destination validation tcp connect"))?
        .map_err(|_| {
            ProxyError::RouteUnavailable(format!(
                "cannot connect to proxy for destination validation: {}",
                destination.authority()
            ))
        })?;

        let mut stream: Box<dyn AsyncProbeStream> = if matches!(protocol, ProxyProtocol::Https) {
            Box::new(
                wrap_tls(
                    stream,
                    &endpoint.host_for_protocol(),
                    self.config.timeouts.tls_handshake,
                    true,
                )
                .await?,
            )
        } else {
            Box::new(stream)
        };

        match protocol {
            ProxyProtocol::Http | ProxyProtocol::Https => {
                send_http_connect(
                    &mut *stream,
                    destination,
                    self.config.timeouts.target_connect,
                    None,
                )
                .await?;
            }
            ProxyProtocol::Socks5 => {
                connect_socks5_with_local_dns_fallback(
                    &mut stream,
                    &record.host,
                    record.port,
                    destination,
                    &self.config.timeouts,
                    &self.dns_resolver,
                )
                .await?;
            }
            ProxyProtocol::Socks4 | ProxyProtocol::Socks4a => {
                let target = prepare_socks4_destination(
                    protocol,
                    destination,
                    self.config.timeouts.target_connect,
                    &self.dns_resolver,
                )
                .await?;
                send_socks4_connect(
                    &mut *stream,
                    &target,
                    self.config.timeouts.target_connect,
                    None,
                )
                .await?;
            }
        }

        Ok(())
    }

    // ── internal helpers ──────────────────────────────────────────────

    fn apply_result(
        &self,
        record: &mut ProxyRecord,
        result: ProtocolProbeResult,
    ) -> Option<ProxyError> {
        if let Some(protocol) = &result.protocol {
            record.protocol = Some(protocol.clone());
        }
        record.latency_ms = result.latency_ms;
        record.http_forwarding = result.http_forwarding;
        record.https_tunnel = result.https_tunnel;
        record.destination_tls = result.destination_tls;
        record.proxy_tls_certificate = result.proxy_tls_certificate;

        let relaxed_fallback = matches!(self.config.mode, ProbeMode::Relaxed)
            && result.endpoint_reachable
            && self.relaxed_fallback_is_usable(record, &result);
        if result.usable
            || (result.endpoint_reachable && !result.capability_configured)
            || relaxed_fallback
        {
            record.status = ProxyRuntimeStatus::Ok;
            if result.usable || !result.capability_configured {
                record.last_validated_at = Some(SystemTime::now());
            }
            record.authentication_required = Some(false);
            record.failure_count = 0;
        } else if result.authentication_required {
            record.status = ProxyRuntimeStatus::AuthenticationRequired;
            record.authentication_required = Some(true);
            record.failure_count += 1;
        } else {
            record.status = ProxyRuntimeStatus::Ko;
            record.authentication_required = Some(false);
            record.failure_count += 1;
        }

        if matches!(record.status, ProxyRuntimeStatus::Ko) {
            result.error
        } else {
            None
        }
    }

    fn relaxed_fallback_is_usable(
        &self,
        record: &ProxyRecord,
        result: &ProtocolProbeResult,
    ) -> bool {
        let protocol = result.protocol.as_ref().or(record.protocol.as_ref());
        let proxy_transport_usable = !matches!(
            result.proxy_tls_certificate,
            ProxyCapabilityStatus::Unavailable
        );
        let http_fallback = proxy_transport_usable
            && matches!(result.http_forwarding, ProxyCapabilityStatus::Unknown)
            && protocol.is_some();
        let https_fallback = proxy_transport_usable
            && !matches!(result.destination_tls, ProxyCapabilityStatus::Unavailable)
            && match result.https_tunnel {
                ProxyCapabilityStatus::Available => true,
                ProxyCapabilityStatus::Unavailable => false,
                ProxyCapabilityStatus::Unknown => match protocol {
                    Some(
                        ProxyProtocol::Socks4 | ProxyProtocol::Socks4a | ProxyProtocol::Socks5,
                    ) => true,
                    Some(ProxyProtocol::Http | ProxyProtocol::Https) => {
                        record.supports_https == Some(true)
                    }
                    None => false,
                },
            };

        http_fallback || https_fallback
    }

    async fn probe_protocol(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
    ) -> Result<ProtocolProbeResult> {
        match protocol {
            ProxyProtocol::Http => self.probe_http(host, port, false).await,
            ProxyProtocol::Https => self.probe_http(host, port, true).await,
            ProxyProtocol::Socks5 => self.probe_socks5(host, port).await,
            ProxyProtocol::Socks4 | ProxyProtocol::Socks4a => {
                self.probe_socks4(host, port, protocol).await
            }
        }
    }

    fn protocol_candidates(&self, record: &ProxyRecord) -> Vec<ProxyProtocol> {
        let mut declared = Vec::new();
        for protocol in record
            .declarations
            .iter()
            .filter_map(|declaration| declaration.protocol.clone())
        {
            if !declared.contains(&protocol) {
                declared.push(protocol);
            }
        }
        if declared.is_empty() {
            if let Some(protocol) = &record.protocol {
                declared.push(protocol.clone());
            }
        }
        if declared.is_empty() {
            return self.config.protocol_detection_order.clone();
        }

        let mut candidates = self
            .config
            .protocol_detection_order
            .iter()
            .filter(|protocol| declared.contains(protocol))
            .cloned()
            .collect::<Vec<_>>();
        for protocol in declared {
            if !candidates.contains(&protocol) {
                candidates.push(protocol);
            }
        }
        if matches!(self.config.mode, ProbeMode::Relaxed) {
            for protocol in &self.config.protocol_detection_order {
                if !candidates.contains(protocol) {
                    candidates.push(protocol.clone());
                }
            }
        }
        candidates
    }

    async fn detect_protocol(
        &self,
        record: &ProxyRecord,
        protocols: &[ProxyProtocol],
    ) -> Result<ProtocolProbeResult> {
        let mut last_error = None;
        let mut last_result = None;
        let mut authentication_result = None;
        let mut relaxed_declared_result = None;
        for protocol in protocols {
            let declared = record
                .declarations
                .iter()
                .any(|declaration| declaration.protocol.as_ref() == Some(protocol))
                || (record.declarations.is_empty() && record.protocol.as_ref() == Some(protocol));
            match self
                .probe_protocol(&record.host, record.port, protocol)
                .await
            {
                Ok(mut result) => {
                    result.protocol = Some(protocol.clone());
                    if result.endpoint_connection_failed {
                        return Ok(result);
                    }
                    if result.usable || (result.endpoint_reachable && !result.capability_configured)
                    {
                        return Ok(result);
                    }
                    if declared
                        && matches!(self.config.mode, ProbeMode::Relaxed)
                        && result.endpoint_reachable
                        && self.relaxed_fallback_is_usable(record, &result)
                    {
                        relaxed_declared_result.get_or_insert(result);
                        continue;
                    }
                    if !declared
                        && matches!(self.config.mode, ProbeMode::Relaxed)
                        && result.endpoint_reachable
                        && (result.http_forwarding == ProxyCapabilityStatus::Available
                            || (result.https_tunnel == ProxyCapabilityStatus::Available
                                && result.destination_tls == ProxyCapabilityStatus::Available))
                    {
                        return Ok(result);
                    }
                    if result.authentication_required {
                        authentication_result = Some(result);
                        continue;
                    }
                    result.protocol = Some(protocol.clone());
                    if let Some(error) = result.error.take() {
                        last_error = Some(error);
                    }
                    last_result = Some(result);
                }
                Err(error) => last_error = Some(error),
            }
        }
        if let Some(result) = relaxed_declared_result {
            return Ok(result);
        }
        if let Some(result) = authentication_result {
            return Ok(result);
        }
        if let Some(mut result) = last_result {
            result.error = result.error.or(last_error);
            return Ok(result);
        }
        Err(last_error.unwrap_or_else(|| {
            ProxyError::Protocol("protocol detection did not attempt any protocol".to_string())
        }))
    }

    async fn probe_http(&self, host: &str, port: u16, tls: bool) -> Result<ProtocolProbeResult> {
        let protocol = if tls {
            ProxyProtocol::Https
        } else {
            ProxyProtocol::Http
        };
        self.probe_capabilities(host, port, &protocol).await
    }

    async fn probe_socks5(&self, host: &str, port: u16) -> Result<ProtocolProbeResult> {
        self.probe_capabilities(host, port, &ProxyProtocol::Socks5)
            .await
    }

    async fn probe_socks4(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
    ) -> Result<ProtocolProbeResult> {
        self.probe_capabilities(host, port, protocol).await
    }

    async fn probe_capabilities(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
    ) -> Result<ProtocolProbeResult> {
        let mut result = ProtocolProbeResult::default();

        if let Some(url) = &self.config.http_probe_url {
            result.capability_configured = true;
            let attempt = match parse_http_probe_url(url, 80) {
                Ok(destination) => {
                    self.probe_capability(
                        host,
                        port,
                        protocol,
                        CapabilityProbeTarget::HttpForwarding(&destination),
                    )
                    .await
                }
                Err(error) => CapabilityProbeAttempt::failed(error),
            };
            result.record_attempt(
                ProxyCapability::HttpForwarding,
                attempt,
                host,
                port,
                protocol,
            );
        }

        if !result.endpoint_connection_failed {
            if let Some(url) = &self.config.https_probe_url {
                result.capability_configured = true;
                let attempt = match parse_https_probe_url(url) {
                    Ok(probe) => {
                        self.probe_capability(
                            host,
                            port,
                            protocol,
                            CapabilityProbeTarget::Https(&probe),
                        )
                        .await
                    }
                    Err(error) => CapabilityProbeAttempt::failed(error),
                };
                result.record_attempt(ProxyCapability::HttpsTunnel, attempt, host, port, protocol);
            }
        }

        if !result.capability_configured {
            let attempt = self.probe_endpoint(host, port, protocol).await;
            result.endpoint_reachable = attempt.endpoint_reachable;
            result.latency_ms = attempt.latency_ms;
            result.proxy_tls_certificate = attempt.proxy_tls_certificate;
            result.error = attempt.error;
        }

        Ok(result)
    }

    async fn probe_endpoint(
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
            Err(error) => return CapabilityProbeAttempt::endpoint_connection_failed(error),
        };
        if matches!(protocol, ProxyProtocol::Https) {
            if let Err(error) =
                wrap_tls(stream, host, self.config.timeouts.tls_handshake, true).await
            {
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

    async fn probe_capability(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
        target: CapabilityProbeTarget<'_>,
    ) -> CapabilityProbeAttempt {
        match time::timeout(
            self.config.timeouts.attempt,
            self.probe_capability_inner(host, port, protocol, target),
        )
        .await
        {
            Ok(attempt) => attempt,
            Err(_) => CapabilityProbeAttempt::failed(ProxyError::Timeout("proxy probe attempt")),
        }
    }

    async fn probe_capability_inner(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
        target: CapabilityProbeTarget<'_>,
    ) -> CapabilityProbeAttempt {
        let start = Instant::now();
        let stream = match tcp_connect(host, port, self.config.timeouts.proxy_connect).await {
            Ok((_, stream)) => stream,
            Err(error) => return CapabilityProbeAttempt::endpoint_connection_failed(error),
        };
        let mut proxy_tls_certificate = ProxyCapabilityStatus::Unknown;
        let mut stream: Box<dyn AsyncProbeStream> = if matches!(protocol, ProxyProtocol::Https) {
            match wrap_tls(stream, host, self.config.timeouts.tls_handshake, true).await {
                Ok(stream) => {
                    proxy_tls_certificate = ProxyCapabilityStatus::Available;
                    Box::new(stream)
                }
                Err(error) => {
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
                    let result = send_http_forward_probe(
                        &mut *stream,
                        probe,
                        self.config.timeouts.proxy_handshake,
                    )
                    .await;
                    let authentication_required =
                        result.as_ref().is_err_and(|error| is_auth_required(error));
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
                    let authentication_required = result
                        .as_ref()
                        .is_err_and(|error| is_socks5_auth_required(error));
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
                                let destination_tls = failed_capability_status(&error);
                                (
                                    ProxyCapabilityStatus::Available,
                                    destination_tls,
                                    false,
                                    Err(error),
                                )
                            }
                            Ok(mut destination_stream) => {
                                let result = send_https_probe_request(
                                    &mut destination_stream,
                                    probe,
                                    self.config.timeouts.proxy_handshake,
                                )
                                .await;
                                let destination_tls = capability_status(&result);
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

// ── Protocol handshake helpers ───────────────────────────────────────

/// Opens a TCP connection to the proxy and measures its latency.
async fn tcp_connect(host: &str, port: u16, timeout: Duration) -> Result<(u64, TcpStream)> {
    let start = Instant::now();
    let stream = time::timeout(timeout, TcpStream::connect((host, port)))
        .await
        .map_err(|_| ProxyError::Timeout("tcp connect to proxy"))?
        .map_err(|e| ProxyError::Io(e))?;
    let latency_ms = start.elapsed().as_millis() as u64;
    Ok((latency_ms, stream))
}

/// Connects through SOCKS5, retrying with local DNS only after a likely remote
/// hostname-resolution failure.
async fn connect_socks5_with_local_dns_fallback(
    stream: &mut Box<dyn AsyncProbeStream>,
    proxy_host: &str,
    proxy_port: u16,
    destination: &Destination,
    timeouts: &ProbeTimeoutConfig,
    dns_resolver: &ProxyDnsResolver,
) -> Result<()> {
    let first_result =
        send_socks5_connect(&mut **stream, destination, timeouts.proxy_handshake, None).await;
    let Err(error) = first_result else {
        return Ok(());
    };
    if !is_socks5_remote_dns_failure(&error)
        || !matches!(destination.address, DestinationAddress::Host(_))
    {
        return Err(error);
    }

    let local_destination =
        resolve_destination_to_ip(destination, timeouts.target_connect, dns_resolver).await?;
    let (_, fallback_stream) = tcp_connect(proxy_host, proxy_port, timeouts.proxy_connect).await?;
    let mut fallback_stream: Box<dyn AsyncProbeStream> = Box::new(fallback_stream);
    send_socks5_connect(
        &mut *fallback_stream,
        &local_destination,
        timeouts.proxy_handshake,
        None,
    )
    .await?;
    *stream = fallback_stream;
    Ok(())
}

/// Prepares the target according to the declared SOCKS4 protocol variant.
async fn prepare_socks4_destination(
    protocol: &ProxyProtocol,
    destination: &Destination,
    timeout: Duration,
    dns_resolver: &ProxyDnsResolver,
) -> Result<Destination> {
    match protocol {
        ProxyProtocol::Socks4 => {
            resolve_destination_to_ipv4(destination, timeout, dns_resolver).await
        }
        ProxyProtocol::Socks4a => Ok(destination.clone()),
        _ => Err(ProxyError::Config(
            "SOCKS4 destination preparation requires SOCKS4 or SOCKS4a".to_string(),
        )),
    }
}

/// Resolves a hostname destination into the first available IP address.
async fn resolve_destination_to_ip(
    destination: &Destination,
    timeout: Duration,
    dns_resolver: &ProxyDnsResolver,
) -> Result<Destination> {
    let ip = match &destination.address {
        DestinationAddress::Ip(ip) => *ip,
        DestinationAddress::Host(host) => time::timeout(timeout, dns_resolver.resolve_ip(host))
            .await
            .map_err(|_| ProxyError::Timeout("local target dns resolution"))??,
    };
    Ok(Destination {
        address: DestinationAddress::Ip(ip),
        port: destination.port,
        protocol: destination.protocol.clone(),
    })
}

/// Resolves a hostname destination into the first available IPv4 address.
async fn resolve_destination_to_ipv4(
    destination: &Destination,
    timeout: Duration,
    dns_resolver: &ProxyDnsResolver,
) -> Result<Destination> {
    let ip = match &destination.address {
        DestinationAddress::Ip(std::net::IpAddr::V4(ip)) => *ip,
        DestinationAddress::Ip(std::net::IpAddr::V6(_)) => {
            return Err(ProxyError::InvalidDestination(
                "socks4 cannot encode ipv6 destinations".to_string(),
            ));
        }
        DestinationAddress::Host(host) => {
            match time::timeout(timeout, dns_resolver.resolve_ipv4(host))
                .await
                .map_err(|_| ProxyError::Timeout("local target ipv4 dns resolution"))??
            {
                std::net::IpAddr::V4(ip) => ip,
                std::net::IpAddr::V6(_) => unreachable!("resolve_ipv4 returned an IPv6 address"),
            }
        }
    };
    Ok(Destination {
        address: DestinationAddress::Ip(std::net::IpAddr::V4(ip)),
        port: destination.port,
        protocol: destination.protocol.clone(),
    })
}

/// Sends an HTTP CONNECT handshake over an existing stream.
///
/// Returns `Ok(())` on a 2xx response. Returns an error carrying the status
/// code on 407 or other failures.
async fn send_http_connect<S>(
    stream: &mut S,
    destination: &Destination,
    timeout: Duration,
    _credentials: Option<&str>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    let authority = destination.authority();
    let request = format!(
        "CONNECT {} HTTP/1.1\r\nHost: {}\r\nProxy-Connection: keep-alive\r\n\r\n",
        authority, authority
    );

    time::timeout(timeout, async {
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;

        let status = read_http_response_status(stream).await?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(ProxyError::UpstreamRejected(format!(
                "http connect returned status {status}"
            )))
        }
    })
    .await
    .map_err(|_| ProxyError::Timeout("http connect handshake"))?
}

async fn read_http_response_status<S>(stream: &mut S) -> Result<u16>
where
    S: AsyncRead + Unpin + ?Sized,
{
    let mut buffer = [0u8; 1024];
    let mut pos = 0;
    loop {
        if pos >= buffer.len() {
            return Err(ProxyError::Protocol(
                "http proxy response headers exceeded 1 KiB".to_string(),
            ));
        }

        let n = stream.read(&mut buffer[pos..]).await?;
        if n == 0 {
            return Err(ProxyError::Protocol(
                "http proxy closed before completing response".to_string(),
            ));
        }
        pos += n;
        if buffer[..pos].windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    let response = &buffer[..pos];
    let line_end = response
        .windows(2)
        .position(|w| w == b"\r\n")
        .ok_or_else(|| ProxyError::Protocol("missing http status line".to_string()))?;
    let line = std::str::from_utf8(&response[..line_end])
        .map_err(|_| ProxyError::Protocol("http status line is not utf-8".to_string()))?;
    line.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| ProxyError::Protocol("invalid http status line".to_string()))
}

/// Sends an HTTP forward request through an HTTP proxy.
///
/// This validates classic HTTP proxy forwarding. It intentionally does not use
/// CONNECT because many HTTP proxies only allow CONNECT to HTTPS ports.
async fn send_http_forward_probe<S>(
    stream: &mut S,
    probe: &HttpProbeTarget,
    timeout: Duration,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nProxy-Connection: close\r\nConnection: close\r\n\r\n",
        probe.request_target, probe.host_header
    );

    time::timeout(timeout, async {
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;

        let status = read_http_response_status(stream).await?;
        if status == 407 {
            return Err(ProxyError::UpstreamRejected(
                "http forward returned status 407".to_string(),
            ));
        }
        if status < 500 {
            Ok(())
        } else {
            Err(ProxyError::UpstreamRejected(format!(
                "http forward returned status {status}"
            )))
        }
    })
    .await
    .map_err(|_| ProxyError::Timeout("http forward probe"))?
}

/// Sends a lightweight HTTP request through an established TLS tunnel.
///
/// A syntactically valid response with status 200 through 499 proves that the
/// proxy carried a complete HTTPS exchange. Redirects are accepted without
/// being followed; server-side 5xx responses are rejected.
async fn send_https_probe_request<S>(
    stream: &mut S,
    probe: &HttpsProbeTarget,
    timeout: Duration,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    let request = format!(
        "HEAD {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        probe.request_target, probe.host_header
    );

    time::timeout(timeout, async {
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;

        let status = read_http_response_status(stream).await?;
        if (200..500).contains(&status) {
            Ok(())
        } else {
            Err(ProxyError::UpstreamRejected(format!(
                "https probe returned status {status}"
            )))
        }
    })
    .await
    .map_err(|_| ProxyError::Timeout("https request through proxy tunnel"))?
}

/// Performs a SOCKS5 CONNECT handshake over an existing stream.
async fn send_socks5_connect<S>(
    stream: &mut S,
    destination: &Destination,
    timeout: Duration,
    _credentials: Option<&str>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    time::timeout(timeout, async {
        // Negotiate: propose no-auth only (0x00)
        stream.write_all(&[0x05, 0x01, 0x00]).await?;
        let mut response = [0u8; 2];
        stream.read_exact(&mut response).await?;
        if response != [0x05, 0x00] {
            let method = response[1];
            if method == 0xFF {
                return Err(ProxyError::UpstreamRejected(
                    "socks5: no acceptable authentication method (0xFF)".to_string(),
                ));
            }
            return Err(ProxyError::UpstreamRejected(format!(
                "socks5: server selected unexpected method 0x{method:02x}"
            )));
        }

        // CONNECT request
        let request = encode_socks5_request(destination)?;
        stream.write_all(&request).await?;

        // Read reply
        let mut reply = [0u8; 4];
        stream.read_exact(&mut reply).await?;
        if reply[0] != 0x05 {
            return Err(ProxyError::Protocol(
                "socks5: invalid reply version".to_string(),
            ));
        }
        if reply[1] != 0x00 {
            return Err(ProxyError::UpstreamRejected(format!(
                "socks5: connect rejected with code 0x{:02x}",
                reply[1]
            )));
        }
        // Read remaining address/port bytes of the SOCKS5 reply
        let addr_len = match reply[3] {
            0x01 => 4,
            0x03 => {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await?;
                len[0] as usize
            }
            0x04 => 16,
            _ => {
                return Err(ProxyError::Protocol(
                    "socks5: unknown address type".to_string(),
                ));
            }
        };
        let mut rest = vec![0u8; addr_len + 2];
        stream.read_exact(&mut rest).await?;

        Ok(())
    })
    .await
    .map_err(|_| ProxyError::Timeout("socks5 connect handshake"))?
}

/// Performs a SOCKS4 or SOCKS4a CONNECT handshake over an existing stream.
async fn send_socks4_connect<S>(
    stream: &mut S,
    destination: &Destination,
    timeout: Duration,
    _user_id: Option<&str>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    time::timeout(timeout, async {
        let mut request = Vec::with_capacity(9);
        request.push(0x04);
        request.push(0x01);
        request.extend_from_slice(&destination.port.to_be_bytes());
        match &destination.address {
            DestinationAddress::Ip(IpAddr::V4(ip)) => {
                request.extend_from_slice(&ip.octets());
                request.push(0x00);
            }
            DestinationAddress::Ip(IpAddr::V6(_)) => {
                return Err(ProxyError::InvalidDestination(
                    "socks4 cannot encode ipv6 destinations".to_string(),
                ));
            }
            DestinationAddress::Host(host) => {
                if host.is_empty() || host.as_bytes().contains(&0) {
                    return Err(ProxyError::InvalidDestination(
                        "socks4a hostnames must be non-empty and contain no nul bytes".to_string(),
                    ));
                }
                request.extend_from_slice(&[0, 0, 0, 1]);
                request.push(0x00);
                request.extend_from_slice(host.as_bytes());
                request.push(0x00);
            }
        }

        stream.write_all(&request).await?;

        let mut reply = [0u8; 8];
        stream.read_exact(&mut reply).await?;
        if reply[0] != 0x00 {
            return Err(ProxyError::Protocol(
                "socks4: invalid reply null byte".to_string(),
            ));
        }
        if reply[1] != 0x5A {
            return Err(ProxyError::UpstreamRejected(format!(
                "socks4: connect rejected with code 0x{:02x}",
                reply[1]
            )));
        }

        Ok(())
    })
    .await
    .map_err(|_| ProxyError::Timeout("socks4 connect handshake"))?
}

/// Encodes a SOCKS5 CONNECT request.
fn encode_socks5_request(destination: &Destination) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(22);
    buf.push(0x05); // version
    buf.push(0x01); // CONNECT
    buf.push(0x00); // reserved

    match &destination.address {
        DestinationAddress::Ip(std::net::IpAddr::V4(ip)) => {
            buf.push(0x01);
            buf.extend_from_slice(&ip.octets());
        }
        DestinationAddress::Ip(std::net::IpAddr::V6(ip)) => {
            buf.push(0x04);
            buf.extend_from_slice(&ip.octets());
        }
        DestinationAddress::Host(host) => {
            buf.push(0x03);
            let host_bytes = host.as_bytes();
            if host_bytes.len() > 255 {
                return Err(ProxyError::Config(
                    "socks5 hostname too long for handshake encoding".to_string(),
                ));
            }
            buf.push(host_bytes.len() as u8);
            buf.extend_from_slice(host_bytes);
        }
    }

    buf.extend_from_slice(&destination.port.to_be_bytes());
    Ok(buf)
}

// ── TLS wrapping ──────────────────────────────────────────────────────

/// Wraps a TCP stream in TLS using WebPKI roots.
async fn wrap_tls<S>(
    stream: S,
    host: &str,
    handshake_timeout: Duration,
    verify: bool,
) -> Result<tokio_rustls::client::TlsStream<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let config = if verify {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| ProxyError::Tls(e.to_string()))?
            .with_root_certificates(roots)
            .with_no_client_auth()
    } else {
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| ProxyError::Tls(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
            .with_no_client_auth()
    };
    let connector = TlsConnector::from(Arc::new(config));
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|_| ProxyError::Tls("invalid tls server name for probe".to_string()))?;
    time::timeout(handshake_timeout, connector.connect(server_name, stream))
        .await
        .map_err(|_| ProxyError::Timeout("tls handshake in probe"))?
        .map_err(|e| ProxyError::Tls(e.to_string()))
}

/// Certificate verifier that accepts any certificate (used when verification
/// is disabled).
#[derive(Debug)]
struct NoCertificateVerification;

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

// ── URL parsing helper ──────────────────────────────────────────────

struct HttpProbeTarget {
    destination: Destination,
    request_target: String,
    host_header: String,
}

struct HttpsProbeTarget {
    destination: Destination,
    server_name: String,
    request_target: String,
    host_header: String,
}

fn parse_http_probe_url(url: &str, default_port: u16) -> Result<HttpProbeTarget> {
    let parsed = parse_probe_url_value(url, default_port)?;
    let host = parsed
        .host_str()
        .ok_or_else(|| ProxyError::Config(format!("probe URL '{url}' has no host")))?;
    let port = parsed.port_or_known_default().unwrap_or(default_port);
    let scheme_default_port = match parsed.scheme() {
        "https" => 443,
        _ => 80,
    };
    let host_header = if port == scheme_default_port {
        host.to_string()
    } else {
        format!("{host}:{port}")
    };

    Ok(HttpProbeTarget {
        destination: Destination::host_port(host.to_string(), port),
        request_target: parsed.as_str().to_string(),
        host_header,
    })
}

fn parse_https_probe_url(url: &str) -> Result<HttpsProbeTarget> {
    let parsed = parse_probe_url_value(url, 443)?;
    if parsed.scheme() != "https" {
        return Err(ProxyError::Config(format!(
            "HTTPS probe URL '{url}' must use the https scheme"
        )));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| ProxyError::Config(format!("probe URL '{url}' has no host")))?;
    let port = parsed.port_or_known_default().unwrap_or(443);
    let host_header = if port == 443 {
        host.to_string()
    } else {
        format!("{host}:{port}")
    };
    let mut request_target = parsed.path().to_string();
    if request_target.is_empty() {
        request_target.push('/');
    }
    if let Some(query) = parsed.query() {
        request_target.push('?');
        request_target.push_str(query);
    }

    Ok(HttpsProbeTarget {
        destination: Destination::host_port(host.to_string(), port),
        server_name: host.to_string(),
        request_target,
        host_header,
    })
}

fn parse_probe_url_value(url: &str, default_port: u16) -> Result<url::Url> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(ProxyError::Config("probe URL cannot be empty".to_string()));
    }

    let default_scheme = if default_port == 443 { "https" } else { "http" };
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("{default_scheme}://{trimmed}")
    };

    url::Url::parse(&candidate)
        .map_err(|error| ProxyError::Config(format!("invalid probe URL '{url}': {error}")))
}

// ── Auth detection helpers ──────────────────────────────────────────

/// Returns `true` when the error indicates the proxy requires authentication.
fn is_auth_required(error: &ProxyError) -> bool {
    if let ProxyError::UpstreamRejected(msg) = error {
        msg.contains("status 407")
    } else {
        false
    }
}

/// Returns `true` when the SOCKS5 error indicates auth is required.
fn is_socks5_auth_required(error: &ProxyError) -> bool {
    if let ProxyError::UpstreamRejected(msg) = error {
        msg.contains("0xFF")
    } else {
        false
    }
}

/// Returns true when a SOCKS5 rejection likely represents remote DNS failure.
fn is_socks5_remote_dns_failure(error: &ProxyError) -> bool {
    let ProxyError::UpstreamRejected(message) = error else {
        return false;
    };
    message.contains("code 0x01") || message.contains("code 0x04")
}

fn capability_status<T>(result: &Result<T>) -> ProxyCapabilityStatus {
    match result {
        Ok(_) => ProxyCapabilityStatus::Available,
        Err(error) => failed_capability_status(error),
    }
}

fn failed_capability_status(error: &ProxyError) -> ProxyCapabilityStatus {
    match error {
        ProxyError::Timeout(_)
        | ProxyError::Io(_)
        | ProxyError::Config(_)
        | ProxyError::RouteUnavailable(_)
        | ProxyError::Dns(_)
        | ProxyError::Unsupported(_) => ProxyCapabilityStatus::Unknown,
        ProxyError::Tls(_)
        | ProxyError::Protocol(_)
        | ProxyError::UpstreamRejected(_)
        | ProxyError::InvalidDestination(_)
        | ProxyError::AccessDenied(_) => ProxyCapabilityStatus::Unavailable,
    }
}

// ── Internal types ─────────────────────────────────────────────────

/// Trait alias for a boxed async read/write stream used during probing.
trait AsyncProbeStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncProbeStream for T {}

#[derive(Clone, Copy)]
enum ProxyCapability {
    HttpForwarding,
    HttpsTunnel,
}

enum CapabilityProbeTarget<'a> {
    HttpForwarding(&'a HttpProbeTarget),
    Https(&'a HttpsProbeTarget),
}

struct CapabilityProbeAttempt {
    endpoint_reachable: bool,
    endpoint_connection_failed: bool,
    latency_ms: Option<u64>,
    proxy_tls_certificate: ProxyCapabilityStatus,
    capability_status: ProxyCapabilityStatus,
    destination_tls: ProxyCapabilityStatus,
    usable: bool,
    authentication_required: bool,
    error: Option<ProxyError>,
}

impl CapabilityProbeAttempt {
    fn failed(error: ProxyError) -> Self {
        let capability_status = failed_capability_status(&error);
        Self {
            endpoint_reachable: false,
            endpoint_connection_failed: false,
            latency_ms: None,
            proxy_tls_certificate: ProxyCapabilityStatus::Unknown,
            capability_status,
            destination_tls: ProxyCapabilityStatus::Unknown,
            usable: false,
            authentication_required: false,
            error: Some(error),
        }
    }

    fn endpoint_connection_failed(error: ProxyError) -> Self {
        let mut attempt = Self::failed(error);
        attempt.endpoint_connection_failed = true;
        attempt
    }
}

/// Intermediate probe result that gets applied to a `ProxyRecord`.
#[derive(Default)]
struct ProtocolProbeResult {
    endpoint_reachable: bool,
    endpoint_connection_failed: bool,
    capability_configured: bool,
    usable: bool,
    latency_ms: Option<u64>,
    authentication_required: bool,
    http_forwarding: ProxyCapabilityStatus,
    https_tunnel: ProxyCapabilityStatus,
    destination_tls: ProxyCapabilityStatus,
    proxy_tls_certificate: ProxyCapabilityStatus,
    protocol: Option<ProxyProtocol>,
    error: Option<ProxyError>,
}

impl ProtocolProbeResult {
    fn record_attempt(
        &mut self,
        capability: ProxyCapability,
        attempt: CapabilityProbeAttempt,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
    ) {
        self.endpoint_reachable |= attempt.endpoint_reachable;
        self.endpoint_connection_failed |= attempt.endpoint_connection_failed;
        self.authentication_required |= attempt.authentication_required;
        self.usable |= attempt.usable;
        if let Some(latency_ms) = attempt.latency_ms {
            self.latency_ms = Some(
                self.latency_ms
                    .map_or(latency_ms, |current| current.min(latency_ms)),
            );
        }
        if !matches!(
            attempt.proxy_tls_certificate,
            ProxyCapabilityStatus::Unknown
        ) {
            self.proxy_tls_certificate = attempt.proxy_tls_certificate;
        }
        if !matches!(attempt.destination_tls, ProxyCapabilityStatus::Unknown) {
            self.destination_tls = attempt.destination_tls;
        }

        match capability {
            ProxyCapability::HttpForwarding => self.http_forwarding = attempt.capability_status,
            ProxyCapability::HttpsTunnel => self.https_tunnel = attempt.capability_status,
        }

        if let Some(error) = attempt.error {
            tracing::debug!(
                proxy = %if host.contains(':') {
                    format!("[{host}]:{port}")
                } else {
                    format!("{host}:{port}")
                },
                ?protocol,
                capability = match capability {
                    ProxyCapability::HttpForwarding => "http_forwarding",
                    ProxyCapability::HttpsTunnel => "https_tunnel",
                },
                %error,
                "dynamic proxy capability probe failed"
            );
            if self.error.is_none() {
                self.error = Some(error);
            }
        }
    }
}
