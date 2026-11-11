//! Effective public translation catalog loaded from Scrapyfy service metadata.
//!
//! The catalog is application-owned: it executes the mandatory
//! `get_translation_metadata` query through the generic service aggregator,
//! validates every normalized metadata payload and exposes only public
//! capabilities. Credentials, request headers and YAML internals never appear
//! in the catalog.
//!
//! A missing or invalid translation manifest only leaves translation
//! unavailable; the stream groups keep loading normally. Administrator
//! activation overrides are applied by Scrapyfy before a service is loaded, so
//! only enabled and valid services are considered here.

use std::collections::{BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use arachnea_core::crypt::hash62_64;
use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{
    load_service_catalog, ScraperAgregator, ScraperDataNode, ScraperServiceCatalogEntry,
};
use arachnea_translation::{
    LanguageCode, SupportedPair, TranslationMetadata, GET_TRANSLATION_METADATA_QUERY,
    TRANSLATION_METADATA_MEDIA_TYPE,
};
use serde::Serialize;

pub use crate::{
    DEFAULT_TRANSLATION_BUDGET_WINDOW_SECONDS, DEFAULT_TRANSLATION_MAX_CONCURRENT_REQUESTS,
    DEFAULT_TRANSLATION_ORDER, TRANSLATION_BUDGET_WINDOW_SECONDS_PARAMETER,
    TRANSLATION_MAX_CONCURRENT_PARAMETER, TRANSLATION_MAX_UNITS_PER_WINDOW_PARAMETER,
    TRANSLATION_ORDER_PARAMETER, TRANSLATION_PREFERRED_PARAMETER,
};

/// One enabled translation service exposed by the effective catalog.
#[derive(Clone, Debug, Serialize)]
pub struct TranslationCatalogEntry {
    /// Stable service identifier.
    pub service_id: String,
    /// Human-readable title declared by the service YAML.
    pub title: String,
    /// All declared language codes.
    pub languages: Vec<LanguageCode>,
    /// Effective source language codes.
    pub source_languages: Vec<LanguageCode>,
    /// Effective target language codes.
    pub target_languages: Vec<LanguageCode>,
    /// Declared directional restrictions, when the service is not fully open.
    pub supported_pairs: Option<Vec<SupportedPair>>,
    /// Whether normalized text translation is declared.
    pub supports_text_translation: bool,
    /// Whether inline URL document translation is declared.
    pub supports_url_translation: bool,
    /// Whether the service can detect a missing source language.
    pub source_detection: bool,
    /// Whether credentials are stored; no credential value is exposed.
    pub credentials_configured: bool,
    /// Administrator selection order, lower values first.
    pub administrator_order: u64,
    /// Whether the administrator marked this service as preferred.
    pub preferred: bool,
    /// Maximum concurrent provider requests for this account.
    pub max_concurrent_requests: usize,
    /// Provider units allowed per quota window, or `None` when unlimited.
    pub max_units_per_window: Option<u64>,
    /// Account quota window duration in seconds.
    pub budget_window_seconds: u64,
    /// Hash of the source YAML, absent when the catalog cannot determine it.
    pub service_yaml_hash: Option<String>,
    /// Configuration revision influencing translation output.
    pub configuration_revision: String,
}

/// One service that could not be published, with a non-secret diagnostic.
#[derive(Clone, Debug, Serialize)]
pub struct TranslationCatalogDiagnostic {
    /// Service identifier when it could be determined.
    pub service_id: Option<String>,
    /// Human-readable failure context.
    pub message: String,
}

/// Effective translation catalog built from loaded Scrapyfy services.
#[derive(Clone, Debug, Default, Serialize)]
pub struct TranslationCatalog {
    services: Vec<TranslationCatalogEntry>,
    diagnostics: Vec<TranslationCatalogDiagnostic>,
    loaded_at_unix_seconds: u64,
}

impl TranslationCatalog {
    /// Loads and validates the catalog for one translation service group.
    ///
    /// # Arguments
    ///
    /// * `aggregator` - Aggregator holding the loaded translation services.
    /// * `group_name` - Service store containing translation providers.
    /// * `manifest_path` - Manifest used to read titles and credential requirements.
    /// * `credentials_store` - Encrypted credential lookup used only to know
    ///   whether a service is configured.
    pub fn load(
        aggregator: &ScraperAgregator,
        group_name: &str,
        manifest_path: &str,
        credentials_store: &dyn CredentialsStore,
    ) -> Self {
        let mut catalog = Self::default();
        let entries = match load_service_catalog(manifest_path, group_name) {
            Ok(entries) => entries,
            Err(error) => {
                catalog.push_diagnostic(
                    None,
                    format!("Translation service catalog could not be read: {error:#}"),
                );
                Vec::new()
            }
        };
        let mut entries_by_id: HashMap<String, ScraperServiceCatalogEntry> = entries
            .into_iter()
            .map(|entry| (entry.source.id.clone(), entry))
            .collect();

        for service_id in aggregator.source_names_in_group(group_name) {
            let entry = entries_by_id.remove(&service_id);
            let title = entry
                .as_ref()
                .and_then(|entry| entry.title.clone())
                .unwrap_or_else(|| service_id.clone());
            let credentials_required = entry
                .as_ref()
                .and_then(|entry| entry.credentials.as_ref())
                .is_some_and(|credentials| credentials.required);

            let credentials_configured =
                match credentials_store.get_credentials_for(group_name, &service_id) {
                    Ok(credentials) => credentials.is_some(),
                    Err(error) => {
                        catalog.push_diagnostic(
                            Some(service_id.clone()),
                            format!("Credentials could not be read: {error:#}"),
                        );
                        false
                    }
                };
            if credentials_required && !credentials_configured {
                catalog.push_diagnostic(
                    Some(service_id.clone()),
                    "Service requires credentials that are not configured.".to_string(),
                );
                continue;
            }

            let media_types = vec![TRANSLATION_METADATA_MEDIA_TYPE.to_string()];
            let rows = match futures::executor::block_on(aggregator.execute_query_on_source(
                group_name,
                &service_id,
                GET_TRANSLATION_METADATA_QUERY,
                &HashMap::new(),
                Some(&media_types),
            )) {
                Ok(rows) => rows,
                Err(error) => {
                    catalog.push_diagnostic(
                        Some(service_id.clone()),
                        format!("Translation metadata query failed for `{service_id}`: {error:#}"),
                    );
                    continue;
                }
            };
            let value = match metadata_value(rows.first()) {
                Ok(value) => value,
                Err(message) => {
                    catalog.push_diagnostic(Some(service_id.clone()), message);
                    continue;
                }
            };
            let metadata = match TranslationMetadata::from_value(&value) {
                Ok(metadata) => metadata,
                Err(error) => {
                    catalog.push_diagnostic(
                        Some(service_id.clone()),
                        format!("Invalid translation metadata for `{service_id}`: {error}"),
                    );
                    continue;
                }
            };

            let parameters: Vec<_> = aggregator
                .query_collection_parameters(group_name, &service_id)
                .unwrap_or_default()
                .into_iter()
                .filter(|parameter| parameter.editor.is_some())
                .collect();
            let administrator_order = parameters
                .iter()
                .find(|parameter| parameter.name == TRANSLATION_ORDER_PARAMETER)
                .and_then(|parameter| parameter.value.trim().parse::<u64>().ok())
                .unwrap_or(DEFAULT_TRANSLATION_ORDER);
            let preferred = parameters
                .iter()
                .find(|parameter| parameter.name == TRANSLATION_PREFERRED_PARAMETER)
                .is_some_and(|parameter| is_truthy(&parameter.value));
            let max_concurrent_requests = parameters
                .iter()
                .find(|parameter| parameter.name == TRANSLATION_MAX_CONCURRENT_PARAMETER)
                .and_then(|parameter| parameter.value.trim().parse::<usize>().ok())
                .unwrap_or(DEFAULT_TRANSLATION_MAX_CONCURRENT_REQUESTS)
                .max(1);
            let max_units_per_window = parameters
                .iter()
                .find(|parameter| parameter.name == TRANSLATION_MAX_UNITS_PER_WINDOW_PARAMETER)
                .and_then(|parameter| parameter.value.trim().parse::<u64>().ok())
                .filter(|value| *value > 0);
            let budget_window_seconds = parameters
                .iter()
                .find(|parameter| parameter.name == TRANSLATION_BUDGET_WINDOW_SECONDS_PARAMETER)
                .and_then(|parameter| parameter.value.trim().parse::<u64>().ok())
                .unwrap_or(DEFAULT_TRANSLATION_BUDGET_WINDOW_SECONDS)
                .max(1);
            let service_yaml_hash = aggregator
                .source_yaml_hash(group_name, &service_id)
                .map(str::to_owned);
            let mut revision_parts = vec![format!(
                "yaml={}",
                service_yaml_hash.as_deref().unwrap_or("unknown")
            )];
            revision_parts.extend(
                parameters
                    .iter()
                    .filter(|parameter| {
                        parameter.name != TRANSLATION_ORDER_PARAMETER
                            && parameter.name != TRANSLATION_PREFERRED_PARAMETER
                    })
                    .map(|parameter| format!("{}={}", parameter.name, parameter.value)),
            );
            revision_parts.sort();
            let configuration_revision = hash62_64(revision_parts.join("\n").as_bytes());

            catalog.services.push(TranslationCatalogEntry {
                service_id,
                title,
                languages: metadata.languages.clone(),
                source_languages: metadata.effective_source_languages().to_vec(),
                target_languages: metadata.effective_target_languages().to_vec(),
                supported_pairs: metadata.supported_pairs.clone(),
                supports_text_translation: metadata.text_translation.is_some(),
                supports_url_translation: metadata.url_translation.is_some(),
                source_detection: metadata.detection_available(),
                credentials_configured,
                administrator_order,
                preferred,
                max_concurrent_requests,
                max_units_per_window,
                budget_window_seconds,
                service_yaml_hash,
                configuration_revision,
            });
        }

        catalog.services.sort_by(|left, right| {
            right
                .preferred
                .cmp(&left.preferred)
                .then(left.administrator_order.cmp(&right.administrator_order))
                .then(left.service_id.cmp(&right.service_id))
        });
        catalog.loaded_at_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        catalog
    }

    /// Returns the published services in deterministic selection order.
    pub fn services(&self) -> &[TranslationCatalogEntry] {
        &self.services
    }

    /// Returns one published service by identifier.
    ///
    /// # Arguments
    ///
    /// * `service_id` - Stable service identifier.
    pub fn service(&self, service_id: &str) -> Option<&TranslationCatalogEntry> {
        self.services
            .iter()
            .find(|entry| entry.service_id == service_id)
    }

    /// Returns non-secret diagnostics for services that could not be published.
    pub fn diagnostics(&self) -> &[TranslationCatalogDiagnostic] {
        &self.diagnostics
    }

    /// Returns whether no translation provider is currently available.
    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }

    /// Returns the union of published language codes in sorted order.
    pub fn available_languages(&self) -> Vec<LanguageCode> {
        let languages: BTreeSet<LanguageCode> = self
            .services
            .iter()
            .flat_map(|entry| entry.languages.iter().cloned())
            .collect();
        languages.into_iter().collect()
    }

    /// Returns the preferred service identifier, after administrator ordering.
    pub fn preferred_service_id(&self) -> Option<&str> {
        self.services
            .iter()
            .find(|entry| entry.preferred)
            .map(|entry| entry.service_id.as_str())
    }

    /// Returns the catalog creation time as Unix seconds.
    pub fn loaded_at_unix_seconds(&self) -> u64 {
        self.loaded_at_unix_seconds
    }

    /// Appends one non-secret diagnostic.
    fn push_diagnostic(&mut self, service_id: Option<String>, message: String) {
        self.diagnostics.push(TranslationCatalogDiagnostic {
            service_id,
            message,
        });
    }
}

/// Extracts the normalized metadata object from one query row.
fn metadata_value(
    row: Option<&HashMap<String, ScraperDataNode>>,
) -> Result<serde_json::Value, String> {
    let row = row.ok_or_else(|| format!("{GET_TRANSLATION_METADATA_QUERY} returned no row"))?;
    let node = row
        .get("translation")
        .or_else(|| (row.len() == 1).then(|| row.values().next()).flatten())
        .ok_or_else(|| "metadata row does not contain a translation object".to_string())?;
    serde_json::to_value(node)
        .map_err(|error| format!("failed to read translation metadata: {error}"))
}

/// Returns whether an administrator parameter value means true.
fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    )
}
