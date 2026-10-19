//! Probe configuration, mode, timeouts and validation.

use crate::core::proxy_probe_target::{ProbeHttpMethod, ProbeTargetPolicy};
use crate::core::{ProxyError, ProxyProtocol, Result, PROXY_PROTOCOL_PRIORITY};
use std::time::Duration;
use tokio::time::Instant;

use super::ProxyProbe;

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
    /// HTTP request. When absent, [`crate::core::ProxyRecord::https_tunnel`] and
    /// [`crate::core::ProxyRecord::destination_tls`] remain unknown.
    pub https_probe_url: Option<String>,
    /// HTTP complementary-target policy. URLs are never followed as redirects.
    pub http_policy: ProbeTargetPolicy,
    /// HTTPS complementary-target policy; HEAD is the compatibility default.
    pub https_policy: ProbeTargetPolicy,
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
            http_policy: ProbeTargetPolicy::default(),
            https_policy: ProbeTargetPolicy {
                method: ProbeHttpMethod::Head,
                ..Default::default()
            },
            timeouts: ProbeTimeoutConfig::default(),
            mode: ProbeMode::Relaxed,
            protocol_detection_order: PROXY_PROTOCOL_PRIORITY.to_vec(),
        }
    }
}

impl ProxyProbe {
    pub(super) fn validate_configuration(&self) -> Result<()> {
        for (url, policy, scheme) in [
            (
                &self.config.http_probe_url,
                &self.config.http_policy,
                "http",
            ),
            (
                &self.config.https_probe_url,
                &self.config.https_policy,
                "https",
            ),
        ] {
            if policy.suspension.distinct_endpoints == 0
                || policy.suspension.observation_window.is_zero()
                || policy
                    .suspension
                    .backoff
                    .iter()
                    .any(|delay| delay.is_zero() || Instant::now().checked_add(*delay).is_none())
            {
                return Err(ProxyError::Config(
                    "probe suspension thresholds and representable delays must be positive".into(),
                ));
            }
            if policy.identity.trim().is_empty()
                || policy
                    .unavailable_statuses
                    .iter()
                    .any(|status| !(500..600).contains(status))
            {
                return Err(ProxyError::Config(
                    "probe policy requires an identity and 5xx service indications".into(),
                ));
            }
            if url.is_some() && url == &policy.fallback_url {
                return Err(ProxyError::Config(
                    "probe fallback must differ from its primary URL".into(),
                ));
            }
            for url in url.iter().chain(policy.fallback_url.iter()) {
                let parsed = url::Url::parse(url)
                    .map_err(|_| ProxyError::Config("invalid complementary probe URL".into()))?;
                if parsed.scheme() != scheme
                    || parsed.host_str().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                {
                    return Err(ProxyError::Config(
                        "probe URL must have the configured scheme and no credentials".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}
