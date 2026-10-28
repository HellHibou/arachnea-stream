//! Candidate hints derived from accepted histories, never target acceptance.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use super::eligibility::{is_eligible, needs_capability_probe};
use super::ProxyInventory;
use crate::core::{
    Destination, ProxyAcceptedEndpoint, ProxyDestinationPreference, ProxyKey, ProxyRecord,
    ProxySearchContext,
};

/// Normalized exact HTTP(S) origin decoded from an existing history key.
struct Origin {
    scheme: String,
    host: String,
    port: u16,
}

impl Origin {
    /// Builds a related-origin grouping key from scheme, hostname suffix and port.
    ///
    /// # Arguments
    /// * `suffix` - Hostname suffix used to group related origins with the same scheme and port.
    fn key(&self, suffix: &str) -> String {
        format!("{}|{suffix}|{}", self.scheme, self.port)
    }

    /// Formats the origin scheme, host and port for diagnostic output.
    fn name(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }
}

/// Decodes an existing length-prefixed history identity without suffix data.
///
/// # Arguments
/// - `key`: Exact-destination history key, preserving the persisted format.
fn origin_from_key(key: &str) -> Option<Origin> {
    /// Consumes one length-prefixed history-key component and returns the remaining suffix.
    ///
    /// # Arguments
    ///
    /// * `value` - Encoded component beginning with its UTF-8 byte length and a colon.
    fn component(value: &str) -> Option<(&str, &str)> {
        let (length, rest) = value.split_once(':')?;
        let length: usize = length.parse().ok()?;
        Some((rest.get(..length)?, rest.get(length..)?.strip_prefix('|')?))
    }
    let (scheme, rest) = component(key)?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    let (host, port) = component(rest)?;
    let port: u16 = port.parse().ok()?;
    if port == 0 {
        return None;
    }
    Some(Origin {
        scheme: scheme.to_string(),
        host: crate::core::parameters::normalize_proxy_domain(host)?,
        port,
    })
}

/// Returns hostname suffixes, stopping after the last two labels.
///
/// # Arguments
/// - `host`: Already normalized DNS hostname with at least two labels.
fn suffixes(mut host: &str) -> Vec<&str> {
    let mut suffixes = Vec::new();
    while host.contains('.') {
        suffixes.push(host);
        host = host.split_once('.').expect("suffix has two labels").1;
    }
    suffixes
}

/// Derived group index shared by inventory clones; never persisted as acceptance.
#[derive(Default)]
pub(super) struct RelatedOrigins {
    groups: HashMap<String, HashMap<String, ProxyDestinationPreference>>,
}

impl RelatedOrigins {
    /// Merges concurrent accepted observations, bounding each source history.
    ///
    /// # Arguments
    /// - `preference`: Exact-origin accepted history; no technical or redirect evidence.
    pub(super) fn merge(&mut self, preference: &ProxyDestinationPreference) {
        let Some(origin) = origin_from_key(&preference.destination) else {
            return;
        };
        for suffix in suffixes(&origin.host) {
            let history = self
                .groups
                .entry(origin.key(suffix))
                .or_default()
                .entry(origin.name())
                .or_insert_with(|| ProxyDestinationPreference {
                    destination: preference.destination.clone(),
                    endpoints: Vec::new(),
                });
            for item in &preference.endpoints {
                history.record_success(item.endpoint.clone(), item.accepted_at);
            }
        }
    }

    /// Prunes an endpoint from every derived group after removal or eviction.
    ///
    /// # Arguments
    /// - `endpoint`: Inventory identity whose accepted references are invalid.
    pub(super) fn remove_endpoint(&mut self, endpoint: &ProxyKey) {
        self.groups.retain(|_, origins| {
            origins.retain(|_, history| {
                history.endpoints.retain(|item| &item.endpoint != endpoint);
                !history.endpoints.is_empty()
            });
            !origins.is_empty()
        });
    }

    /// Orders candidates by explicit group, then nearest eligible hostname suffix.
    ///
    /// # Arguments
    /// - `origin`: Normalized exact target; its own history is excluded.
    /// - `domain_group`: Optional canonical group, applied only to its members.
    fn candidates(
        &self,
        origin: &Origin,
        domain_group: &[String],
    ) -> Vec<(String, ProxyAcceptedEndpoint, &'static str)> {
        let mut levels = Vec::new();
        if domain_group
            .iter()
            .any(|domain| origin.host == *domain || origin.host.ends_with(&format!(".{domain}")))
        {
            levels.push((domain_group.to_vec(), "domain_group"));
        }
        let hierarchy = suffixes(&origin.host);
        let skip = usize::from(hierarchy.len() > 1);
        for suffix in hierarchy.into_iter().skip(skip) {
            levels.push((vec![suffix.to_string()], "related_origin"));
        }
        let target = origin.name();
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for (roots, provenance) in levels {
            let mut candidates: Vec<_> = roots
                .iter()
                .flat_map(|root| {
                    self.groups
                        .get(&origin.key(root))
                        .into_iter()
                        .flat_map(|origins| {
                            origins
                                .iter()
                                .filter(|(source, _)| source.as_str() != target)
                                .flat_map(|(source, history)| {
                                    history
                                        .endpoints
                                        .iter()
                                        .cloned()
                                        .map(move |item| (source.clone(), item, provenance))
                                })
                        })
                })
                .collect();
            candidates.sort_by(|a, b| {
                b.1.accepted_at
                    .cmp(&a.1.accepted_at)
                    .then_with(|| a.0.cmp(&b.0))
                    .then_with(|| a.1.endpoint.host.cmp(&b.1.endpoint.host))
                    .then_with(|| a.1.endpoint.port.cmp(&b.1.endpoint.port))
            });
            candidates.retain(|(_, item, _)| seen.insert(item.endpoint.clone()));
            result.extend(candidates);
        }
        result
    }
}

impl ProxyInventory {
    /// Loads the durable index once; cancellations leave it retryable.
    #[cfg(feature = "persistence")]
    async fn load_related_origins(&self) {
        let mut loaded = self.related_origins_loaded.lock().await;
        if *loaded {
            return;
        }
        let Some(repository) = &self.preferences else {
            *loaded = true;
            return;
        };
        #[cfg(feature = "performance-diagnostics")]
        let started = std::time::Instant::now();
        let _commit = self.persistence_commits.lock().await;
        match repository.load_all().await {
            Ok(histories) => {
                #[cfg(feature = "performance-diagnostics")]
                let count = histories.len();
                let mut inner = self.inner.write().await;
                for history in histories {
                    inner.related_origins.merge(&history);
                }
                let concurrent: Vec<_> = inner.preferred_by_destination.values().cloned().collect();
                for history in concurrent {
                    inner.related_origins.merge(&history);
                }
                *loaded = true;
                #[cfg(feature = "performance-diagnostics")]
                tracing::debug!(
                    histories = count,
                    duration_us = started.elapsed().as_micros() as u64,
                    "loaded related-origin proxy candidate index"
                );
            }
            Err(error) => tracing::warn!(%error, "failed to load related-origin proxy histories"),
        }
    }

    /// Offers a proven endpoint for another host without promoting target history.
    ///
    /// # Arguments
    /// - `countries`: Canonical allowed country set.
    /// - `require_https`: Whether tunnel and destination TLS capabilities are required.
    /// - `destination`: New target whose exclusions and attempt history still apply.
    /// - `domain_group`: Optional domain set defining cross-domain defaults.
    ///
    /// # Returns
    /// An eligible record, its source origin and diagnostic candidate category,
    /// or `None` for ordinary exploration.
    pub(super) async fn select_related_origin(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
        domain_group: &[String],
    ) -> Option<(ProxyRecord, String, &'static str)> {
        let destination = destination?;
        let key = ProxyDestinationPreference::destination_key(destination);
        let origin = origin_from_key(&key)?;
        let domain_group = crate::core::canonical_proxy_domain_group(domain_group);
        #[cfg(feature = "persistence")]
        self.load_related_origins().await;
        let candidates = self
            .inner
            .read()
            .await
            .related_origins
            .candidates(&origin, &domain_group);
        let search = ProxySearchContext::current();
        for (source, item, candidate_source) in candidates {
            let authority =
                if item.endpoint.host.contains(':') && !item.endpoint.host.starts_with('[') {
                    format!("[{}]:{}", item.endpoint.host, item.endpoint.port)
                } else {
                    format!("{}:{}", item.endpoint.host, item.endpoint.port)
                };
            if search.as_ref().is_some_and(|search| {
                search.attempted(&authority, Some(destination))
                    || search.preparation_exclusions.contains(&authority)
            }) {
                continue;
            }
            #[cfg(feature = "persistence")]
            if !self.inner.read().await.records.contains_key(&authority) {
                if let Some(repository) = &self.persistence {
                    match repository.get(&item.endpoint).await {
                        Ok(Some(record)) => {
                            self.add_or_update(vec![record]).await;
                        }
                        Ok(None) => {
                            self.inner
                                .write()
                                .await
                                .related_origins
                                .remove_endpoint(&item.endpoint);
                            continue;
                        }
                        Err(error) => {
                            tracing::warn!(%error, "failed to restore related-origin proxy endpoint");
                            continue;
                        }
                    }
                }
            }
            let mut inner = self.inner.write().await;
            let Some(record) = inner.records.get_mut(&authority) else {
                continue;
            };
            if record.key() != item.endpoint {
                continue;
            }
            if let Some(probe) = &self.probe {
                probe.prepare_context(record);
            }
            let record = record.clone();
            drop(inner);
            if !needs_capability_probe(&record, require_https)
                && record.country.as_ref().is_some_and(|country| {
                    countries.contains(country)
                        && is_eligible(
                            &record,
                            country,
                            require_https,
                            Some(destination),
                            SystemTime::now(),
                            self.probe_mode(),
                        )
                })
            {
                return Some((record, source, candidate_source));
            }
        }
        None
    }
}
