//! Administration service: state, access control, and route registration.
//!
//! The API is mounted under `api/admin/<operation>` over HTTP and exposed to
//! the desktop frontend through the Tauri invoke handler under the same
//! command names. Sources are addressed by the composite pair
//! `(service_store_id, service_id)`; application-specific behaviors (persisted
//! settings, REST hot application, encrypted credentials, runtime rebuilds)
//! are reached through the [`AdminRuntimeAdapter`] injected by the executable.

pub mod auth;
pub mod dto;
mod ops;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde::de::DeserializeOwned;

use arachnea_core::controler::{
    deserialize_input, ControlerJsonInput, ControlerJsonOutput, ControlerService,
    JsonControlerFunction, RequestControlerContext,
};
use arachnea_core::crypt::generate_random_password;
use arachnea_core::persistence::TypedEntityStore;

use crate::scrapyfy::{PersistenceSourceEnabled, SourceServiceKey, SourceServiceRecord};

use auth::{LoginRateLimiter, SessionStore};
use dto::SettingSource;
use ops::AdminError;

pub use auth::ADMIN_SESSION_COOKIE;
pub use ops::{register_admin_service, AdminReply};

/// Length of the temporary administrator password generated when no permanent
/// hash is configured.
const TEMPORARY_PASSWORD_LENGTH: usize = 24;

/// Runtime mode exposed by the administration API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminMode {
    /// Desktop application mode backed by the Tauri controller.
    Desktop,
    /// Headless HTTP server mode backed by the REST controller.
    Server,
}

/// Effective runtime settings used by the running server.
///
/// These values reflect what the running process actually uses; they are the
/// source of truth reported by the `settings` operation even when the persisted
/// configuration file was changed and a restart is pending.
#[derive(Clone)]
pub struct AdminRuntimeSettings {
    /// Backend runtime mode.
    pub mode: AdminMode,
    /// Effective REST server port.
    pub server_port: u16,
    /// Where the effective port comes from.
    pub server_port_source: SettingSource,
    /// Effective network mode: `local`, `private` or `public`.
    pub network_mode: String,
    /// Where the effective network mode comes from.
    pub network_mode_source: SettingSource,
    /// Effective public root path, when set.
    pub entrypoint_root: Option<String>,
    /// Where the effective root comes from.
    pub entrypoint_root_source: SettingSource,
}

/// Settings the running REST server would apply right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminServerSettings {
    /// Effective HTTP port.
    pub server_port: u16,
    /// Effective network access mode: `local`, `private` or `public`.
    pub network_mode: String,
    /// Effective public root path, when set.
    pub entrypoint_root: Option<String>,
}

/// Report of a hot application attempt of the server settings.
#[derive(Clone, Debug)]
pub struct AdminServerSettingsReport {
    /// Whether the target settings are now active on the running server.
    pub applied: bool,
    /// Failure context when the settings could not be applied.
    pub apply_error: Option<String>,
    /// Effective port after the application attempt.
    pub server_port: u16,
    /// Effective network mode after the application attempt.
    pub network_mode: String,
    /// Effective entrypoint root after the application attempt.
    pub entrypoint_root: Option<String>,
}

/// Persisted application settings owned by the executable configuration.
///
/// Scrapyfy reads and writes them through [`AdminRuntimeAdapter`] without
/// knowing the concrete configuration file format.
#[derive(Clone, Debug, Default)]
pub struct AdminPersistedSettings {
    /// Optional REST listener port.
    pub server_port: Option<u16>,
    /// Optional public root path prefix.
    pub entrypoint_root: Option<String>,
    /// Optional network mode: `local`, `private` or `public`.
    pub network_mode: Option<String>,
    /// Argon2id encoded administrator password hash.
    pub password_hash: Option<String>,
}

/// Application-specific rebuild outcome for one service store (group).
#[derive(Clone, Debug, Default)]
pub struct AdminGroupReload {
    /// Whether the group runtime accepted the new catalog state.
    pub applied: bool,
    /// Build failure message when the group runtime could not be rebuilt.
    pub build_error: Option<String>,
}

/// Declaration of one administrable service store (group).
///
/// Group titles and descriptions are frontend locale data and intentionally
/// absent from this structure.
#[derive(Clone, Debug)]
pub struct AdminServiceGroupConfig {
    /// Technical identifier of the group, used as the persistence namespace
    /// qualifier (`service_store_id`).
    pub service_store_id: String,
    /// Services manifest path, relative to application resources when not
    /// absolute.
    pub manifest_path: String,
}

/// Plaintext credential pair exchanged with the adapter.
///
/// Scrapyfy never stores this value; the adapter encrypts it before
/// persistence.
pub type PlaintextCredentials = arachnea_core::persistence::credentials_store::StoredCredentials;

/// Runtime hooks an executable provides to the administration service.
///
/// Scrapyfy owns the routes, DTOs, sessions, write checks, catalog resolution
/// and reload orchestration; the adapter keeps the application-specific side
/// (configuration persistence, REST hot application, encrypted credentials,
/// runtime rebuilds) without any dependency on concrete facades.
#[async_trait]
pub trait AdminRuntimeAdapter: Send + Sync {
    /// Reads the persisted application settings.
    ///
    /// # Errors
    /// Returns an error when the persisted configuration cannot be read.
    fn persisted_settings(&self) -> anyhow::Result<AdminPersistedSettings>;

    /// Validates and persists the application settings.
    ///
    /// # Errors
    /// Returns an error when a value is invalid or the configuration cannot
    /// be written.
    fn save_persisted_settings(&self, settings: &AdminPersistedSettings) -> anyhow::Result<()>;

    /// Returns the settings the running REST server would apply right now, or
    /// `None` when hot application is unavailable (desktop mode or no REST
    /// handle).
    fn target_server_settings(&self) -> Option<AdminServerSettings>;

    /// Applies the target server settings to the running REST server.
    ///
    /// Blocking; only called in server mode when the target settings exist.
    fn apply_server_settings(&self) -> AdminServerSettingsReport;

    /// Returns the stored credentials of one namespaced source, if any.
    ///
    /// # Errors
    /// Returns an error when the credentials store cannot be read or the
    /// stored values cannot be decrypted.
    fn stored_credentials(
        &self,
        service_store_id: &str,
        service_id: &str,
    ) -> anyhow::Result<Option<PlaintextCredentials>>;

    /// Writes the credentials of one namespaced source.
    ///
    /// # Errors
    /// Returns an error when the credentials cannot be encrypted or persisted.
    fn set_stored_credentials(
        &self,
        service_store_id: &str,
        service_id: &str,
        credentials: PlaintextCredentials,
    ) -> anyhow::Result<()>;

    /// Removes the credentials of one namespaced source.
    ///
    /// # Errors
    /// Returns an error when the credentials store cannot be updated.
    fn clear_stored_credentials(
        &self,
        service_store_id: &str,
        service_id: &str,
    ) -> anyhow::Result<()>;

    /// Rebuilds the application-specific runtime of one service store.
    ///
    /// Returns `Ok(None)` when the group has no application-specific runtime
    /// and Scrapyfy's generic catalog validation is enough. The hook must
    /// never partially apply a failed rebuild.
    ///
    /// # Errors
    /// Returns an error for unexpected internal failures only.
    async fn rebuild_group(
        &self,
        service_store_id: &str,
    ) -> anyhow::Result<Option<AdminGroupReload>>;
}

/// Shared administration state bound to the registered routes.
pub struct AdminState {
    settings: RwLock<AdminRuntimeSettings>,
    temp_password: Option<String>,
    sessions: SessionStore,
    login_limiter: LoginRateLimiter,
    source_enabled: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
    groups: Vec<AdminServiceGroupConfig>,
    adapter: Arc<dyn AdminRuntimeAdapter>,
}

/// Outcome of the administration state construction.
pub struct AdminStateBuild {
    /// Shared administration state.
    pub state: Arc<AdminState>,
    /// Newly generated temporary administrator password, when the server mode
    /// runs without a permanent password hash. It must be displayed once and
    /// never persisted.
    pub temporary_password: Option<String>,
}

impl AdminState {
    /// Creates the administration state shared by all admin operations.
    ///
    /// A temporary password is generated through the stateless core primitive
    /// when the server runs without a permanent hash; it is returned to the
    /// caller for one-time display instead of being printed from this crate.
    ///
    /// # Arguments
    /// * `settings` - Effective runtime settings of the running server.
    /// * `groups` - Declared service stores, in display order.
    /// * `source_enabled` - Typed store holding the `arachnea-services`
    ///   records, qualified by `(service_store_id, source_id)`.
    /// * `adapter` - Application adapter for persisted settings, hot apply,
    ///   credentials, and runtime rebuilds.
    ///
    /// # Errors
    /// Returns an error when the persisted settings cannot be read.
    pub fn build(
        settings: AdminRuntimeSettings,
        groups: Vec<AdminServiceGroupConfig>,
        source_enabled: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
        adapter: Arc<dyn AdminRuntimeAdapter>,
    ) -> anyhow::Result<AdminStateBuild> {
        let temporary_password = if settings.mode == AdminMode::Server
            && adapter.persisted_settings()?.password_hash.is_none()
        {
            Some(generate_random_password(TEMPORARY_PASSWORD_LENGTH))
        } else {
            None
        };
        let state = Arc::new(Self {
            settings: RwLock::new(settings),
            temp_password: temporary_password.clone(),
            sessions: SessionStore::new(),
            login_limiter: LoginRateLimiter::new(),
            source_enabled,
            groups,
            adapter,
        });
        Ok(AdminStateBuild {
            state,
            temporary_password,
        })
    }

    /// Returns the effective runtime settings.
    pub(crate) fn settings(&self) -> AdminRuntimeSettings {
        self.settings
            .read()
            .expect("admin settings lock poisoned")
            .clone()
    }

    /// Returns the runtime mode.
    pub(crate) fn mode(&self) -> AdminMode {
        self.settings().mode
    }

    /// Returns the temporary administrator password, when one was generated.
    pub(crate) fn temp_password(&self) -> Option<&str> {
        self.temp_password.as_deref()
    }

    /// Returns the declared service stores, in display order.
    pub(crate) fn groups(&self) -> &[AdminServiceGroupConfig] {
        &self.groups
    }

    /// Returns the application runtime adapter.
    pub(crate) fn adapter(&self) -> &Arc<dyn AdminRuntimeAdapter> {
        &self.adapter
    }

    /// Updates the effective server settings after a hot application.
    ///
    /// Only the values that actually changed are updated, and their provenance
    /// becomes `Configuration`; command-line pinned values stay untouched.
    pub(crate) fn update_effective_server_settings(
        &self,
        server_port: u16,
        network_mode: String,
        entrypoint_root: Option<String>,
    ) {
        let mut settings = self.settings.write().expect("admin settings lock poisoned");
        if settings.server_port != server_port {
            settings.server_port = server_port;
            settings.server_port_source = SettingSource::Configuration;
        }
        if settings.network_mode != network_mode {
            settings.network_mode = network_mode;
            settings.network_mode_source = SettingSource::Configuration;
        }
        if settings.entrypoint_root != entrypoint_root {
            settings.entrypoint_root = entrypoint_root;
            settings.entrypoint_root_source = SettingSource::Configuration;
        }
    }

    /// Returns the session store.
    pub(crate) fn sessions(&self) -> &SessionStore {
        &self.sessions
    }

    /// Returns the login rate limiter.
    pub(crate) fn login_limiter(&self) -> &LoginRateLimiter {
        &self.login_limiter
    }

    /// Builds the persistent activation policy over the services namespace.
    pub(crate) fn activation_policy(&self) -> PersistenceSourceEnabled {
        PersistenceSourceEnabled::with_typed_store(Arc::clone(&self.source_enabled))
    }

    /// Builds the composite identity of one source.
    pub(crate) fn source_key(&self, service_store_id: &str, source_id: &str) -> SourceServiceKey {
        SourceServiceKey::new(service_store_id, source_id)
    }
}

/// Registers one admin operation under `admin/<operation>`.
///
/// The operation accepts both JSON (POST/invoke) and query-string (GET)
/// payloads. Every reply carries `Cache-Control: no-store`.
fn register_admin_operation<I, F, Fut>(
    controler: &mut dyn ControlerService,
    state: &Arc<AdminState>,
    operation: &str,
    handler: F,
) where
    I: DeserializeOwned + Send + 'static,
    F: Fn(Arc<AdminState>, RequestControlerContext, I) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<AdminReply, AdminError>> + Send + 'static,
{
    let name = format!("admin/{operation}");
    let state_ref = Arc::clone(state);
    let handler_ref = Arc::new(handler);
    let call: JsonControlerFunction = Arc::new(move |input: ControlerJsonInput| {
        let state = Arc::clone(&state_ref);
        let handler = Arc::clone(&handler_ref);
        Box::pin(async move {
            let reply = match deserialize_input::<I>(input.payload) {
                Ok(payload) => handler(state, input.context, payload).await,
                Err(error) => Err(AdminError::bad_request(error)),
            };
            Ok(reply_to_output(reply))
        })
    });
    controler.register_json_function(&name, call);
}

/// Turns an operation result into a header-aware JSON output.
fn reply_to_output(reply: Result<AdminReply, AdminError>) -> ControlerJsonOutput {
    let (status, value, mut headers) = match reply {
        Ok(reply) => (reply.status, reply.value, reply.headers),
        Err(error) => (error.status(), error.value(), HashMap::new()),
    };
    headers.insert("cache-control".to_string(), "no-store".to_string());
    ControlerJsonOutput {
        value,
        status,
        headers,
    }
}
