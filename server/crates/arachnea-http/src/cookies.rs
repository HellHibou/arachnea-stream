use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime},
};

use http::HeaderMap;
use tokio::sync::RwLock;
use url::Url;

use crate::{cloudflare::CloudflareCookieState, error::ArachneaHttpError};

/// Global process-wide cookie cache.
/// Global process-wide cookie cache shared by all Arachnea HTTP clients.
///
/// Initialized lazily on first access through global_cookie_cache().
static COOKIE_CACHE: OnceLock<Arc<RwLock<SharedCookieCache>>> = OnceLock::new();

/// Returns the shared process-wide cookie cache.
///
/// # Returns
///
/// A clone of the global cookie cache handle.
pub fn global_cookie_cache() -> Arc<RwLock<SharedCookieCache>> {
    COOKIE_CACHE
        .get_or_init(|| Arc::new(RwLock::new(SharedCookieCache::default())))
        .clone()
}

/// A single HTTP cookie stored in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieEntry {
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
    /// Cookie domain.
    pub domain: String,
    /// Cookie path.
    pub path: String,
    /// Expiration time, when known.
    pub expires_at: Option<SystemTime>,
    /// Whether the cookie is restricted to secure requests.
    pub secure: bool,
    /// Whether the cookie is marked as HTTP-only.
    pub http_only: bool,
    /// SameSite attribute value, when present.
    pub same_site: Option<String>,
}

impl CookieEntry {
    /// Checks whether the cookie is expired at `now`.
    ///
    /// # Parameters
    ///
    /// - `now`: Current time used for the expiration comparison.
    ///
    /// # Returns
    ///
    /// `true` when the cookie has an expiration at or before `now`.
    pub fn is_expired(&self, now: SystemTime) -> bool {
        self.expires_at
            .map(|expires_at| expires_at <= now)
            .unwrap_or(false)
    }
}

/// In-memory cookie cache shared by all Arachnea HTTP clients in a process.
#[derive(Debug, Default)]
pub struct SharedCookieCache {
    /// Cookies keyed by normalized domain, path, and name.
    cookies: HashMap<(String, String, String), CookieEntry>,
}

impl SharedCookieCache {
    /// Inserts or replaces one cookie.
    ///
    /// # Parameters
    ///
    /// - `cookie`: Cookie to store.
    pub fn insert_cookie(&mut self, cookie: CookieEntry) {
        let key = cookie_key(&cookie.domain, &cookie.path, &cookie.name);
        self.cookies.insert(key, cookie);
    }

    /// Removes one cookie by domain, name, and path.
    ///
    /// # Parameters
    ///
    /// - `domain`: Cookie domain.
    /// - `name`: Cookie name.
    /// - `path`: Cookie path.
    pub fn remove_cookie(&mut self, domain: &str, name: &str, path: &str) {
        self.cookies.remove(&cookie_key(domain, path, name));
    }

    /// Clears every cookie from the cache.
    pub fn clear_all(&mut self) {
        self.cookies.clear();
    }

    /// Removes expired cookies.
    ///
    /// # Parameters
    ///
    /// - `now`: Current time used for expiration checks.
    pub fn clear_expired(&mut self, now: SystemTime) {
        self.cookies.retain(|_, cookie| !cookie.is_expired(now));
    }

    /// Parses and stores cookies from `Set-Cookie` headers.
    ///
    /// # Parameters
    ///
    /// - `url`: URL whose response carried the headers.
    /// - `headers`: Response headers to inspect.
    /// - `default_session_cookie_ttl`: TTL used for session cookies.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` when `url` cannot be parsed.
    pub fn set_from_set_cookie_headers(
        &mut self,
        url: &str,
        headers: &HeaderMap,
        default_session_cookie_ttl: Duration,
    ) -> Result<(), ArachneaHttpError> {
        let url = Url::parse(url).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
        for value in headers.get_all(http::header::SET_COOKIE) {
            let Ok(raw) = value.to_str() else {
                continue;
            };
            if let Some(cookie) = parse_set_cookie(raw, &url, default_session_cookie_ttl) {
                if cookie.is_expired(SystemTime::now()) {
                    self.remove_cookie(&cookie.domain, &cookie.name, &cookie.path);
                } else {
                    self.insert_cookie(cookie);
                }
            }
        }
        Ok(())
    }

    /// Builds a valid `Cookie` header for a URL.
    ///
    /// # Parameters
    ///
    /// - `url`: Request URL.
    ///
    /// # Returns
    ///
    /// A cookie header value when at least one cookie matches the URL.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` when `url` cannot be parsed.
    pub fn cookie_header_for(&mut self, url: &str) -> Result<Option<String>, ArachneaHttpError> {
        let url = Url::parse(url).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
        let now = SystemTime::now();
        self.clear_expired(now);
        let pairs = self
            .cookies
            .values()
            .filter(|cookie| cookie_matches_url(cookie, &url, now))
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>();
        Ok((!pairs.is_empty()).then(|| pairs.join("; ")))
    }

    /// Returns cookie name/value pairs currently applicable to one URL.
    ///
    /// # Parameters
    ///
    /// - `url`: Request URL.
    ///
    /// # Returns
    ///
    /// A map of matching cookie names to their current values.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` when `url` cannot be parsed.
    pub fn cookies_for_url(
        &mut self,
        url: &str,
    ) -> Result<HashMap<String, String>, ArachneaHttpError> {
        let url = Url::parse(url).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
        let now = SystemTime::now();
        self.clear_expired(now);
        Ok(self
            .cookies
            .values()
            .filter(|cookie| cookie_matches_url(cookie, &url, now))
            .map(|cookie| (cookie.name.clone(), cookie.value.clone()))
            .collect())
    }

    /// Returns Cloudflare cookie state for an origin URL.
    ///
    /// # Parameters
    ///
    /// - `origin`: Origin or URL to inspect.
    ///
    /// # Returns
    ///
    /// Cloudflare cookie presence and expiry state.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` when `origin` cannot be parsed.
    pub fn cloudflare_state(
        &mut self,
        origin: &str,
    ) -> Result<CloudflareCookieState, ArachneaHttpError> {
        let url =
            Url::parse(origin).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
        let now = SystemTime::now();
        self.clear_expired(now);
        let mut state = CloudflareCookieState::default();
        for cookie in self
            .cookies
            .values()
            .filter(|cookie| cookie_matches_url(cookie, &url, now))
        {
            match cookie.name.as_str() {
                "cf_clearance" => {
                    state.has_clearance = true;
                    state.clearance_expires_at = cookie.expires_at;
                }
                "__cf_bm" => state.has_bot_management = true,
                "_cfuvid" => state.has_cfuvid = true,
                _ => {}
            }
        }
        Ok(state)
    }

    /// Determines whether Cloudflare cookies need refresh.
    ///
    /// # Parameters
    ///
    /// - `origin`: Origin or URL to inspect.
    /// - `margin`: Proactive refresh margin before expiry.
    ///
    /// # Returns
    ///
    /// `true` when `cf_clearance` is absent, expired, or near expiry.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` when `origin` cannot be parsed.
    pub fn needs_refresh(
        &mut self,
        origin: &str,
        margin: Duration,
    ) -> Result<bool, ArachneaHttpError> {
        let state = self.cloudflare_state(origin)?;
        if !state.has_clearance {
            return Ok(true);
        }
        let Some(expires_at) = state.clearance_expires_at else {
            return Ok(false);
        };
        Ok(expires_at <= SystemTime::now() + margin)
    }
}

/// Builds the canonical cookie cache key.
///
/// # Parameters
///
/// - `domain`: Cookie domain.
/// - `path`: Cookie path.
/// - `name`: Cookie name.
///
/// # Returns
///
/// A normalized tuple key.
fn cookie_key(domain: &str, path: &str, name: &str) -> (String, String, String) {
    (
        domain.trim_start_matches('.').to_ascii_lowercase(),
        path.to_string(),
        name.to_string(),
    )
}

/// Parses one `Set-Cookie` header value.
///
/// # Parameters
///
/// - `raw`: Raw `Set-Cookie` header value.
/// - `url`: Response URL.
/// - `default_session_cookie_ttl`: TTL for cookies without explicit expiry.
///
/// # Returns
///
/// A parsed cookie when the header contains a valid name-value pair.
fn parse_set_cookie(
    raw: &str,
    url: &Url,
    default_session_cookie_ttl: Duration,
) -> Option<CookieEntry> {
    let mut parts = raw.split(';').map(str::trim);
    let first = parts.next()?;
    let (name, value) = first.split_once('=')?;
    if name.is_empty() {
        return None;
    }

    let mut domain = url.host_str()?.to_ascii_lowercase();
    let mut path = default_path(url.path());
    let mut expires_at = Some(SystemTime::now() + default_session_cookie_ttl);
    let mut max_age_seen = false;
    let mut secure = false;
    let mut http_only = false;
    let mut same_site = None;

    for attr in parts {
        let (key, value) = attr.split_once('=').unwrap_or((attr, ""));
        match key.to_ascii_lowercase().as_str() {
            "domain" if !value.is_empty() => {
                domain = value.trim_start_matches('.').to_ascii_lowercase();
            }
            "path" if !value.is_empty() => path = value.to_string(),
            "max-age" => {
                if let Ok(seconds) = value.parse::<i64>() {
                    max_age_seen = true;
                    expires_at = if seconds <= 0 {
                        Some(SystemTime::UNIX_EPOCH)
                    } else {
                        Some(SystemTime::now() + Duration::from_secs(seconds as u64))
                    };
                }
            }
            "expires" if !max_age_seen => {
                if let Ok(time) = httpdate::parse_http_date(value) {
                    expires_at = Some(time);
                }
            }
            "secure" => secure = true,
            "httponly" => http_only = true,
            "samesite" if !value.is_empty() => same_site = Some(value.to_string()),
            _ => {}
        }
    }

    Some(CookieEntry {
        name: name.to_string(),
        value: value.to_string(),
        domain,
        path,
        expires_at,
        secure,
        http_only,
        same_site,
    })
}

/// Calculates the RFC-style default cookie path.
///
/// # Parameters
///
/// - `request_path`: Path of the response URL.
///
/// # Returns
///
/// The default cookie path.
fn default_path(request_path: &str) -> String {
    if !request_path.starts_with('/') || request_path == "/" {
        return "/".to_string();
    }
    match request_path.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(index) => request_path[..index].to_string(),
    }
}

/// Checks whether a cookie can be sent to a URL.
///
/// # Parameters
///
/// - `cookie`: Cookie being considered.
/// - `url`: Request URL.
/// - `now`: Current time.
///
/// # Returns
///
/// `true` when domain, path, scheme, and expiry constraints match.
fn cookie_matches_url(cookie: &CookieEntry, url: &Url, now: SystemTime) -> bool {
    if cookie.is_expired(now) {
        return false;
    }
    if cookie.secure && url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    let domain = cookie.domain.trim_start_matches('.').to_ascii_lowercase();
    let domain_match = host == domain || host.ends_with(&format!(".{domain}"));
    domain_match && url.path().starts_with(&cookie.path)
}

#[cfg(test)]
/// Unit tests for the shared cookie cache.
mod tests {
    use http::header::SET_COOKIE;

    use super::*;

    /// Builds a header map with one `Set-Cookie` header.
    ///
    /// # Parameters
    ///
    /// - `value`: Cookie header value.
    ///
    /// # Returns
    ///
    /// A header map containing the supplied cookie.
    fn set_cookie_header(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.append(SET_COOKIE, value.parse().expect("valid Set-Cookie header"));
        headers
    }

    /// Verifies that Max-Age cookies are parsed and emitted.
    #[test]
    fn parses_max_age_cookie() {
        let mut cache = SharedCookieCache::default();
        cache
            .set_from_set_cookie_headers(
                "https://example.com/a/b",
                &set_cookie_header("cf_clearance=ok; Max-Age=60; Path=/"),
                Duration::from_secs(3600),
            )
            .expect("cookie parse succeeds");

        let header = cache
            .cookie_header_for("https://example.com/a")
            .expect("cookie header succeeds")
            .expect("cookie exists");
        assert_eq!(header, "cf_clearance=ok");
    }

    /// Verifies that Max-Age takes precedence over an expired Expires attribute.
    #[test]
    fn max_age_takes_precedence_over_expires() {
        let mut cache = SharedCookieCache::default();
        cache
            .set_from_set_cookie_headers(
                "https://example.com/",
                &set_cookie_header(
                    "cf_clearance=ok; Max-Age=60; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/",
                ),
                Duration::from_secs(3600),
            )
            .expect("cookie parse succeeds");

        assert_eq!(
            cache
                .cookie_header_for("https://example.com/")
                .expect("cookie header succeeds")
                .as_deref(),
            Some("cf_clearance=ok")
        );
    }

    /// Verifies that Max-Age=0 removes a stored cookie.
    #[test]
    fn max_age_zero_removes_cookie() {
        let mut cache = SharedCookieCache::default();
        cache
            .set_from_set_cookie_headers(
                "https://example.com/",
                &set_cookie_header("cf_clearance=ok; Max-Age=60; Path=/"),
                Duration::from_secs(3600),
            )
            .expect("initial cookie parse succeeds");
        cache
            .set_from_set_cookie_headers(
                "https://example.com/",
                &set_cookie_header("cf_clearance=gone; Max-Age=0; Path=/"),
                Duration::from_secs(3600),
            )
            .expect("deletion cookie parse succeeds");

        assert!(cache
            .cookie_header_for("https://example.com/")
            .expect("cookie header succeeds")
            .is_none());
    }

    /// Verifies domain, path, and secure cookie filtering.
    #[test]
    fn filters_by_domain_path_and_secure() {
        let mut cache = SharedCookieCache::default();
        cache.insert_cookie(CookieEntry {
            name: "a".to_string(),
            value: "b".to_string(),
            domain: "example.com".to_string(),
            path: "/private".to_string(),
            expires_at: Some(SystemTime::now() + Duration::from_secs(60)),
            secure: true,
            http_only: false,
            same_site: None,
        });

        assert!(cache
            .cookie_header_for("http://example.com/private")
            .expect("cookie header succeeds")
            .is_none());
        assert!(cache
            .cookie_header_for("https://example.com/public")
            .expect("cookie header succeeds")
            .is_none());
        assert_eq!(
            cache
                .cookie_header_for("https://sub.example.com/private/page")
                .expect("cookie header succeeds")
                .as_deref(),
            Some("a=b")
        );
    }
}
