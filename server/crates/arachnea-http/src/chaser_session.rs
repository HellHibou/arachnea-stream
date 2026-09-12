use std::sync::Arc;
#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
use std::time::SystemTime;
use std::time::{Duration, UNIX_EPOCH};

use arachnea_core::persistence::{
    EntityReader, EntitySchema, EntityWriter, Field, MemoryEntityStore, PersistenceStoreConfig,
    PersistentEntity, TypedEntityStore,
};
#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
use http::{header::SET_COOKIE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
use url::Url;

#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
use crate::engine::SOLVER_USER_AGENT_HEADER;
#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
use crate::error::ArachneaHttpError;

/// Stable store name for cached Cloudflare sessions.
pub const CLOUDFLARE_SESSION_STORE_NAME: &str = "cloudflare-session";

/// TTL of cached sessions whose `cf_clearance` carries no expiration.
pub(crate) const CACHE_TTL_NO_EXPIRY: Duration = Duration::from_secs(1_800);

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

/// Normalized origin key used to key cached Cloudflare sessions.
///
/// Strips paths and query strings so a session solved for one URL on an origin
/// is reusable for every URL of that origin, exactly like the chaser-cf
/// adapter did.
#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
pub(crate) fn cache_origin_key(value: &str) -> Result<String, ArachneaHttpError> {
    let url = Url::parse(value).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| ArachneaHttpError::InvalidUrl("URL must contain a host".to_string()))?;
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Ok(format!("{}://{host}{port}/", url.scheme()))
}

/// Current Unix timestamp in whole seconds.
#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
pub(crate) fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Formats an absolute Unix timestamp as an HTTP `Expires` header value.
#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
pub(crate) fn expires_http_date(expires: Option<f64>) -> Option<String> {
    let expires = expires?;
    if !expires.is_finite() || expires < 0.0 {
        return None;
    }
    let duration = Duration::try_from_secs_f64(expires).ok()?;
    let instant = UNIX_EPOCH.checked_add(duration)?;
    Some(httpdate::fmt_http_date(instant))
}

/// `cf_clearance` expiration from a structured cookie list, when known.
///
/// The Obscura facade does not expose cookie expiration, in which case the
/// returned value is `None` and the session cache falls back to its no-expiry
/// TTL bound (`CACHE_TTL_NO_EXPIRY`).
#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
pub(crate) fn clearance_expires_at(cookies: &[StructuredCookie]) -> Option<u64> {
    cookies
        .iter()
        .find(|cookie| cookie.name == "cf_clearance")
        .and_then(|cookie| cookie.expires)
        .filter(|expires| expires.is_finite() && *expires >= 0.0)
        .map(|expires| expires as u64)
}

/// Serializes one structured cookie into a `Set-Cookie` header value.
#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
pub(crate) fn set_cookie_header(cookie: &StructuredCookie) -> String {
    let mut value = format!("{}={}", cookie.name, cookie.value);
    if let Some(domain) = &cookie.domain {
        value.push_str("; Domain=");
        value.push_str(domain);
    }
    if let Some(path) = &cookie.path {
        value.push_str("; Path=");
        value.push_str(path);
    }
    if let Some(expires) = expires_http_date(cookie.expires) {
        value.push_str("; Expires=");
        value.push_str(&expires);
    }
    if cookie.secure.unwrap_or(false) {
        value.push_str("; Secure");
    }
    if cookie.http_only.unwrap_or(false) {
        value.push_str("; HttpOnly");
    }
    if let Some(same_site) = &cookie.same_site {
        value.push_str("; SameSite=");
        value.push_str(same_site);
    }
    value
}

/// Builds the response header map for a cached Cloudflare session: synthesized
/// `Set-Cookie` headers plus the observed solver user-agent.
#[cfg(any(feature = "chaser-cf", feature = "obscura"))]
pub(crate) fn cached_session_headers(
    session: &CachedChaserSession,
) -> Result<HeaderMap, ArachneaHttpError> {
    let mut headers = HeaderMap::new();
    for cookie in &session.cookies {
        headers.append(
            SET_COOKIE,
            HeaderValue::from_str(&set_cookie_header(cookie))
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
        );
    }
    if let Some(user_agent) = &session.user_agent {
        headers.insert(
            SOLVER_USER_AGENT_HEADER,
            HeaderValue::from_str(user_agent)
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
        );
    }
    Ok(headers)
}

impl CachedChaserSession {
    /// Returns true while the clearance is still usable: before `cf_clearance`
    /// expiry minus the proactive refresh margin, or within the no-expiry TTL
    /// bound when the cookie carried no expiration.
    #[cfg(any(feature = "chaser-cf", feature = "obscura"))]
    pub(crate) fn is_usable(&self, refresh_margin: Duration) -> bool {
        match self.clearance_expires_at {
            Some(expires_at) => {
                expires_at > unix_timestamp().saturating_add(refresh_margin.as_secs())
            }
            None => self.stored_at.saturating_add(CACHE_TTL_NO_EXPIRY.as_secs()) > unix_timestamp(),
        }
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
mod tests {
    use super::*;
    use arachnea_core::persistence::{EntityQuery, FileEntityStore, SqliteEntityStore};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "arachnea-session-{}-{}-{}",
            label,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn store_config() -> PersistenceStoreConfig {
        PersistenceStoreConfig::new(CLOUDFLARE_SESSION_STORE_NAME, CachedChaserSession::schema())
            .expect("valid config")
    }

    fn unix_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX_EPOCH")
            .as_secs()
    }

    fn sample(origin: &str, clearance: Option<u64>, stored_at: u64) -> CachedChaserSession {
        CachedChaserSession {
            origin: origin.to_string(),
            cookies: vec![StructuredCookie {
                name: "cf_clearance".into(),
                value: "secret".into(),
                domain: Some(".example.com".into()),
                path: Some("/".into()),
                expires: Some(1234.5),
                http_only: Some(true),
                secure: Some(true),
                same_site: Some("Lax".into()),
            }],
            user_agent: Some("ua".into()),
            clearance_expires_at: clearance,
            stored_at,
        }
    }

    async fn assert_full_roundtrip(
        store: &dyn TypedEntityStore<CachedChaserSession>,
        origin: &str,
    ) -> anyhow::Result<()> {
        let stored_at = unix_now();
        store
            .put(&sample(origin, Some(stored_at + 3_600), stored_at))
            .await?;
        let loaded = store
            .get(&origin.to_string())
            .await?
            .expect("session must survive the roundtrip");
        assert_eq!(loaded.origin, origin);
        assert_eq!(loaded.user_agent.as_deref(), Some("ua"));
        assert_eq!(loaded.clearance_expires_at, Some(stored_at + 3_600));
        assert_eq!(loaded.cookies.len(), 1);
        assert_eq!(loaded.cookies[0].name, "cf_clearance");
        assert_eq!(loaded.cookies[0].domain.as_deref(), Some(".example.com"));
        assert_eq!(loaded.cookies[0].path.as_deref(), Some("/"));
        assert_eq!(loaded.cookies[0].expires, Some(1234.5));
        assert_eq!(loaded.cookies[0].http_only, Some(true));
        assert_eq!(loaded.cookies[0].secure, Some(true));
        assert_eq!(loaded.cookies[0].same_site.as_deref(), Some("Lax"));

        // Optional fields stay absent through the roundtrip.
        let mut minimal = sample(origin, None, stored_at);
        minimal.user_agent = None;
        store.put(&minimal).await?;
        let loaded = store
            .get(&origin.to_string())
            .await?
            .expect("minimal session must survive the roundtrip");
        assert_eq!(loaded.user_agent, None);
        assert_eq!(loaded.clearance_expires_at, None);
        assert_eq!(loaded.cookies.len(), 1);
        Ok(())
    }

    /// Item 1: the scalar `origin` key roundtrips in the memory backend.
    #[tokio::test]
    async fn memory_roundtrip_by_origin() -> anyhow::Result<()> {
        let store = MemoryEntityStore::<CachedChaserSession>::new(store_config())?;
        assert_full_roundtrip(&store, "https://memory.example.com").await?;
        store
            .delete(&"https://memory.example.com".to_string())
            .await?;
        assert!(store
            .get(&"https://memory.example.com".to_string())
            .await?
            .is_none());
        Ok(())
    }

    /// Item 1: the scalar `origin` key roundtrips in the file backend and
    /// survives a reopen.
    #[tokio::test]
    async fn file_roundtrip_survives_reopen() -> anyhow::Result<()> {
        let dir = temp_dir("file");
        let stored_at = unix_now();
        let config = store_config();
        {
            let store = FileEntityStore::<CachedChaserSession>::new(&dir, config.clone())?;
            store
                .put(&sample(
                    "https://file.example.com",
                    Some(stored_at + 3_600),
                    stored_at,
                ))
                .await?;
        }
        let store = FileEntityStore::<CachedChaserSession>::new(&dir, config)?;
        let loaded = store
            .get(&"https://file.example.com".to_string())
            .await?
            .expect("file session must survive the reopen");
        assert_eq!(loaded.origin, "https://file.example.com");
        store
            .delete(&"https://file.example.com".to_string())
            .await?;
        assert!(store
            .get(&"https://file.example.com".to_string())
            .await?
            .is_none());
        Ok(())
    }

    /// Item 2: flattened optional fields roundtrip through SQLite.
    #[tokio::test]
    async fn sqlite_optional_fields_roundtrip() -> anyhow::Result<()> {
        let root = temp_dir("sqlite-fields");
        let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
        assert_full_roundtrip(&store, "https://sqlite.example.com").await?;
        Ok(())
    }

    /// Item 4: SQLite reopen keeps the sessions, the origin lookup works, and
    /// an invalid batch writes nothing.
    #[tokio::test]
    async fn sqlite_reopen_batch_and_origin_lookup() -> anyhow::Result<()> {
        let root = temp_dir("sqlite-reopen");
        let stored_at = unix_now();
        let first = sample(
            "https://first.example.com",
            Some(stored_at + 3_600),
            stored_at,
        );
        let second = sample("https://second.example.com", None, stored_at);
        {
            let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
            store.put_all(&[first.clone(), second.clone()]).await?;
        }
        let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
        assert_eq!(
            store
                .get(&"https://first.example.com".to_string())
                .await?
                .expect("first session"),
            first
        );
        assert_eq!(
            store
                .get(&"https://second.example.com".to_string())
                .await?
                .expect("second session"),
            second
        );
        // Origin lookup through a query predicate.
        let schema = CachedChaserSession::schema();
        let found = store
            .find(&EntityQuery::<CachedChaserSession>::new().where_string(
                schema.field_named("origin").expect("origin field"),
                "https://first.example.com",
            ))
            .await?;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].origin, "https://first.example.com");

        // A batch carrying an invalid entity is rejected atomically.
        let mut invalid = sample("https://third.example.com", None, u64::MAX);
        invalid.user_agent = None;
        assert!(store.put_all(&[invalid, second]).await.is_err());
        assert!(
            store
                .get(&"https://third.example.com".to_string())
                .await?
                .is_none(),
            "invalid batch must write nothing"
        );
        Ok(())
    }

    /// Item 3: expiration honors the `cf_clearance` deadline and the fallback
    /// TTL when no clearance expiration is available.
    #[tokio::test]
    async fn expiration_deadline_and_fallback_ttl() -> anyhow::Result<()> {
        let root = temp_dir("sqlite-expiry");
        let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
        let now = unix_now();

        // Live `cf_clearance` deadline keeps the session readable.
        store
            .put(&sample("https://live.example.com", Some(now + 3_600), now))
            .await?;
        assert!(store
            .get(&"https://live.example.com".to_string())
            .await?
            .is_some());

        // Without a clearance expiration the 1_800s fallback TTL applies.
        store
            .put(&sample("https://stale.example.com", None, now - 3_600))
            .await?;
        assert!(
            store
                .get(&"https://stale.example.com".to_string())
                .await?
                .is_none(),
            "fallback TTL must expire the session"
        );

        store
            .put(&sample("https://fresh.example.com", None, now - 60))
            .await?;
        assert!(store
            .get(&"https://fresh.example.com".to_string())
            .await?
            .is_some());
        Ok(())
    }

    /// Item 5: a legacy `session TEXT NOT NULL` cache is strictly rejected and
    /// a manual deletion of the store directory allows opening a fresh store.
    #[tokio::test]
    async fn legacy_cache_rejected_until_manual_deletion() -> anyhow::Result<()> {
        let root = temp_dir("sqlite-legacy");
        let store_dir = root.join("cloudflare-session");
        // Establish the metadata layout with a fresh flattened store first.
        drop(SqliteEntityStore::<CachedChaserSession>::new(
            store_config(),
            &root,
        )?);

        // Rewrite the database as the legacy cache: table plus managed-column
        // metadata exactly as an older version would have left them.
        let connection = rusqlite::Connection::open(store_dir.join("records.sqlite3"))?;
        connection.execute("DROP TABLE \"cloudflare-session\"", [])?;
        connection.execute(
            "CREATE TABLE \"cloudflare-session\" (\
             \"origin\" TEXT NOT NULL, \
             \"expires_at\" INTEGER NOT NULL, \
             \"session\" TEXT NOT NULL, \
             PRIMARY KEY (\"origin\")) STRICT",
            [],
        )?;
        connection.execute("DELETE FROM arachnea_columns", [])?;
        connection.execute("DELETE FROM arachnea_indexes", [])?;
        for (name, logical_type) in [
            ("origin", "string"),
            ("expires_at", "date_time"),
            ("session", "string"),
        ] {
            connection.execute(
                "INSERT INTO arachnea_columns (name, logical_type) VALUES (?1, ?2)",
                rusqlite::params![name, logical_type],
            )?;
        }
        let legacy_marker = "legacy-cookie-value";
        let expires_at_nanos = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX_EPOCH")
            .as_nanos()
            + 3_600_000_000_000) as i64;
        connection.execute(
            "INSERT INTO \"cloudflare-session\" (\"origin\", \"expires_at\", \"session\") \
             VALUES (?1, ?2, ?3)",
            rusqlite::params![
                "https://legacy.example.com",
                expires_at_nanos,
                format!("{legacy_marker}")
            ],
        )?;
        drop(connection);

        // Reopening succeeds (reconciliation only adds nullable columns), but
        // the legacy cache is strictly rejected: reads and writes fail.
        let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
        let read_error = store
            .get(&"https://legacy.example.com".to_string())
            .await
            .expect_err("legacy row must fail to decode");
        let write_error = store
            .put(&sample("https://legacy.example.com", None, unix_now()))
            .await
            .expect_err("legacy table must reject flattened inserts");
        let write_error = write_error.to_string();
        assert!(
            write_error.contains("NOT NULL constraint failed"),
            "unexpected write error: {write_error}"
        );
        for error in [read_error.to_string(), write_error] {
            assert!(
                !error.contains(legacy_marker) && !error.contains("secret"),
                "error text must never contain cookie values: {error}"
            );
        }
        drop(store);

        // Manual deletion of the store directory (WAL/SHM included) allows
        // opening a fresh store.
        std::fs::remove_dir_all(&store_dir)?;
        let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
        let stored_at = unix_now();
        store
            .put(&sample(
                "https://legacy.example.com",
                Some(stored_at + 3_600),
                stored_at,
            ))
            .await?;
        assert!(store
            .get(&"https://legacy.example.com".to_string())
            .await?
            .is_some());
        Ok(())
    }

    /// Item 2: corrupted persisted fields fail with contextualized errors that
    /// exclude the corrupted payload and cookie values.
    #[tokio::test]
    async fn corrupted_fields_fail_without_leaking_values() -> anyhow::Result<()> {
        let root = temp_dir("sqlite-corrupt");
        let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
        let db = root.join("cloudflare-session").join("records.sqlite3");
        let origin = "https://corrupt.example.com";
        store.put(&sample(origin, None, unix_now())).await?;
        drop(store);

        let connection = rusqlite::Connection::open(&db)?;
        // Valid JSON with the wrong shape reaches `read_from`, which
        // contextualizes the failure without the payload.
        connection.execute(
            "UPDATE \"cloudflare-session\" SET \"cookies\" = ?1 WHERE \"origin\" = ?2",
            rusqlite::params!["{\"unexpected\": true}", origin],
        )?;
        drop(connection);

        let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
        let error = store
            .get(&origin.to_string())
            .await
            .expect_err("corrupted cookies must fail to decode");
        let error = error.to_string();
        assert!(
            !error.contains("legacy-cookie-value") && !error.contains("secret"),
            "error text must never contain cookie values: {error}"
        );
        assert!(
            error.contains("malformed cookie data"),
            "unexpected error: {error}"
        );
        drop(store);

        // Invalid JSON fails earlier, in the SQLite layer, with an error that
        // still excludes the stored value.
        let connection = rusqlite::Connection::open(&db)?;
        connection.execute(
            "UPDATE \"cloudflare-session\" SET \"cookies\" = ?1 WHERE \"origin\" = ?2",
            rusqlite::params!["legacy-cookie-value", origin],
        )?;
        drop(connection);

        let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
        let error = store
            .get(&origin.to_string())
            .await
            .expect_err("invalid cookie JSON must fail to decode");
        let error = error.to_string();
        assert!(
            !error.contains("legacy-cookie-value") && !error.contains("secret"),
            "error text must never contain cookie values: {error}"
        );
        drop(store);

        // An out-of-range clearance expiration is rejected as well.
        let connection = rusqlite::Connection::open(&db)?;
        connection.execute(
            "UPDATE \"cloudflare-session\" SET \"cookies\" = ?1, \"clearance_expires_at\" = -5 \
             WHERE \"origin\" = ?2",
            rusqlite::params!["[]", origin],
        )?;
        drop(connection);
        let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
        let error = store
            .get(&origin.to_string())
            .await
            .expect_err("out-of-range clearance must fail");
        let error = error.to_string();
        assert!(!error.contains("-5"), "error leaked an integer: {error}");
        assert!(
            error.contains("out-of-range clearance expiration"),
            "unexpected error: {error}"
        );
        Ok(())
    }
}
