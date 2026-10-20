//! Request-owned deadlines, work tracking and replay policy for localized proxies.

use crate::{PROXY_SEARCH_DEFAULT_EXTENSION, PROXY_SEARCH_DEFAULT_TARGET};

use super::{Destination, ProxyError, ProxyRequestDelivery, ProxySelectionProvenance, Result};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{watch, OwnedMutexGuard, Semaphore},
    time::Instant,
};

/// Time limits shared by selection, connection and HTTP retries.
#[derive(Clone, Debug)]
pub struct ProxySearchLimits {
    /// Time during which new work may begin.
    pub target: Duration,
    /// Grace period for an already engaged business attempt only.
    pub extension: Duration,
}
impl Default for ProxySearchLimits {
    fn default() -> Self {
        Self {
            target: PROXY_SEARCH_DEFAULT_TARGET,
            extension: PROXY_SEARCH_DEFAULT_EXTENSION,
        }
    }
}

#[derive(Default, Debug)]
struct Work {
    technical: HashSet<(bool, String)>,
    endpoints: HashSet<String>,
    attempts: HashSet<(String, String)>,
    provenance: HashMap<(String, String), ProxySelectionProvenance>,
    discovery: Option<OwnedMutexGuard<()>>,
    reused_origin: bool,
    active: bool,
    accepted: bool,
    prechecks_completed: usize,
    prechecks_reachable: usize,
    validations_completed: usize,
    validations_eligible: usize,
}

/// Cloneable request-local search. It is never persisted or sent over the wire.
#[derive(Clone, Debug)]
pub struct ProxySearchContext {
    pub(crate) continuation: Option<Arc<super::proxy_inventory::preparation::ProbeContinuation>>,
    pub(crate) speculative: bool,
    pub(crate) preparation_exclusions: HashSet<String>,
    started: Instant,
    limits: ProxySearchLimits,
    replayable: bool,
    target_key: Option<String>,
    work: Arc<Mutex<Work>>,
    accepted: watch::Sender<bool>,
    validation_failures: watch::Sender<usize>,
    /// Per-search TCP concurrency; waiting consumes the target deadline.
    pub(crate) tcp_slots: Arc<Semaphore>,
    /// Per-search full-validation concurrency.
    pub(crate) validation_slots: Arc<Semaphore>,
    /// Per-search provider concurrency.
    pub(crate) provider_slots: Arc<Semaphore>,
}

tokio::task_local! { static SEARCH: Option<ProxySearchContext>; }

impl Default for ProxySearchContext {
    fn default() -> Self {
        Self::new(ProxySearchLimits::default())
    }
}

impl ProxySearchContext {
    /// Creates one monotonic budget for a logical business call.
    ///
    /// # Arguments
    /// - `limits`: Duration limits, unchanged across retries.
    pub fn new(limits: ProxySearchLimits) -> Self {
        Self {
            continuation: Some(Arc::new(
                super::proxy_inventory::preparation::ProbeContinuation::new(
                    Instant::now() + limits.target,
                ),
            )),
            speculative: false,
            preparation_exclusions: HashSet::new(),
            started: Instant::now(),
            limits,
            replayable: false,
            target_key: None,
            work: Arc::new(Mutex::new(Work::default())),
            accepted: watch::channel(false).0,
            validation_failures: watch::channel(0).0,
            tcp_slots: Arc::new(Semaphore::new(8)),
            validation_slots: Arc::new(Semaphore::new(2)),
            provider_slots: Arc::new(Semaphore::new(2)),
        }
    }
    /// Keeps worker ownership outside the context captured by retained probes.
    pub(crate) fn without_continuation(&self) -> Self {
        let mut scoped = self.clone();
        scoped.continuation = None;
        scoped.target_key = None;
        scoped
    }

    /// Authorizes replay of this operation when its body can be reproduced.
    ///
    /// # Arguments
    /// - `replayable`: Explicit operation-level guarantee, never inferred from status.
    pub fn with_replayable(mut self, replayable: bool) -> Self {
        self.replayable = replayable;
        self
    }
    /// Whether the caller explicitly authorized replay of modifying operations.
    pub fn replayable(&self) -> bool {
        self.replayable
    }
    /// Stable in-process identity for adapters bound to this logical call.
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.work) as usize
    }

    /// Attaches business evidence to an exact endpoint and origin before engagement.
    ///
    /// # Arguments
    /// - `endpoint`: Selected proxy authority.
    /// - `destination`: Origin for which the evidence was observed.
    /// - `provenance`: Type of prior business evidence, independent of probe health.
    pub(crate) fn mark_selection(
        &self,
        endpoint: &str,
        destination: &Destination,
        provenance: ProxySelectionProvenance,
    ) {
        self.work.lock().unwrap().provenance.insert(
            (
                endpoint.into(),
                super::ProxyDestinationPreference::destination_key(destination),
            ),
            provenance,
        );
    }

    /// Returns evidence for this exact route, defaulting to discovery.
    ///
    /// # Arguments
    /// - `endpoint`: Concrete proxy authority.
    /// - `destination`: Origin whose evidence must match.
    pub(crate) fn selection_provenance(
        &self,
        endpoint: &str,
        destination: &Destination,
    ) -> ProxySelectionProvenance {
        self.work
            .lock()
            .unwrap()
            .provenance
            .get(&(
                endpoint.into(),
                super::ProxyDestinationPreference::destination_key(destination),
            ))
            .copied()
            .unwrap_or(ProxySelectionProvenance::Discovery)
    }

    /// Retains discovery ownership until it is claimed by a selection token.
    ///
    /// # Arguments
    /// - `guard`: Exclusive ownership for this destination and country set.
    pub(crate) fn hold_discovery(&self, guard: OwnedMutexGuard<()>) {
        self.work.lock().unwrap().discovery = Some(guard);
    }

    /// Transfers ownership to a token, or releases it on cancellation or supersession.
    pub(crate) fn take_discovery(&self) -> Option<OwnedMutexGuard<()>> {
        self.work.lock().unwrap().discovery.take()
    }

    /// Connection envelope for the exact origin attempt, including destination TLS.
    pub fn connection_timeout(&self) -> Duration {
        Duration::from_secs(if self.work.lock().unwrap().reused_origin {
            60
        } else {
            15
        })
    }
    /// Remaining hard-deadline time for a business attempt already engaged.
    pub fn origin_remaining(&self) -> Duration {
        (self.started + self.limits.target + self.limits.extension)
            .saturating_duration_since(Instant::now())
    }
    /// Scopes redirect hops by actual URL while retaining the common deadline.
    ///
    /// # Arguments
    /// - `url`: Actual request URL, distinguishing redirects from retries.
    pub fn for_url(&self, url: &str) -> Self {
        let mut scoped = self.clone();
        scoped.target_key = Some(url.to_string());
        scoped
    }
    /// Returns the explicitly scoped context, if this execution supports correlation.
    pub fn current() -> Option<Self> {
        SEARCH.try_with(Clone::clone).ok().flatten()
    }
    /// Propagates this context through a future without spawning background work.
    ///
    /// # Arguments
    /// - `future`: Request execution, including its redirects and retries.
    pub async fn scope<F: Future>(&self, future: F) -> F::Output {
        SEARCH.scope(Some(self.clone()), future).await
    }
    pub(crate) async fn unscoped<F: Future>(future: F) -> F::Output {
        SEARCH.scope(None, future).await
    }
    /// Whether another operation can begin before the target deadline.
    pub fn can_start(&self) -> bool {
        Instant::now() < self.started + self.limits.target && !self.work.lock().unwrap().accepted
    }
    /// Time remaining for technical discovery; grace never applies here.
    pub fn remaining(&self) -> Duration {
        (self.started + self.limits.target).saturating_duration_since(Instant::now())
    }
    /// Bounds discovery and resource waits to the target deadline.
    ///
    /// # Arguments
    /// - `future`: Work cancelled when this caller loses interest.
    ///
    /// # Errors
    /// Returns a search timeout when the target deadline expires.
    pub async fn technical<F: Future>(&self, future: F) -> Result<F::Output> {
        if !self.can_start() {
            return Err(ProxyError::Timeout("proxy search target deadline"));
        }
        tokio::time::timeout_at(self.started + self.limits.target, future)
            .await
            .map_err(|_| ProxyError::Timeout("proxy search target deadline"))
    }
    /// Bounds an engaged origin attempt, including its classification, to the hard deadline.
    ///
    /// # Arguments
    /// - `future`: Transport or response observation for the selected endpoint.
    ///
    /// # Errors
    /// Returns a timeout; no grace is available without an active attempt.
    pub async fn origin<F: Future>(&self, future: F) -> Result<F::Output> {
        let (active, completed) = {
            let work = self.work.lock().unwrap();
            (work.active, work.accepted)
        };
        if completed
            || (Instant::now() >= self.started + self.limits.target && !active)
            || Instant::now() >= self.started + self.limits.target + self.limits.extension
        {
            return Err(ProxyError::Timeout("proxy search origin deadline"));
        }
        let mut accepted = self.accepted.subscribe();
        tokio::pin!(future);
        tokio::select! {
            result = &mut future => Ok(result),
            _ = async { let _ = accepted.wait_for(|accepted| *accepted).await; } => Ok(future.await),
            _ = tokio::time::sleep_until(self.started + self.limits.target) => {
                let (active, completed) = { let work = self.work.lock().unwrap(); (work.active, work.accepted) };
                if completed { return Ok(future.await); }
                if !active { return Err(ProxyError::Timeout("proxy search target deadline")); }
                tokio::select! {
                    result = &mut future => Ok(result),
                    _ = async { let _ = accepted.wait_for(|accepted| *accepted).await; } => Ok(future.await),
                    _ = tokio::time::sleep_until(self.started + self.limits.target + self.limits.extension) => Err(ProxyError::Timeout("proxy search origin deadline")),
                }
            }
        }
    }
    pub(crate) fn reserve(&self, validation: bool, key: &str) -> bool {
        if !self.can_start() {
            return false;
        }
        let mut work = self.work.lock().unwrap();
        let key = (validation, key.to_string());
        if work.technical.contains(&key) {
            return false;
        }
        work.technical.insert(key);
        true
    }
    /// Allows a later selection to resume a phase cancelled before completion.
    pub(crate) fn release_incomplete_work(&self, validation: bool, key: &str) {
        self.work
            .lock()
            .unwrap()
            .technical
            .remove(&(validation, key.to_string()));
    }
    /// Caps connection phases only for endpoints engaged by this localized search.
    ///
    /// # Arguments
    /// - `endpoint`: Concrete proxy authority, checked against this search's attempts.
    /// - `configured`: Ordinary transport limits; shorter limits and idle policy are retained.
    pub(crate) fn connection_timeouts(
        &self,
        endpoint: &str,
        configured: &super::TimeoutConfig,
    ) -> super::TimeoutConfig {
        let mut timeouts = configured.clone();
        let work = self.work.lock().unwrap();
        if work.endpoints.contains(endpoint) {
            timeouts.connect_ms =
                timeouts
                    .connect_ms
                    .min(if work.reused_origin { 30_000 } else { 3_000 });
            timeouts.handshake_ms =
                timeouts
                    .handshake_ms
                    .min(if work.reused_origin { 30_000 } else { 5_000 });
        }
        timeouts
    }
    pub(crate) fn completed_work(&self, validation: bool, usable: bool) {
        let mut work = self.work.lock().unwrap();
        if validation {
            work.validations_completed += 1;
            work.validations_eligible += usize::from(usable);
        } else {
            work.prechecks_completed += 1;
            work.prechecks_reachable += usize::from(usable);
        }
    }
    /// Logs bounded search counters without URLs, cookies or request bodies.
    /// Reservations include queued and completed work; cancelled reservations are released.
    ///
    /// # Arguments
    /// - `stage`: Static lifecycle description supplied by the orchestration layer.
    pub fn log_progress(&self, stage: &'static str) {
        let work = self.work.lock().unwrap();
        let prechecks_reserved = work
            .technical
            .iter()
            .filter(|(validation, _)| !validation)
            .count();
        let validations_reserved = work.technical.len() - prechecks_reserved;
        tracing::info!(
            search_id = self.id(),
            stage,
            elapsed_ms = self.started.elapsed().as_millis(),
            remaining_ms = self.remaining().as_millis(),
            prechecks_reserved,
            prechecks_completed = work.prechecks_completed,
            prechecks_reachable = work.prechecks_reachable,
            validations_reserved,
            validations_completed = work.validations_completed,
            validations_eligible = work.validations_eligible,
            business_endpoints = work.endpoints.len(),
            origin_active = work.active,
            reused_origin = work.reused_origin,
            accepted = work.accepted,
            "localized proxy search progress"
        );
    }
    pub(crate) fn failed_validation(&self) {
        self.validation_failures.send_modify(|count| *count += 1);
    }
    pub(crate) fn validation_failures(&self) -> watch::Receiver<usize> {
        self.validation_failures.subscribe()
    }
    pub(crate) fn can_discover(&self) -> bool {
        self.can_start()
    }
    /// Records an endpoint before engaging a real attempt, never during grace.
    ///
    /// # Arguments
    /// - `endpoint`: Concrete proxy endpoint.
    /// - `destination`: Exact origin; redirect hops retain independent eligibility.
    ///
    /// # Errors
    /// Returns an error on repeated attempts or an exhausted target deadline.
    pub fn start_origin(&self, endpoint: &str, destination: &Destination) -> Result<()> {
        if !self.can_start() {
            return Err(ProxyError::Timeout("proxy search target deadline"));
        }
        let mut work = self.work.lock().unwrap();
        if work.attempts.contains(&(
            endpoint.to_string(),
            self.target_key
                .clone()
                .unwrap_or_else(|| destination.to_string()),
        )) {
            return Err(ProxyError::RouteUnavailable(
                "proxy search repeated endpoint".into(),
            ));
        }
        work.endpoints.insert(endpoint.to_string());
        work.attempts.insert((
            endpoint.to_string(),
            self.target_key
                .clone()
                .unwrap_or_else(|| destination.to_string()),
        ));
        work.active = true;
        work.reused_origin = work
            .provenance
            .get(&(
                endpoint.into(),
                super::ProxyDestinationPreference::destination_key(destination),
            ))
            .is_some_and(|source| *source != ProxySelectionProvenance::Discovery);
        tracing::info!(search_id = self.id(), proxy = endpoint, %destination,
            provenance = ?work.provenance.get(&(endpoint.into(), super::ProxyDestinationPreference::destination_key(destination))).copied().unwrap_or(ProxySelectionProvenance::Discovery),
            "localized proxy origin selected");
        Ok(())
    }
    pub(crate) fn attempted(&self, endpoint: &str, destination: Option<&Destination>) -> bool {
        let work = self.work.lock().unwrap();
        destination.is_some_and(|destination| {
            work.attempts.contains(&(
                endpoint.to_string(),
                self.target_key
                    .clone()
                    .unwrap_or_else(|| destination.to_string()),
            ))
        })
    }
    /// Completes a failed/rejected attempt without resetting deadlines or counters.
    pub fn end_attempt(&self) {
        let mut work = self.work.lock().unwrap();
        work.active = false;
        work.discovery.take();
    }
    /// Closes discovery after an accepted final response; transfer deadlines are separate.
    pub fn accept(&self) {
        let mut work = self.work.lock().unwrap();
        work.accepted = true;
        work.active = false;
        self.accepted.send_replace(true);
    }
}

/// Conservative replay rule, independent of destination health classification.
///
/// # Arguments
/// - `method`: Actual HTTP method.
/// - `replayable`: Explicit operation-level authorization and reproducible body.
/// - `delivery`: Evidence at the failing boundary, never inferred from an opaque error.
/// - `rejected`: Explicit destination rejection, or a transport failure before headers.
pub fn proxy_retry_allowed(
    method: &str,
    replayable: bool,
    delivery: ProxyRequestDelivery,
    rejected: bool,
) -> bool {
    if delivery == ProxyRequestDelivery::NotSent {
        return true;
    }
    if delivery == ProxyRequestDelivery::ResponseReceived && !rejected {
        return false;
    }
    replayable || method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD")
}
