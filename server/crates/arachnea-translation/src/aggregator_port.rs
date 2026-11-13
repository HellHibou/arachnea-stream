//! Scrapyfy aggregator adapter for translation queries.

use crate::bridge::ScrapyfyQueryPort;
use anyhow::Result;
use arachnea_scrapyfy::ScraperAgregator;
use async_trait::async_trait;
use std::collections::{BTreeMap, HashMap};

/// Translation query port backed by one loaded Scrapyfy service group.
pub struct AggregatorTranslationPort<'a> {
    aggregator: &'a ScraperAgregator,
    group_name: &'a str,
    service_id: String,
}

impl<'a> AggregatorTranslationPort<'a> {
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
impl ScrapyfyQueryPort for AggregatorTranslationPort<'_> {
    fn measure_translation_query(
        &self,
        query: &str,
        params: &BTreeMap<String, String>,
    ) -> Result<u64> {
        self.aggregator.measure_query_body_on_source(
            self.group_name,
            &self.service_id,
            query,
            &params.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        )
    }

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
