//! HTTP probe exchanges, complementary target policies and URL parsing.

use crate::core::http::response_classifier::{
    HttpStatusProxyResponseClassifier, ProxyBodyRequirement, ProxyBodyState, ProxyResponseDecision,
    ProxyResponsePolicy, ProxyResponseView, MAX_PROXY_RESPONSE_PREFIX,
    PROXY_RESPONSE_INSPECTION_TIMEOUT,
};
use crate::core::proxy_probe_target::ProbeTargetPolicy;
use crate::core::{Destination, ProxyError, ProxyProtocol, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time;

use super::results::{CapabilityProbeAttempt, CapabilityProbeTarget};
use super::ProxyProbe;
/// Parsed HTTP complementary target for absolute-form forwarding requests.
pub(super) struct HttpProbeTarget {
    /// Final destination address used by the probe connection.
    pub(super) destination: Destination,
    /// Serialized HTTP request-target, absolute-form for forwarding or origin-form after CONNECT.
    pub(super) request_target: String,
    /// Host header value, including a non-default port when present.
    pub(super) host_header: String,
}
/// Parsed HTTPS complementary target for CONNECT, TLS and origin-form HTTP requests.
pub(super) struct HttpsProbeTarget {
    /// Final destination address used by the probe connection.
    pub(super) destination: Destination,
    /// Destination hostname used for the TLS server name and certificate validation.
    pub(super) server_name: String,
    /// Serialized HTTP request-target, absolute-form for forwarding or origin-form after CONNECT.
    pub(super) request_target: String,
    /// Host header value, including a non-default port when present.
    pub(super) host_header: String,
}

/// Sends an HTTP CONNECT handshake over an existing stream.
///
/// Returns `Ok(())` on a 2xx response. Returns an error carrying the status
/// code on 407 or other failures.
///
/// # Arguments
/// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
/// * `timeout` - Maximum duration allowed for this network phase.
/// * `_credentials` - Reserved credentials; authentication is not sent by this probe implementation.
///
/// # Errors
///
/// Returns timeout or I/O errors, malformed-response errors, or upstream rejection for a non-2xx CONNECT status.
pub(super) async fn send_http_connect<S>(
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

/// Reads the HTTP status line and consumes the remaining response headers.
///
/// # Arguments
/// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
///
/// # Errors
///
/// Returns I/O or protocol errors on incomplete, oversized or invalid response headers or status lines.
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

/// Formats a probe target for observations without exposing user information or query data.
///
/// # Arguments
/// * `value` - Absolute probe URL whose credentials, query and fragment are removed.
fn sanitized_probe_target(value: &str) -> Option<String> {
    url::Url::parse(value).ok().map(|mut url| {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        url.to_string()
    })
}

/// Returns the URL path and query for an origin-form request, or `/` if parsing fails.
///
/// # Arguments
/// * `value` - Absolute probe URL from which the path and query are extracted.
pub(super) fn probe_origin_target(value: &str) -> String {
    url::Url::parse(value)
        .map(|url| match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_string(),
        })
        .unwrap_or_else(|_| "/".to_string())
}

/// Reads headers with a 64 KiB block limit and an eight-response parsing budget.
///
/// # Arguments
/// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
///
/// # Errors
///
/// Returns I/O or protocol errors on incomplete or invalid headers, oversized blocks or too many interim responses.
async fn read_probe_headers<S: AsyncRead + Unpin + ?Sized>(
    stream: &mut S,
) -> Result<(u16, http::HeaderMap)> {
    for _ in 0..8 {
        let mut raw = Vec::new();
        while !raw.ends_with(b"\r\n\r\n") {
            if raw.len() >= 64 * 1024 {
                return Err(ProxyError::Protocol(
                    "probe response headers exceed 64 KiB".into(),
                ));
            }
            raw.push(stream.read_u8().await?);
        }
        let head = std::str::from_utf8(&raw)
            .map_err(|_| ProxyError::Protocol("probe response headers are not UTF-8".into()))?;
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|status| status.parse::<u16>().ok())
            .filter(|status| (100..600).contains(status))
            .ok_or_else(|| ProxyError::Protocol("invalid probe HTTP status".into()))?;
        let mut headers = http::HeaderMap::new();
        for line in lines.filter(|line| !line.is_empty()) {
            let (name, value) = line
                .split_once(':')
                .ok_or_else(|| ProxyError::Protocol("invalid probe header".into()))?;
            let name = http::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ProxyError::Protocol("invalid probe header name".into()))?;
            let value = http::header::HeaderValue::from_str(value.trim())
                .map_err(|_| ProxyError::Protocol("invalid probe header value".into()))?;
            headers.append(name, value);
        }
        if status >= 200 || status == 101 {
            return Ok((status, headers));
        }
    }
    Err(ProxyError::Protocol(
        "too many informational probe responses".into(),
    ))
}

/// Parses an HTTP probe target into an absolute request-target and Host value.
///
/// # Arguments
/// * `url` - Complementary probe URL used for this measurement or observation.
/// * `default_port` - Port hint selecting the inferred scheme: 443 for HTTPS, otherwise HTTP.
///
/// # Errors
///
/// Returns a configuration error for an invalid URL, non-HTTP scheme or absent host.
pub(super) fn parse_http_probe_url(url: &str, default_port: u16) -> Result<HttpProbeTarget> {
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

/// Parses an HTTPS probe target into tunnel, TLS and origin-form HTTP metadata.
///
/// # Arguments
/// * `url` - Complementary probe URL used for this measurement or observation.
///
/// # Errors
///
/// Returns a configuration error for an invalid URL, non-HTTPS scheme or absent host.
pub(super) fn parse_https_probe_url(url: &str) -> Result<HttpsProbeTarget> {
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

/// Parses a probe URL, inferring a missing scheme from the default-port hint.
///
/// # Arguments
/// * `url` - Complementary probe URL used for this measurement or observation.
/// * `default_port` - Port hint selecting the inferred scheme: 443 for HTTPS, otherwise HTTP.
///
/// # Errors
///
/// Returns a configuration error for blank or syntactically invalid URL input.
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

impl ProxyProbe {
    /// Checks the newest acceptance observation matching the target and current probe configuration.
    ///
    /// # Arguments
    /// * `url` - Complementary probe URL used for this measurement or observation.
    pub(super) fn target_accepted(&self, url: &str) -> bool {
        let target = sanitized_probe_target(url);
        self.observations
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|observation| {
                observation.context.phase == crate::core::ProxyObservationPhase::ProbeTargetAccepted
                    && observation.context.target == target
                    && observation.context.configuration
                        == self.observation_configuration(Some(url))
            })
            .is_some_and(|observation| {
                observation.outcome == crate::core::ProxyObservationOutcome::Succeeded
            })
    }

    /// Runs a single HTTP exchange against a configured complementary target through the endpoint.
    ///
    /// # Arguments
    /// * `host` - Hostname or IP address to resolve or connect to, without a port.
    /// * `port` - TCP port of the proxy endpoint.
    /// * `protocol` - Proxy transport protocol used for the endpoint handshake.
    /// * `target` - Destination or complementary target used by the probe.
    /// * `url` - Complementary probe URL used for this measurement or observation.
    pub(super) async fn probe_target_once(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
        target: CapabilityProbeTarget<'_>,
        url: &str,
    ) -> CapabilityProbeAttempt {
        let key = format!("{}|{}", self.configuration_id(), url);
        let policy = if url.starts_with("https:") {
            &self.config.https_policy
        } else {
            &self.config.http_policy
        };
        let Some(lease) = self
            .target_states
            .enter(key, &policy.suspension, self.attempt_deadline)
        else {
            return self.probe_endpoint(host, port, protocol).await;
        };
        let attempt = self
            .probe_capability_inner(host, port, protocol, target)
            .await;
        let sanitized = sanitized_probe_target(url);
        let status = self
            .observations
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|observation| {
                observation.context.phase == crate::core::ProxyObservationPhase::DestinationHttp
                    && observation.context.target == sanitized
                    && observation.context.configuration
                        == self.observation_configuration(Some(url))
            })
            .and_then(|observation| observation.http_status);
        let healthy = status.and_then(|status| {
            if self.target_accepted(url) || status < 500 {
                Some(true)
            } else if policy.unavailable_statuses.contains(&status) {
                Some(false)
            } else {
                None
            }
        });
        lease.finish(format!("{host}:{port}"), healthy);
        attempt
    }

    /// Sends the configured probe method and classifies the target response without following redirects.
    ///
    /// # Arguments
    /// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
    /// * `request_target` - HTTP request-target sent through the tested connection.
    /// * `host` - Host header value for the complementary target, including a non-default port.
    /// * `url` - Complementary probe URL used for this measurement or observation.
    /// * `policy` - Acceptance and suspension policy for the complementary target.
    /// * `protocol` - Proxy transport protocol used for the endpoint handshake.
    ///
    /// # Errors
    ///
    /// Returns timeout, I/O or protocol errors, or HTTP-proxy authentication rejection.
    /// Target-policy rejection and classifier errors are recorded as observations, not returned as errors.
    pub(super) async fn exchange_probe<S: AsyncRead + AsyncWrite + Unpin + Send + ?Sized>(
        &self,
        stream: &mut S,
        request_target: &str,
        host: &str,
        url: &str,
        policy: &ProbeTargetPolicy,
        protocol: &ProxyProtocol,
    ) -> Result<u16> {
        let method = policy.method.as_str();
        let request = format!(
            "{method} {request_target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        );
        let (status, headers) = time::timeout(self.config.timeouts.proxy_handshake, async {
            stream.write_all(request.as_bytes()).await?;
            stream.flush().await?;
            read_probe_headers(stream).await
        })
        .await
        .map_err(|_| ProxyError::Timeout("complementary probe response headers"))??;
        if status == 407 && request_target.starts_with("http") {
            return Err(ProxyError::UpstreamRejected(
                "http forward returned status 407".into(),
            ));
        }
        self.observe(
            crate::core::ProxyObservationPhase::DestinationHttp,
            protocol,
            Some(url),
            true,
            Some(status),
        );
        let default = ProxyResponsePolicy(Arc::new(HttpStatusProxyResponseClassifier::default()));
        let handler = policy.response_policy.as_ref().unwrap_or(&default);
        let mut body = Vec::new();
        let mut body_state = ProxyBodyState::Unavailable;
        if let ProxyBodyRequirement::Prefix(limit) = handler.0.body_requirement() {
            let limit = limit.min(MAX_PROXY_RESPONSE_PREFIX);
            if method == "HEAD" || matches!(status, 101 | 204 | 304) {
                body_state = ProxyBodyState::Complete;
            } else {
                use crate::core::http::body_readers::{
                    ChunkedBodyReader, ContentLengthBodyReader, UntilEofBodyReader,
                };
                let chunked = headers
                    .get("transfer-encoding")
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| {
                        value
                            .split(',')
                            .any(|value| value.trim().eq_ignore_ascii_case("chunked"))
                    });
                let length = headers
                    .get("content-length")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<usize>().ok());
                let reader: Box<dyn AsyncRead + Send + Unpin + '_> = if chunked {
                    Box::new(ChunkedBodyReader::new(stream))
                } else if let Some(length) = length {
                    Box::new(ContentLengthBodyReader::new(stream, length))
                } else {
                    Box::new(UntilEofBodyReader::new(stream))
                };
                let mut prefix = reader.take(limit as u64);
                if matches!(
                    time::timeout(
                        PROXY_RESPONSE_INSPECTION_TIMEOUT,
                        prefix.read_to_end(&mut body)
                    )
                    .await,
                    Ok(Ok(_))
                ) {
                    body_state = if body.len() < limit {
                        ProxyBodyState::Complete
                    } else {
                        ProxyBodyState::Limited
                    };
                }
            }
        }
        let view = ProxyResponseView {
            method,
            requested_url: url,
            effective_url: url,
            redirect_urls: &[],
            status,
            headers: &headers,
            selection: None,
            body: &body,
            body_state,
        };
        let decision = handler.0.classify(&view).unwrap_or_else(|error| {
            tracing::warn!(%error, "complementary target handler failed; result remains inconclusive");
            ProxyResponseDecision::Inconclusive
        });
        self.observe(
            crate::core::ProxyObservationPhase::ProbeTargetAccepted,
            protocol,
            Some(url),
            matches!(decision, ProxyResponseDecision::Accepted),
            Some(status),
        );
        Ok(status)
    }
}
