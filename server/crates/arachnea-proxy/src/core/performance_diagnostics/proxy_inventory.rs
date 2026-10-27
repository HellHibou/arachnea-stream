//! Optional inventory statistics and lifecycle observations.

use super::eligibility::{
    can_reach_https_destination, has_active_destination_cooldown, is_eligible,
    needs_capability_probe, probe_is_expired,
};
use super::ProxyInventory;
use crate::core::{
    Destination, ProbeMode, ProxyCapabilityStatus, ProxyRecord, ProxyRuntimeStatus,
    ProxySearchContext,
};
use crate::PROXY_RANKING_DIAGNOSTIC_LIMIT as RANKING_DIAGNOSTIC_LIMIT;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Counts cached capability and exclusion states for diagnostic snapshots.
#[derive(Clone, Copy, Debug, Default)]
struct SelectionStats {
    total: usize,
    ok: usize,
    unknown: usize,
    ko: usize,
    authentication_required: usize,
    authentication_required_flag: usize,
    cooldown: usize,
    probe_expired: usize,
    destination_cooldown: usize,
    https_rejected: usize,
    http_forward_available: usize,
    http_forward_unavailable: usize,
    http_forward_unknown: usize,
    https_tunnel_available: usize,
    https_tunnel_unavailable: usize,
    https_tunnel_unknown: usize,
    destination_tls_available: usize,
    destination_tls_unavailable: usize,
    destination_tls_unknown: usize,
    proxy_tls_valid: usize,
    proxy_tls_invalid: usize,
    proxy_tls_unknown: usize,
    eligible: usize,
}

/// Counts capability and exclusion states without mutating cached records.
///
/// # Arguments
/// - `records`: Cached snapshots to inspect.
/// - `country`: Country used by the normal eligibility predicate.
/// - `require_https`: Requested capability.
/// - `destination`: Exact origin whose exclusions apply.
/// - `now`: Existing freshness reference.
/// - `probe_ttl`: Configured evidence freshness window.
/// - `probe_mode`: Existing capability interpretation policy.
fn selection_stats<'a>(
    records: impl IntoIterator<Item = &'a ProxyRecord>,
    country: &str,
    require_https: bool,
    destination: Option<&Destination>,
    now: SystemTime,
    probe_ttl: Duration,
    probe_mode: ProbeMode,
) -> SelectionStats {
    let mut stats = SelectionStats::default();

    for record in records {
        stats.total += 1;
        match record.status {
            ProxyRuntimeStatus::Ok => stats.ok += 1,
            ProxyRuntimeStatus::Unknown => stats.unknown += 1,
            ProxyRuntimeStatus::Ko => stats.ko += 1,
            ProxyRuntimeStatus::AuthenticationRequired => stats.authentication_required += 1,
        }

        if record.authentication_required == Some(true) {
            stats.authentication_required_flag += 1;
        }

        match record.http_forwarding {
            ProxyCapabilityStatus::Available => stats.http_forward_available += 1,
            ProxyCapabilityStatus::Unavailable => stats.http_forward_unavailable += 1,
            ProxyCapabilityStatus::Unknown => stats.http_forward_unknown += 1,
        }
        match record.https_tunnel {
            ProxyCapabilityStatus::Available => stats.https_tunnel_available += 1,
            ProxyCapabilityStatus::Unavailable => stats.https_tunnel_unavailable += 1,
            ProxyCapabilityStatus::Unknown => stats.https_tunnel_unknown += 1,
        }
        match record.destination_tls {
            ProxyCapabilityStatus::Available => stats.destination_tls_available += 1,
            ProxyCapabilityStatus::Unavailable => stats.destination_tls_unavailable += 1,
            ProxyCapabilityStatus::Unknown => stats.destination_tls_unknown += 1,
        }
        match record.proxy_tls_certificate {
            ProxyCapabilityStatus::Available => stats.proxy_tls_valid += 1,
            ProxyCapabilityStatus::Unavailable => stats.proxy_tls_invalid += 1,
            ProxyCapabilityStatus::Unknown => stats.proxy_tls_unknown += 1,
        }

        if record.cooldown_until.is_some_and(|cooldown| cooldown > now) {
            stats.cooldown += 1;
        }

        if probe_is_expired(record, now, probe_ttl) {
            stats.probe_expired += 1;
        }

        if destination
            .is_some_and(|destination| has_active_destination_cooldown(record, destination, now))
        {
            stats.destination_cooldown += 1;
        }

        if require_https && !can_reach_https_destination(record, probe_mode) {
            stats.https_rejected += 1;
        }

        if is_eligible(record, country, require_https, destination, now, probe_mode) {
            stats.eligible += 1;
        }
    }

    stats
}

/// Process-local identity separating repeated rankings within one search.
pub(super) static NEXT_RANKING_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// Reports sampled pipeline completion or cancellation without owning network work.
pub(super) struct CandidateDiagnostic {
    ranking_id: u64,
    search_id: Option<usize>,
    position: usize,
    endpoint: String,
    started: std::time::Instant,
    completion_order: Option<usize>,
    outcome: &'static str,
}

impl Drop for CandidateDiagnostic {
    fn drop(&mut self) {
        tracing::debug!(target: "arachnea_proxy::core::proxy_inventory::selection",
            ranking_id = self.ranking_id,
            search_id = self.search_id,
            position = self.position,
            proxy = %self.endpoint,
            completion_order = self.completion_order,
            outcome = self.outcome,
            duration_ms = self.started.elapsed().as_millis() as u64,
            "finished sampled proxy candidate pipeline"
        );
    }
}

impl ProxyInventory {
    /// Collects a diagnostic snapshot while holding the country inventory read guard.
    ///
    /// # Arguments
    /// - `country`: Country to inspect.
    /// - `require_https`: Requested capability.
    async fn selection_stats_for_country(
        &self,
        country: &str,
        require_https: bool,
    ) -> Option<SelectionStats> {
        let inner = self.inner.read().await;
        let now = SystemTime::now();
        let keys = inner.country_records.get(country)?;
        let records = keys.iter().filter_map(|k| inner.records.get(k));
        Some(selection_stats(
            records,
            country,
            require_https,
            None,
            now,
            self.config.probe_ttl,
            self.probe_mode(),
        ))
    }
}

/// Shared counts retained across progressive provider publications.
#[derive(Clone, Default)]
pub(super) struct ProviderLoadProgress {
    /// Provider publications received before merging.
    pub(super) received_batches: usize,
    /// Provider records received before deduplication.
    pub(super) received_records: usize,
    /// Provider publications merged into inventory.
    pub(super) merged_batches: usize,
    /// Prepared records merged into inventory.
    pub(super) merged_records: usize,
    /// Instant of the first completed merge, when any.
    pub(super) first_merge: Option<Instant>,
}

/// Reports termination even when the last subscriber aborts a partial load.
pub(super) struct ProviderLoadObservation {
    /// Country whose progressive provider refresh is observed.
    pub(super) country: String,
    /// Monotonic start of the observed operation.
    pub(super) started: Instant,
    /// Static lifecycle outcome reported when the observation is dropped.
    pub(super) outcome: &'static str,
    /// Shared publication counters retained through cancellation.
    pub(super) progress: Arc<std::sync::Mutex<ProviderLoadProgress>>,
}

impl Drop for ProviderLoadObservation {
    fn drop(&mut self) {
        let progress = self
            .progress
            .lock()
            .map(|progress| progress.clone())
            .unwrap_or_default();
        tracing::debug!(target: "arachnea_proxy::core::proxy_inventory::discovery",country = %self.country, outcome = self.outcome,
            duration_ms = self.started.elapsed().as_millis(),
            received_batches = progress.received_batches, received_records = progress.received_records,
            merged_batches = progress.merged_batches, merged_records = progress.merged_records,
            first_merge_ms = progress.first_merge.map(|at| at.duration_since(self.started).as_millis()),
            partial = self.outcome != "completed" && progress.merged_batches > 0,
            empty = self.outcome == "completed" && progress.received_records == 0,
            "finished dynamic proxy provider refresh");
    }
}

/// Counts distinct provider protocol claims for one diagnostic snapshot.
///
/// # Arguments
/// - `record`: Endpoint with persisted provider declarations.
fn declared_protocol_count(record: &ProxyRecord) -> usize {
    let mut protocols = record
        .declarations
        .iter()
        .filter_map(|declaration| declaration.protocol.as_ref())
        .collect::<Vec<_>>();
    protocols.sort_by_key(|protocol| protocol.to_string());
    protocols.dedup();
    protocols.len()
}

/// Checks whether the measured protocol is absent from provider claims.
///
/// # Arguments
/// - `record`: Endpoint with runtime evidence and provider declarations.
fn runtime_protocol_is_undeclared(record: &ProxyRecord) -> bool {
    let Some(protocol) = &record.protocol else {
        return false;
    };
    !record.declarations.is_empty()
        && !record
            .declarations
            .iter()
            .any(|declaration| declaration.protocol.as_ref() == Some(protocol))
}

/// Reports cancellation separately from a preparation that reaches its deadline.
pub(super) struct PreparationExecution {
    /// Identity of the request that admitted reserve preparation.
    pub(super) source_search_id: Option<usize>,
    /// Identity of the runtime-owned task being observed.
    pub(super) operation_id: tokio::task::Id,
    /// Monotonic start of the observed operation.
    pub(super) started: std::time::Instant,
    /// Static lifecycle outcome reported when the observation is dropped.
    pub(super) outcome: &'static str,
}

impl Drop for PreparationExecution {
    fn drop(&mut self) {
        tracing::debug!(target: "arachnea_proxy::core::proxy_inventory::preparation",source_search_id = self.source_search_id,
            preparation_id = %self.operation_id, outcome = self.outcome,
            duration_us = self.started.elapsed().as_micros(),
            "finished bounded proxy reserve lifecycle");
    }
}

impl ProxyInventory {
    /// Reports candidate capability counters without changing eligibility.
    ///
    /// # Arguments
    /// - `records`: Cached snapshots still protected by the inventory read guard.
    /// - `countries`: Allowed countries.
    /// - `require_https`: Requested capability.
    /// - `destination`: Exact business origin.
    /// - `now`: Selection's original freshness reference.
    pub(super) fn log_selection_candidates(
        &self,
        records: &[&ProxyRecord],
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
        now: SystemTime,
    ) {
        for country in countries {
            let stats = selection_stats(
                records
                    .iter()
                    .copied()
                    .filter(|record| record.country.as_deref() == Some(country.as_str())),
                country,
                require_https,
                destination,
                now,
                self.config.probe_ttl,
                self.probe_mode(),
            );
            tracing::debug!(target: "arachnea_proxy::core::proxy_inventory::selection",
                country = %country,
                require_https,
                total = stats.total,
                ok = stats.ok,
                unknown = stats.unknown,
                ko = stats.ko,
                auth_required = stats.authentication_required,
                auth_flag = stats.authentication_required_flag,
                cooldown = stats.cooldown,
                probe_expired = stats.probe_expired,
                destination_cooldown = stats.destination_cooldown,
                https_rejected = stats.https_rejected,
                http_forward_available = stats.http_forward_available,
                http_forward_unavailable = stats.http_forward_unavailable,
                http_forward_unknown = stats.http_forward_unknown,
                https_tunnel_available = stats.https_tunnel_available,
                https_tunnel_unavailable = stats.https_tunnel_unavailable,
                https_tunnel_unknown = stats.https_tunnel_unknown,
                destination_tls_available = stats.destination_tls_available,
                destination_tls_unavailable = stats.destination_tls_unavailable,
                destination_tls_unknown = stats.destination_tls_unknown,
                eligible = stats.eligible,
                "proxy inventory selection candidates"
            );
        }
    }
}

impl ProxyInventory {
    /// Logs the bounded ranking sample using the unchanged business rank function.
    ///
    /// # Arguments
    /// - `diagnostics_enabled`: DEBUG guard captured when selection began.
    /// - `ranking_id`: Process-local ranking identity.
    /// - `search`: Active request search, when any.
    /// - `candidates`: Already sorted candidate snapshots.
    /// - `destination`: Exact business origin.
    /// - `require_https`: Requested capability.
    /// - `now`: Selection's original freshness reference.
    /// - `rank`: Business comparator's capability rank.
    pub(super) fn log_ranked_candidates(
        &self,
        diagnostics_enabled: bool,
        ranking_id: u64,
        search: Option<&crate::core::ProxySearchContext>,
        candidates: &[&ProxyRecord],
        destination: Option<&Destination>,
        require_https: bool,
        now: SystemTime,
        rank: impl Fn(&ProxyRecord) -> i32,
    ) {
        if diagnostics_enabled {
            for (position, candidate) in
                candidates.iter().take(RANKING_DIAGNOSTIC_LIMIT).enumerate()
            {
                tracing::debug!(target: "arachnea_proxy::core::proxy_inventory::selection",
                    ranking_id,
                    search_id = search.map(ProxySearchContext::id),
                    destination = ?destination,
                    require_https,
                    probe_configuration = ?self.probe.as_ref().map(|probe| probe.configuration_id()),
                    position = position + 1,
                    proxy = %candidate.authority(),
                    country = ?candidate.country,
                    capability_rank = rank(candidate),
                    needs_probe = needs_capability_probe(candidate, require_https),
                    probe_expired = probe_is_expired(candidate, now, self.config.probe_ttl),
                    latency_ms = candidate.latency_ms,
                    failure_count = candidate.failure_count,
                    protocol = ?candidate.protocol,
                    tcp_reachable = candidate.tcp_reachable,
                    probe_age_ms = candidate.last_checked.and_then(|checked| now.duration_since(checked).ok()).map(|age| age.as_millis() as u64),
                    http_forwarding = ?candidate.http_forwarding,
                    https_tunnel = ?candidate.https_tunnel,
                    destination_tls = ?candidate.destination_tls,
                    proxy_tls_certificate = ?candidate.proxy_tls_certificate,
                    "ranked proxy candidate snapshot"
                );
            }
        }
    }
}

impl CandidateDiagnostic {
    /// Observes a sampled pipeline without owning its network work.
    ///
    /// # Arguments
    /// - `ranking_id`: Ranking identity.
    /// - `position`: One-based sorted candidate position.
    /// - `candidate`: Snapshot entering the pipeline.
    pub(super) fn new(ranking_id: u64, position: usize, candidate: &ProxyRecord) -> Self {
        tracing::debug!(target: "arachnea_proxy::core::proxy_inventory::selection", ranking_id, position,
            proxy = %candidate.authority(), "started sampled proxy candidate pipeline");
        Self {
            ranking_id,
            search_id: crate::core::ProxySearchContext::current().map(|search| search.id()),
            position,
            endpoint: candidate.authority(),
            started: std::time::Instant::now(),
            completion_order: None,
            outcome: "dropped_before_completion",
        }
    }

    /// Records completion before the observation is dropped.
    ///
    /// # Arguments
    /// - `completion_order`: Completion count after this pipeline finished.
    /// - `eligible`: Whether this pipeline returned an eligible candidate.
    pub(super) fn completed(&mut self, completion_order: usize, eligible: bool) {
        self.completion_order = Some(completion_order);
        self.outcome = if eligible { "eligible" } else { "not_eligible" };
    }
}

/// Reports completion of the existing scoped selection.
///
/// # Arguments
/// - `search`: Selection's request-local context.
/// - `require_https`: Requested capability.
/// - `result`: Original selection result.
/// - `selection_started`: Instant captured before selection.
pub(super) fn finished_selection(
    search: &crate::core::ProxySearchContext,
    require_https: bool,
    result: &crate::core::Result<ProxyRecord>,
    selection_started: Instant,
) {
    tracing::debug!(target: "arachnea_proxy::core::proxy_inventory::selection",
        search_id = search.id(),
        require_https,
        succeeded = result.is_ok(),
        duration_us = selection_started.elapsed().as_micros(),
        "finished localized proxy selection"
    );
}

impl ProxyInventory {
    /// Reports provider metadata and probe availability before merging a batch.
    ///
    /// # Arguments
    /// - `country`: Country whose provider is being refreshed.
    /// - `records`: Prepared records awaiting merge.
    /// - `existing`: Cached records observed before preparation.
    /// - `loaded_count`: Number received before deduplication.
    /// - `pending_probe_count`: Prepared records requiring capability checks.
    /// - `duplicate_count`: Duplicate provider records.
    /// - `cached_count`: Records already represented in the cache.
    pub(super) fn log_prepared_provider_batch(
        &self,
        country: &str,
        records: &[ProxyRecord],
        existing: &std::collections::HashMap<String, ProxyRecord>,
        loaded_count: usize,
        pending_probe_count: usize,
        duplicate_count: usize,
        cached_count: usize,
    ) {
        {
            let new_endpoint_count = records
                .iter()
                .filter(|record| !existing.contains_key(&record.authority()))
                .count();
            let changed_declarations_count = records
                .iter()
                .filter(|record| {
                    existing.get(&record.authority()).is_some_and(|cached| {
                        record
                            .declarations
                            .iter()
                            .any(|declaration| !cached.declarations.contains(declaration))
                    })
                })
                .count();
            let probe_stats = selection_stats(
                records.iter(),
                country,
                false,
                None,
                SystemTime::now(),
                self.config.probe_ttl,
                self.probe_mode(),
            );
            let protocol_conflict_count = records
                .iter()
                .filter(|record| declared_protocol_count(record) > 1)
                .count();
            let undeclared_protocol_count = records
                .iter()
                .filter(|record| runtime_protocol_is_undeclared(record))
                .count();
            tracing::debug!(target: "arachnea_proxy::core::proxy_inventory::discovery",
                country = %country,
                loaded_count,
                deduplicated_count = records.len(),
                duplicate_count,
                protocol_conflict_count,
                undeclared_protocol_count,
                cached_count,
                new_endpoint_count,
                changed_declarations_count,
                pending_probe_count,
                ok = probe_stats.ok,
                unknown = probe_stats.unknown,
                ko = probe_stats.ko,
                auth_required = probe_stats.authentication_required,
                auth_flag = probe_stats.authentication_required_flag,
                http_forward_available = probe_stats.http_forward_available,
                http_forward_unavailable = probe_stats.http_forward_unavailable,
                http_forward_unknown = probe_stats.http_forward_unknown,
                https_tunnel_available = probe_stats.https_tunnel_available,
                https_tunnel_unavailable = probe_stats.https_tunnel_unavailable,
                https_tunnel_unknown = probe_stats.https_tunnel_unknown,
                destination_tls_available = probe_stats.destination_tls_available,
                destination_tls_unavailable = probe_stats.destination_tls_unavailable,
                destination_tls_unknown = probe_stats.destination_tls_unknown,
                proxy_tls_valid = probe_stats.proxy_tls_valid,
                proxy_tls_invalid = probe_stats.proxy_tls_invalid,
                proxy_tls_unknown = probe_stats.proxy_tls_unknown,
                "loaded dynamic proxies prepared for on-demand probing"
            );
        }
    }
}

impl ProxyInventory {
    /// Reports inventory state after the existing batch merge.
    ///
    /// # Arguments
    /// - `country`: Country whose published batch was merged.
    pub(super) async fn log_updated_inventory(&self, country: &str) {
        {
            let http_stats = self
                .selection_stats_for_country(country, false)
                .await
                .unwrap_or_default();
            let https_stats = self
                .selection_stats_for_country(country, true)
                .await
                .unwrap_or_default();
            tracing::debug!(target: "arachnea_proxy::core::proxy_inventory::discovery",
                country = %country,
                total = http_stats.total,
                ok = http_stats.ok,
                eligible_http = http_stats.eligible,
                eligible_https = https_stats.eligible,
                http_forward_available = http_stats.http_forward_available,
                http_forward_unavailable = http_stats.http_forward_unavailable,
                http_forward_unknown = http_stats.http_forward_unknown,
                https_tunnel_available = https_stats.https_tunnel_available,
                https_tunnel_unavailable = https_stats.https_tunnel_unavailable,
                https_tunnel_unknown = https_stats.https_tunnel_unknown,
                destination_tls_available = https_stats.destination_tls_available,
                destination_tls_unavailable = https_stats.destination_tls_unavailable,
                destination_tls_unknown = https_stats.destination_tls_unknown,
                https_rejected = https_stats.https_rejected,
                ko = http_stats.ko,
                auth_required = http_stats.authentication_required,
                cooldown = http_stats.cooldown,
                "dynamic proxy inventory updated"
            );
        }
    }
}

/// Reports completion or cancellation of one shared probe task.
pub(super) struct SharedExecution {
    /// Identity of the shared technical operation.
    pub(super) key: String,
    /// Identity of the runtime-owned task being observed.
    pub(super) operation_id: tokio::task::Id,
    /// Whether the shared operation returned before destruction.
    pub(super) completed: bool,
    /// Monotonic start of the observed operation.
    pub(super) started: std::time::Instant,
}

impl Drop for SharedExecution {
    fn drop(&mut self) {
        tracing::debug!(target: "arachnea_proxy::core::proxy_inventory::probing",work = %self.key, completed = self.completed,
            operation_id = %self.operation_id,
            elapsed_ms = self.started.elapsed().as_millis(),
            elapsed_us = self.started.elapsed().as_micros(),
            "finished shared proxy operation");
    }
}
