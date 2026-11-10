//! Stream-backed adapter for the generic translation bridge.
//!
//! The adapter borrows the already loaded Scrapyfy aggregator, so credentials
//! and editable parameters stay resolved by the same service configuration that
//! built the effective translation catalog.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use arachnea_scrapyfy::ScraperAgregator;
use arachnea_translation::{
    bridge::{ScrapyfyQueryPort, ScrapyfyTranslationBridge},
    TranslationError,
};
use async_trait::async_trait;

use crate::translation_catalog::TranslationCatalog;

/// Translation query port backed by one loaded Stream service group.
pub struct StreamTranslationPort<'a> {
    aggregator: &'a ScraperAgregator,
    group_name: &'a str,
    service_id: String,
}

impl<'a> StreamTranslationPort<'a> {
    /// Creates a port for one loaded translation service.
    ///
    /// # Arguments
    ///
    /// * `aggregator` - Aggregator containing the translation service group.
    /// * `group_name` - Service store containing the translation providers.
    /// * `service_id` - Stable service identifier used in diagnostics.
    pub fn new(
        aggregator: &'a ScraperAgregator,
        group_name: &'a str,
        service_id: impl Into<String>,
    ) -> Self {
        Self {
            aggregator,
            group_name,
            service_id: service_id.into(),
        }
    }
}

#[async_trait]
impl ScrapyfyQueryPort for StreamTranslationPort<'_> {
    async fn execute_translation_query(
        &self,
        query: &str,
        params: &BTreeMap<String, String>,
        media_types: &[String],
    ) -> Result<Vec<HashMap<String, arachnea_scrapyfy::ScraperDataNode>>> {
        let params = params
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<HashMap<_, _>>();
        let media_types = media_types.to_vec();
        self.aggregator
            .execute_query_on_source(
                self.group_name,
                &self.service_id,
                query,
                &params,
                Some(&media_types),
            )
            .await
    }
}

/// Loads the generic translation bridge for one published service.
///
/// # Arguments
///
/// * `aggregator` - Aggregator containing the translation service group.
/// * `catalog` - Effective catalog providing the configuration revision.
/// * `group_name` - Service store containing the translation providers.
/// * `service_id` - Published service identifier.
///
/// # Errors
///
/// Returns [`TranslationError::MetadataUnavailable`] when the service is not
/// published, or another bridge error when the metadata query or validation
/// fails.
pub async fn load_translation_bridge<'a>(
    aggregator: &'a ScraperAgregator,
    catalog: &TranslationCatalog,
    group_name: &'a str,
    service_id: &str,
) -> Result<ScrapyfyTranslationBridge<StreamTranslationPort<'a>>, TranslationError> {
    let entry =
        catalog
            .service(service_id)
            .ok_or_else(|| TranslationError::MetadataUnavailable {
                detail: format!("translation service `{service_id}` is not published"),
            })?;
    let port = StreamTranslationPort::new(aggregator, group_name, service_id);
    ScrapyfyTranslationBridge::load(port, service_id, entry.configuration_revision.clone()).await
}
