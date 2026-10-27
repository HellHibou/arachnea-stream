//! On-demand, target-scoped diagnostic state. Countries never determine eligibility.

use super::http::response_classifier::ProxyResponsePolicy;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;

/// HTTP method used for a complementary probe. There is no automatic method fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeHttpMethod {
    /// Inspect response headers without a response body.
    Head,
    /// Allow a bounded response prefix for a content-aware policy.
    Get,
}
impl ProbeHttpMethod {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Head => "HEAD",
            Self::Get => "GET",
        }
    }
}

/// Adjustable suspicion and on-demand recovery thresholds.
#[derive(Clone, Debug)]
pub struct ProbeTargetSuspension {
    /// Independent endpoint indications required before complementary work is suspended.
    pub distinct_endpoints: usize,
    /// Validity window for service-unavailability indications.
    pub observation_window: Duration,
    /// Initial suspension and successive recovery backoffs; the last value is the cap.
    pub backoff: [Duration; 3],
}
impl Default for ProbeTargetSuspension {
    fn default() -> Self {
        Self {
            distinct_endpoints: crate::PROXY_PROBE_DEFAULT_SUSPENSION_DISTINCT_ENDPOINTS,
            observation_window: crate::PROXY_PROBE_DEFAULT_SUSPENSION_OBSERVATION_WINDOW,
            backoff: crate::PROXY_PROBE_DEFAULT_SUSPENSION_BACKOFF,
        }
    }
}

/// Policy for one optional complementary probe URL.
#[derive(Clone, Debug)]
pub struct ProbeTargetPolicy {
    /// Method used at this URL and its optional fallback.
    pub method: ProbeHttpMethod,
    /// Optional code policy; None means conservative 2xx acceptance.
    pub response_policy: Option<ProxyResponsePolicy>,
    /// Stable non-secret policy identity. Change it when custom handler behavior changes.
    pub identity: String,
    /// HTTP statuses treated as service-unavailability indications after transport succeeds.
    /// They create suspicion only, never a proxy or country exclusion.
    pub unavailable_statuses: Vec<u16>,
    /// One optional same-scheme fallback URL; no recursive fallback or new external default.
    pub fallback_url: Option<String>,
    /// Runtime-only, target-specific suspicion policy.
    pub suspension: ProbeTargetSuspension,
}
impl Default for ProbeTargetPolicy {
    fn default() -> Self {
        Self {
            method: ProbeHttpMethod::Get,
            response_policy: None,
            identity: crate::PROXY_PROBE_DEFAULT_POLICY_IDENTITY.into(),
            unavailable_statuses: crate::PROXY_PROBE_DEFAULT_UNAVAILABLE_STATUSES.to_vec(),
            fallback_url: None,
            suspension: ProbeTargetSuspension::default(),
        }
    }
}

/// Diagnostic state for one URL/configuration, without a global availability verdict.
#[derive(Clone, Debug, Default)]
pub struct ProbeTargetDiagnostics {
    /// Number of distinct recent endpoints reporting configured unavailability indications.
    pub distinct_failures: usize,
    /// Whether observations suggest degraded target service.
    pub suspected: bool,
    /// Remaining temporary suspension of complementary work only.
    pub suspended_for: Duration,
    /// Whether an on-demand recovery attempt currently has a subscriber.
    pub recovery_in_progress: bool,
}

#[derive(Default)]
struct Health {
    failures: VecDeque<(Instant, String)>,
    step: u32,
    generation: u64,
    until: Option<Instant>,
    recovering: bool,
    touched: Option<Instant>,
    window: Duration,
}

/// Shared runtime-only target observations; expired entries are pruned on demand.
#[derive(Clone, Default)]
pub(crate) struct TargetStates(Arc<Mutex<HashMap<String, Health>>>);

impl TargetStates {
    pub(crate) fn enter(
        &self,
        key: String,
        policy: &ProbeTargetSuspension,
        deadline: Option<Instant>,
    ) -> Option<TargetLease> {
        let mut states = self.0.lock().unwrap();
        let now = Instant::now();
        states.retain(|_, state| {
            state.recovering
                || state.until.is_some_and(|until| until > now)
                || state
                    .touched
                    .is_some_and(|at| now.duration_since(at) < Duration::from_secs(600))
        });
        if states.len() >= 256 && !states.contains_key(&key) {
            return None;
        }
        let state = states.entry(key.clone()).or_default();
        state.touched = Some(now);
        state.window = policy.observation_window;
        if state.until.is_some_and(|until| until > now) || state.recovering {
            return None;
        }
        let recovery = state.until.is_some();
        if recovery {
            state.recovering = true;
        }
        Some(TargetLease {
            states: self.clone(),
            key,
            recovery,
            finished: false,
            policy: policy.clone(),
            deadline,
            generation: state.generation,
        })
    }
    pub(crate) fn diagnostics(&self, key: &str) -> ProbeTargetDiagnostics {
        let mut states = self.0.lock().unwrap();
        let Some(state) = states.get_mut(key) else {
            return ProbeTargetDiagnostics::default();
        };
        let now = Instant::now();
        state
            .failures
            .retain(|(at, _)| now.duration_since(*at) <= state.window);
        ProbeTargetDiagnostics {
            distinct_failures: state.failures.len(),
            suspected: state.step > 0,
            suspended_for: state
                .until
                .map(|until| until.saturating_duration_since(now))
                .unwrap_or_default(),
            recovery_in_progress: state.recovering,
        }
    }
}

pub(crate) struct TargetLease {
    states: TargetStates,
    key: String,
    recovery: bool,
    finished: bool,
    policy: ProbeTargetSuspension,
    deadline: Option<Instant>,
    generation: u64,
}
impl TargetLease {
    /// True means a healthy response; false an explicit service indication; None is ambiguous.
    pub(crate) fn finish(mut self, endpoint: String, healthy: Option<bool>) {
        let mut states = self.states.0.lock().unwrap();
        let state = states.entry(self.key.clone()).or_default();
        if state.generation != self.generation {
            self.finished = true;
            return;
        }
        let now = Instant::now();
        state.touched = Some(now);
        if self.recovery || healthy == Some(true) {
            state.recovering = false;
        }
        if healthy == Some(true) {
            state.generation = state.generation.wrapping_add(1);
            state.failures.clear();
            state.step = 0;
            state.until = None;
        } else {
            state.failures.retain(|(at, old)| {
                now.duration_since(*at) <= self.policy.observation_window && old != &endpoint
            });
            if healthy == Some(false) {
                state.failures.push_back((now, endpoint));
            }
            if self.recovery || state.failures.len() >= self.policy.distinct_endpoints {
                if self.recovery || state.step == 0 {
                    state.step = (state.step + 1).min(3);
                    state.until = Some(now + self.policy.backoff[(state.step - 1) as usize]);
                }
            }
        }
        self.finished = true;
    }
}
impl Drop for TargetLease {
    fn drop(&mut self) {
        if self.recovery && !self.finished {
            if let Some(state) = self.states.0.lock().unwrap().get_mut(&self.key) {
                if state.generation != self.generation {
                    return;
                }
                state.recovering = false;
                if self
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
                {
                    state.step = (state.step + 1).min(3);
                    state.until =
                        Some(Instant::now() + self.policy.backoff[(state.step - 1) as usize]);
                }
            }
        }
    }
}
