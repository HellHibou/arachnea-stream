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
/// Generic validation and reporting primitives for service-group reloads.
pub mod reload;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde::de::DeserializeOwned;

use arachnea_core::controler::{
    deserialize_input,
    options::{ApplicationMode, CoreApplicationOptions, ServerNetworkMode, SettingSource},
    ControlerJsonInput, ControlerJsonOutput, ControlerService, JsonControlerFunction,
    RequestControlerContext,
};
use arachnea_core::crypt::generate_random_password;
use arachnea_core::persistence::TypedEntityStore;

use crate::scrapyfy::{PersistenceSourceEnabled, SourceServiceKey, SourceServiceRecord};

use auth::{LoginRateLimiter, SessionStore};
use ops::AdminError;

pub use auth::ADMIN_SESSION_COOKIE;
pub use ops::{register_admin_service, AdminReply};
pub use reload::{
    prepare_group_reload, ReloadCoordinator, ReloadGroupReport, ReloadSummary, RuntimeReloadReport,
    TrayReloadSummary, ValidatedReloadGroup,
};

/// Length of the temporary administrator password generated when no permanent
/// hash is configured.
const TEMPORARY_PASSWORD_LENGTH: usize = 24;

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
    fn persisted_settings(&self) -> anyhow::Result<CoreApplicationOptions>;

    /// Validates and persists the application settings.
    ///
    /// # Errors
    /// Returns an error when a value is invalid or the configuration cannot
    /// be written.
    fn save_persisted_settings(&self, settings: &CoreApplicationOptions) -> anyhow::Result<()>;

    /// Returns the settings the running REST server would apply right now, or
    /// `None` when hot application is unavailable (desktop mode or no REST
    /// handle).
    fn target_server_settings(&self) -> Option<CoreApplicationOptions>;

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

    /// Rebuilds the application-specific runtime of a validated service group.
    ///
    /// Scrapyfy only invokes this hook after validating the group manifest and
    /// synchronizing its activation defaults. Returns `Ok(None)` when the group
    /// has no application-specific runtime. The hook must never partially apply
    /// a failed rebuild.
    ///
    /// # Errors
    /// Returns an error for unexpected internal failures only.
    async fn rebuild_validated_group(
        &self,
        group: &ValidatedReloadGroup,
    ) -> anyhow::Result<Option<RuntimeReloadReport>>;
}

/// Shared administration state bound to the registered routes.
pub struct AdminState {
    settings: RwLock<CoreApplicationOptions>,
    temp_password: Option<String>,
    sessions: SessionStore,
    login_limiter: LoginRateLimiter,
    source_enabled: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
    groups: Vec<AdminServiceGroupConfig>,
    adapter: Arc<dyn AdminRuntimeAdapter>,
    reload_coordinator: Arc<ReloadCoordinator>,
}

/// Outcome of the administration state construction.
pub struct AdminStateBuild {
    /// Shared administration state.
    pub state: Arc<AdminState>,
    /// Shared coordinator used by both the administration endpoint and tray callbacks.
    pub reload_coordinator: Arc<ReloadCoordinator>,
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
        settings: CoreApplicationOptions,
        groups: Vec<AdminServiceGroupConfig>,
        source_enabled: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
        adapter: Arc<dyn AdminRuntimeAdapter>,
    ) -> anyhow::Result<AdminStateBuild> {
        let temporary_password = if settings.application_mode.unwrap_or_default()
            == ApplicationMode::Server
            && adapter.persisted_settings()?.password_hash.is_none()
        {
            Some(generate_random_password(TEMPORARY_PASSWORD_LENGTH))
        } else {
            None
        };
        let reload_coordinator = Arc::new(ReloadCoordinator::new(
            groups.clone(),
            Arc::clone(&source_enabled),
            Arc::clone(&adapter),
        ));
        let state = Arc::new(Self {
            settings: RwLock::new(settings),
            temp_password: temporary_password.clone(),
            sessions: SessionStore::new(),
            login_limiter: LoginRateLimiter::new(),
            source_enabled,
            groups,
            adapter,
            reload_coordinator: Arc::clone(&reload_coordinator),
        });
        Ok(AdminStateBuild {
            state,
            reload_coordinator,
            temporary_password,
        })
    }

    /// Returns the effective runtime settings.
    pub(crate) fn settings(&self) -> CoreApplicationOptions {
        self.settings
            .read()
            .expect("admin settings lock poisoned")
            .clone()
    }

    /// Returns the runtime mode.
    pub(crate) fn mode(&self) -> ApplicationMode {
        self.settings().application_mode.unwrap_or_default()
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

    /// Returns the shared reload coordinator used by every reload entry point.
    pub(crate) fn reload_coordinator(&self) -> &Arc<ReloadCoordinator> {
        &self.reload_coordinator
    }

    /// Updates the effective server settings after a hot application.
    ///
    /// Only the values that actually changed are updated, and their provenance
    /// becomes `Configuration`; command-line pinned values stay untouched.
    pub(crate) fn update_effective_server_settings(
        &self,
        server_port: u16,
        network_mode: ServerNetworkMode,
        entrypoint_root: Option<String>,
    ) {
        let mut settings = self.settings.write().expect("admin settings lock poisoned");
        if settings.server_port != Some(server_port) {
            settings.server_port = Some(server_port);
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
