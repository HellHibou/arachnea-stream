//! Application-owned common profiles and streaming persistence.
//!
//! Account and profile adapters share persistent authorization and lifecycle guards.

/// Public account/session service and transport authorization.
pub mod accounts;

/// Common account/profile identities and preferences.
pub mod common;
/// Durable coordination and recovery of operations spanning the two stores.
pub mod coordination;
/// Composite key implementations for profile resources.
pub mod keys;
mod stores;
/// Streaming documents reusing authoritative common profile identities.
pub mod streaming;

pub use coordination::ProfileCoordinator;
pub use stores::ProfileStores;

use anyhow::Result;
use arachnea_core::persistence::{EntityQuery, Field, PersistentEntity};
use serde::{de::DeserializeOwned, Serialize};
use uuid::Uuid;

/// Builds a UUID text equality filter for a declared owner/reference field.
///
/// # Arguments
/// * `field` - Explicit UUID text column.
/// * `id` - Identity to match without regenerating it.
pub(crate) fn by_id<E: PersistentEntity>(field: &str, id: Uuid) -> EntityQuery<E> {
    EntityQuery::new().where_string(&Field::string(field), id.to_string())
}

/// Encodes a lifecycle enum as its stable snake_case string.
///
/// # Arguments
/// * `value` - String-serialized lifecycle enum.
///
/// # Errors
/// Returns a serialization or non-string enum error.
fn enum_string(value: &impl Serialize) -> Result<String> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("lifecycle enum must serialize as string"))
}

/// Decodes a persisted lifecycle enum, rejecting unknown states.
///
/// # Arguments
/// * `value` - Persisted snake_case state.
///
/// # Errors
/// Returns an error for unsupported states.
fn read_enum<T: DeserializeOwned>(value: &str) -> Result<T> {
    Ok(serde_json::from_value(serde_json::Value::String(
        value.to_string(),
    ))?)
}

/// Embedded avatar catalogue and protected profile transport adapters.
pub mod routes;
mod validation;
