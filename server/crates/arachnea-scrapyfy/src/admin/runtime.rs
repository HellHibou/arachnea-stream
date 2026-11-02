//! Generic administration composition with narrowly scoped application hooks.

use super::configuration::AdminConfigurationStore;
use super::{
    AdminRuntimeAdapter, AdminServerSettingsReport, PlaintextCredentials, RuntimeReloadReport,
    ValidatedReloadGroup,
};
use crate::{ScraperAdminSettings, TypedServiceCredentialsStore};
use anyhow::Result;
use arachnea_core::controler::{options::CoreApplicationOptions, RestServerHandle};
use async_trait::async_trait;
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

/// Hooks that require the executable's concrete scraper facade.
#[async_trait]
pub trait AdminApplicationRuntime: Send + Sync {
    /// Rebuilds the application after a validated service-group change.
    ///
    /// # Arguments
    /// * `group` - Group whose sources have been validated by Scrapyfy.
    ///
    /// # Errors
    /// Returns unexpected reconstruction failures; a rejected build belongs in its report.
    async fn rebuild_validated_group(
        &self,
        group: &ValidatedReloadGroup,
    ) -> Result<Option<RuntimeReloadReport>>;

    /// Applies generic scraper settings to the concrete application runtime.
    ///
    /// # Arguments
    /// * `settings` - Effective settings already validated and merged by Scrapyfy.
    ///
    /// # Errors
    /// Returns application runtime failures.
    async fn apply_scraper_settings(&self, settings: &ScraperAdminSettings) -> Result<()>;

    /// Returns the application-owned shared proxy runtime, when present.
    #[cfg(feature = "arachnea-proxy")]
    fn proxy_runtime(&self) -> Option<Arc<arachnea_proxy::server::LoopbackProxyRuntime>> {
        None
    }

    /// Runs a generic proxy transition under the application's reconstruction lock.
    ///
    /// Implementations delegate policy handling to `apply_proxy_runtime` and
    /// supply only their concrete rebuild operation.
    ///
    /// # Arguments
    /// * `settings` - Resolved target policy.
    /// * `keys` - User verifiers to install.
    /// * `previous_keys` - Verifiers to restore if reconstruction fails.
    ///
    /// # Errors
    /// Returns proxy transition or application reconstruction failures.
    #[cfg(feature = "arachnea-proxy")]
    async fn apply_proxy_settings(
        &self,
        _settings: arachnea_proxy::server::ProxyListenerSettings,
        _keys: Vec<arachnea_proxy::server::ProxyApiKey>,
        _previous_keys: Vec<arachnea_proxy::server::ProxyApiKey>,
    ) -> Result<()> {
        anyhow::bail!("Proxy administration is unavailable.")
    }
}

/// Reusable configuration, credentials, REST and proxy administration adapter.
///
/// The application supplies paths, keys, handles and reconstruction hooks; this
/// type owns the generic policy and never references application crate types.
pub struct ScrapyfyAdminRuntimeAdapter<A: AdminApplicationRuntime> {
    configuration: AdminConfigurationStore,
    credentials: Arc<TypedServiceCredentialsStore>,
    /// Concrete application hooks, without a back-reference to this adapter.
    pub(super) application: Arc<A>,
    rest_server: Option<Arc<RestServerHandle>>,
    /// Explicit command-line overrides retained independently of persisted settings.
    #[cfg(feature = "arachnea-proxy")]
    pub(super) proxy_cli: std::collections::BTreeMap<String, String>,
    /// Last proxy application failure exposed without authentication secrets.
    #[cfg(feature = "arachnea-proxy")]
    pub(super) proxy_error: RwLock<Option<String>>,
}

impl<A: AdminApplicationRuntime> ScrapyfyAdminRuntimeAdapter<A> {
    /// Creates a generic adapter from application-supplied dependencies.
    ///
    /// # Arguments
    /// * `configuration` - Loaded persisted settings, before CLI overrides.
    /// * `configuration_path` - Destination chosen by the executable.
    /// * `credentials` - Typed encrypted source credentials store.
    /// * `application` - Hooks requiring the concrete application runtime.
    /// * `rest_server` - Optional controller handle for live Web settings.
    pub fn new(
        configuration: CoreApplicationOptions,
        configuration_path: PathBuf,
        credentials: Arc<TypedServiceCredentialsStore>,
        application: Arc<A>,
        rest_server: Option<Arc<RestServerHandle>>,
    ) -> Self {
        Self {
            configuration: AdminConfigurationStore::new(configuration, configuration_path),
            credentials,
            application,
            rest_server,
            #[cfg(feature = "arachnea-proxy")]
            proxy_cli: Default::default(),
            #[cfg(feature = "arachnea-proxy")]
            proxy_error: RwLock::new(None),
        }
    }

    /// Returns the persisted configuration shared with the controller's settings resolver.
    pub fn configuration(&self) -> Arc<RwLock<CoreApplicationOptions>> {
        self.configuration.shared()
    }
}

#[async_trait]
impl<A: AdminApplicationRuntime> AdminRuntimeAdapter for ScrapyfyAdminRuntimeAdapter<A> {
    #[cfg(feature = "arachnea-proxy")]
    fn proxy_settings(&self) -> Result<Option<super::proxy::ProxySettingsResponse>> {
        if self.application.proxy_runtime().is_none() {
            return Ok(None);
        }
        Ok(Some(self.proxy_snapshot()?))
    }

    #[cfg(feature = "arachnea-proxy")]
    fn validate_proxy_settings(&self, input: &super::proxy::ProxySettingsUpdate) -> Result<()> {
        self.application
            .proxy_runtime()
            .ok_or_else(|| anyhow::anyhow!("Proxy administration is unavailable."))?;
        self.proxy_candidate(input).map(|_| ())
    }

    #[cfg(feature = "arachnea-proxy")]
    async fn update_proxy_settings(&self, input: &super::proxy::ProxySettingsUpdate) -> Result<()> {
        self.update_proxy(input).await
    }

    #[cfg(feature = "arachnea-proxy")]
    async fn create_proxy_key(&self, label: &str) -> Result<super::proxy::CreatedProxyKey> {
        self.create_key(label).await
    }

    #[cfg(feature = "arachnea-proxy")]
    async fn delete_proxy_key(&self, id: &str) -> Result<()> {
        self.delete_key(id).await
    }

    fn persisted_settings(&self) -> Result<CoreApplicationOptions> {
        Ok(self.configuration.snapshot())
    }
    fn save_persisted_settings(&self, settings: &CoreApplicationOptions) -> Result<()> {
        self.configuration.save(settings)
    }

    fn target_server_settings(&self) -> Option<CoreApplicationOptions> {
        let target = self.rest_server.as_ref()?.target_settings()?;
        Some(CoreApplicationOptions {
            server_port: Some(target.server_port),
            network_mode: Some(target.network_mode),
            entrypoint_root: target.entrypoint_root,
            ..Default::default()
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
    async fn rebuild_validated_group(
        &self,
        group: &ValidatedReloadGroup,
    ) -> Result<Option<RuntimeReloadReport>> {
        self.application.rebuild_validated_group(group).await
    }
    async fn apply_scraper_settings(&self, settings: &ScraperAdminSettings) -> Result<()> {
        self.application.apply_scraper_settings(settings).await
    }
}
