//! Reusable browser session management and in-page HTTP fetch execution.
//!
//! This module defines the contracts for maintaining persistent browser page
//! contexts and executing JavaScript `fetch()`/XHR calls inside them. It is
//! designed to support flows like PapaDuStream's `getxfield` POST which
//! requires a Turnstile callback token obtained in the page context.
//!
//! # Security
//!
//! - Cookie values, Turnstile tokens, and sensitive request headers are never
//!   logged, stored persistently, or included in diagnostic output.
//! - The resolved-iframe cache stores only the final embed URL, not the
//!   Turnstile token, session cookies, or intermediate request data.
//! - Browser sessions are scoped to a single origin and are never shared
//!   across different origins or proxy routes.
//! - Token values are held in memory only. They are single-use by default and
//!   may be reused only when the caller explicitly scopes them to the retained
//!   origin/profile/proxy browser session.

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use http::HeaderMap;
use tokio::sync::Mutex as AsyncMutex;

/// Uniquely identifies a reusable browser session.
///
/// Sessions are scoped by **origin**, **browser profile**, and optional
/// **proxy route** so that Cloudflare state, cookies, and Turnstile tokens
/// are never accidentally shared across different target sites.
#[derive(Debug, Clone)]
pub struct BrowserSessionKey {
    /// Origin scheme + host (e.g. `https://example.com`).
    pub origin: String,
    /// Browser user-agent profile identifier used to create the session.
    pub profile: String,
    /// Proxy route discriminator when multiple proxy routes are configured.
    pub proxy_route: Option<String>,
}

impl Hash for BrowserSessionKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.origin.hash(state);
        self.profile.hash(state);
        self.proxy_route.hash(state);
    }
}

impl PartialEq for BrowserSessionKey {
    fn eq(&self, other: &Self) -> bool {
        self.origin == other.origin
            && self.profile == other.profile
            && self.proxy_route == other.proxy_route
    }
}

impl Eq for BrowserSessionKey {}

/// Reference-counted handle to a reusable browser page session.
///
/// Internally holds a lock-guarded inner state that tracks session validity,
/// last-used timestamp, the active page context, and the Turnstile token
/// lifecycle. Callers must lock the handle before performing page-scoped
/// operations.
///
/// The handle is clonable — all clones share the same underlying session.
#[derive(Clone)]
pub struct BrowserSessionHandle {
    state: Arc<BrowserSessionState>,
}

impl std::fmt::Debug for BrowserSessionHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BrowserSessionHandle")
            .field("valid", &self.is_valid())
            .finish_non_exhaustive()
    }
}

/// Lock-free and asynchronous state used by one browser page session.
struct BrowserSessionState {
    valid: AtomicBool,
    last_used: Mutex<Instant>,
    turnstile_token: Mutex<Option<String>>,
    inner: AsyncMutex<BrowserSessionInner>,
}

/// Mutable state of a browser page session.
pub(crate) struct BrowserSessionInner {
    /// Browser-specific page retained while the logical session remains valid.
    pub page: Option<Box<dyn BrowserPageSession>>,
}

impl BrowserSessionHandle {
    /// Creates a new fresh session handle.
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(BrowserSessionState {
                valid: AtomicBool::new(true),
                last_used: Mutex::new(Instant::now()),
                turnstile_token: Mutex::new(None),
                inner: AsyncMutex::new(BrowserSessionInner { page: None }),
            }),
        }
    }

    /// Returns whether the manager has kept this handle valid.
    pub fn is_valid(&self) -> bool {
        self.state.valid.load(Ordering::Acquire)
    }

    /// Marks the handle invalid and prevents further browser operations.
    pub(crate) fn invalidate(&self) {
        self.state.valid.store(false, Ordering::Release);
    }

    /// Updates the successful activity timestamp used by cache eviction.
    pub(crate) fn touch(&self) {
        if let Ok(mut last_used) = self.state.last_used.lock() {
            *last_used = Instant::now();
        }
    }

    /// Returns the cached opaque token without exposing it in diagnostics.
    pub(crate) fn cached_turnstile_token(&self) -> Option<String> {
        self.state
            .turnstile_token
            .lock()
            .ok()
            .and_then(|token| token.clone())
    }

    /// Replaces the token associated with this browser session.
    pub(crate) fn cache_turnstile_token(&self, token: String) {
        if let Ok(mut cached) = self.state.turnstile_token.lock() {
            *cached = Some(token);
        }
    }

    /// Removes the token associated with this browser session.
    pub(crate) fn clear_turnstile_token(&self) {
        if let Ok(mut cached) = self.state.turnstile_token.lock() {
            *cached = None;
        }
    }

    /// Returns whether the session can be evicted after the supplied timeout.
    pub(crate) fn is_idle(&self, timeout: Duration) -> bool {
        self.state
            .last_used
            .lock()
            .map(|last_used| last_used.elapsed() >= timeout)
            .unwrap_or(true)
    }

    /// Returns the session activity timestamp for deterministic capacity eviction.
    pub(crate) fn last_used(&self) -> Instant {
        self.state
            .last_used
            .lock()
            .map(|last_used| *last_used)
            .unwrap_or_else(|_| Instant::now())
    }

    /// Tests whether two handles reference one logical browser session.
    #[cfg(test)]
    fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    /// Serializes an operation that requires exclusive access to the browser page.
    pub(crate) async fn lock(&self) -> tokio::sync::MutexGuard<'_, BrowserSessionInner> {
        self.state.inner.lock().await
    }

    /// Closes the retained browser page, if any, and invalidates this handle.
    pub(crate) async fn close(&self) {
        self.invalidate();
        self.clear_turnstile_token();
        let page = self.lock().await.page.take();
        if let Some(page) = page {
            page.close().await;
        }
    }
}

/// A browser-specific persistent page context.
///
/// Implementations must retain the browser's page, cookies, and JavaScript
/// execution state between `navigate`, `click_and_wait`, and `fetch` calls. All methods are
/// invoked while the owning `BrowserSessionHandle` is exclusively locked.
#[async_trait]
pub trait BrowserPageSession: Send {
    /// Navigates the page to the source URL and waits for the configured state.
    async fn navigate(
        &mut self,
        request: PageNavigationRequest,
    ) -> Result<PageNavigationResponse, crate::ArachneaHttpError>;

    /// Executes one JavaScript `fetch()` request inside the current page.
    async fn fetch(
        &mut self,
        request: PageFetchRequest,
    ) -> Result<PageFetchResponse, crate::ArachneaHttpError>;

    /// Clicks an element in the current page and returns HTML after a selector appears.
    async fn click_and_wait(
        &mut self,
        _request: PageClickRequest,
    ) -> Result<PageClickResponse, crate::ArachneaHttpError> {
        Err(crate::ArachneaHttpError::UnsupportedEngineOperation {
            engine: "browser-page-session",
            operation: "click_and_wait",
        })
    }

    /// Returns browser cookies and the observed user-agent for HTTP handoff.
    async fn metadata(&mut self) -> Result<BrowserSessionMetadata, crate::ArachneaHttpError>;

    /// Reads one application Turnstile callback token from the current page.
    ///
    /// The client, rather than the browser engine, owns the token's cache and
    /// invalidation lifecycle.
    async fn read_turnstile_token(&mut self) -> Result<Option<String>, crate::ArachneaHttpError>;

    /// Clears the callback token from the current page after explicit rejection.
    async fn clear_turnstile_token(&mut self) -> Result<(), crate::ArachneaHttpError> {
        Ok(())
    }

    /// Closes the underlying browser page and releases its resources.
    async fn close(self: Box<Self>);
}

/// Parameters for navigating a reusable browser page.
#[derive(Debug, Clone)]
pub struct PageNavigationRequest {
    /// Absolute source page URL.
    pub url: String,
    /// Additional headers to apply to the page navigation.
    pub headers: HeaderMap,
    /// Whether the stable HTML source should be returned after navigation.
    pub collect_body: bool,
}

/// Result of navigating a reusable browser page.
#[derive(Debug, Clone)]
pub struct PageNavigationResponse {
    /// URL after browser navigation and redirects.
    pub url: String,
    /// Stable page HTML when `PageNavigationRequest::collect_body` is enabled.
    pub body: Option<String>,
}

/// Parameters for a browser-page click followed by a DOM wait.
#[derive(Debug, Clone)]
pub struct PageClickRequest {
    /// CSS selector identifying the element to click.
    pub selector: String,
    /// CSS selector that must appear after the click completes.
    pub wait_for_selector: String,
}

/// Stable HTML captured after a browser-page click completes.
#[derive(Debug, Clone)]
pub struct PageClickResponse {
    /// Current page HTML after the configured selector appears.
    pub body: String,
}

/// Cookies and browser fingerprint metadata that can be handed off to HTTP.
#[derive(Debug, Clone)]
pub struct BrowserSessionMetadata {
    /// Final page URL associated with the metadata.
    pub url: String,
    /// Synthesized `Set-Cookie` headers for the browser cookie store.
    pub headers: HeaderMap,
}

/// Configuration for the browser session manager.
#[derive(Debug, Clone, Copy)]
pub struct BrowserSessionConfig {
    /// Maximum number of concurrent browser sessions.
    ///
    /// When this limit is reached, the oldest idle session is evicted before
    /// creating a new one.
    pub max_sessions: usize,
    /// Duration after which an idle session is eligible for eviction.
    ///
    /// Idle eviction happens on `get_or_create` when the session is not found
    /// and the manager is at capacity.
    pub idle_timeout: Duration,
}

impl Default for BrowserSessionConfig {
    fn default() -> Self {
        Self {
            max_sessions: 8,
            idle_timeout: Duration::from_secs(300),
        }
    }
}

/// Manages the lifecycle of reusable browser page sessions.
///
/// The manager is thread-safe, reference-counted, and designed to be shared
/// across async tasks. Sessions are cached by `BrowserSessionKey` and
/// automatically evicted when idle or when the capacity limit is reached.
///
/// # Locking
///
/// - Internal session map is protected by `std::sync::RwLock`. Lock
///   acquisitions are short (HashMap lookups / inserts) and never held across
///   await points.
/// - Individual session handles use `std::sync::Mutex` for per-session
///   state. The caller must serialize page-scoped browser operations.
#[derive(Debug)]
pub struct BrowserSessionManager {
    sessions: RwLock<HashMap<BrowserSessionKey, BrowserSessionHandle>>,
    config: BrowserSessionConfig,
}

impl BrowserSessionManager {
    /// Creates a new session manager.
    ///
    /// # Arguments
    ///
    /// * `config` — Session lifecycle configuration.
    pub fn new(config: BrowserSessionConfig) -> Self {
        Self {
            sessions: RwLock::new(HashMap::with_capacity(config.max_sessions)),
            config,
        }
    }

    /// Returns or creates a browser session for the given key.
    ///
    /// When the manager is at capacity and the key is not already cached, the
    /// oldest idle session is evicted before creating the new entry.
    ///
    /// # Arguments
    ///
    /// * `key` — Session key scoping origin, profile, and proxy route.
    ///
    /// # Returns
    ///
    /// A handle to an existing or newly created session.
    pub fn get_or_create(&self, key: &BrowserSessionKey) -> BrowserSessionHandle {
        // Fast path: existing session.
        {
            let guard = self.sessions.read().expect("session map lock poisoned");
            if let Some(handle) = guard.get(key) {
                handle.touch();
                return handle.clone();
            }
        }

        // Slow path: create or evict.
        let mut guard = self.sessions.write().expect("session map lock poisoned");

        // Double-check after acquiring the write lock.
        if let Some(handle) = guard.get(key) {
            handle.touch();
            return handle.clone();
        }

        // Evict idle sessions when at capacity.
        let mut evicted = Vec::new();
        if guard.len() >= self.config.max_sessions {
            let idle_keys = guard
                .iter()
                .filter(|(_, handle)| {
                    !handle.is_valid() || handle.is_idle(self.config.idle_timeout)
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            for idle_key in idle_keys {
                if let Some(handle) = guard.remove(&idle_key) {
                    evicted.push(handle);
                }
            }
        }

        // If still at capacity after evicting idle sessions, remove the oldest.
        if guard.len() >= self.config.max_sessions {
            let oldest_key = guard
                .iter()
                .min_by_key(|(_, handle)| handle.last_used())
                .map(|(k, _)| k.clone());
            if let Some(oldest_key) = oldest_key {
                if let Some(handle) = guard.remove(&oldest_key) {
                    evicted.push(handle);
                }
            }
        }

        let handle = BrowserSessionHandle::new();
        guard.insert(key.clone(), handle.clone());
        drop(guard);
        Self::close_evicted(evicted);
        handle
    }

    /// Invalidates all sessions matching the given origin.
    ///
    /// Sessions that have already been evicted or dropped are silently ignored.
    /// Existing handles remain usable but will be invalidated on their next
    /// locked access.
    ///
    /// # Arguments
    ///
    /// * `origin` — Origin to invalidate (e.g. `https://example.com`).
    pub fn invalidate_origin(&self, origin: &str) {
        let mut guard = self.sessions.write().expect("session map lock poisoned");
        let keys = guard
            .keys()
            .filter(|key| key.origin == origin)
            .cloned()
            .collect::<Vec<_>>();
        let mut evicted = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(handle) = guard.remove(&key) {
                handle.invalidate();
                evicted.push(handle);
            }
        }
        drop(guard);
        Self::close_evicted(evicted);
    }

    /// Invalidates and removes all sessions.
    ///
    /// Existing handles remain usable but report `valid: false` on their next
    /// locked access.
    pub fn invalidate_all(&self) {
        let mut guard = self.sessions.write().expect("session map lock poisoned");
        let evicted = guard
            .drain()
            .map(|(_, handle)| {
                handle.invalidate();
                handle
            })
            .collect::<Vec<_>>();
        drop(guard);
        Self::close_evicted(evicted);
    }

    /// Evicts sessions that have been idle beyond the configured timeout.
    ///
    /// Returns the number of evicted sessions.
    pub fn evict_idle(&self) -> usize {
        let mut guard = self.sessions.write().expect("session map lock poisoned");
        let keys = guard
            .iter()
            .filter(|(_, handle)| !handle.is_valid() || handle.is_idle(self.config.idle_timeout))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let mut evicted = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(handle) = guard.remove(&key) {
                handle.invalidate();
                evicted.push(handle);
            }
        }
        let count = evicted.len();
        drop(guard);
        Self::close_evicted(evicted);
        count
    }

    /// Number of sessions currently tracked by the manager.
    pub fn len(&self) -> usize {
        self.sessions.read().map(|g| g.len()).unwrap_or(0)
    }

    /// Whether the manager is currently tracking any sessions.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the manager's configuration.
    pub fn config(&self) -> &BrowserSessionConfig {
        &self.config
    }

    /// Closes removed browser pages without blocking the manager's map lock.
    fn close_evicted(handles: Vec<BrowserSessionHandle>) {
        if handles.is_empty() {
            return;
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                for handle in handles {
                    handle.close().await;
                }
            });
        }
    }
}

/// Parameters for executing an in-page HTTP fetch.
///
/// Mirrors `EngineRequest` but is tailored for execution inside a browser
/// page context where the page's own cookies, authentication state, and
/// Turnstile tokens are available.
#[derive(Debug, Clone)]
pub struct PageFetchRequest {
    /// HTTP method for the in-page fetch call.
    pub method: http::Method,
    /// Target URL for the in-page fetch call.
    pub url: String,
    /// Request headers (page-origin headers are set by the browser engine).
    pub headers: http::HeaderMap,
    /// Optional request body for POST/PUT requests.
    pub body: Option<bytes::Bytes>,
    /// Optional literal placeholder in a UTF-8 request body replaced with one
    /// callback token from the current browser page immediately before fetch.
    ///
    /// The replacement is intentionally performed only in memory. Callers are
    /// responsible for selecting a body template whose encoding is accepted by
    /// the target endpoint.
    pub turnstile_token_placeholder: Option<String>,
    /// Reuses the session-scoped token for this origin/profile/proxy context.
    ///
    /// When false, a fresh callback token is read for each request and is not
    /// retained after submission.
    pub reuse_turnstile_token: bool,
    /// HTTP statuses which the caller's source contract classifies as a token
    /// rejection when a callback token was submitted.
    pub token_rejection_statuses: Vec<http::StatusCode>,
    /// Response-body fragments which classify a submitted callback token as
    /// rejected. Matching is case-sensitive to avoid broad false positives.
    pub token_rejection_body_markers: Vec<String>,
}

impl PageFetchRequest {
    /// Replaces the configured token placeholder in the UTF-8 body.
    ///
    /// The token is never written to logs or errors. Session-scoped retention,
    /// when explicitly enabled, is managed by the owning HTTP client.
    pub(crate) fn inject_turnstile_token(
        mut self,
        token: &str,
    ) -> Result<Self, crate::ArachneaHttpError> {
        let Some(placeholder) = self.turnstile_token_placeholder.take() else {
            return Ok(self);
        };
        let body = self.body.take().ok_or_else(|| {
            crate::ArachneaHttpError::InvalidConfiguration(
                "a Turnstile token placeholder requires a request body".to_string(),
            )
        })?;
        let body = String::from_utf8(body.to_vec()).map_err(|_| {
            crate::ArachneaHttpError::InvalidConfiguration(
                "a Turnstile token placeholder requires a UTF-8 request body".to_string(),
            )
        })?;
        if !body.contains(&placeholder) {
            return Err(crate::ArachneaHttpError::InvalidConfiguration(
                "the configured Turnstile token placeholder is absent from the request body"
                    .to_string(),
            ));
        }
        self.body = Some(bytes::Bytes::from(body.replace(&placeholder, token)));
        Ok(self)
    }

    /// Returns whether the supplied response rejects the callback token.
    pub(crate) fn is_token_rejected_by(&self, response: &PageFetchResponse) -> bool {
        self.token_rejection_statuses.contains(&response.status)
            || self
                .token_rejection_body_markers
                .iter()
                .any(|marker| !marker.is_empty() && response.body.contains(marker))
    }
}

/// Response from an in-page HTTP fetch execution.
///
/// The body is already decoded as text because the scraper pipeline
/// downstream expects string content for HTML parsing.
#[derive(Debug, Clone)]
pub struct PageFetchResponse {
    /// Final URL after any redirects within the page context.
    pub url: String,
    /// HTTP status code returned by the in-page request.
    pub status: http::StatusCode,
    /// Response headers from the in-page request.
    pub headers: http::HeaderMap,
    /// Response body decoded as UTF-8 text.
    pub body: String,
}

/// Lifecycle state of a Turnstile callback token captured from the page.
pub enum TurnstileTokenState {
    /// No token has been requested or captured yet.
    Pending,
    /// A valid callback token is available and has not been used.
    Ready {
        /// The opaque token value (never logged or persisted).
        token: String,
    },
    /// The token has been submitted in one fetch and is consumed.
    Consumed,
    /// The token was rejected by the server; a new one is needed.
    Rejected,
}

impl std::fmt::Debug for TurnstileTokenState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => formatter.write_str("Pending"),
            Self::Ready { .. } => formatter.write_str("Ready(<redacted>)"),
            Self::Consumed => formatter.write_str("Consumed"),
            Self::Rejected => formatter.write_str("Rejected"),
        }
    }
}

/// Bounded in-memory cache for resolved iframe embed URLs.
///
/// Avoids repeating server round-trips (e.g. `getxfield` POSTs) for the same
/// player and episode. Entries are evicted by TTL or when the cache exceeds
/// its configured capacity. No Turnstile tokens, cookies, or intermediate
/// session data are stored in this cache.
#[derive(Debug)]
pub struct ResolvedIframeCache {
    entries: HashMap<IframeCacheKey, IframeCacheEntry>,
    capacity: usize,
    ttl: Duration,
}

/// Internal cache key for a resolved iframe.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct IframeCacheKey {
    episode_url: String,
    player_id: String,
    player_field: String,
    player_type: String,
}

/// Internal cache entry with creation timestamp.
#[derive(Debug, Clone)]
struct IframeCacheEntry {
    embed_url: String,
    created_at: Instant,
}

impl ResolvedIframeCache {
    /// Creates a new bounded iframe embed cache.
    ///
    /// # Arguments
    ///
    /// * `capacity` — Maximum number of entries before the oldest are evicted.
    /// * `ttl` — Entry time-to-live since creation.
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            capacity,
            ttl,
        }
    }

    /// Returns a cached embed URL if a fresh entry exists.
    ///
    /// Returns `None` when the entry is absent, expired, or evicted.
    pub fn get(
        &self,
        episode_url: &str,
        player_id: &str,
        player_field: &str,
        player_type: &str,
    ) -> Option<&str> {
        let key = IframeCacheKey {
            episode_url: episode_url.to_owned(),
            player_id: player_id.to_owned(),
            player_field: player_field.to_owned(),
            player_type: player_type.to_owned(),
        };
        let entry = self.entries.get(&key)?;
        if entry.created_at.elapsed() > self.ttl {
            return None;
        }
        Some(&entry.embed_url)
    }

    /// Inserts or replaces a resolved embed URL.
    ///
    /// When the cache is at capacity the oldest entry (by creation time) is
    /// evicted before insertion.
    pub fn insert(
        &mut self,
        episode_url: &str,
        player_id: &str,
        player_field: &str,
        player_type: &str,
        embed_url: String,
    ) {
        if self.entries.len() >= self.capacity {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.created_at)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest_key);
            }
        }
        let key = IframeCacheKey {
            episode_url: episode_url.to_owned(),
            player_id: player_id.to_owned(),
            player_field: player_field.to_owned(),
            player_type: player_type.to_owned(),
        };
        self.entries.insert(
            key,
            IframeCacheEntry {
                embed_url,
                created_at: Instant::now(),
            },
        );
    }

    /// Invalidates one specific cache entry by its composite key.
    pub fn invalidate(
        &mut self,
        episode_url: &str,
        player_id: &str,
        player_field: &str,
        player_type: &str,
    ) {
        let key = IframeCacheKey {
            episode_url: episode_url.to_owned(),
            player_id: player_id.to_owned(),
            player_field: player_field.to_owned(),
            player_type: player_type.to_owned(),
        };
        self.entries.remove(&key);
    }

    /// Removes all cached entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // --- BrowserSessionKey tests ---

    #[test]
    fn test_browser_session_key_equality() {
        let a = BrowserSessionKey {
            origin: "https://example.com".into(),
            profile: "chrome".into(),
            proxy_route: None,
        };
        let b = BrowserSessionKey {
            origin: "https://example.com".into(),
            profile: "chrome".into(),
            proxy_route: None,
        };
        let c = BrowserSessionKey {
            origin: "https://other.com".into(),
            profile: "chrome".into(),
            proxy_route: None,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_browser_session_key_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(BrowserSessionKey {
            origin: "https://a.com".into(),
            profile: "firefox".into(),
            proxy_route: None,
        });
        set.insert(BrowserSessionKey {
            origin: "https://a.com".into(),
            profile: "firefox".into(),
            proxy_route: None,
        });
        assert_eq!(set.len(), 1);
    }

    // --- BrowserSessionManager tests ---

    #[test]
    fn test_manager_get_or_create_new() {
        let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
        let key = BrowserSessionKey {
            origin: "https://example.com".into(),
            profile: "chrome".into(),
            proxy_route: None,
        };
        let h1 = mgr.get_or_create(&key);
        let h2 = mgr.get_or_create(&key);
        assert!(h1.ptr_eq(&h2));
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn test_manager_different_keys_different_sessions() {
        let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
        let k1 = BrowserSessionKey {
            origin: "https://a.com".into(),
            profile: "chrome".into(),
            proxy_route: None,
        };
        let k2 = BrowserSessionKey {
            origin: "https://b.com".into(),
            profile: "chrome".into(),
            proxy_route: None,
        };
        let h1 = mgr.get_or_create(&k1);
        let h2 = mgr.get_or_create(&k2);
        assert!(!h1.ptr_eq(&h2));
        assert_eq!(mgr.len(), 2);
    }

    #[test]
    fn test_manager_capacity_eviction() {
        let cfg = BrowserSessionConfig {
            max_sessions: 2,
            idle_timeout: Duration::from_secs(300),
        };
        let mgr = BrowserSessionManager::new(cfg);
        let k1 = BrowserSessionKey {
            origin: "https://a.com".into(),
            profile: "p".into(),
            proxy_route: None,
        };
        let k2 = BrowserSessionKey {
            origin: "https://b.com".into(),
            profile: "p".into(),
            proxy_route: None,
        };
        let k3 = BrowserSessionKey {
            origin: "https://c.com".into(),
            profile: "p".into(),
            proxy_route: None,
        };

        mgr.get_or_create(&k1);
        mgr.get_or_create(&k2);
        assert_eq!(mgr.len(), 2);

        // k3 should evict k1 (oldest).
        mgr.get_or_create(&k3);
        assert_eq!(mgr.len(), 2);

        // k1 should get a new handle (it was evicted).
        let h1_new = mgr.get_or_create(&k1);
        assert!(h1_new.is_valid());
    }

    #[test]
    fn test_manager_invalidate_origin() {
        let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
        let k1 = BrowserSessionKey {
            origin: "https://a.com".into(),
            profile: "p".into(),
            proxy_route: None,
        };
        let k2 = BrowserSessionKey {
            origin: "https://a.com".into(),
            profile: "q".into(),
            proxy_route: None,
        };
        let k3 = BrowserSessionKey {
            origin: "https://b.com".into(),
            profile: "p".into(),
            proxy_route: None,
        };

        mgr.get_or_create(&k1);
        mgr.get_or_create(&k2);
        mgr.get_or_create(&k3);
        assert_eq!(mgr.len(), 3);

        mgr.invalidate_origin("https://a.com");
        assert_eq!(mgr.len(), 1); // only k3 remains
    }

    #[test]
    fn test_manager_invalidate_all() {
        let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
        mgr.get_or_create(&BrowserSessionKey {
            origin: "https://a.com".into(),
            profile: "p".into(),
            proxy_route: None,
        });
        mgr.get_or_create(&BrowserSessionKey {
            origin: "https://b.com".into(),
            profile: "p".into(),
            proxy_route: None,
        });
        assert_eq!(mgr.len(), 2);

        mgr.invalidate_all();
        assert_eq!(mgr.len(), 0);
    }

    #[test]
    fn test_manager_evict_idle() {
        let cfg = BrowserSessionConfig {
            max_sessions: 8,
            idle_timeout: Duration::from_nanos(1),
        };
        let mgr = BrowserSessionManager::new(cfg);
        mgr.get_or_create(&BrowserSessionKey {
            origin: "https://a.com".into(),
            profile: "p".into(),
            proxy_route: None,
        });
        mgr.get_or_create(&BrowserSessionKey {
            origin: "https://b.com".into(),
            profile: "p".into(),
            proxy_route: None,
        });
        // Let the idle timeouts elapse.
        std::thread::sleep(Duration::from_nanos(100));
        let evicted = mgr.evict_idle();
        assert_eq!(evicted, 2);
        assert_eq!(mgr.len(), 0);
    }

    #[test]
    fn test_manager_reuses_existing_after_get_or_create() {
        let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
        let key = BrowserSessionKey {
            origin: "https://example.com".into(),
            profile: "chrome".into(),
            proxy_route: None,
        };
        let h1 = mgr.get_or_create(&key);
        let h2 = mgr.get_or_create(&key);
        assert!(h1.ptr_eq(&h2));
    }

    // --- ResolvedIframeCache tests ---

    #[test]
    fn test_resolved_iframe_cache_get_miss() {
        let cache = ResolvedIframeCache::new(10, Duration::from_secs(60));
        assert_eq!(cache.get("https://episode", "p1", "f1", "t1"), None);
    }

    #[test]
    fn test_resolved_iframe_cache_insert_and_get() {
        let mut cache = ResolvedIframeCache::new(10, Duration::from_secs(60));
        cache.insert("https://episode", "p1", "f1", "t1", "https://embed".into());
        assert_eq!(
            cache.get("https://episode", "p1", "f1", "t1"),
            Some("https://embed")
        );
    }

    #[test]
    fn test_resolved_iframe_cache_eviction() {
        let mut cache = ResolvedIframeCache::new(2, Duration::from_secs(60));
        cache.insert("ep1", "p1", "f1", "t1", "url1".into());
        cache.insert("ep2", "p2", "f2", "t2", "url2".into());
        cache.insert("ep3", "p3", "f3", "t3", "url3".into());
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get("ep1", "p1", "f1", "t1"), None);
    }

    #[test]
    fn test_resolved_iframe_cache_ttl() {
        let mut cache = ResolvedIframeCache::new(10, Duration::from_nanos(1));
        cache.insert("ep", "p", "f", "t", "url".into());
        std::thread::sleep(Duration::from_nanos(100));
        assert_eq!(cache.get("ep", "p", "f", "t"), None);
    }

    #[test]
    fn test_resolved_iframe_cache_invalidate() {
        let mut cache = ResolvedIframeCache::new(10, Duration::from_secs(60));
        cache.insert("ep", "p", "f", "t", "url".into());
        cache.invalidate("ep", "p", "f", "t");
        assert_eq!(cache.get("ep", "p", "f", "t"), None);
    }

    #[test]
    fn test_resolved_iframe_cache_clear() {
        let mut cache = ResolvedIframeCache::new(10, Duration::from_secs(60));
        cache.insert("ep1", "p1", "f1", "t1", "url1".into());
        cache.insert("ep2", "p2", "f2", "t2", "url2".into());
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_turnstile_token_state_transition() {
        let mut state = TurnstileTokenState::Pending;
        assert!(matches!(state, TurnstileTokenState::Pending));

        state = TurnstileTokenState::Ready {
            token: "dummy-token".into(),
        };
        assert!(matches!(state, TurnstileTokenState::Ready { .. }));

        state = TurnstileTokenState::Consumed;
        assert!(matches!(state, TurnstileTokenState::Consumed));

        state = TurnstileTokenState::Rejected;
        assert!(matches!(state, TurnstileTokenState::Rejected));
    }

    #[test]
    fn test_turnstile_token_debug_redacts_value() {
        let state = TurnstileTokenState::Ready {
            token: "sensitive-token".to_string(),
        };
        assert_eq!(format!("{state:?}"), "Ready(<redacted>)");
    }

    #[test]
    fn test_page_fetch_classifies_configured_token_rejection() {
        let request = PageFetchRequest {
            method: http::Method::POST,
            url: "https://example.com/ajax".to_string(),
            headers: HeaderMap::new(),
            body: None,
            turnstile_token_placeholder: Some("{token}".to_string()),
            reuse_turnstile_token: true,
            token_rejection_statuses: vec![http::StatusCode::FORBIDDEN],
            token_rejection_body_markers: vec!["captcha invalid".to_string()],
        };
        assert!(request.is_token_rejected_by(&PageFetchResponse {
            url: "https://example.com/ajax".to_string(),
            status: http::StatusCode::OK,
            headers: HeaderMap::new(),
            body: "captcha invalid".to_string(),
        }));
        assert!(!request.is_token_rejected_by(&PageFetchResponse {
            url: "https://example.com/ajax".to_string(),
            status: http::StatusCode::OK,
            headers: HeaderMap::new(),
            body: "accepted".to_string(),
        }));
    }

    // --- BrowserSessionHandle token cache lifecycle tests ---

    #[test]
    fn test_handle_token_cache_starts_empty() {
        let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
        let key = BrowserSessionKey {
            origin: "https://token-cache.example".into(),
            profile: "p".into(),
            proxy_route: None,
        };
        let handle = mgr.get_or_create(&key);
        assert_eq!(handle.cached_turnstile_token(), None);
    }

    #[test]
    fn test_handle_token_cache_roundtrip() {
        let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
        let key = BrowserSessionKey {
            origin: "https://token-cache.example".into(),
            profile: "p".into(),
            proxy_route: None,
        };
        let handle = mgr.get_or_create(&key);
        handle.cache_turnstile_token("test-token".to_string());
        assert_eq!(handle.cached_turnstile_token(), Some("test-token".to_string()));
    }

    #[test]
    fn test_handle_token_cache_clear() {
        let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
        let key = BrowserSessionKey {
            origin: "https://token-cache.example".into(),
            profile: "p".into(),
            proxy_route: None,
        };
        let handle = mgr.get_or_create(&key);
        handle.cache_turnstile_token("test-token".to_string());
        handle.clear_turnstile_token();
        assert_eq!(handle.cached_turnstile_token(), None);
    }

    #[test]
    fn test_handle_token_cache_replace() {
        let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
        let key = BrowserSessionKey {
            origin: "https://token-cache.example".into(),
            profile: "p".into(),
            proxy_route: None,
        };
        let handle = mgr.get_or_create(&key);
        handle.cache_turnstile_token("first".to_string());
        handle.cache_turnstile_token("second".to_string());
        assert_eq!(handle.cached_turnstile_token(), Some("second".to_string()));
    }

    #[test]
    fn test_handle_token_cache_does_not_share_between_handles() {
        let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
        let k1 = BrowserSessionKey {
            origin: "https://a.example".into(),
            profile: "p".into(),
            proxy_route: None,
        };
        let k2 = BrowserSessionKey {
            origin: "https://b.example".into(),
            profile: "p".into(),
            proxy_route: None,
        };
        let h1 = mgr.get_or_create(&k1);
        let h2 = mgr.get_or_create(&k2);
        h1.cache_turnstile_token("token-a".to_string());
        assert_eq!(h2.cached_turnstile_token(), None);
    }
}
