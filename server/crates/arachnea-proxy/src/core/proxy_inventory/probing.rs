//! Shared cancellable probe work and candidate capability validation.

use std::sync::Arc;
use std::time::SystemTime;

use futures::future::{BoxFuture, WeakShared};
use futures::FutureExt;
use tokio::sync::Mutex;

use super::ProxyInventory;
use crate::core::{Destination, ProxyRecord, ProxyRuntimeStatus, ProxySearchContext};

use super::eligibility::{
    complementary_trial_allowed, epoch_millis, is_eligible, is_selection_candidate,
    needs_capability_probe, probe_is_expired, recent_probe_attempt,
};

pub(super) type SharedProbe = WeakShared<BoxFuture<'static, Option<ProxyRecord>>>;

#[cfg(feature = "performance-diagnostics")]
struct SharedExecution {
    key: String,
    operation_id: tokio::task::Id,
    completed: bool,
    started: std::time::Instant,
}

#[cfg(feature = "performance-diagnostics")]
impl Drop for SharedExecution {
    fn drop(&mut self) {
        tracing::debug!(work = %self.key, completed = self.completed,
            operation_id = %self.operation_id,
            elapsed_ms = self.started.elapsed().as_millis(),
            elapsed_us = self.started.elapsed().as_micros(),
            "finished shared proxy operation");
    }
}

/// Releases unfinished request-local work when a candidate pipeline is cancelled.
struct SearchReservation {
    search: ProxySearchContext,
    key: String,
    validation: bool,
    completed: bool,
}

impl Drop for SearchReservation {
    fn drop(&mut self) {
        if !self.completed {
            self.search
                .release_incomplete_work(self.validation, &self.key);
        }
    }
}

impl ProxyInventory {
    /// Atomically leaves spare shared capacity when admitting speculative work.
    ///
    /// # Arguments
    /// - `validation`: Selects capability slots instead of TCP slots.
    /// - `speculative`: Rejects admission immediately unless spare capacity remains.
    pub(super) async fn acquire_probe_slot(
        &self,
        validation: bool,
        speculative: bool,
    ) -> Option<tokio::sync::OwnedSemaphorePermit> {
        let slots = if validation {
            self.validation_slots.clone()
        } else {
            self.tcp_slots.clone()
        };
        #[cfg(feature = "performance-diagnostics")]
        let queued = std::time::Instant::now();
        let permit = if speculative {
            let spare = if validation { 2 } else { 8 };
            let mut permits = slots.try_acquire_many_owned(spare + 1).ok()?;
            permits.split(1)?
        } else {
            slots.acquire_owned().await.ok()?
        };
        #[cfg(feature = "performance-diagnostics")]
        tracing::debug!(
            validation,
            speculative,
            waiting_ms = queued.elapsed().as_millis(),
            waiting_us = queued.elapsed().as_micros(),
            "acquired shared proxy probe slot"
        );
        Some(permit)
    }

    /// Shares an owned probe task without extending the caller's polling stack.
    /// The last subscriber aborts the task; caller deadlines and slots stay local.
    pub(super) async fn shared_operation(
        &self,
        key: String,
        validation: bool,
        operation: BoxFuture<'static, Option<ProxyRecord>>,
    ) -> Option<ProxyRecord> {
        let search = ProxySearchContext::current();
        #[cfg(feature = "performance-diagnostics")]
        let search_id = search.as_ref().map(ProxySearchContext::id);
        let wait = async {
            #[cfg(feature = "performance-diagnostics")]
            let queued = std::time::Instant::now();
            let _local = if let Some(search) = &search {
                let slots = if validation {
                    &search.validation_slots
                } else {
                    &search.tcp_slots
                };
                Some(slots.clone().acquire_owned().await.ok()?)
            } else {
                None
            };
            #[cfg(feature = "performance-diagnostics")]
            tracing::debug!(search_id, work = %key, validation,
                speculative = search.as_ref().is_some_and(|search| search.speculative),
                waiting_us = queued.elapsed().as_micros(),
                "acquired request-local proxy probe slot");
            if search
                .as_ref()
                .is_some_and(|search| !search.reserve(validation, &key))
            {
                return None;
            }
            let mut reservation = search.as_ref().map(|search| SearchReservation {
                search: search.without_continuation(),
                key: key.clone(),
                validation,
                completed: false,
            });
            let shared = {
                let mut registry = self.shared_work.lock().await;
                registry.retain(|_, weak| weak.upgrade().is_some());
                if let Some(existing) = registry.get(&key).and_then(WeakShared::upgrade) {
                    #[cfg(feature = "performance-diagnostics")]
                    tracing::debug!(search_id, work = %key, validation, joined = true, "subscribed to shared proxy work");
                    existing
                } else {
                    #[cfg(feature = "performance-diagnostics")]
                    tracing::debug!(search_id, work = %key, validation, joined = false, "subscribed to shared proxy work");
                    #[cfg(feature = "performance-diagnostics")]
                    let work_key = key.clone();
                    let future = async move {
                        let mut tasks = tokio::task::JoinSet::new();
                        tasks.spawn(ProxySearchContext::unscoped(async move {
                            #[cfg(feature = "performance-diagnostics")]
                            let mut execution = SharedExecution {
                                key: work_key,
                                operation_id: tokio::task::id(),
                                completed: false,
                                started: std::time::Instant::now(),
                            };
                            #[cfg(feature = "performance-diagnostics")]
                            tracing::debug!(work = %execution.key, operation_id = %execution.operation_id,
                                "started shared proxy operation");
                            let result = operation.await;
                            #[cfg(feature = "performance-diagnostics")]
                            { execution.completed = true; }
                            result
                        }));
                        match tasks.join_next().await {
                            Some(Ok(result)) => result,
                            Some(Err(error)) => {
                                tracing::warn!(
                                    panicked = error.is_panic(),
                                    "shared proxy probe task failed"
                                );
                                None
                            }
                            None => None,
                        }
                    }
                    .boxed()
                    .shared();
                    registry.insert(key, future.downgrade()?);
                    future
                }
            };
            let worker_search = search
                .as_ref()
                .map(ProxySearchContext::without_continuation);
            let finish = async move {
                let _local = _local;
                let result = shared.await;
                if let Some(reservation) = &mut reservation {
                    reservation.completed = true;
                }
                if let Some(search) = &worker_search {
                    #[cfg(feature = "performance-diagnostics")]
                    search.completed_work(validation, result.is_some());
                    if validation {
                        #[cfg(feature = "performance-diagnostics")]
                        search.log_progress("capability validation completed");
                        if result.is_none() {
                            search.failed_validation();
                        }
                    }
                }
                result
            };
            if let Some(owner) = search
                .as_ref()
                .and_then(|search| search.continuation.as_ref())
                .filter(|owner| owner.is_routed())
            {
                owner.retain(finish).await.ok().flatten()
            } else {
                finish.await
            }
        };
        match &search {
            Some(search) => search.technical(wait).await.ok().flatten(),
            None => wait.await,
        }
    }

    /// Persists phase attempts independently of health and capability results,
    /// so completed inconclusive validations remain throttled after restart.
    /// Cancellation without a completed result never writes a synthetic attempt.
    pub(super) async fn save_probe_attempt(
        &self,
        authority: &str,
        phase: &str,
        started: u64,
        tcp: Option<bool>,
    ) {
        let updated = {
            let mut inner = self.inner.write().await;
            let Some(record) = inner.records.get_mut(authority) else {
                return;
            };
            record.probe_attempts.insert(phase.to_string(), started);
            if let Some(reachable) = tcp {
                record.tcp_reachable = Some(reachable);
                if reachable && record.status == ProxyRuntimeStatus::Ko {
                    record.status = ProxyRuntimeStatus::Unknown;
                    record.cooldown_until = None;
                }
                let now = SystemTime::now();
                record.record_observation(crate::core::ProxyObservation {
                    context: crate::core::ProxyProbeContext {
                        phase: crate::core::ProxyObservationPhase::EndpointConnect,
                        protocol: None,
                        target: None,
                        method: None,
                        configuration: "tcp-precheck-v1".into(),
                    },
                    outcome: if reachable {
                        crate::core::ProxyObservationOutcome::Succeeded
                    } else {
                        crate::core::ProxyObservationOutcome::Failed {
                            scope: crate::core::ProxyFailureScope::Proxy,
                            cause: "endpoint_connection_failed".into(),
                        }
                    },
                    completed_at: now,
                    last_succeeded_at: reachable.then_some(now),
                    http_status: None,
                });
            }
            record.clone()
        };
        #[cfg(feature = "persistence")]
        self.persist_records(std::slice::from_ref(&updated)).await;
        #[cfg(not(feature = "persistence"))]
        drop(updated);
        if tcp == Some(false) {
            self.record_global_failure(authority).await;
        }
    }

    /// Validates missing capabilities on demand without holding inventory data
    /// locks during network I/O. Inconclusive attempts are throttled by probe TTL.
    pub(super) async fn validate_selection_candidate(
        &self,
        candidate: ProxyRecord,
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        let Some(probe) = &self.probe else {
            return is_eligible(
                &candidate,
                candidate.country.as_deref()?,
                require_https,
                destination,
                SystemTime::now(),
                self.probe_mode(),
            )
            .then_some(candidate);
        };
        let authority = candidate.authority();
        let endpoint_lock = {
            let mut locks = self.selection_probe_locks.lock().await;
            locks.retain(|_, lock| Arc::strong_count(lock) > 1);
            Arc::clone(
                locks
                    .entry(authority.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _guard = endpoint_lock.lock().await;
        let mut record = self.inner.read().await.records.get(&authority)?.clone();
        let country = record.country.clone()?;
        if !is_selection_candidate(
            &record,
            &country,
            require_https,
            destination,
            SystemTime::now(),
            self.probe_mode(),
        ) {
            return None;
        }
        if needs_capability_probe(&record, require_https)
            || probe_is_expired(&record, SystemTime::now(), self.config.probe_ttl)
        {
            let phase = if require_https { "https" } else { "http" };
            if recent_probe_attempt(&record, phase, self.config.probe_ttl) {
                tracing::debug!(proxy = %authority, source = "cache", require_https,
                    capability_proven = !needs_capability_probe(&record, require_https),
                    "reusing recent proxy capability attempt");
                return (complementary_trial_allowed(&record, self.probe_mode())
                    && is_eligible(
                        &record,
                        &country,
                        require_https,
                        destination,
                        SystemTime::now(),
                        self.probe_mode(),
                    ))
                .then_some(record);
            }
            let started = epoch_millis();
            tracing::debug!(proxy = %authority, require_https, "probing proxy capabilities before selection");
            let result = probe.probe_required(&mut record, require_https).await;
            #[cfg(feature = "performance-diagnostics")]
            for observation in &record.observations {
                if observation
                    .completed_at
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    >= u128::from(started)
                {
                    if let crate::core::ProxyObservationOutcome::Failed { scope, cause } =
                        &observation.outcome
                    {
                        tracing::debug!(proxy = %authority, source = "exchange", phase = ?observation.context.phase,
                            protocol = ?observation.context.protocol, target = ?observation.context.target,
                            ?scope, %cause, "proxy capability probe observed failure");
                    }
                }
            }
            tracing::debug!(
                proxy = %authority,
                source = "exchange",
                admissible = is_eligible(&record, &country, require_https, destination, SystemTime::now(), self.probe_mode()),
                require_https,
                http_forwarding = ?record.http_forwarding,
                https_tunnel = ?record.https_tunnel,
                destination_tls = ?record.destination_tls,
                capability_proven = !needs_capability_probe(&record, require_https),
                error = ?result.err(),
                "completed on-demand proxy capability probe"
            );
            let endpoint_failed = record.observations.iter().any(|observation| {
                observation.context.phase == crate::core::ProxyObservationPhase::EndpointConnect
                    && observation
                        .completed_at
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                        >= u128::from(started)
                    && matches!(
                        observation.outcome,
                        crate::core::ProxyObservationOutcome::Failed { .. }
                    )
            });
            record.probe_attempts.insert(phase.to_string(), started);
            let retained = self.add_or_update(vec![record]).await;
            #[cfg(feature = "persistence")]
            self.persist_records(&retained).await;
            if endpoint_failed {
                self.record_global_failure(&authority).await;
                return None;
            }
            record = retained.into_iter().next()?;
        }
        if needs_capability_probe(&record, require_https)
            && !complementary_trial_allowed(&record, self.probe_mode())
        {
            return None;
        }
        is_eligible(
            &record,
            &country,
            require_https,
            destination,
            SystemTime::now(),
            self.probe_mode(),
        )
        .then_some(record)
    }
}
