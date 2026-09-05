//! Stream executable adapters for the generic Scrapyfy administration service.

use anyhow::Result;
use async_trait::async_trait;
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

use arachnea_core::controler::{options::CoreApplicationOptions, RestServerHandle};
use arachnea_scrapyfy::admin::{
    AdminRuntimeAdapter, AdminServerSettingsReport, PlaintextCredentials, RuntimeReloadReport,
    ValidatedReloadGroup,
};

use crate::{ReloadableStreamScraper, TypedServiceCredentialsStore};

/// Application-specific adapter injected into Scrapyfy's admin service.
pub struct StreamAdminRuntimeAdapter {
    configuration: Arc<RwLock<CoreApplicationOptions>>,
    configuration_path: PathBuf,
    credentials: Arc<TypedServiceCredentialsStore>,
    reloadable: Arc<ReloadableStreamScraper>,
    rest_server: Option<Arc<RestServerHandle>>,
}

impl StreamAdminRuntimeAdapter {
    /// Creates the Stream runtime adapter.
    pub fn new(
        configuration: CoreApplicationOptions,
        configuration_path: PathBuf,
        credentials: Arc<TypedServiceCredentialsStore>,
        reloadable: Arc<ReloadableStreamScraper>,
        rest_server: Option<Arc<RestServerHandle>>,
    ) -> Self {
        Self {
            configuration: Arc::new(RwLock::new(configuration)),
            configuration_path,
            credentials,
            reloadable,
            rest_server,
        }
    }

    /// Returns the shared persisted configuration for the REST settings resolver.
    pub fn configuration(&self) -> Arc<RwLock<CoreApplicationOptions>> {
        Arc::clone(&self.configuration)
    }
}

#[async_trait]
impl AdminRuntimeAdapter for StreamAdminRuntimeAdapter {
    fn persisted_settings(&self) -> Result<CoreApplicationOptions> {
        Ok(self
            .configuration
            .read()
            .expect("configuration lock poisoned")
            .clone())
    }

    fn save_persisted_settings(&self, settings: &CoreApplicationOptions) -> Result<()> {
        let mut config = self
            .configuration
            .write()
            .expect("configuration lock poisoned");
        config.server_port = settings.server_port;
        config.entrypoint_root = settings.entrypoint_root.clone();
        config.network_mode = settings.network_mode;
        config.password_hash = settings.password_hash.clone();
        config.save(&self.configuration_path)
    }

    fn target_server_settings(&self) -> Option<CoreApplicationOptions> {
        let rest_server = self.rest_server.as_ref()?;
        let target = rest_server.target_settings()?;
        let mut settings = CoreApplicationOptions::default();
        settings.server_port = Some(target.server_port);
        settings.network_mode = Some(target.network_mode);
        settings.entrypoint_root = target.entrypoint_root;
        Some(settings)
    }

    fn apply_server_settings(&self) -> AdminServerSettingsReport {
        let Some(rest_server) = &self.rest_server else {
            return AdminServerSettingsReport {
                applied: false,
                apply_error: Some("REST server hot application is unavailable.".to_string()),
                server_port: 0,
                network_mode: "private".to_string(),
                entrypoint_root: None,
            };
        };
        let report = rest_server.apply_settings();
        AdminServerSettingsReport {
            applied: report.applied,
            apply_error: report.apply_error,
            server_port: report.server_port,
            network_mode: report.network_mode,
            entrypoint_root: report.entrypoint_root,
        }
    }

    fn stored_credentials(&self, store: &str, id: &str) -> Result<Option<PlaintextCredentials>> {
        self.credentials.get_credentials_for(store, id)
    }

    fn set_stored_credentials(
        &self,
        store: &str,
        id: &str,
        value: PlaintextCredentials,
    ) -> Result<()> {
        self.credentials.set_credentials_for(store, id, value)
    }

    fn clear_stored_credentials(&self, store: &str, id: &str) -> Result<()> {
        self.credentials.clear_credentials_for(store, id)
    }

    async fn rebuild_validated_group(
        &self,
        group: &ValidatedReloadGroup,
    ) -> Result<Option<RuntimeReloadReport>> {
        if group.service_store_id != crate::stream_scraper::STREAM_SERVICE_GROUP_NAME {
            return Ok(None);
        }
        Ok(Some(self.reloadable.rebuild_validated().await?))
    }
}
