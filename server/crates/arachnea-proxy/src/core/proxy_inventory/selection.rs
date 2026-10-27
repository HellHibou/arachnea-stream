//! Request-scoped selection across cached candidates and provider discovery.

use std::sync::Arc;
use std::time::SystemTime;

use futures::future::BoxFuture;
use futures::{stream, FutureExt, StreamExt};
use tokio::sync::Mutex;

#[cfg(feature = "performance-diagnostics")]
use super::performance_diagnostics::{self, CandidateDiagnostic, NEXT_RANKING_ID};
use super::ProxyInventory;
use crate::core::{
    Destination, ProbeMode, ProxyCapabilityStatus, ProxyError, ProxyRecord, ProxySearchContext,
    Result,
};
#[cfg(feature = "performance-diagnostics")]
use crate::PROXY_RANKING_DIAGNOSTIC_LIMIT as RANKING_DIAGNOSTIC_LIMIT;

use super::eligibility::{
    epoch_millis, is_eligible, is_selection_candidate, needs_capability_probe, probe_is_expired,
    recent_probe_attempt,
};

struct DiscoveryReservation {
    search: ProxySearchContext,
    selected: bool,
}

impl Drop for DiscoveryReservation {
    fn drop(&mut self) {
        if !self.selected {
            self.search.take_discovery();
            if let Some(owner) = &self.search.continuation {
                owner.cancel();
            }
        }
    }
}

impl ProxyInventory {
    // ── Selection ────────────────────────────────────────────────────

    /// Selects the best available proxy for a country.
    ///
    /// When no candidate is available and a provider is configured, triggers a
    /// lazy load and retries once.
    ///
    /// # Parameters
    ///
    /// - `country`: ISO country code.
    /// - `require_https`: If `true`, only proxies with confirmed HTTPS support
    ///   are eligible.
    ///
    /// # Returns
    ///
    /// The best matching proxy record.
    ///
    /// # Errors
    ///
    /// Returns an error when no candidate is available after loading.
    pub async fn select(&self, country: &str, require_https: bool) -> Result<ProxyRecord> {
        self.select_for_destination(country, require_https, None)
            .await
    }

    /// Selects the best cached candidate across the allowed countries,
    /// independently of their input order, before consulting providers.
    pub async fn select_any(
        &self,
        countries: &[String],
        require_https: bool,
    ) -> Result<ProxyRecord> {
        self.select_any_for_destination(countries, require_https, None)
            .await
    }

    /// Selects the best available proxy for a country and destination.
    ///
    /// Destination-specific cooldowns are applied when `destination` is
    /// supplied. When no candidate is available and a provider is configured,
    /// this triggers a lazy load and retries once.
    ///
    /// # Arguments
    ///
    /// - `country`: ISO country code.
    /// - `require_https`: If `true`, only proxies that can reach HTTPS
    ///   destinations are eligible.
    /// - `destination`: Optional final destination used to exclude proxies
    ///   temporarily cooled down for that origin.
    ///
    /// # Errors
    ///
    /// Returns an error when no candidate is available after loading.
    pub async fn select_for_destination(
        &self,
        country: &str,
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Result<ProxyRecord> {
        self.select_any_for_destination(&[country.to_string()], require_https, destination)
            .await
    }

    /// Selects the best eligible candidate across an unordered set of countries.
    ///
    /// Exact accepted history is consulted first. Domain-group and hierarchical hints
    /// follow exact redirect evidence and explicit affinity, before country imports.
    /// Cold business discovery is exclusive per origin/country set until its outcome;
    /// accepted endpoints remain available for parallel requests.
    pub async fn select_any_for_destination(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Result<ProxyRecord> {
        self.select_any_for_destination_with_affinity(countries, require_https, destination, None)
            .await
    }

    /// Selects a proxy for a destination while reusing an eligible proxy bound
    /// to the supplied opaque affinity key.
    pub async fn select_any_for_destination_with_affinity(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
        affinity: Option<&str>,
    ) -> Result<ProxyRecord> {
        self.select_any_for_destination_with_domain_group(
            countries,
            require_https,
            destination,
            affinity,
            &[],
        )
        .await
    }

    /// Selects with exact preferences, optional domain-group defaults and hostname hierarchy.
    ///
    /// # Arguments
    /// - `countries`: Allowed proxy country set.
    /// - `require_https`: Whether HTTPS capabilities are required.
    /// - `destination`: Exact target of the business request.
    /// - `affinity`: Optional existing request affinity, retaining priority.
    /// - `domain_group`: Optional domains whose accepted endpoints can supply defaults.
    ///
    /// # Errors
    /// Returns an error when the search expires or no eligible endpoint is found.
    pub async fn select_any_for_destination_with_domain_group(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
        affinity: Option<&str>,
        domain_group: &[String],
    ) -> Result<ProxyRecord> {
        let search = ProxySearchContext::current()
            .unwrap_or_default()
            .for_refresh_destination(destination);
        #[cfg(feature = "performance-diagnostics")]
        let selection_started = std::time::Instant::now();
        // A new pool selection supersedes any selection not yet claimed by a token.
        search.take_discovery();
        let mut reservation = DiscoveryReservation {
            search: search.clone(),
            selected: false,
        };
        let result = search
            .scope(search.technical(Box::pin(self.select_any_with_search(
                countries,
                require_https,
                destination,
                affinity,
                domain_group,
            ))))
            .await
            .and_then(|result| result);
        reservation.selected = result.is_ok();
        #[cfg(feature = "performance-diagnostics")]
        performance_diagnostics::finished_selection(
            &search,
            require_https,
            &result,
            selection_started,
        );
        if let Err(error) = &result {
            let failure_kind = match error {
                ProxyError::Timeout(_) => "timeout",
                ProxyError::RouteUnavailable(_) => "route_unavailable",
                _ => "selection",
            };
            tracing::debug!(
                search_id = search.id(),
                failure_kind,
                phase = "selection",
                remaining_ms = search.remaining().as_millis(),
                "localized proxy selection failed"
            );
        }
        result
    }

    async fn select_any_with_search(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
        affinity: Option<&str>,
        domain_group: &[String],
    ) -> Result<ProxyRecord> {
        let attempted = crate::core::canonical_proxy_countries(countries);
        if attempted.is_empty() {
            return Err(ProxyError::RouteUnavailable(
                "no proxy countries were requested".to_string(),
            ));
        }

        if let Some(continuation) =
            ProxySearchContext::current().and_then(|search| search.continuation)
        {
            continuation.set_route(attempted.clone(), require_https, destination.cloned());
        }
        let affinity = affinity.map(str::trim).filter(|value| !value.is_empty());
        if let Some(record) = self
            .select_preferred_destination(&attempted, require_https, destination)
            .await
        {
            self.bind_affinity(affinity, &record).await;
            return Ok(record);
        }
        if let Some((related, source_origin, candidate_source)) = self
            .select_related_origin(&attempted, require_https, destination, domain_group)
            .await
        {
            let record = if let Some(record) = self
                .select_preferred_destination(&attempted, require_https, destination)
                .await
            {
                record
            } else if let Some(record) = self
                .select_redirect_destination(&attempted, require_https, destination)
                .await
            {
                record
            } else if let Some(record) = self
                .select_affinity_binding(&attempted, require_https, destination, affinity)
                .await
            {
                record
            } else {
                if let (Some(search), Some(destination)) =
                    (ProxySearchContext::current(), destination)
                {
                    search.mark_selection(
                        &related.authority(),
                        destination,
                        crate::core::ProxySelectionProvenance::Discovery,
                    );
                    tracing::debug!(search_id = search.id(), candidate_source,
                        source_origin, %destination, proxy = %related.authority(),
                        "selected related-origin proxy candidate with discovery profile");
                }
                related
            };
            self.bind_affinity(affinity, &record).await;
            return Ok(record);
        }
        #[cfg(feature = "persistence")]
        stream::iter(
            attempted
                .clone()
                .into_iter()
                .map(|country| async move { self.load_cached_country(&country).await }),
        )
        .buffer_unordered(2)
        .collect::<Vec<_>>()
        .await;
        let search = ProxySearchContext::current().expect("selection is search scoped");
        if let Some(record) = self
            .select_redirect_destination(&attempted, require_https, destination)
            .await
        {
            self.bind_affinity(affinity, &record).await;
            return Ok(record);
        }
        if let Some(destination) = destination {
            let key = format!(
                "{}|{}",
                crate::core::ProxyDestinationPreference::destination_key(destination),
                attempted.join(",")
            );
            let gate = {
                let mut gates = self.destination_searches.lock().await;
                gates.retain(|_, gate| gate.strong_count() > 0);
                if let Some(gate) = gates.get(&key).and_then(std::sync::Weak::upgrade) {
                    gate
                } else {
                    let gate = Arc::new(Mutex::new(()));
                    gates.insert(key, Arc::downgrade(&gate));
                    gate
                }
            };
            let mut changes = self.discovery_revision.subscribe();
            let acquire = gate.lock_owned();
            tokio::pin!(acquire);
            tracing::debug!(search_id = search.id(), %destination, countries = ?attempted, "coordinating destination proxy discovery");
            loop {
                if let Some(record) = self
                    .select_preferred_destination(&attempted, require_https, Some(destination))
                    .await
                {
                    self.bind_affinity(affinity, &record).await;
                    return Ok(record);
                }
                if let Some(record) = self
                    .select_redirect_destination(&attempted, require_https, Some(destination))
                    .await
                {
                    self.bind_affinity(affinity, &record).await;
                    return Ok(record);
                }
                tokio::select! {
                    guard = &mut acquire => {
                        search.hold_discovery(guard);
                        break;
                    }
                    _ = changes.changed() => {}
                }
            }
            // The previous leader may have published a success immediately before release.
            if let Some(record) = self
                .select_preferred_destination(&attempted, require_https, Some(destination))
                .await
            {
                search.take_discovery();
                self.bind_affinity(affinity, &record).await;
                return Ok(record);
            }
            if let Some(record) = self
                .select_redirect_destination(&attempted, require_https, Some(destination))
                .await
            {
                search.take_discovery();
                self.bind_affinity(affinity, &record).await;
                return Ok(record);
            }
        }
        if let Some(record) = self
            .select_affinity_binding(&attempted, require_https, destination, affinity)
            .await
        {
            return Ok(record);
        }
        let mut failures = search.validation_failures();
        let mut revisions = self.discovery_revision.subscribe();
        let (refresh_deadline, _) = search.provider_refresh_progress(
            self.config.exploration_before_refresh,
            self.config.failures_before_refresh,
        );
        let refresh_after = tokio::time::sleep_until(refresh_deadline);
        futures::pin_mut!(refresh_after);
        let mut selectors =
            futures::stream::FuturesUnordered::<BoxFuture<'_, Option<ProxyRecord>>>::new();
        selectors.push(
            self.select_cached_any(&attempted, require_https, destination)
                .boxed(),
        );
        let countries = self
            .discovery_order(&attempted, require_https, destination)
            .await;
        let loads = async {
            let loads = stream::iter(countries.into_iter().map(|country| async move {
                self.load_if_needed(&country).await;
            }))
            .buffer_unordered(2);
            loads.collect::<Vec<_>>().await;
        }
        .boxed();
        futures::pin_mut!(loads);
        let mut refreshing = false;
        let mut loaded = false;
        let mut dirty = false;
        loop {
            if !refreshing
                && (selectors.is_empty()
                    || search
                        .provider_refresh_progress(
                            self.config.exploration_before_refresh,
                            self.config.failures_before_refresh,
                        )
                        .1)
            {
                refreshing = true;
                search.allow_provider_refresh();
                tracing::debug!(
                    search_id = search.id(),
                    ?destination,
                    reason = if selectors.is_empty() {
                        "cached_candidates_exhausted"
                    } else {
                        "retained_exploration_progress"
                    },
                    "enabled localized proxy provider refresh"
                );
            }
            if dirty && selectors.len() < 2 {
                dirty = false;
                selectors.push(
                    self.select_cached_any(&attempted, require_https, destination)
                        .boxed(),
                );
            }
            if loaded && selectors.is_empty() {
                break;
            }
            tokio::select! {
                result = selectors.next(), if !selectors.is_empty() => {
                    if let Some(Some(record)) = result {
                        if let Some(preferred) = self.select_preferred_destination(&attempted, require_https, destination).await {
                            search.take_discovery();
                            self.bind_affinity(affinity, &preferred).await;
                            return Ok(preferred);
                        }
                        self.bind_affinity(affinity, &record).await;
                        return Ok(record);
                    }
                }
                _ = &mut refresh_after, if !refreshing => {
                    refreshing = true;
                    search.allow_provider_refresh();
                    tracing::debug!(search_id = search.id(), ?destination,
                        reason = "exploration_deadline", "enabled localized proxy provider refresh");
                }
                _ = failures.changed(), if !refreshing => {}
                _ = revisions.changed() => {
                    if let Some(record) = self.select_preferred_destination(&attempted, require_https, destination).await {
                        search.take_discovery();
                        self.bind_affinity(affinity, &record).await;
                        return Ok(record);
                    }
                    if let Some(record) = self.select_redirect_destination(&attempted, require_https, destination).await {
                        search.take_discovery();
                        self.bind_affinity(affinity, &record).await;
                        return Ok(record);
                    }
                    dirty = true;
                }
                _ = &mut loads, if refreshing && !loaded => { loaded = true; dirty = true; }
            }
        }

        if self.probe_mode() == ProbeMode::Strict
            && self.inner.read().await.records.values().any(|record| {
                record
                    .country
                    .as_ref()
                    .is_some_and(|country| attempted.contains(country))
                    && record.tcp_reachable == Some(true)
                    && needs_capability_probe(record, require_https)
                    && is_selection_candidate(
                        record,
                        record.country.as_deref().unwrap_or_default(),
                        require_https,
                        destination,
                        SystemTime::now(),
                        ProbeMode::Strict,
                    )
            })
        {
            return Err(ProxyError::RouteUnavailable(format!(
                "strict proxy validation remained inconclusive for countries [{}]",
                attempted.join(", ")
            )));
        }
        Err(ProxyError::RouteUnavailable(format!(
            "no working proxy available for requested countries [{}]",
            attempted.join(", ")
        )))
    }

    /// Ranks cached candidates across all allowed countries before any probes.
    pub(super) async fn select_cached_any(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        #[cfg(feature = "performance-diagnostics")]
        let ranking_started = std::time::Instant::now();
        #[cfg(feature = "performance-diagnostics")]
        let ranking_id = NEXT_RANKING_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(feature = "performance-diagnostics")]
        let diagnostics_enabled = tracing::enabled!(tracing::Level::DEBUG);
        if let Some(probe) = &self.probe {
            let mut inner = self.inner.write().await;
            probe.prepare_contexts(inner.records.values_mut().filter(|record| {
                record
                    .country
                    .as_ref()
                    .is_some_and(|country| countries.contains(country))
            }));
        }
        let inner = self.inner.read().await;
        let now = SystemTime::now();

        let records: Vec<&ProxyRecord> = countries
            .iter()
            .filter_map(|country| inner.country_records.get(country))
            .flatten()
            .filter_map(|key| inner.records.get(key))
            .collect();
        #[cfg(feature = "performance-diagnostics")]
        let record_count = records.len();
        #[cfg(feature = "performance-diagnostics")]
        self.log_selection_candidates(&records, countries, require_https, destination, now);

        let search = ProxySearchContext::current();
        let mut candidates: Vec<&ProxyRecord> = records
            .into_iter()
            .filter(|r| {
                !search.as_ref().is_some_and(|search| {
                    search.attempted(&r.authority(), destination)
                        || search.preparation_exclusions.contains(&r.authority())
                }) && is_selection_candidate(
                    r,
                    r.country.as_deref().unwrap_or_default(),
                    require_https,
                    destination,
                    now,
                    self.probe_mode(),
                )
            })
            .collect();

        if candidates.is_empty() {
            #[cfg(feature = "performance-diagnostics")]
            tracing::debug!(
                search_id = search.as_ref().map(ProxySearchContext::id),
                ranking_id,
                records = record_count,
                candidates = 0,
                duration_us = ranking_started.elapsed().as_micros(),
                "prepared proxy candidate ranking"
            );
            return None;
        }

        let rank = |record: &ProxyRecord| {
            if !needs_capability_probe(record, require_https) {
                if probe_is_expired(record, now, self.config.probe_ttl) {
                    1
                } else {
                    0
                }
            } else if if require_https {
                record.https_tunnel == ProxyCapabilityStatus::Available
            } else {
                record.http_forwarding == ProxyCapabilityStatus::Available
            } {
                2
            } else {
                3
            }
        };
        candidates.sort_by(|a, b| {
            rank(a)
                .cmp(&rank(b))
                .then_with(|| {
                    a.latency_ms
                        .unwrap_or(u64::MAX)
                        .cmp(&b.latency_ms.unwrap_or(u64::MAX))
                })
                .then(a.failure_count.cmp(&b.failure_count))
                .then_with(|| a.authority().cmp(&b.authority()))
        });

        #[cfg(feature = "performance-diagnostics")]
        self.log_ranked_candidates(
            diagnostics_enabled,
            ranking_id,
            search.as_ref(),
            &candidates,
            destination,
            require_https,
            now,
            rank,
        );
        let candidates: Vec<ProxyRecord> = candidates.into_iter().cloned().collect();
        drop(inner);
        #[cfg(feature = "performance-diagnostics")]
        tracing::debug!(
            search_id = search.as_ref().map(ProxySearchContext::id),
            ranking_id,
            destination = ?destination,
            require_https,
            records = record_count,
            candidates = candidates.len(),
            sampled_candidates = if diagnostics_enabled { candidates.len().min(RANKING_DIAGNOSTIC_LIMIT) } else { 0 },
            duration_us = ranking_started.elapsed().as_micros(),
            "prepared proxy candidate ranking"
        );
        let mut pending = Vec::new();
        for (_position, candidate) in candidates.into_iter().enumerate() {
            #[cfg(feature = "performance-diagnostics")]
            let position = _position;
            let Some(candidate) = self
                .inner
                .read()
                .await
                .records
                .get(&candidate.authority())
                .cloned()
            else {
                continue;
            };
            let Some(country) = candidate
                .country
                .as_ref()
                .filter(|country| countries.contains(country))
            else {
                continue;
            };
            if !is_selection_candidate(
                &candidate,
                country,
                require_https,
                destination,
                SystemTime::now(),
                self.probe_mode(),
            ) {
                continue;
            }
            if !needs_capability_probe(&candidate, require_https)
                && !probe_is_expired(&candidate, SystemTime::now(), self.config.probe_ttl)
            {
                tracing::debug!(country = ?candidate.country, proxy = %candidate.authority(), require_https,
                    "selected validated cached proxy without prechecks");
                #[cfg(feature = "performance-diagnostics")]
                tracing::debug!(
                    ranking_id,
                    search_id = search.as_ref().map(ProxySearchContext::id),
                    position = position + 1,
                    proxy = %candidate.authority(),
                    path = "ready_cache",
                    "selected proxy ranking result"
                );
                return Some(candidate);
            }
            #[cfg(feature = "performance-diagnostics")]
            pending.push((position + 1, candidate));
            #[cfg(not(feature = "performance-diagnostics"))]
            pending.push(candidate);
        }
        // Poll both stages together: a suspended precheck can own a commit lock
        // needed by a validation, even while all validation slots are occupied.
        let speculative = search.as_ref().is_some_and(|search| search.speculative);
        // Keep buffered candidate state off the deeply nested media polling stack.
        #[cfg(feature = "performance-diagnostics")]
        let completion_count = std::sync::atomic::AtomicUsize::new(0);
        #[cfg(feature = "performance-diagnostics")]
        let completion_count = &completion_count;
        let validations = stream::iter(pending.into_iter().map(|queued| async move {
            #[cfg(feature = "performance-diagnostics")]
            let (position, candidate) = queued;
            #[cfg(not(feature = "performance-diagnostics"))]
            let candidate = queued;
            #[cfg(feature = "performance-diagnostics")]
            let mut diagnostic = (diagnostics_enabled && position <= RANKING_DIAGNOSTIC_LIMIT)
                .then(|| CandidateDiagnostic::new(ranking_id, position, &candidate));
            let result = async {
            let candidate = self.current_selection_candidate(&candidate.authority(), countries, require_https, destination).await?;
            if !needs_capability_probe(&candidate, require_https)
                && !probe_is_expired(&candidate, SystemTime::now(), self.config.probe_ttl) {
                tracing::debug!(proxy = %candidate.authority(), phase = "pipeline", reason = "fresh_capabilities",
                    "reused proxy evidence updated while queued");
                return Some(candidate);
            }
            let candidate = async {
                let Some(probe) = &self.probe else {
                    return Some(candidate);
                };
                if recent_probe_attempt(&candidate, "tcp", self.config.probe_ttl) {
                    return (candidate.tcp_reachable == Some(true)).then_some(candidate);
                }
                let key = format!("tcp:{}", candidate.authority());
                let inventory = self.clone_for_probe();
                let probe = probe.clone();
                self.shared_operation(
                    key,
                    false,
                    async move {
                        let _slot = inventory.acquire_probe_slot(false, speculative).await?;
                        let authority = candidate.authority();
                        let latest = inventory.inner.read().await.records.get(&authority).cloned();
                        let Some(candidate) = latest else {
                            tracing::debug!(proxy = %authority, phase = "tcp", reason = "removed",
                                "skipped stale queued proxy candidate");
                            return None;
                        };
                        if !is_selection_candidate(&candidate, candidate.country.as_deref()?, require_https, None, SystemTime::now(), inventory.probe_mode()) {
                            tracing::debug!(proxy = %authority, phase = "tcp", reason = "currently_ineligible",
                                "skipped stale queued proxy candidate");
                            return None;
                        }
                        if !needs_capability_probe(&candidate, require_https)
                            && !probe_is_expired(&candidate, SystemTime::now(), inventory.config.probe_ttl) {
                            tracing::debug!(proxy = %authority, phase = "tcp", reason = "fresh_capabilities",
                                "reused proxy evidence updated while queued");
                            return Some(candidate);
                        }
                        if recent_probe_attempt(&candidate, "tcp", inventory.config.probe_ttl) {
                            tracing::debug!(proxy = %authority, phase = "tcp", reason = "recent_tcp_attempt",
                                "reused proxy evidence updated while queued");
                            return (candidate.tcp_reachable == Some(true)).then_some(candidate);
                        }
                        let started = epoch_millis();
                        #[cfg(feature = "performance-diagnostics")]
                        let network_started = std::time::Instant::now();
                        tracing::debug!(proxy = %candidate.authority(), "started proxy TCP network precheck");
                        let result = probe.precheck_endpoint(&candidate).await;
                        let reachable = result.is_ok();
                        #[cfg(feature = "performance-diagnostics")]
                        tracing::debug!(proxy = %candidate.authority(), reachable,
                            duration_ms = network_started.elapsed().as_millis(), "completed proxy TCP network precheck");
                        inventory
                            .save_probe_attempt(
                                &candidate.authority(),
                                "tcp",
                                started,
                                Some(reachable),
                            )
                            .await;
                        reachable.then_some(candidate)
                    }
                    .boxed(),
                )
                .await
            }
            .await?;
            let candidate = self.current_selection_candidate(&candidate.authority(), countries, require_https, destination).await?;
            if !needs_capability_probe(&candidate, require_https)
                && !probe_is_expired(&candidate, SystemTime::now(), self.config.probe_ttl) {
                tracing::debug!(proxy = %candidate.authority(), phase = "capability", reason = "fresh_capabilities",
                    "reused proxy evidence updated while queued");
                return Some(candidate);
            }
            if self.probe.is_none() {
                return self
                    .validate_selection_candidate(candidate, require_https, destination)
                    .await;
            }
            let key = format!(
                "validate:{}:{}:{}",
                candidate.authority(),
                require_https,
                self.probe
                    .as_ref()
                    .map(|p| p.configuration_id())
                    .unwrap_or_default()
            );
            let inventory = self.clone_for_probe();
            let validated = self
                .shared_operation(
                    key,
                    true,
                    async move {
                        let _slot = inventory.acquire_probe_slot(true, speculative).await?;
                        inventory
                            .validate_selection_candidate(candidate, require_https, None)
                            .await
                    }
                    .boxed(),
                )
                .await?;
            let latest = self.current_selection_candidate(&validated.authority(), countries, require_https, destination).await?;
            (latest
                .country
                .as_ref()
                .is_some_and(|country| countries.contains(country))
                && is_eligible(
                    &latest,
                    latest.country.as_deref()?,
                    require_https,
                    destination,
                    SystemTime::now(),
                    self.probe_mode(),
                ))
            .then_some(latest)
            }.await;
            #[cfg(feature = "performance-diagnostics")]
            let completion_order = completion_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            #[cfg(feature = "performance-diagnostics")]
            if let Some(diagnostic) = &mut diagnostic {
                diagnostic.completed(completion_order, result.is_some());
            }
            #[cfg(feature = "performance-diagnostics")]
            { (position, result) }
            #[cfg(not(feature = "performance-diagnostics"))]
            { result }
        }.boxed()))
        .buffer_unordered(if speculative {
            1
        } else {
            self.config.probe_batch_size.clamp(1, 8)
        });
        futures::pin_mut!(validations);
        while let Some(completed) = validations.next().await {
            #[cfg(feature = "performance-diagnostics")]
            let (_position, result) = completed;
            #[cfg(not(feature = "performance-diagnostics"))]
            let result = completed;
            if let Some(_candidate) = &result {
                #[cfg(feature = "performance-diagnostics")]
                tracing::debug!(
                    ranking_id,
                    search_id = search.as_ref().map(ProxySearchContext::id),
                    position = _position,
                    proxy = %_candidate.authority(),
                    completed_candidates = completion_count.load(std::sync::atomic::Ordering::Relaxed),
                    path = "validation_pipeline",
                    "selected proxy ranking result"
                );
                return result;
            }
        }
        #[cfg(feature = "performance-diagnostics")]
        tracing::debug!(
            ranking_id,
            completed_candidates = completion_count.load(std::sync::atomic::Ordering::Relaxed),
            "exhausted proxy candidate ranking"
        );
        None
    }

    /// Rechecks a queued snapshot against current exclusions before its next phase.
    ///
    /// # Arguments
    /// - `authority`: Endpoint whose snapshot may have changed while waiting.
    /// - `countries`: Countries still permitted by this selection.
    /// - `require_https`: Whether the route requires HTTPS capability.
    /// - `destination`: Exact business origin, when known.
    async fn current_selection_candidate(
        &self,
        authority: &str,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        let candidate = self.inner.read().await.records.get(authority).cloned()?;
        let search = ProxySearchContext::current();
        let reason = if search
            .as_ref()
            .is_some_and(|search| search.attempted(authority, destination))
        {
            Some("already_attempted")
        } else if search
            .as_ref()
            .is_some_and(|search| search.preparation_exclusions.contains(authority))
        {
            Some("preparation_exclusion")
        } else if !candidate.country.as_ref().is_some_and(|country| {
            countries.contains(country)
                && is_selection_candidate(
                    &candidate,
                    country,
                    require_https,
                    destination,
                    SystemTime::now(),
                    self.probe_mode(),
                )
        }) {
            Some("currently_ineligible")
        } else {
            None
        };
        if let Some(reason) = reason {
            tracing::debug!(
                search_id = search.as_ref().map(ProxySearchContext::id),
                proxy = authority,
                reason,
                "skipped stale queued proxy candidate"
            );
            return None;
        }
        Some(candidate)
    }
}
