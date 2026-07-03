use std::sync::Arc;
use std::time::{Duration, SystemTime};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{self, Instant};
use tokio_rustls::TlsConnector;

use crate::core::{
    Destination, DestinationAddress, ProxyProtocol, ProxyRecord, ProxyRuntimeStatus, ProxyError,
    Result,
};

/// Probe mode that controls how strictly capabilities are verified.
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeMode {
    /// Use configured probe URLs; reject records whose `supports_https` cannot
    /// be verified when the configuration requires it.
    Strict,
    /// Accept source-claimed values when probe URLs are absent.
    Relaxed,
}

/// Configuration for proxy probing.
#[derive(Clone, Debug)]
pub struct ProbeConfig {
    /// URL for HTTP reachability probe.
    ///
    /// When absent, TCP connectivity is verified but HTTP forwarding capability
    /// is not tested.
    pub http_probe_url: Option<String>,
    /// URL for HTTPS tunnel probe.
    ///
    /// When absent, the proxy is not tested for HTTPS support and
    /// `supports_https` is not set to `true` by the probe.
    pub https_probe_url: Option<String>,
    /// Connection and handshake timeout.
    pub timeout: Duration,
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
            timeout: Duration::from_secs(5),
            mode: ProbeMode::Relaxed,
            protocol_detection_order: vec![
                ProxyProtocol::Http,
                ProxyProtocol::Socks5,
                ProxyProtocol::Socks4a,
                ProxyProtocol::Https,
            ],
        }
    }
}

/// Probes proxy records by connecting to the proxy endpoint and testing
/// protocol handshakes, HTTP forwarding, and HTTPS tunnelling.
///
/// Each probe updates the supplied [`ProxyRecord`] in place so that callers
/// never have to reconcile a separate report structure.
pub struct ProxyProbe {
    config: ProbeConfig,
}

impl ProxyProbe {
    /// Creates a new proxy prober.
    pub fn new(config: ProbeConfig) -> Self {
        Self { config }
    }

    /// Returns a reference to the probe configuration.
    pub fn config(&self) -> &ProbeConfig {
        &self.config
    }

    /// Probes a single proxy record, updating every measurable field in place.
    ///
    /// Always sets `last_checked`. Updates `status`, `protocol`,
    /// `supports_https`, `latency_ms`, `authentication_required` and
    /// `failure_count`.
    ///
    /// When `protocol` is `Some`, the probe tests the declared protocol. When
    /// `None`, it iterates through `protocol_detection_order` and sets the
    /// first protocol that succeeds.
    pub async fn probe(&self, record: &mut ProxyRecord) -> Result<()> {
        record.last_checked = Some(SystemTime::now());

        if let Some(ref protocol) = record.protocol {
            let result = self.probe_protocol(&record.host, record.port, protocol).await?;
            self.apply_result(record, result);
        } else {
            let result = self.detect_protocol(&record.host, record.port).await?;
            self.apply_result(record, result);
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
        let protocol = record
            .protocol
            .as_ref()
            .ok_or_else(|| ProxyError::Config("cannot validate destination without a known protocol on the record".to_string()))?;

        let authority = record.authority();
        let endpoint = Destination::from_authority(&authority, crate::core::ApplicationProtocol::Tcp)?;

        let _start = Instant::now();
        let stream = time::timeout(self.config.timeout, TcpStream::connect((&*endpoint.host_for_protocol(), endpoint.port)))
            .await
            .map_err(|_| ProxyError::Timeout("destination validation tcp connect"))?
            .map_err(|_| ProxyError::RouteUnavailable(format!("cannot connect to proxy for destination validation: {}", destination.authority())))?;

        let mut stream: Box<dyn AsyncProbeStream> = if matches!(protocol, ProxyProtocol::Https) {
            Box::new(wrap_tls(stream, &endpoint.host_for_protocol(), self.config.timeout, true).await?)
        } else {
            Box::new(stream)
        };

        match protocol {
            ProxyProtocol::Http | ProxyProtocol::Https => {
                send_http_connect(&mut *stream, destination, self.config.timeout, None).await?;
            }
            ProxyProtocol::Socks5 => {
                send_socks5_connect(&mut *stream, destination, self.config.timeout, None).await?;
            }
            ProxyProtocol::Socks4 | ProxyProtocol::Socks4a => {
                send_socks4_connect(&mut *stream, destination, self.config.timeout, None).await?;
            }
        }

        Ok(())
    }

    // ── internal helpers ──────────────────────────────────────────────

    fn apply_result(&self, record: &mut ProxyRecord, result: ProtocolProbeResult) {
        if result.success {
            record.status = ProxyRuntimeStatus::Ok;
            if result.protocol.is_some() {
                record.protocol = result.protocol;
            }
            record.latency_ms = Some(result.latency_ms);
            if result.supports_https.is_some() {
                record.supports_https = result.supports_https;
            }
            record.authentication_required = Some(false);
            record.failure_count = 0;
        } else if result.authentication_required {
            record.status = ProxyRuntimeStatus::AuthenticationRequired;
            record.authentication_required = Some(true);
            record.failure_count += 1;
        } else {
            record.status = ProxyRuntimeStatus::Ko;
            record.failure_count += 1;
        }
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
            ProxyProtocol::Socks4 | ProxyProtocol::Socks4a => self.probe_socks4(host, port).await,
        }
    }

    async fn detect_protocol(
        &self,
        host: &str,
        port: u16,
    ) -> Result<ProtocolProbeResult> {
        for protocol in &self.config.protocol_detection_order {
            let result = self.probe_protocol(host, port, protocol).await?;
            if result.success {
                return Ok(ProtocolProbeResult {
                    success: true,
                    latency_ms: result.latency_ms,
                    authentication_required: false,
                    supports_https: result.supports_https,
                    protocol: Some(protocol.clone()),
                });
            }
        }
        Ok(ProtocolProbeResult {
            success: false,
            latency_ms: 0,
            authentication_required: false,
            supports_https: None,
            protocol: None,
        })
    }

    async fn probe_http(
        &self,
        host: &str,
        port: u16,
        tls: bool,
    ) -> Result<ProtocolProbeResult> {
        let (tcp_latency, stream) = tcp_connect(host, port, self.config.timeout).await?;
        let mut latency_ms = tcp_latency;

        let mut stream: Box<dyn AsyncProbeStream> = if tls {
            Box::new(wrap_tls(stream, host, self.config.timeout, true).await?)
        } else {
            Box::new(stream)
        };

        let http_probe = match &self.config.http_probe_url {
            Some(url) => parse_probe_url(url, 80)?,
            None => {
                return Ok(ProtocolProbeResult {
                    success: true,
                    latency_ms,
                    authentication_required: false,
                    supports_https: None,
                    protocol: None,
                });
            }
        };

        let connect_start = Instant::now();
        let result = send_http_connect(
            &mut *stream,
            &http_probe,
            self.config.timeout,
            None,
        )
        .await;

        let https_supported = self.check_https_support(host, port, tls).await;

        match result {
            Ok(()) => {
                latency_ms += connect_start.elapsed().as_millis() as u64;
                Ok(ProtocolProbeResult {
                    success: true,
                    latency_ms,
                    authentication_required: false,
                    supports_https: https_supported,
                    protocol: None,
                })
            }
            Err(ref error) if is_auth_required(error) => Ok(ProtocolProbeResult {
                success: false,
                latency_ms,
                authentication_required: true,
                supports_https: None,
                protocol: None,
            }),
            Err(_) => Ok(ProtocolProbeResult {
                success: false,
                latency_ms,
                authentication_required: false,
                supports_https: None,
                protocol: None,
            }),
        }
    }

    async fn probe_socks5(&self, host: &str, port: u16) -> Result<ProtocolProbeResult> {
        let (tcp_latency, mut stream) = tcp_connect(host, port, self.config.timeout).await?;

        let probe = match &self.config.http_probe_url {
            Some(url) => parse_probe_url(url, 80)?,
            None => Destination::host_port("example.com", 80),
        };

        let connect_start = Instant::now();
        let result = send_socks5_connect(&mut stream, &probe, self.config.timeout, None).await;

        let latency_ms = tcp_latency + connect_start.elapsed().as_millis() as u64;

        match result {
            Ok(()) => {
                let https_supported = self.check_https_support(host, port, false).await;
                Ok(ProtocolProbeResult {
                    success: true,
                    latency_ms,
                    authentication_required: false,
                    supports_https: https_supported,
                    protocol: None,
                })
            }
            Err(ref error) if is_socks5_auth_required(error) => Ok(ProtocolProbeResult {
                success: false,
                latency_ms,
                authentication_required: true,
                supports_https: None,
                protocol: None,
            }),
            Err(_) => Ok(ProtocolProbeResult {
                success: false,
                latency_ms,
                authentication_required: false,
                supports_https: None,
                protocol: None,
            }),
        }
    }

    async fn probe_socks4(&self, host: &str, port: u16) -> Result<ProtocolProbeResult> {
        let (tcp_latency, mut stream) = tcp_connect(host, port, self.config.timeout).await?;

        let probe = match &self.config.http_probe_url {
            Some(url) => parse_probe_url(url, 80)?,
            None => Destination::host_port("example.com", 80),
        };

        let connect_start = Instant::now();
        let result = send_socks4_connect(&mut stream, &probe, self.config.timeout, None).await;

        let latency_ms = tcp_latency + connect_start.elapsed().as_millis() as u64;

        match result {
            Ok(()) => {
                let https_supported = self.check_https_support(host, port, false).await;
                Ok(ProtocolProbeResult {
                    success: true,
                    latency_ms,
                    authentication_required: false,
                    supports_https: https_supported,
                    protocol: None,
                })
            }
            Err(_) => Ok(ProtocolProbeResult {
                success: false,
                latency_ms,
                authentication_required: false,
                supports_https: None,
                protocol: None,
            }),
        }
    }

    async fn check_https_support(&self, host: &str, port: u16, tls: bool) -> Option<bool> {
        let https_url = self.config.https_probe_url.as_ref()?;
        let probe = parse_probe_url(https_url, 443).ok()?;

        let (_tcp_latency, stream) = tcp_connect(host, port, self.config.timeout).await.ok()?;
        let mut stream: Box<dyn AsyncProbeStream> = if tls {
            Box::new(wrap_tls(stream, host, self.config.timeout, true).await.ok()?)
        } else {
            Box::new(stream)
        };

        send_http_connect(&mut *stream, &probe, self.config.timeout, None)
            .await
            .ok()?;

        Some(true)
    }
}

// ── Protocol handshake helpers ───────────────────────────────────────

/// Opens a TCP connection to the proxy and measures its latency.
async fn tcp_connect(
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

        let mut buffer = [0u8; 1024];
        let mut pos = 0;
        loop {
            let n = stream.read(&mut buffer[pos..]).await?;
            if n == 0 {
                return Err(ProxyError::Protocol(
                    "http proxy closed before completing response".to_string(),
                ));
            }
            pos += n;
            if pos >= 4 && buffer[..pos].ends_with(b"\r\n\r\n") {
                break;
            }
            if pos >= buffer.len() {
                return Err(ProxyError::Protocol(
                    "http proxy response headers exceeded 1 KiB".to_string(),
                ));
            }
        }

        let response = &buffer[..pos];
        let line_end = response
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| ProxyError::Protocol("missing http status line".to_string()))?;
        let line = std::str::from_utf8(&response[..line_end])
            .map_err(|_| ProxyError::Protocol("http status line is not utf-8".to_string()))?;
        let status = line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or_else(|| ProxyError::Protocol("invalid http status line".to_string()))?;

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
            return Err(ProxyError::Protocol("socks5: invalid reply version".to_string()));
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
                return Err(ProxyError::Protocol("socks5: unknown address type".to_string()));
            }
        };
        let mut rest = vec![0u8; addr_len + 2];
        stream.read_exact(&mut rest).await?;

        Ok(())
    })
    .await
    .map_err(|_| ProxyError::Timeout("socks5 connect handshake"))?
}

/// Performs a SOCKS4/4a CONNECT handshake over an existing stream.
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
        let (ip_bytes, _has_hostname): ([u8; 4], bool) = match &destination.address {
            DestinationAddress::Ip(ip) => match ip {
                std::net::IpAddr::V4(v4) => (v4.octets(), false),
                std::net::IpAddr::V6(_) => {
                    return Err(ProxyError::Config(
                        "SOCKS4 does not support IPv6; use SOCKS5".to_string(),
                    ));
                }
            },
            DestinationAddress::Host(h) => {
                match h.parse::<std::net::Ipv4Addr>() {
                    Ok(v4) => (v4.octets(), false),
                    Err(_) => {
                        return Err(ProxyError::Config(
                            "SOCKS4a hostname forwarding not supported in probe; use SOCKS5 for hostname proxies".to_string(),
                        ));
                    }
                }
            }
        };

        let port = destination.port;
        let mut request = Vec::with_capacity(9);
        request.push(0x04);
        request.push(0x01);
        request.extend_from_slice(&port.to_be_bytes());
        request.extend_from_slice(&ip_bytes);
        request.push(0x00); // empty user-id

        stream.write_all(&request).await?;

        let mut reply = [0u8; 8];
        stream.read_exact(&mut reply).await?;
        if reply[0] != 0x00 {
            return Err(ProxyError::Protocol("socks4: invalid reply null byte".to_string()));
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
async fn wrap_tls(
    stream: TcpStream,
    host: &str,
    handshake_timeout: Duration,
    verify: bool,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
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

/// Parses a probe URL into a `Destination`.
fn parse_probe_url(url: &str, default_port: u16) -> Result<Destination> {
    let url = url.trim();
    let (rest, port) = if let Some((host_part, port_str)) = url.rsplit_once(':') {
        if let Ok(p) = port_str.trim_end_matches('/').parse::<u16>() {
            (host_part.trim_start_matches("http://").trim_start_matches("https://"), p)
        } else {
            (url.trim_start_matches("http://").trim_start_matches("https://"), default_port)
        }
    } else {
        (url.trim_start_matches("http://").trim_start_matches("https://"), default_port)
    };

    let rest = rest.trim_end_matches('/');
    Ok(Destination::host_port(rest.to_string(), port))
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

// ── Internal types ─────────────────────────────────────────────────

/// Trait alias for a boxed async read/write stream used during probing.
trait AsyncProbeStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncProbeStream for T {}

/// Intermediate probe result that gets applied to a `ProxyRecord`.
struct ProtocolProbeResult {
    success: bool,
    latency_ms: u64,
    authentication_required: bool,
    supports_https: Option<bool>,
    protocol: Option<ProxyProtocol>,
}
