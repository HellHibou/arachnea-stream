//! Public account/session service, independent of provider and admin credentials.

mod credentials;
mod dto;
pub mod guard;
mod native;
pub mod routes;
mod service;
pub(crate) mod sessions;
#[cfg(windows)]
mod windows_vault;

use super::common::CommonRepository;
use anyhow::{Context, Result};
pub use dto::{AccountView, SessionView};
use serde_json::Value;
pub use service::AccountService;
use std::sync::Arc;

/// Runs synchronous account storage/crypto work outside the async executor.
async fn blocking<T: Send + 'static>(
    common: Arc<CommonRepository>,
    work: impl FnOnce(&CommonRepository) -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(move || work(&common))
        .await
        .context("public account task failed")?
}

/// Stable application error with a transport-independent HTTP status.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct AccountError {
    /// Stable code translated by frontend clients.
    pub code: &'static str,
    /// Intended REST status.
    pub status: u16,
    /// Non-secret English diagnostic.
    pub message: &'static str,
    /// Optional input field receiving a validation message.
    pub field: Option<&'static str>,
}

/// Builds a typed account/profile rejection usable through contextual anyhow errors.
///
/// # Arguments
/// * `code` - Stable client error identifier.
/// * `status` - HTTP status shared by REST and native transports.
/// * `message` - Non-secret English diagnostic.
pub(crate) fn reject(code: &'static str, status: u16, message: &'static str) -> anyhow::Error {
    AccountError {
        code,
        status,
        message,
        field: None,
    }
    .into()
}

/// Formats an API failure without exposing database/credential diagnostics.
///
/// # Arguments
/// * `error` - Contextual account/profile rejection or internal storage failure.
pub(crate) fn failure(error: &anyhow::Error) -> (u16, Value) {
    let (status, code, message) = error
        .downcast_ref::<AccountError>()
        .map(|e| (e.status, e.code, e.message))
        .unwrap_or((
            503,
            "storage_unavailable",
            "Account storage is unavailable.",
        ));
    if status == 503 {
        tracing::error!(error = %error, "public account operation failed");
    }
    let fields = error
        .downcast_ref::<AccountError>()
        .and_then(|error| {
            error
                .field
                .map(|field| serde_json::json!({(field): error.message}))
        })
        .unwrap_or_else(|| serde_json::json!({}));
    (
        status,
        serde_json::json!({"data":null,"error":{"code":code,"message":message,"field_errors":fields,"operation_id":null}}),
    )
}

/// Adds a safe field path to a validation error, retaining its stable code.
///
/// # Arguments
/// * `field` - Stable request field path, without client data.
/// * `error` - Typed rejection or contextual storage failure to preserve.
pub(crate) fn for_field(field: &'static str, error: anyhow::Error) -> anyhow::Error {
    match error.downcast::<AccountError>() {
        Ok(mut error) => {
            error.field = Some(field);
            error.into()
        }
        Err(error) => error,
    }
}
