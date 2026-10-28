//! Protocol candidates, detection order and capability aggregation.

use crate::core::{ProxyCapabilityStatus, ProxyError, ProxyProtocol, ProxyRecord, Result};

use super::config::ProbeMode;
use super::http::{parse_http_probe_url, parse_https_probe_url};
use super::results::{
    CapabilityProbeAttempt, CapabilityProbeTarget, ProtocolProbeResult, ProxyCapability,
};
use super::ProxyProbe;

impl ProxyProbe {
    /// Dispatches capability measurement to the requested HTTP or SOCKS protocol.
    ///
    /// # Arguments
    /// * `host` - Hostname or IP address to resolve or connect to, without a port.
    /// * `port` - TCP port of the proxy endpoint.
    /// * `protocol` - Proxy transport protocol used for the endpoint handshake.
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

    /// Orders declared protocols by configured priority, adding alternatives only in relaxed mode.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    pub(super) fn protocol_candidates(&self, record: &ProxyRecord) -> Vec<ProxyProtocol> {
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

    /// Tries protocol candidates until usable or terminal endpoint evidence is obtained.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    /// * `protocols` - Ordered proxy protocols to try for the endpoint.
    pub(super) async fn detect_protocol(
        &self,
        record: &ProxyRecord,
        protocols: &[ProxyProtocol],
    ) -> Result<ProtocolProbeResult> {
        let mut last_error = None;
        let mut last_result = None;
        let mut authentication_result = None;
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
                        return Ok(result);
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

    /// Measures HTTP forwarding and configured HTTPS capabilities over an HTTP(S) proxy.
    ///
    /// # Arguments
    /// * `host` - Hostname or IP address to resolve or connect to, without a port.
    /// * `port` - TCP port of the proxy endpoint.
    /// * `tls` - Whether to negotiate TLS with the proxy before sending the HTTP probe.
    async fn probe_http(&self, host: &str, port: u16, tls: bool) -> Result<ProtocolProbeResult> {
        let protocol = if tls {
            ProxyProtocol::Https
        } else {
            ProxyProtocol::Http
        };
        self.probe_capabilities(host, port, &protocol).await
    }

    /// Measures configured complementary capabilities through SOCKS5.
    ///
    /// # Arguments
    /// * `host` - Hostname or IP address to resolve or connect to, without a port.
    /// * `port` - TCP port of the proxy endpoint.
    async fn probe_socks5(&self, host: &str, port: u16) -> Result<ProtocolProbeResult> {
        self.probe_capabilities(host, port, &ProxyProtocol::Socks5)
            .await
    }

    /// Measures configured complementary capabilities through SOCKS4 or SOCKS4a.
    ///
    /// # Arguments
    /// * `host` - Hostname or IP address to resolve or connect to, without a port.
    /// * `port` - TCP port of the proxy endpoint.
    /// * `protocol` - Proxy transport protocol used for the endpoint handshake.
    async fn probe_socks4(
        &self,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
    ) -> Result<ProtocolProbeResult> {
        self.probe_capabilities(host, port, protocol).await
    }

    /// Aggregates configured HTTP and HTTPS complementary measurements for one endpoint.
    ///
    /// # Arguments
    /// * `host` - Hostname or IP address to resolve or connect to, without a port.
    /// * `port` - TCP port of the proxy endpoint.
    /// * `protocol` - Proxy transport protocol used for the endpoint handshake.
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
}
