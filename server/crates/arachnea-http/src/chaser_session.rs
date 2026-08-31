use std::{sync::Arc, time::Duration};

use arachnea_core::persistence::{
    EntityReader, EntitySchema, EntityWriter, Field, MemoryEntityStore, PersistenceStoreConfig,
    PersistentEntity, TypedEntityStore,
};
use serde::{Deserialize, Serialize};

/// Stable store name for cached Cloudflare sessions.
pub const CLOUDFLARE_SESSION_STORE_NAME: &str = "cloudflare-session";

/// TTL of cached sessions whose `cf_clearance` carries no expiration.
pub(crate) const CACHE_TTL_NO_EXPIRY: Duration = Duration::from_secs(1_800);

/// Serialized Cloudflare session extracted from the shared browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
            .field(Field::date_time("expires_at").expiration())
            .field(Field::json("session"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> anyhow::Result<()> {
        writer.string("origin", &self.origin)?;
        let expires_at = self
            .clearance_expires_at
            .map(|value| std::time::UNIX_EPOCH + Duration::from_secs(value))
            .unwrap_or_else(|| {
                std::time::UNIX_EPOCH
                    + Duration::from_secs(self.stored_at + CACHE_TTL_NO_EXPIRY.as_secs())
            });
        writer.date_time("expires_at", expires_at)?;
        writer.json("session", serde_json::to_value(self)?)
    }
    fn read_from(reader: &EntityReader<'_>) -> anyhow::Result<Self> {
        let origin = reader.string("origin")?.to_string();
        let mut session: Self = serde_json::from_value(reader.json("session")?.clone())?;
        if !session.origin.is_empty() && session.origin != origin {
            anyhow::bail!("persisted Cloudflare session origin does not match its record");
        }
        session.origin = origin;
        Ok(session)
    }
}

/// Creates an in-memory typed session store used by compatibility constructors.
pub fn memory_session_store() -> anyhow::Result<Arc<dyn TypedEntityStore<CachedChaserSession>>> {
    let config =
        PersistenceStoreConfig::new(CLOUDFLARE_SESSION_STORE_NAME, CachedChaserSession::schema())?;
    Ok(Arc::new(MemoryEntityStore::<CachedChaserSession>::new(config)?))
}
