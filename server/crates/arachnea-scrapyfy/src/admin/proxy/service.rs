//! Generic proxy administration and persistence coordination.

use super::{CreatedProxyKey, ProxyKeyInfo, ProxySettingsResponse, ProxySettingsUpdate};
use crate::admin::{
    runtime::{AdminApplicationRuntime, ScrapyfyAdminRuntimeAdapter},
    AdminRuntimeAdapter,
};
use crate::scrapyfy::proxy_settings::configuration;
use crate::{PROXY_ENABLED_KEY, PROXY_KEYS_KEY, PROXY_NETWORK_KEY, PROXY_PORT_KEY};
use anyhow::{bail, Context, Result};
use arachnea_core::controler::options::{CoreApplicationOptions, SettingSource};
use arachnea_proxy::server::{api_key_digest, generate_api_key, ProxyApiKey};
use std::collections::BTreeMap;

impl<A: AdminApplicationRuntime> ScrapyfyAdminRuntimeAdapter<A> {
    /// Attaches explicit CLI proxy overrides to this adapter.
    ///
    /// # Arguments
    /// * `cli` - Nonsecret proxy options passed explicitly on the command line.
    pub fn with_proxy_cli(mut self, cli: BTreeMap<String, String>) -> Self {
        self.proxy_cli = cli;
        self
    }

    /// Returns requested and effective proxy settings without secrets or verifiers.
    ///
    /// # Errors
    /// Returns configuration parsing errors.
    pub(in crate::admin) fn proxy_snapshot(&self) -> Result<ProxySettingsResponse> {
        let config = self.persisted_settings()?;
        let (requested, keys) = configuration(&config, &self.proxy_cli)?;
        let runtime = self
            .application
            .proxy_runtime()
            .context("Proxy administration is unavailable.")?;
        let effective = runtime.settings();
        let source = |name| {
            if self.proxy_cli.contains_key(name) {
                SettingSource::CommandLine
            } else if config.additional_option(name).is_some() {
                SettingSource::Configuration
            } else {
                SettingSource::Default
            }
        };
        Ok(ProxySettingsResponse {
            enabled: requested.enabled,
            enabled_source: source(PROXY_ENABLED_KEY),
            server_port: (requested.port != 0).then_some(requested.port),
            server_port_source: source(PROXY_PORT_KEY),
            network_mode: requested.network.to_string(),
            network_mode_source: source(PROXY_NETWORK_KEY),
            effective_enabled: effective.enabled,
            effective_port: runtime.local_addr().map(|addr| addr.port()).unwrap_or(0),
            effective_network_mode: effective.network.to_string(),
            apply_error: self
                .proxy_error
                .read()
                .expect("proxy error lock poisoned")
                .clone(),
            keys: keys.iter().map(key_info).collect(),
        })
    }

    /// Builds a validated candidate without mutating persisted state.
    ///
    /// # Arguments
    /// * `input` - Partial changes; CLI-pinned values cannot be supplied.
    ///
    /// # Errors
    /// Rejects invalid settings and attempts to edit CLI-pinned fields.
    pub(in crate::admin) fn proxy_candidate(
        &self,
        input: &ProxySettingsUpdate,
    ) -> Result<CoreApplicationOptions> {
        let mut config = self.persisted_settings()?;
        let changes = [
            (
                PROXY_ENABLED_KEY,
                input.enabled.map(|value| value.to_string()),
            ),
            (
                PROXY_PORT_KEY,
                input.server_port.map(|value| value.to_string()),
            ),
            (PROXY_NETWORK_KEY, input.network_mode.clone()),
        ];
        for (name, value) in changes {
            if let Some(value) = value {
                if self.proxy_cli.contains_key(name) {
                    bail!("Proxy setting {name} is pinned by the command line.");
                }
                config.set_additional_option(name, Some(value));
            }
        }
        configuration(&config, &self.proxy_cli)?;
        Ok(config)
    }

    /// Validates and applies proxy settings while retaining unrelated options.
    ///
    /// # Arguments
    /// * `input` - Partial proxy changes.
    ///
    /// # Errors
    /// Returns validation, runtime or persistence errors.
    pub(in crate::admin) async fn update_proxy(&self, input: &ProxySettingsUpdate) -> Result<()> {
        self.commit_proxy(self.proxy_candidate(input)?).await
    }

    /// Creates a key and returns the usable secret exactly once.
    ///
    /// # Arguments
    /// * `label` - Nonempty administrator label, at most 128 UTF-8 bytes.
    ///
    /// # Errors
    /// Returns validation, runtime or persistence errors.
    pub(in crate::admin) async fn create_key(&self, label: &str) -> Result<CreatedProxyKey> {
        let label = label.trim();
        if label.is_empty() || label.len() > crate::PROXY_KEY_LABEL_MAX_BYTES {
            bail!("Proxy key label must contain 1 to 128 bytes.");
        }
        let mut config = self.persisted_settings()?;
        let (_, mut keys) = configuration(&config, &self.proxy_cli)?;
        if keys.len() >= crate::PROXY_MAX_USER_KEYS {
            bail!("Proxy API key limit reached.");
        }
        let token = generate_api_key();
        let key = ProxyApiKey {
            id: format!("{:032x}", rand::random::<u128>()),
            label: label.to_string(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            digest: api_key_digest(&token),
        };
        let info = key_info(&key);
        keys.push(key);
        config.set_additional_option(PROXY_KEYS_KEY, Some(serde_json::to_string(&keys)?));
        self.commit_proxy(config).await?;
        Ok(CreatedProxyKey { key: info, token })
    }

    /// Removes a verifier and drains already authenticated user tunnels within 30 seconds.
    ///
    /// # Arguments
    /// * `id` - Public identifier of an existing user key.
    ///
    /// # Errors
    /// Rejects unknown keys and returns runtime or persistence errors.
    pub(in crate::admin) async fn delete_key(&self, id: &str) -> Result<()> {
        let mut config = self.persisted_settings()?;
        let (_, mut keys) = configuration(&config, &self.proxy_cli)?;
        let count = keys.len();
        keys.retain(|key| key.id != id);
        if keys.len() == count {
            bail!("Unknown proxy API key identifier.");
        }
        config.set_additional_option(PROXY_KEYS_KEY, Some(serde_json::to_string(&keys)?));
        self.commit_proxy(config).await
    }

    /// Applies runtime changes before persistence, restoring the previous policy on failure.
    async fn commit_proxy(&self, candidate: CoreApplicationOptions) -> Result<()> {
        let previous = self.persisted_settings()?;
        let (old_settings, old_keys) = configuration(&previous, &self.proxy_cli)?;
        let (settings, keys) = configuration(&candidate, &self.proxy_cli)?;
        let candidate_keys = keys.clone();
        let result = self
            .application
            .apply_proxy_settings(settings, keys, old_keys.clone())
            .await;
        if let Err(error) = result {
            *self.proxy_error.write().expect("proxy error lock poisoned") = Some(error.to_string());
            return Err(error);
        }
        if let Err(error) = self.save_persisted_settings(&candidate) {
            self.application
                .apply_proxy_settings(old_settings, old_keys, candidate_keys)
                .await
                .context("Failed to restore proxy after persistence failure")?;
            *self.proxy_error.write().expect("proxy error lock poisoned") = Some(error.to_string());
            return Err(error);
        }
        *self.proxy_error.write().expect("proxy error lock poisoned") = None;
        Ok(())
    }
}

fn key_info(key: &ProxyApiKey) -> ProxyKeyInfo {
    ProxyKeyInfo {
        id: key.id.clone(),
        label: key.label.clone(),
        created_at: key.created_at,
    }
}
