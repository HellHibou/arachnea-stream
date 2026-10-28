//! Typed Cloudflare session persistence, expiry checks and cookie conversion.

use arachnea_core::persistence::TypedEntityStore;
use async_trait::async_trait;
use chaser_cf::{Cookie as ChaserCookie, WafSession};
use http::{header::SET_COOKIE, HeaderMap, HeaderValue};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use url::Url;

use crate::{
    chaser_session::{CachedChaserSession, StructuredCookie, CACHE_TTL_NO_EXPIRY},
    engine::SOLVER_USER_AGENT_HEADER,
    error::ArachneaHttpError,
};

/// Asynchronous cache for chaser-cf Cloudflare sessions backed by a typed repository.
pub(super) struct ChaserSessionCache {
    /// Shared typed session repository.
    pub(super) repository: Arc<dyn CloudflareSessionRepository>,
    /// Proactive refresh margin before `cf_clearance` expiry.
    pub(super) refresh_margin: Duration,
}

/// Repository for persisted Cloudflare sessions.
#[async_trait]
pub(super) trait CloudflareSessionRepository: Send + Sync {
    /// Loads the cached session for one origin key.
    ///
    /// # Arguments
    ///
    /// - `origin`: Scheme, host, and explicit port key produced by
    ///   [`cache_origin_key`].
    ///
    /// # Errors
    ///
    /// Returns the persistence backend failure.
    async fn get(&self, origin: &str) -> anyhow::Result<Option<CachedChaserSession>>;

    /// Saves a session under its origin key.
    ///
    /// # Arguments
    ///
    /// - `session`: Session to insert or replace.
    ///
    /// # Errors
    ///
    /// Returns the persistence backend failure.
    async fn save(&self, session: &CachedChaserSession) -> anyhow::Result<()>;

    /// Deletes the cached session for one origin key.
    ///
    /// # Arguments
    ///
    /// - `origin`: Origin key to remove.
    ///
    /// # Errors
    ///
    /// Returns the persistence backend failure.
    async fn delete(&self, origin: &str) -> anyhow::Result<()>;
}

/// Repository adapter backed by a typed entity store.
pub(super) struct TypedCloudflareSessionRepository {
    store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
}

impl TypedCloudflareSessionRepository {
    /// Wraps a typed session store.
    ///
    /// # Arguments
    ///
    /// - `store`: Store used for `CachedChaserSession` records.
    pub(super) fn new(store: Arc<dyn TypedEntityStore<CachedChaserSession>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl CloudflareSessionRepository for TypedCloudflareSessionRepository {
    async fn get(&self, origin: &str) -> anyhow::Result<Option<CachedChaserSession>> {
        self.store.get(&origin.to_string()).await
    }
    async fn save(&self, session: &CachedChaserSession) -> anyhow::Result<()> {
        self.store.put(session).await
    }
    async fn delete(&self, origin: &str) -> anyhow::Result<()> {
        self.store.delete(&origin.to_string()).await
    }
}

impl ChaserSessionCache {
    /// Returns cached headers for a still-valid Cloudflare session.
    ///
    /// A cached session that is expired or inside the refresh margin is deleted
    /// before returning a miss.
    ///
    /// # Arguments
    ///
    /// - `url`: URL whose origin keys the session.
    ///
    /// # Returns
    ///
    /// `Set-Cookie` and solver user-agent headers, or `None` on a cache miss.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` for an unusable URL, `InvalidHeader` for an
    /// unrepresentable header, or `ChaserCfFailure` for repository failures.
    pub(super) async fn headers_for_url(
        &self,
        url: &str,
    ) -> Result<Option<HeaderMap>, ArachneaHttpError> {
        let origin = cache_origin_key(url)?;
        let session = self
            .repository
            .get(&origin)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let Some(session) = session else {
            return Ok(None);
        };
        if !session.is_usable(self.refresh_margin) {
            self.repository
                .delete(&origin)
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
            return Ok(None);
        }
        cached_session_headers(&session).map(Some)
    }

    /// Stores a newly solved Cloudflare session.
    ///
    /// Sessions without `cf_clearance` are ignored.
    ///
    /// # Arguments
    ///
    /// - `url`: URL whose origin keys the session.
    /// - `session`: Freshly solved browser session.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` for an unusable URL or `ChaserCfFailure` when the
    /// repository write fails.
    pub(super) async fn store_session(
        &self,
        url: &str,
        session: &WafSession,
    ) -> Result<(), ArachneaHttpError> {
        let origin = cache_origin_key(url)?;
        let Some(mut session) = CachedChaserSession::from_waf_session(session) else {
            return Ok(());
        };
        session.origin = origin;
        self.repository
            .save(&session)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))
    }
}

impl CachedChaserSession {
    /// Converts a solved WAF session into a cache record.
    ///
    /// # Arguments
    ///
    /// - `session`: Solved browser session.
    ///
    /// # Returns
    ///
    /// `None` when the session lacks `cf_clearance`. The returned record has an
    /// empty origin that the caller must replace with the cache key.
    fn from_waf_session(session: &WafSession) -> Option<Self> {
        if !session
            .cookies
            .iter()
            .any(|cookie| cookie.name == "cf_clearance")
        {
            return None;
        }
        Some(Self {
            origin: String::new(),
            cookies: session.cookies.iter().map(StructuredCookie::from).collect(),
            user_agent: session
                .headers
                .get("user-agent")
                .filter(|value| !value.trim().is_empty())
                .cloned(),
            clearance_expires_at: clearance_expires_at(&session.cookies),
            stored_at: unix_timestamp(),
        })
    }

    /// Returns whether the cached session is still usable.
    ///
    /// A known `cf_clearance` expiry must be later than now plus
    /// `refresh_margin`. Without a known expiry, the session is usable for
    /// [`CACHE_TTL_NO_EXPIRY`] from `stored_at`.
    ///
    /// # Arguments
    ///
    /// - `refresh_margin`: Proactive refresh margin.
    fn is_usable(&self, refresh_margin: Duration) -> bool {
        match self.clearance_expires_at {
            Some(expires_at) => {
                expires_at > unix_timestamp().saturating_add(refresh_margin.as_secs())
            }
            None => self.stored_at.saturating_add(CACHE_TTL_NO_EXPIRY.as_secs()) > unix_timestamp(),
        }
    }
}

impl From<&ChaserCookie> for StructuredCookie {
    fn from(cookie: &ChaserCookie) -> Self {
        Self {
            name: cookie.name.clone(),
            value: cookie.value.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
            expires: cookie.expires,
            http_only: cookie.http_only,
            secure: cookie.secure,
            same_site: cookie.same_site.clone(),
        }
    }
}

impl From<&StructuredCookie> for ChaserCookie {
    fn from(cookie: &StructuredCookie) -> Self {
        Self {
            name: cookie.name.clone(),
            value: cookie.value.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
            expires: cookie.expires,
            http_only: cookie.http_only,
            secure: cookie.secure,
            same_site: cookie.same_site.clone(),
        }
    }
}

/// Builds HTTP headers from a cached session.
///
/// # Arguments
///
/// - `session`: Cached cookies and optional user-agent.
///
/// # Returns
///
/// One `Set-Cookie` header per cached cookie, plus the solver user-agent header
/// when present.
///
/// # Errors
///
/// Returns `InvalidHeader` when a value cannot be represented as a header.
fn cached_session_headers(session: &CachedChaserSession) -> Result<HeaderMap, ArachneaHttpError> {
    let mut headers = HeaderMap::new();
    for cookie in &session.cookies {
        let chaser_cookie = ChaserCookie::from(cookie);
        headers.append(
            SET_COOKIE,
            HeaderValue::from_str(&set_cookie_header(&chaser_cookie))
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

/// Serializes one browser cookie as a `Set-Cookie` value.
///
/// # Arguments
///
/// - `cookie`: Browser cookie to serialize.
///
/// # Returns
///
/// A `name=value` string with the available attributes. Invalid or missing
/// expiry values omit `Expires`.
pub(super) fn set_cookie_header(cookie: &ChaserCookie) -> String {
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

/// Returns the `cf_clearance` expiration as a Unix timestamp.
///
/// # Arguments
///
/// - `cookies`: Browser cookies to inspect.
///
/// # Returns
///
/// The first finite, non-negative `cf_clearance` expiry truncated to whole
/// seconds, or `None` when absent or invalid.
fn clearance_expires_at(cookies: &[ChaserCookie]) -> Option<u64> {
    cookies
        .iter()
        .find(|cookie| cookie.name == "cf_clearance")
        .and_then(|cookie| cookie.expires)
        .filter(|expires| expires.is_finite() && *expires >= 0.0)
        .map(|expires| expires as u64)
}

/// Formats a Unix timestamp as an HTTP date.
///
/// # Arguments
///
/// - `expires`: Optional Unix timestamp in seconds.
///
/// # Returns
///
/// An HTTP date, or `None` when the value is absent, non-finite, negative, or out
/// of range.
fn expires_http_date(expires: Option<f64>) -> Option<String> {
    let expires = expires?;
    if !expires.is_finite() || expires < 0.0 {
        return None;
    }
    let duration = Duration::try_from_secs_f64(expires).ok()?;
    let instant = UNIX_EPOCH.checked_add(duration)?;
    Some(httpdate::fmt_http_date(instant))
}

/// Builds the session-cache key for a URL.
///
/// # Arguments
///
/// - `value`: Absolute URL.
///
/// # Returns
///
/// `scheme://host[:explicit-port]/`. User info, path, and query are discarded;
/// default ports are omitted.
///
/// # Errors
///
/// Returns `InvalidUrl` when the URL cannot be parsed or has no host.
fn cache_origin_key(value: &str) -> Result<String, ArachneaHttpError> {
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

/// Returns the current Unix timestamp in whole seconds.
///
/// A clock before the Unix epoch maps to `0`.
fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
#[path = "../../../tests/engine/chaser_cf/session_cache.rs"]
mod tests;
