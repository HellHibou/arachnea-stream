//! Destination preferences, redirect evidence and request affinity.

use std::sync::Arc;
use std::time::{Instant, SystemTime};

use tokio::sync::Mutex;

use super::config::PROXY_AFFINITY_TTL;
use super::{ProxyAffinityBinding, ProxyInventory};
use crate::core::{Destination, ProxyRecord, ProxySearchContext};

use super::clear_superseded_destination_failures;
use super::eligibility::{
    has_active_destination_cooldown, is_eligible, is_selection_candidate, needs_capability_probe,
    probe_is_expired,
};

impl ProxyInventory {
    /// Returns a shared per-destination lock, pruning unused lock entries.
    ///
    /// # Arguments
    /// * `key` - Exact-destination history key identifying the shared commit/selection lock.
    pub(super) async fn destination_lock(&self, key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.destination_locks.lock().await;
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        locks
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Reuses the proxy accepted by this exact origin before any new endpoint
    /// prechecks. Probe age alone does not invalidate a successful origin binding.
    ///
    /// # Arguments
    /// * `countries` - Allowed ISO alpha-2 country codes.
    /// * `require_https` - Whether selection requires an HTTPS tunnel and destination TLS capability.
    /// * `destination` - Final origin used for capability checks and destination-specific exclusions.
    ///
    pub(super) async fn select_preferred_destination(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        let destination = destination?;
        let key = crate::core::ProxyDestinationPreference::destination_key(destination);
        let lock = self.destination_lock(&key).await;
        let _guard = lock.lock().await;
        #[cfg(feature = "persistence")]
        if !self
            .inner
            .read()
            .await
            .preferred_by_destination
            .contains_key(&key)
        {
            if let Some(repository) = &self.preferences {
                match repository.get(&key).await {
                    Ok(Some(mut preference)) => {
                        let mut present = Vec::new();
                        if let Some(inventory) = &self.persistence {
                            for item in &preference.endpoints {
                                if self
                                    .inner
                                    .read()
                                    .await
                                    .records
                                    .values()
                                    .any(|record| record.key() == item.endpoint)
                                {
                                    present.push(item.clone());
                                    continue;
                                }
                                match inventory.get(&item.endpoint).await {
                                    Ok(Some(record)) => {
                                        self.add_or_update(vec![record]).await;
                                        present.push(item.clone());
                                    }
                                    Ok(None) => {
                                        if let Err(error) =
                                            repository.remove_endpoint(&item.endpoint).await
                                        {
                                            tracing::warn!(%error, destination = %key, "failed to clean orphan proxy preference");
                                        }
                                    }
                                    Err(error) => {
                                        tracing::warn!(%error, destination = %key, "failed to restore preferred proxy endpoint");
                                        present.push(item.clone());
                                    }
                                }
                            }
                            preference.endpoints = present;
                        }
                        self.inner
                            .write()
                            .await
                            .preferred_by_destination
                            .insert(key.clone(), preference);
                    }
                    Err(error) => {
                        tracing::warn!(%error, destination = %key, "failed to restore proxy preference")
                    }
                    _ => {}
                }
            }
        }
        let repaired = {
            let mut inner = self.inner.write().await;
            let endpoints = inner.preferred_by_destination.get(&key)?.endpoints.clone();
            let mut repaired = Vec::new();
            for item in endpoints {
                if let Some(record) = inner
                    .records
                    .values_mut()
                    .find(|record| record.key() == item.endpoint)
                {
                    if clear_superseded_destination_failures(record, destination, item.accepted_at)
                    {
                        repaired.push((record.clone(), item.accepted_at));
                    }
                }
            }
            repaired
        };
        #[cfg(feature = "persistence")]
        for (record, accepted_at) in &repaired {
            self.persist_accepted_record(record, destination, *accepted_at)
                .await;
        }
        #[cfg(not(feature = "persistence"))]
        drop(repaired);
        let inner = self.inner.read().await;
        let preference = inner.preferred_by_destination.get(&key)?;
        for item in &preference.endpoints {
            let Some(record) = inner
                .records
                .values()
                .find(|record| record.key() == item.endpoint)
            else {
                continue;
            };
            let Some(country) = record.country.as_ref().filter(|country| {
                countries
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(country))
            }) else {
                continue;
            };
            if !ProxySearchContext::current()
                .is_some_and(|search| search.attempted(&record.authority(), Some(destination)))
                && is_eligible(
                    record,
                    country,
                    require_https,
                    Some(destination),
                    SystemTime::now(),
                    self.probe_mode(),
                )
            {
                if let Some(search) = ProxySearchContext::current() {
                    search.mark_selection(
                        &record.authority(),
                        destination,
                        crate::core::ProxySelectionProvenance::AcceptedDestination,
                    );
                }
                tracing::debug!(
                    search_id = ProxySearchContext::current().map(|search| search.id()),
                    provenance = "accepted_destination",
                    destination = %destination,
                    proxy = %record.authority(),
                    country,
                    "reusing proxy validated for destination without discovery"
                );
                return Some(record.clone());
            }
            tracing::debug!(search_id = ProxySearchContext::current().map(|search| search.id()),
                %destination, proxy = %record.authority(),
                destination_cooldown = has_active_destination_cooldown(record, destination, SystemTime::now()),
                status = ?record.status,
                already_attempted = ProxySearchContext::current().is_some_and(|search| search.attempted(&record.authority(), Some(destination))),
                "preferred proxy currently ineligible");
        }
        None
    }

    /// Reuses eligible accepted history recorded along the current redirect chain.
    ///
    /// # Arguments
    /// * `countries` - Allowed ISO alpha-2 country codes.
    /// * `require_https` - Whether selection requires an HTTPS tunnel and destination TLS capability.
    /// * `destination` - Final origin used for capability checks and destination-specific exclusions.
    pub(super) async fn select_redirect_destination(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        let destination = destination?;
        let target = destination.to_string();
        let now = SystemTime::now();
        let search = ProxySearchContext::current();
        let inner = self.inner.read().await;
        let record = inner
            .records
            .values()
            .filter(|record| {
                record.country.as_ref().is_some_and(|country| {
                    countries.contains(country)
                        && is_eligible(
                            record,
                            country,
                            require_https,
                            Some(destination),
                            now,
                            self.probe_mode(),
                        )
                }) && !search
                    .as_ref()
                    .is_some_and(|search| search.attempted(&record.authority(), Some(destination)))
            })
            .filter_map(|record| {
                record
                    .observations
                    .iter()
                    .filter(|observation| {
                        observation.context.configuration == "business-redirect-v1"
                            && observation.context.target.as_deref() == Some(target.as_str())
                            && observation.outcome
                                == crate::core::ProxyObservationOutcome::Succeeded
                    })
                    .map(|observation| (observation.completed_at, record))
                    .max_by_key(|(date, _)| *date)
            })
            .max_by(|(a_date, a), (b_date, b)| {
                a_date
                    .cmp(b_date)
                    .then_with(|| b.authority().cmp(&a.authority()))
            })?
            .1;
        if let Some(search) = &search {
            search.mark_selection(
                &record.authority(),
                destination,
                crate::core::ProxySelectionProvenance::RedirectDestination,
            );
        }
        tracing::debug!(search_id = search.map(|search| search.id()), %destination, proxy = %record.authority(), provenance = "redirect_destination", "reusing proxy with followed redirect evidence");
        Some(record.clone())
    }

    /// Reuses an unexpired affinity binding only while its endpoint remains eligible.
    ///
    /// # Arguments
    /// * `countries` - Allowed ISO alpha-2 country codes.
    /// * `require_https` - Whether selection requires an HTTPS tunnel and destination TLS capability.
    /// * `destination` - Final origin used for capability checks and destination-specific exclusions.
    /// * `affinity` - Optional opaque affinity key; absent or blank keys disable affinity reuse.
    pub(super) async fn select_affinity_binding(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
        affinity: Option<&str>,
    ) -> Option<ProxyRecord> {
        let affinity = affinity?;
        let now = Instant::now();
        let system_now = SystemTime::now();
        let mut inner = self.inner.write().await;
        inner
            .affinity_bindings
            .retain(|_, binding| binding.expires_at > now);
        let authority = inner.affinity_bindings.get(affinity)?.authority.clone();
        let record = inner.records.get(&authority).filter(|record| {
            record
                .country
                .as_ref()
                .is_some_and(|country| countries.iter().any(|wanted| wanted == country))
                && !ProxySearchContext::current()
                    .is_some_and(|search| search.attempted(&record.authority(), destination))
                && is_selection_candidate(
                    record,
                    record.country.as_deref().unwrap_or_default(),
                    require_https,
                    destination,
                    system_now,
                    self.probe_mode(),
                )
        });
        if let Some(record) = record {
            let record = record.clone();
            drop(inner);
            if !needs_capability_probe(&record, require_https)
                && !probe_is_expired(&record, system_now, self.config.probe_ttl)
            {
                return is_eligible(
                    &record,
                    record.country.as_deref()?,
                    require_https,
                    destination,
                    system_now,
                    self.probe_mode(),
                )
                .then_some(record);
            }
            return None;
        }
        inner.affinity_bindings.remove(affinity);
        None
    }

    /// Binds a supplied affinity key to the selected endpoint with a fresh expiry.
    ///
    /// # Arguments
    /// * `affinity` - Optional opaque key; `None` skips binding. Callers normalize blank keys.
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    pub(super) async fn bind_affinity(&self, affinity: Option<&str>, record: &ProxyRecord) {
        let Some(affinity) = affinity else {
            return;
        };
        self.inner.write().await.affinity_bindings.insert(
            affinity.to_string(),
            ProxyAffinityBinding {
                authority: record.authority(),
                expires_at: Instant::now() + PROXY_AFFINITY_TTL,
            },
        );
    }

    /// Removes an affinity binding when its selected proxy is rejected.
    ///
    /// # Arguments
    /// * `affinity` - Affinity key whose binding is removed only if it still names this endpoint.
    /// * `authority` - Proxy endpoint identity in host:port form, with IPv6 brackets where needed.
    ///
    pub async fn clear_affinity(&self, affinity: &str, authority: &str) {
        let mut inner = self.inner.write().await;
        if inner
            .affinity_bindings
            .get(affinity)
            .is_some_and(|binding| binding.authority == authority)
        {
            inner.affinity_bindings.remove(affinity);
        }
    }

    /// Renews an affinity binding after an accepted origin response.
    ///
    /// # Arguments
    /// * `affinity` - Affinity key assigned to this endpoint, replacing any prior binding and resetting its TTL.
    /// * `authority` - Proxy endpoint identity in host:port form, with IPv6 brackets where needed.
    ///
    pub async fn renew_affinity(&self, affinity: &str, authority: &str) {
        self.inner.write().await.affinity_bindings.insert(
            affinity.to_string(),
            ProxyAffinityBinding {
                authority: authority.to_string(),
                expires_at: Instant::now() + PROXY_AFFINITY_TTL,
            },
        );
    }
}
