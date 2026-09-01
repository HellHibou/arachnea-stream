//! Administration API: state, access control, and route registration.
//!
//! The API is mounted under `api/admin/<operation>` over HTTP and exposed to
//! the desktop frontend through the Tauri invoke handler under the same
//! command names.

pub mod auth;
pub mod dto;
mod ops;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};

use serde::de::DeserializeOwned;

use arachnea_core::controler::{
    deserialize_input, ControlerJsonInput, ControlerJsonOutput, ControlerService,
    JsonControlerFunction, RequestControlerContext,
};
use arachnea_core::persistence::{CredentialsStore, TypedEntityStore};
use arachnea_scrapyfy::{PersistenceSourceEnabled, SourceServiceRecord};

use crate::configuration::ApplicationConfiguration;
use crate::reloadable_stream_scraper::ReloadableStreamScraper;

use auth::{LoginRateLimiter, SessionStore};
use dto::SettingSource;
use ops::{AdminError, AdminReply};

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

/// Shared administration state bound to the registered routes.
pub struct AdminState {
    settings: RwLock<AdminRuntimeSettings>,
    configuration: RwLock<ApplicationConfiguration>,
    temp_password: Option<String>,
    sessions: SessionStore,
    login_limiter: LoginRateLimiter,
    reloadable: Arc<ReloadableStreamScraper>,
}

impl AdminState {
    /// Creates the administration state shared by all admin operations.
    ///
    /// # Arguments
    /// * `settings` - Effective runtime settings of the running server.
    /// * `configuration` - Persisted application configuration.
    /// * `temp_password` - One-shot temporary password when no permanent hash
    ///   is configured, kept in memory only.
    /// * `reloadable` - Reloadable scraper facade exposed to the admin API.
    pub fn new(
        settings: AdminRuntimeSettings,
        configuration: ApplicationConfiguration,
        temp_password: Option<String>,
        reloadable: Arc<ReloadableStreamScraper>,
    ) -> Self {
        Self {
            settings: RwLock::new(settings),
            configuration: RwLock::new(configuration),
            temp_password,
            sessions: SessionStore::new(),
            login_limiter: LoginRateLimiter::new(),
            reloadable,
        }
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

    /// Returns the persisted application configuration.
    pub(crate) fn configuration(&self) -> ApplicationConfiguration {
        self.configuration
            .read()
            .expect("admin configuration lock poisoned")
            .clone()
    }

    /// Replaces the persisted application configuration held in memory.
    ///
    /// # Arguments
    /// * `configuration` - New configuration value.
    pub(crate) fn set_configuration(&self, configuration: ApplicationConfiguration) {
        *self
            .configuration
            .write()
            .expect("admin configuration lock poisoned") = configuration;
    }

    /// Returns the session store.
    pub(crate) fn sessions(&self) -> &SessionStore {
        &self.sessions
    }

    /// Returns the login rate limiter.
    pub(crate) fn login_limiter(&self) -> &LoginRateLimiter {
        &self.login_limiter
    }

    /// Returns the typed store used for activation overrides.
    pub(crate) fn source_enabled_store(&self) -> Arc<dyn TypedEntityStore<SourceServiceRecord>> {
        self.reloadable.source_enabled_store()
    }

    /// Returns the shared encrypted credentials store.
    pub(crate) fn credentials_store(&self) -> Arc<dyn CredentialsStore> {
        self.reloadable.credentials_store()
    }

    /// Returns the reloadable scraper facade.
    pub(crate) fn reloadable(&self) -> &ReloadableStreamScraper {
        self.reloadable.as_ref()
    }

    /// Builds the persistent activation policy over the services namespace.
    pub(crate) fn activation_policy(&self) -> PersistenceSourceEnabled {
        PersistenceSourceEnabled::with_typed_store(self.source_enabled_store())
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
    Fut: Future<Output = Result<AdminReply, AdminError>> + Send + 'static,
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

/// Registers every administration operation against the controller.
///
/// # Arguments
/// * `state` - Shared administration state.
/// * `controler` - Controller receiving the `admin/*` routes.
pub fn register_admin_service(state: &Arc<AdminState>, controler: &mut dyn ControlerService) {
    register_admin_operation(controler, state, "status", ops::op_status);
    register_admin_operation(controler, state, "login", ops::op_login);
    register_admin_operation(controler, state, "logout", ops::op_logout);
    register_admin_operation(controler, state, "services", ops::op_services);
    register_admin_operation(
        controler,
        state,
        "set-service-enabled",
        ops::op_set_service_enabled,
    );
    register_admin_operation(
        controler,
        state,
        "reset-service-enabled",
        ops::op_reset_service_enabled,
    );
    register_admin_operation(controler, state, "credentials", ops::op_credentials);
    register_admin_operation(controler, state, "set-credentials", ops::op_set_credentials);
    register_admin_operation(
        controler,
        state,
        "clear-credentials",
        ops::op_clear_credentials,
    );
    register_admin_operation(controler, state, "settings", ops::op_settings);
    register_admin_operation(controler, state, "update-settings", ops::op_update_settings);
    register_admin_operation(
        controler,
        state,
        "set-admin-password",
        ops::op_set_admin_password,
    );
    register_admin_operation(controler, state, "reload", ops::op_reload);
}
