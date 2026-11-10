//! Application-only hooks for Scrapyfy's reusable administration adapter.

use crate::ReloadableStreamScraper;
use anyhow::Result;
use arachnea_scrapyfy::{
    admin::{AdminApplicationRuntime, RuntimeReloadReport, ValidatedReloadGroup},
    ScraperAdminSettings,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Compatibility resolver path; proxy configuration is owned by Scrapyfy.
pub use arachnea_scrapyfy::scrapyfy::proxy_settings::configuration as proxy_configuration;

/// Compatibility adapter name using Stream's concrete reconstruction hooks.
pub type StreamAdminRuntimeAdapter =
    arachnea_scrapyfy::admin::ScrapyfyAdminRuntimeAdapter<ReloadableStreamScraper>;

#[async_trait]
impl AdminApplicationRuntime for ReloadableStreamScraper {
    fn proxy_runtime(&self) -> Option<Arc<arachnea_proxy::server::LoopbackProxyRuntime>> {
        Some(self.current().proxy_handle().proxy_runtime())
    }

    async fn apply_proxy_settings(
        &self,
        settings: arachnea_proxy::server::ProxyListenerSettings,
        keys: Vec<arachnea_proxy::server::ProxyApiKey>,
        previous_keys: Vec<arachnea_proxy::server::ProxyApiKey>,
    ) -> Result<()> {
        ReloadableStreamScraper::apply_proxy_settings(self, settings, keys, previous_keys).await
    }

    async fn rebuild_validated_group(
        &self,
        group: &ValidatedReloadGroup,
    ) -> Result<Option<RuntimeReloadReport>> {
        if !matches!(
            group.service_store_id.as_str(),
            crate::stream_scraper::STREAM_SERVICE_GROUP_NAME
                | crate::TRANSLATION_SERVICE_GROUP_NAME
                | "arachnea-proxies"
                | "arachnea-ip-countries"
        ) {
            return Ok(None);
        }
        Ok(Some(self.rebuild_validated().await?))
    }

    async fn apply_scraper_settings(&self, settings: &ScraperAdminSettings) -> Result<()> {
        ReloadableStreamScraper::apply_scraper_settings(self, settings).await
    }
}
