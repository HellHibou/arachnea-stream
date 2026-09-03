//! Stream executable adapters for the generic Scrapyfy administration service.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::{Arc, RwLock};

use arachnea_core::controler::{RestServerHandle, ServerNetworkMode};
use arachnea_scrapyfy::admin::{
    AdminGroupReload, AdminPersistedSettings, AdminRuntimeAdapter, AdminServerSettings,
    AdminServerSettingsReport, PlaintextCredentials,
};

use crate::configuration::ApplicationConfiguration;
use crate::{ReloadableStreamScraper, TypedServiceCredentialsStore};

/// Application-specific adapter injected into Scrapyfy's admin service.
pub struct StreamAdminRuntimeAdapter {
    configuration: Arc<RwLock<ApplicationConfiguration>>,
    credentials: Arc<TypedServiceCredentialsStore>,
    reloadable: Arc<ReloadableStreamScraper>,
    rest_server: Option<Arc<RestServerHandle>>,
}

impl StreamAdminRuntimeAdapter {
    /// Creates the Stream runtime adapter.
    pub fn new(
        configuration: ApplicationConfiguration,
        credentials: Arc<TypedServiceCredentialsStore>,
        reloadable: Arc<ReloadableStreamScraper>,
        rest_server: Option<Arc<RestServerHandle>>,
    ) -> Self {
        Self {
            configuration: Arc::new(RwLock::new(configuration)),
            credentials,
            reloadable,
            rest_server,
        }
    }

    /// Returns the shared persisted configuration for the REST settings resolver.
    pub fn configuration(&self) -> Arc<RwLock<ApplicationConfiguration>> {
        Arc::clone(&self.configuration)
    }
}

#[async_trait]
impl AdminRuntimeAdapter for StreamAdminRuntimeAdapter {
    fn persisted_settings(&self) -> Result<AdminPersistedSettings> {
        let config = self
            .configuration
            .read()
            .expect("configuration lock poisoned")
            .clone();
        Ok(AdminPersistedSettings {
            server_port: config.server_port,
            entrypoint_root: config.entrypoint_root,
            network_mode: config.network_mode,
            password_hash: config.password_hash,
        })
    }

    fn save_persisted_settings(&self, settings: &AdminPersistedSettings) -> Result<()> {
        let mut config = self
            .configuration
            .write()
            .expect("configuration lock poisoned");
        config.server_port = settings.server_port;
        config.entrypoint_root = settings.entrypoint_root.clone();
        config.network_mode = settings.network_mode.clone();
        config.password_hash = settings.password_hash.clone();
        config.save()
    }

    fn target_server_settings(&self) -> Option<AdminServerSettings> {
        let rest_server = self.rest_server.as_ref()?;
        let target = rest_server.target_settings()?;
        Some(AdminServerSettings {
            server_port: target.server_port,
            network_mode: network_mode_name(target.network_mode).to_string(),
            entrypoint_root: target.entrypoint_root,
        })
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

    async fn rebuild_group(&self, store: &str) -> Result<Option<AdminGroupReload>> {
        if store != crate::stream_scraper::STREAM_SERVICE_GROUP_NAME {
            return Ok(None);
        }
        let report = self.reloadable.reload().await?;
        Ok(Some(AdminGroupReload {
            applied: report.applied,
            loaded: report.loaded,
            disabled: report.disabled,
            ignored: report
                .ignored
                .into_iter()
                .map(
                    |s| arachnea_scrapyfy::admin::dto::AdminReloadSkippedSource {
                        id: s.id,
                        path: s.path,
                    },
                )
                .collect(),
            errors: report
                .errors
                .into_iter()
                .map(|e| arachnea_scrapyfy::admin::dto::AdminReloadSourceError {
                    id: e.id,
                    path: e.path,
                    message: e.message,
                })
                .collect(),
            build_error: report.build_error,
        }))
    }
}

/// Serializes Core's network mode for the generic administration DTO.
fn network_mode_name(mode: ServerNetworkMode) -> &'static str {
    match mode {
        ServerNetworkMode::Local => "local",
        ServerNetworkMode::Private => "private",
        ServerNetworkMode::Public => "public",
    }
}
