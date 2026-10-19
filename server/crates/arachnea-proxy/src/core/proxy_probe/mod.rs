//! Proxy probe facade, observations and public probe orchestration.

use crate::core::dns_resolver::ProxyDnsResolver;
use crate::core::proxy_probe_target::{ProbeTargetDiagnostics, TargetStates};
use crate::core::{
    ProxyCapabilityStatus, ProxyError, ProxyProtocol, ProxyRecord, ProxyRuntimeStatus, Result,
};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::time;
use tokio::time::Instant;

use self::capabilities::tcp_connect;

mod capabilities;
mod config;
mod detection;
mod http;
mod results;
mod socks;
mod tls;
mod validation;

pub use config::{ProbeConfig, ProbeMode, ProbeTimeoutConfig};

/// Probes proxy records by connecting to the proxy endpoint and testing
/// protocol handshakes, HTTP forwarding, and HTTPS tunnelling.
///
/// Each probe updates the supplied [`ProxyRecord`] in place so that callers
/// never have to reconcile a separate report structure.
#[derive(Clone)]
pub struct ProxyProbe {
    configuration_identity: Option<String>,
    observations: Arc<Mutex<Vec<crate::core::ProxyObservation>>>,
    target_states: TargetStates,
    fallback_used: Arc<AtomicBool>,
    attempt_deadline: Option<Instant>,
    config: ProbeConfig,
    dns_resolver: ProxyDnsResolver,
}

impl ProxyProbe {
    /// Creates a new proxy prober.
    pub fn new(config: ProbeConfig) -> Self {
        Self {
            configuration_identity: None,
            observations: Arc::new(Mutex::new(Vec::new())),
            target_states: TargetStates::default(),
            fallback_used: Arc::new(AtomicBool::new(false)),
            attempt_deadline: None,
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

    /// Returns a versioned, non-secret identity for this probe configuration.
    pub fn configuration_id(&self) -> String {
        if let Some(identity) = &self.configuration_identity {
            return identity.clone();
        }
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", self.config).hash(&mut hash);
        format!("probe-v1:{:016x}", hash.finish())
    }

    /// Returns diagnostic state for a configured or fallback URL without starting work.
    ///
    /// # Arguments
    /// - `url`: Exact URL used by the probe.
    pub fn target_diagnostics(&self, url: &str) -> ProbeTargetDiagnostics {
        self.target_states
            .diagnostics(&format!("{}|{}", self.configuration_id(), url))
    }

    fn observation_configuration(&self, target: Option<&str>) -> String {
        use std::hash::{Hash, Hasher};
        if let Some(target) = target {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            target.hash(&mut hash);
            format!("{}:target:{:016x}", self.configuration_id(), hash.finish())
        } else {
            self.configuration_id()
        }
    }

    /// Invalidates legacy aggregate results when their probe context changes.
    ///
    /// # Arguments
    /// - `record`: Record whose contextual results must match this probe.
    pub(crate) fn prepare_context(&self, record: &mut ProxyRecord) {
        let identity = self.configuration_id();
        let key = format!("context:{identity}");
        if record.probe_attempts.contains_key(&key) {
            return;
        }
        record.probe_attempts.retain(|phase, _| phase == "tcp");
        record.probe_attempts.insert(key, 0);
        record.http_forwarding = ProxyCapabilityStatus::Unknown;
        record.https_tunnel = ProxyCapabilityStatus::Unknown;
        record.destination_tls = ProxyCapabilityStatus::Unknown;
        record.last_checked = None;
    }

    fn observe(
        &self,
        phase: crate::core::ProxyObservationPhase,
        protocol: &ProxyProtocol,
        target: Option<&str>,
        succeeded: bool,
        status: Option<u16>,
    ) {
        let configuration = self.observation_configuration(target);
        let target = target
            .and_then(|target| url::Url::parse(target).ok())
            .map(|mut url| {
                let _ = url.set_username("");
                let _ = url.set_password(None);
                url.set_query(None);
                url.set_fragment(None);
                url.to_string()
            });
        let completed_at = SystemTime::now();
        self.observations
            .lock()
            .expect("probe observation lock poisoned")
            .push(crate::core::ProxyObservation {
                context: crate::core::ProxyProbeContext {
                    phase,
                    protocol: Some(protocol.clone()),
                    method: match phase {
                        crate::core::ProxyObservationPhase::DestinationHttp
                        | crate::core::ProxyObservationPhase::ProbeTargetAccepted => Some(
                            if target
                                .as_deref()
                                .is_some_and(|url| url.starts_with("https:"))
                            {
                                self.config.https_policy.method.as_str()
                            } else {
                                self.config.http_policy.method.as_str()
                            }
                            .into(),
                        ),
                        _ => None,
                    },
                    target,
                    configuration,
                },
                outcome: if succeeded {
                    crate::core::ProxyObservationOutcome::Succeeded
                } else if phase == crate::core::ProxyObservationPhase::EndpointConnect {
                    crate::core::ProxyObservationOutcome::Failed {
                        scope: crate::core::ProxyFailureScope::Proxy,
                        cause: "endpoint_connection_failed".into(),
                    }
                } else {
                    crate::core::ProxyObservationOutcome::Inconclusive
                },
                completed_at,
                last_succeeded_at: succeeded.then_some(completed_at),
                http_status: status,
            });
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
        targeted.configuration_identity = Some(self.configuration_id());
        targeted.config.timeouts.attempt = self.config.timeouts.attempt.min(Duration::from_secs(8));
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
        self.validate_configuration()?;
        self.prepare_context(record);
        record.last_checked = Some(SystemTime::now());
        let mut scoped = self.clone();
        scoped.observations = Arc::new(Mutex::new(Vec::new()));
        scoped.fallback_used = Arc::new(AtomicBool::new(false));
        scoped.attempt_deadline = Some(Instant::now() + self.config.timeouts.attempt);
        let protocols = self.protocol_candidates(record);
        let result = time::timeout(
            self.config.timeouts.attempt,
            scoped.detect_protocol(record, &protocols),
        )
        .await;
        for observation in scoped.observations.lock().unwrap().drain(..) {
            if observation.context.protocol == record.protocol
                && observation.outcome == crate::core::ProxyObservationOutcome::Succeeded
            {
                match observation.context.phase {
                    crate::core::ProxyObservationPhase::ProxyTls => {
                        record.proxy_tls_certificate = ProxyCapabilityStatus::Available
                    }
                    crate::core::ProxyObservationPhase::DestinationTls => {
                        record.destination_tls = ProxyCapabilityStatus::Available
                    }
                    crate::core::ProxyObservationPhase::Tunnel
                        if observation
                            .context
                            .target
                            .as_deref()
                            .is_some_and(|target| target.starts_with("https:")) =>
                    {
                        record.https_tunnel = ProxyCapabilityStatus::Available
                    }
                    crate::core::ProxyObservationPhase::EndpointConnect => {
                        record.tcp_reachable = Some(true);
                        if record.status == ProxyRuntimeStatus::Unknown {
                            record.status = ProxyRuntimeStatus::Ok;
                        }
                    }
                    _ => {}
                }
            }
            record.record_observation(observation);
        }
        let result =
            result.map_err(|_| ProxyError::Timeout("complete proxy probe including fallback"))?;
        match result {
            Ok(probe_result) => {
                if let Some(error) = self.apply_result(record, probe_result) {
                    return Err(error);
                }
            }
            Err(error) => {
                return Err(error);
            }
        }

        Ok(())
    }
}
