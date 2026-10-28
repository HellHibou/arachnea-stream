//! Request-owned probe subscriptions and bounded runtime-owned fallback preparation.

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use tokio::{sync::watch, task::JoinSet, time::Instant};

#[cfg(feature = "persistence")]
use futures::{stream, StreamExt};

#[cfg(feature = "performance-diagnostics")]
use super::performance_diagnostics::PreparationExecution;

use super::{
    eligibility::{is_eligible, needs_capability_probe, probe_is_expired},
    ProxyInventory,
};
use crate::core::{Destination, ProxyRecord, ProxySearchContext, ProxySearchLimits};
/// Exact accepted origin and capability constraints retained for fallback preparation.
#[derive(Clone, Debug)]
struct PreparationRoute {
    countries: Vec<String>,
    require_https: bool,
    destination: Destination,
}

/// Owns only already subscribed technical operations, never business requests.
#[derive(Debug)]
pub(crate) struct ProbeContinuation {
    tasks: Mutex<JoinSet<()>>,
    deadline: watch::Sender<Instant>,
    route: Mutex<Option<PreparationRoute>>,
}

impl ProbeContinuation {
    /// Creates request-owned technical subscriptions.
    ///
    /// # Arguments
    /// - `deadline`: Initial technical deadline, independent of business grace.
    pub(crate) fn new(deadline: Instant) -> Self {
        Self {
            tasks: Mutex::new(JoinSet::new()),
            deadline: watch::channel(deadline).0,
            route: Mutex::new(None),
        }
    }

    /// Updates routing metadata without storing the business request.
    ///
    /// # Arguments
    /// - `countries`: Canonical allowed country set.
    /// - `require_https`: Required technical destination capability.
    /// - `destination`: Exact origin, or no retained business selection.
    pub(crate) fn set_route(
        &self,
        countries: Vec<String>,
        require_https: bool,
        destination: Option<Destination>,
    ) {
        *self.route.lock().unwrap() = destination.map(|destination| PreparationRoute {
            countries,
            require_https,
            destination,
        });
    }

    /// Returns whether this continuation retains an exact-origin route for reserve preparation.
    pub(crate) fn is_routed(&self) -> bool {
        self.route.lock().unwrap().is_some()
    }

    /// Aborts all technical subscriptions owned by this continuation.
    pub(crate) fn cancel(&self) {
        self.tasks.lock().unwrap().abort_all();
    }

    /// Retains a subscriber while its selection is being tried. Workers never
    /// capture this owner, so dropping the request aborts its unfinished tasks.
    ///
    /// # Arguments
    /// - `future`: Already subscribed technical phase, including its permit.
    pub(crate) fn retain<F>(&self, future: F) -> tokio::sync::oneshot::Receiver<Option<ProxyRecord>>
    where
        F: Future<Output = Option<ProxyRecord>> + Send + 'static,
    {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let mut deadline = self.deadline.subscribe();
        let mut tasks = self.tasks.lock().unwrap();
        while tasks.try_join_next().is_some() {}
        tasks.spawn(async move {
            tokio::pin!(future);
            loop {
                let expires = *deadline.borrow_and_update();
                tokio::select! {
                    result = &mut future => { let _ = sender.send(result); break; }
                    _ = tokio::time::sleep_until(expires) => break,
                    changed = deadline.changed() => { if changed.is_err() { break; } }
                }
            }
        });
        receiver
    }
}

/// Dropping the inventory cancels preparations. Tasks capture an inventory clone
/// with a separate, empty supervisor to avoid retaining their own task owner.
#[derive(Default)]
pub(super) struct PreparationRuntime {
    tasks: Mutex<HashMap<String, PreparationEntry>>,
}
/// Owned reserve-preparation tasks and their shared technical deadline.
struct PreparationEntry {
    tasks: JoinSet<()>,
    retry_at: Instant,
}

/// Collects eligible fallback endpoints other than the already accepted endpoint.
///
/// # Arguments
/// * `inventory` - Inventory owning the selected endpoint and its runtime observations.
/// * `inner` - Locked inventory state used to inspect currently cached candidates.
/// * `route` - Accepted origin and allowed countries defining the fallback reserve.
/// * `endpoint` - Concrete proxy endpoint excluded or tracked by this operation.
fn ready_reserve(
    inventory: &ProxyInventory,
    inner: &super::InventoryInner,
    route: &PreparationRoute,
    endpoint: &str,
) -> std::collections::HashSet<String> {
    let now = SystemTime::now();
    route
        .countries
        .iter()
        .filter_map(|country| inner.country_records.get(country))
        .flatten()
        .filter_map(|key| inner.records.get(key))
        .filter_map(|record| {
            let country = record.country.as_deref()?;
            if record.authority() == endpoint
                || !route.countries.iter().any(|allowed| allowed == country)
            {
                return None;
            }
            let mut record = record.clone();
            if let Some(probe) = &inventory.probe {
                probe.prepare_context(&mut record);
            }
            (is_eligible(
                &record,
                country,
                route.require_https,
                Some(&route.destination),
                now,
                inventory.probe_mode(),
            ) && !probe_is_expired(&record, now, inventory.config.probe_ttl)
                && (!needs_capability_probe(&record, route.require_https)
                    || super::eligibility::recent_probe_attempt(
                        &record,
                        if route.require_https { "https" } else { "http" },
                        inventory.config.probe_ttl,
                    )))
            .then(|| record.authority())
        })
        .take(inventory.config.reserve_size.min(2))
        .collect()
}

impl ProxyInventory {
    /// Clones the inventory with an independent empty preparation supervisor to avoid ownership cycles.
    pub(super) fn clone_for_probe(&self) -> Self {
        let mut inventory = self.clone();
        inventory.preparations = Arc::new(PreparationRuntime::default());
        inventory
    }

    /// Transfers live subscriptions without awaiting imports, probes or reserve commits.
    /// A deficient reserve imports persistent country records once in its owned
    /// task, including when the in-memory country inventory is partially loaded.
    ///
    /// # Arguments
    /// - `continuation`: Subscriptions owned by the accepted selection.
    /// - `endpoint`: Accepted endpoint, excluded from the fallback count.
    /// - `destination`: Exact accepted origin, required to match the selection.
    pub(crate) fn prepare_after_acceptance(
        &self,
        continuation: &ProbeContinuation,
        endpoint: &str,
        destination: &Destination,
    ) {
        let route = continuation.route.lock().unwrap().clone();
        let Some(route) = route.filter(|route| route.destination == *destination) else {
            return;
        };
        if self.config.reserve_size == 0 || self.config.reserve_timeout.is_zero() {
            continuation.cancel();
            return;
        }
        if self.inner.try_read().is_ok_and(|inner| {
            ready_reserve(self, &inner, &route, endpoint).len() >= self.config.reserve_size.min(2)
        }) {
            continuation.cancel();
            return;
        }
        let key = format!(
            "{}|{}|{}|{}",
            crate::core::ProxyDestinationPreference::destination_key(destination),
            route.countries.join(","),
            route.require_https,
            self.probe
                .as_ref()
                .map(|probe| probe.configuration_id())
                .unwrap_or_default()
        );
        let mut registry = self.preparations.tasks.lock().unwrap();
        let now = Instant::now();
        registry.retain(|_, entry| {
            while entry.tasks.try_join_next().is_some() {}
            !entry.tasks.is_empty() || now < entry.retry_at
        });
        if registry.contains_key(&key)
            || registry
                .values()
                .filter(|entry| !entry.tasks.is_empty())
                .count()
                >= 2
        {
            continuation.cancel();
            return;
        }
        if registry.len() >= 32 {
            let oldest = registry
                .iter()
                .filter(|(_, entry)| entry.tasks.is_empty())
                .min_by_key(|(_, entry)| entry.retry_at)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                registry.remove(&oldest);
            }
        }
        let deadline = Instant::now() + self.config.reserve_timeout;
        continuation.deadline.send_replace(deadline);
        let pending = std::mem::replace(&mut *continuation.tasks.lock().unwrap(), JoinSet::new());
        let mut inventory = self.clone();
        inventory.preparations = Arc::new(PreparationRuntime::default());
        let endpoint = endpoint.to_string();
        #[cfg(feature = "performance-diagnostics")]
        let source_search_id = ProxySearchContext::current().map(|search| search.id());
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            #[cfg(feature = "performance-diagnostics")]
            let mut execution = PreparationExecution {
                source_search_id,
                operation_id: tokio::task::id(),
                started: std::time::Instant::now(),
                outcome: "cancelled",
            };
            #[cfg(feature = "performance-diagnostics")]
            tracing::debug!(source_search_id, preparation_id = %execution.operation_id,
                reserve_size = inventory.config.reserve_size.min(2),
                timeout_ms = inventory.config.reserve_timeout.as_millis(),
                "started bounded proxy reserve preparation");
            let work = async {
                // Finish the subscriptions admitted by the active call before
                // admitting any additional speculative work.
                let mut pending = pending;
                while let Some(result) = pending.join_next().await {
                    if let Err(error) = result {
                        tracing::warn!(%error, "retained proxy subscriber failed");
                    }
                }
                let mut search = ProxySearchContext::new(ProxySearchLimits {
                    target: deadline.saturating_duration_since(Instant::now()),
                    extension: std::time::Duration::ZERO,
                });
                search.continuation = None;
                search.speculative = true;
                search.tcp_slots = Arc::new(tokio::sync::Semaphore::new(1));
                search.validation_slots = Arc::new(tokio::sync::Semaphore::new(1));
                #[cfg(feature = "performance-diagnostics")]
                tracing::debug!(source_search_id, preparation_id = %execution.operation_id,
                    search_id = search.id(), "started proxy reserve candidate search");
                let mut tried = std::collections::HashSet::new();
                #[cfg(feature = "persistence")]
                let mut imported = inventory.persistence.is_none();
                loop {
                    let mut ready = {
                        let inner = inventory.inner.read().await;
                        ready_reserve(&inventory, &inner, &route, &endpoint)
                    };
                    if ready.len() >= inventory.config.reserve_size.min(2) || !search.can_start() {
                        break;
                    }
                    #[cfg(feature = "persistence")]
                    if !imported {
                        imported = true;
                        #[cfg(feature = "performance-diagnostics")]
                        let import_started = std::time::Instant::now();
                        let _results = search
                            .scope(async {
                                let imports = route
                                    .countries
                                    .iter()
                                    .cloned()
                                    .map(|country| {
                                        let inventory = inventory.clone();
                                        async move { inventory.load_cached_country(&country).await }
                                    })
                                    .collect::<Vec<_>>();
                                let imports = stream::iter(imports).buffer_unordered(2);
                                #[cfg(feature = "performance-diagnostics")]
                                { imports.collect::<Vec<_>>().await }
                                #[cfg(not(feature = "performance-diagnostics"))]
                                { imports.for_each(|_| async {}).await; }
                            })
                            .await;
                        #[cfg(feature = "performance-diagnostics")]
                        tracing::debug!(source_search_id, preparation_id = %execution.operation_id,
                            search_id = search.id(), countries = route.countries.len(),
                            imported_countries = _results.into_iter().filter(|imported| *imported).count(),
                            duration_us = import_started.elapsed().as_micros(),
                            "finished reserve persistent country imports");
                        continue;
                    }
                    ready.insert(endpoint.clone());
                    ready.extend(tried.iter().cloned());
                    search.preparation_exclusions = ready;
                    let selected = search
                        .scope(search.technical(inventory.select_cached_any(
                            &route.countries,
                            route.require_https,
                            Some(&route.destination),
                        )))
                        .await
                        .ok()
                        .flatten();
                    let Some(selected) = selected else {
                        break;
                    };
                    if !tried.insert(selected.authority()) {
                        break;
                    }
                }
            };
            let _result = tokio::time::timeout_at(deadline, work).await;
            #[cfg(feature = "performance-diagnostics")]
            { execution.outcome = if _result.is_err() {
                "deadline"
            } else {
                "completed"
            }; }

        });
        registry.insert(
            key,
            PreparationEntry {
                tasks,
                retry_at: deadline,
            },
        );
    }
}
