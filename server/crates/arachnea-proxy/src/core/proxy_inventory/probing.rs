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

struct SharedExecution {
    key: String,
    completed: bool,
}

impl Drop for SharedExecution {
    fn drop(&mut self) {
        if !self.completed {
            tracing::debug!(work = %self.key, "cancelled shared proxy work after last subscriber departed");
        }
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
    /// Shares an owned probe task without extending the caller's polling stack.
    /// The last subscriber aborts the task; caller deadlines and slots stay local.
    pub(super) async fn shared_operation(
        &self,
        key: String,
        validation: bool,
        operation: BoxFuture<'static, Option<ProxyRecord>>,
    ) -> Option<ProxyRecord> {
        let search = ProxySearchContext::current();
        let wait = async {
            let _local = if let Some(search) = &search {
                let slots = if validation {
                    &search.validation_slots
                } else {
                    &search.tcp_slots
                };
                Some(slots.acquire().await.ok()?)
            } else {
                None
            };
            if search
                .as_ref()
                .is_some_and(|search| !search.reserve(validation, &key))
            {
                return None;
            }
            let mut reservation = search.as_ref().map(|search| SearchReservation {
                search: search.clone(),
                key: key.clone(),
                validation,
                completed: false,
            });
            let shared = {
                let mut registry = self.shared_work.lock().await;
                registry.retain(|_, weak| weak.upgrade().is_some());
                if let Some(existing) = registry.get(&key).and_then(WeakShared::upgrade) {
                    existing
                } else {
                    let work_key = key.clone();
                    let future = async move {
                        let mut tasks = tokio::task::JoinSet::new();
                        tasks.spawn(ProxySearchContext::unscoped(async move {
                            let mut execution = SharedExecution {
                                key: work_key,
                                completed: false,
                            };
                            let result = operation.await;
                            execution.completed = true;
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
            let result = shared.await;
            if let Some(reservation) = &mut reservation {
                reservation.completed = true;
            }
            if let Some(search) = &search {
                search.completed_work(validation, result.is_some());
                if validation {
                    search.log_progress("capability validation completed");
                }
            }
            if validation && result.is_none() {
                if let Some(search) = &search {
                    search.failed_validation();
                }
            }
            result
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
            tracing::info!(proxy = %authority, require_https, "probing proxy capabilities before selection");
            let result = probe.probe_required(&mut record, require_https).await;
            tracing::info!(
                proxy = %authority,
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
