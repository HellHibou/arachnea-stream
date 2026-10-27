use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use arachnea_core::persistence::{
    EntityReader, EntitySchema, EntityWriter, Field, MemoryEntityStore, PersistenceStoreConfig,
    PersistentEntity, TypedEntityStore,
};
use serde::{Deserialize, Serialize};

/// Stable store name for cached Cloudflare sessions.
pub use crate::CLOUDFLARE_SESSION_STORE_NAME;

/// TTL of cached sessions whose `cf_clearance` carries no expiration.
pub(crate) use crate::HTTP_SESSION_CACHE_TTL_NO_EXPIRY as CACHE_TTL_NO_EXPIRY;

/// Serialized Cloudflare session extracted from the shared browser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedChaserSession {
    /// Origin primary key for this cached session.
    pub(crate) origin: String,
    /// Structured cookies produced from browser cookies.
    pub(crate) cookies: Vec<StructuredCookie>,
    /// Browser user-agent observed by the solver.
    pub(crate) user_agent: Option<String>,
    /// Unix timestamp for `cf_clearance` expiration when available.
    pub(crate) clearance_expires_at: Option<u64>,
    /// Unix timestamp when the session was stored.
    pub(crate) stored_at: u64,
}

/// Structured cookie representation persisted in the session record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct StructuredCookie {
    /// Cookie name.
    pub(crate) name: String,
    /// Cookie value.
    pub(crate) value: String,
    /// Cookie domain.
    pub(crate) domain: Option<String>,
    /// Cookie path.
    pub(crate) path: Option<String>,
    /// Cookie expiration as Unix timestamp.
    pub(crate) expires: Option<f64>,
    /// Whether the cookie is HTTP-only.
    pub(crate) http_only: Option<bool>,
    /// Whether the cookie is secure.
    pub(crate) secure: Option<bool>,
    /// SameSite attribute.
    pub(crate) same_site: Option<String>,
}

impl PersistentEntity for CachedChaserSession {
    type Key = String;
    fn key(&self) -> Self::Key {
        self.origin.clone()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("origin"))
            .field(Field::json("cookies"))
            .field(Field::string("user_agent").nullable())
            .field(Field::integer("clearance_expires_at").nullable())
            .field(Field::integer("stored_at"))
            .field(Field::date_time("expires_at").expiration().indexed())
    }
    fn write_to(&self, writer: &mut EntityWriter) -> anyhow::Result<()> {
        writer.string("origin", &self.origin)?;
        writer.json("cookies", serde_json::to_value(&self.cookies)?)?;
        if let Some(user_agent) = &self.user_agent {
            writer.string("user_agent", user_agent)?;
        }
        if let Some(clearance_expires_at) = self.clearance_expires_at {
            writer.integer(
                "clearance_expires_at",
                i64::try_from(clearance_expires_at).map_err(|_| {
                    anyhow::anyhow!(
                        "persisted Cloudflare session '{}' clearance expiration is out of range",
                        self.origin
                    )
                })?,
            )?;
        }
        writer.integer(
            "stored_at",
            i64::try_from(self.stored_at).map_err(|_| {
                anyhow::anyhow!(
                    "persisted Cloudflare session '{}' storage timestamp is out of range",
                    self.origin
                )
            })?,
        )?;
        let expires_at = self
            .clearance_expires_at
            .map(|value| UNIX_EPOCH + Duration::from_secs(value))
            .unwrap_or_else(|| {
                UNIX_EPOCH + Duration::from_secs(self.stored_at + CACHE_TTL_NO_EXPIRY.as_secs())
            });
        writer.date_time("expires_at", expires_at)
    }
    fn read_from(reader: &EntityReader<'_>) -> anyhow::Result<Self> {
        let origin = reader.string("origin")?.to_string();
        // Cookie values are deliberately excluded from error text: they are
        // authentication-bound credentials.
        let cookies: Vec<StructuredCookie> =
            serde_json::from_value(reader.json("cookies")?.clone()).map_err(|_| {
                anyhow::anyhow!(
                    "persisted Cloudflare session '{origin}' carries malformed cookie data"
                )
            })?;
        let user_agent = reader.optional_string("user_agent")?.map(str::to_string);
        let clearance_expires_at = reader
            .optional_integer("clearance_expires_at")?
            .map(|value| {
                u64::try_from(value).map_err(|_| {
                    anyhow::anyhow!(
                        "persisted Cloudflare session '{origin}' has an out-of-range clearance expiration"
                    )
                })
            })
            .transpose()?;
        let stored_at = u64::try_from(reader.integer("stored_at")?).map_err(|_| {
            anyhow::anyhow!(
                "persisted Cloudflare session '{origin}' has an out-of-range storage timestamp"
            )
        })?;
        Ok(Self {
            origin,
            cookies,
            user_agent,
            clearance_expires_at,
            stored_at,
        })
    }
}

/// Creates an in-memory typed session store used by compatibility constructors.
pub fn memory_session_store() -> anyhow::Result<Arc<dyn TypedEntityStore<CachedChaserSession>>> {
    let config =
        PersistenceStoreConfig::new(CLOUDFLARE_SESSION_STORE_NAME, CachedChaserSession::schema())?;
    Ok(Arc::new(MemoryEntityStore::<CachedChaserSession>::new(
        config,
    )?))
}

#[cfg(test)]
#[path = "../tests/chaser_session.rs"]
mod tests;
