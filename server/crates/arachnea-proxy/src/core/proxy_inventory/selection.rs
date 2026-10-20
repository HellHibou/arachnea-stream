//! Request-scoped selection across cached candidates and provider discovery.

use std::sync::Arc;
use std::time::SystemTime;

use futures::future::BoxFuture;
use futures::{stream, FutureExt, StreamExt};
use tokio::sync::Mutex;

use super::eligibility::SelectionStats;
use super::ProxyInventory;
use crate::core::{
    Destination, ProbeMode, ProxyCapabilityStatus, ProxyError, ProxyRecord, ProxySearchContext,
    Result,
};

use super::eligibility::{
    epoch_millis, is_eligible, is_selection_candidate, needs_capability_probe, probe_is_expired,
    recent_probe_attempt, selection_stats,
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
    /// Accepted history is consulted first. Persistent country caches are imported
    /// before redirect evidence, coordinated affinity reuse and candidate exploration.
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
        let search = ProxySearchContext::current().unwrap_or_default();
        // A new pool selection supersedes any selection not yet claimed by a token.
        search.take_discovery();
        let mut reservation = DiscoveryReservation {
            search: search.clone(),
            selected: false,
        };
        let result = search
            .scope(search.technical(self.select_any_with_search(
                countries,
                require_https,
                destination,
                affinity,
            )))
            .await
            .and_then(|result| result);
        reservation.selected = result.is_ok();
        if let Err(error) = &result {
            let failure_kind = match error {
                ProxyError::Timeout(_) => "timeout",
                ProxyError::RouteUnavailable(_) => "route_unavailable",
                _ => "selection",
            };
            tracing::warn!(
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
            tracing::info!(search_id = search.id(), %destination, countries = ?attempted, "coordinating destination proxy discovery");
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
        let initial_failures = *failures.borrow();
        let mut revisions = self.discovery_revision.subscribe();
        let refresh_after = tokio::time::sleep(self.config.exploration_before_refresh);
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
                    || (*failures.borrow()).saturating_sub(initial_failures)
                        >= self.config.failures_before_refresh.max(1))
            {
                refreshing = true;
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
                _ = &mut refresh_after, if !refreshing => { refreshing = true; }
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
        if let Some(probe) = &self.probe {
            let mut inner = self.inner.write().await;
            for record in inner.records.values_mut().filter(|record| {
                record
                    .country
                    .as_ref()
                    .is_some_and(|country| countries.contains(country))
            }) {
                probe.prepare_context(record);
            }
        }
        let inner = self.inner.read().await;
        let now = SystemTime::now();

        let records: Vec<&ProxyRecord> = countries
            .iter()
            .filter_map(|country| inner.country_records.get(country))
            .flatten()
            .filter_map(|key| inner.records.get(key))
            .collect();
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
            tracing::debug!(
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
            return None;
        }

        candidates.sort_by(|a, b| {
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

        let candidates: Vec<ProxyRecord> = candidates.into_iter().cloned().collect();
        drop(inner);
        let mut pending = Vec::new();
        for candidate in candidates {
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
                tracing::info!(country = ?candidate.country, proxy = %candidate.authority(), require_https,
                    "selected validated cached proxy without prechecks");
                return Some(candidate);
            }
            pending.push(candidate);
        }
        // Poll both stages together: a suspended precheck can own a commit lock
        // needed by a validation, even while all validation slots are occupied.
        let speculative = search.as_ref().is_some_and(|search| search.speculative);
        let validations = stream::iter(pending.into_iter().map(|candidate| async move {
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
                        let started = epoch_millis();
                        let network_started = std::time::Instant::now();
                        tracing::debug!(proxy = %candidate.authority(), "started proxy TCP network precheck");
                        let result = probe.precheck_endpoint(&candidate).await;
                        let reachable = result.is_ok();
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
            let latest = self
                .inner
                .read()
                .await
                .records
                .get(&validated.authority())
                .cloned()?;
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
        }))
        .buffer_unordered(if speculative {
            1
        } else {
            self.config.probe_batch_size.clamp(1, 8)
        });
        futures::pin_mut!(validations);
        while let Some(result) = validations.next().await {
            if result.is_some() {
                return result;
            }
        }
        None
    }

    pub(super) async fn selection_stats_for_country(
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
