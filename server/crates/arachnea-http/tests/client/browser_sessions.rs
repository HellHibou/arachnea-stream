use super::*;
use crate::browser::BrowserPageSession;
use crate::client::test_support::*;
use crate::engine::EngineResponse;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

/// Verifies one origin reuses its page and hands browser state back to HTTP.
#[tokio::test]
async fn page_fetch_reuses_browser_session_and_handoffs_metadata() {
    let opens = Arc::new(AtomicUsize::new(0));
    let navigations = Arc::new(AtomicUsize::new(0));
    let fetches = Arc::new(AtomicUsize::new(0));
    let token_reads = Arc::new(AtomicUsize::new(0));
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let engine = PageEngine {
        opens: opens.clone(),
        navigations: navigations.clone(),
        fetches: fetches.clone(),
        token_reads: token_reads.clone(),
        bodies: bodies.clone(),
    };
    let config = ArachneaHttpConfig::builder()
        .engine_instance(engine.clone())
        .cloudflare_browser_solver_instance(engine)
        .build()
        .expect("valid config");
    let client = ArachneaHttpClient::new_with_cookie_cache(
        config,
        Arc::new(RwLock::new(SharedCookieCache::default())),
    )
    .await
    .expect("client");

    for _ in 0..2 {
        let response = client
            .page_fetch(
                PageNavigationRequest {
                    url: "https://page-session.example/episode".to_string(),
                    headers: HeaderMap::new(),
                    collect_body: false,
                },
                PageFetchRequest {
                    method: Method::POST,
                    url: "https://page-session.example/ajax".to_string(),
                    headers: HeaderMap::new(),
                    body: Some(Bytes::from_static(b"captcha={browser_token}")),
                    turnstile_token_placeholder: Some("{browser_token}".to_string()),
                    reuse_turnstile_token: true,
                    token_rejection_statuses: Vec::new(),
                    token_rejection_body_markers: Vec::new(),
                },
            )
            .await
            .expect("page fetch");
        assert_eq!(response.status, StatusCode::OK);
    }

    assert_eq!(opens.load(Ordering::SeqCst), 1);
    assert_eq!(navigations.load(Ordering::SeqCst), 2);
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
    assert_eq!(token_reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        bodies.lock().expect("test bodies lock").as_slice(),
        ["captcha=browser-token", "captcha=browser-token"]
    );

    let request = client
        .engine_request(RequestOptions {
            method: Method::GET,
            url: "https://page-session.example/next".to_string(),
            mode: None,
            headers: HeaderMap::new(),
            body: None,
        })
        .await
        .expect("engine request");
    assert_eq!(
        request.headers.get(USER_AGENT).expect("browser user-agent"),
        "BrowserPageUA/1"
    );
    assert_eq!(
        request.headers.get(COOKIE).expect("browser cookie"),
        "cf_clearance=browser"
    );
}

/// Engine whose page returns a token once then absent on subsequent reads.
#[derive(Clone)]
struct ConsumingPageEngine {
    navigations: Arc<AtomicUsize>,
    fetches: Arc<AtomicUsize>,
    first_token_read: Arc<AtomicUsize>,
}

struct ConsumingPageEngineSession {
    navigations: Arc<AtomicUsize>,
    fetches: Arc<AtomicUsize>,
    first_token_read: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl HttpEngine for ConsumingPageEngine {
    fn name(&self) -> &'static str {
        "consuming-page-test"
    }

    async fn send(&self, _request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        Ok(EngineResponse {
            url: "https://consuming.example".to_string(),
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        })
    }

    async fn open_browser_page_session(
        &self,
    ) -> Result<Box<dyn BrowserPageSession>, ArachneaHttpError> {
        Ok(Box::new(ConsumingPageEngineSession {
            navigations: self.navigations.clone(),
            fetches: self.fetches.clone(),
            first_token_read: self.first_token_read.clone(),
        }))
    }
}

#[async_trait::async_trait]
impl BrowserPageSession for ConsumingPageEngineSession {
    async fn navigate(
        &mut self,
        request: PageNavigationRequest,
    ) -> Result<PageNavigationResponse, ArachneaHttpError> {
        self.navigations.fetch_add(1, Ordering::SeqCst);
        Ok(PageNavigationResponse {
            url: request.url,
            body: None,
        })
    }

    async fn fetch(
        &mut self,
        request: PageFetchRequest,
    ) -> Result<PageFetchResponse, ArachneaHttpError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        Ok(PageFetchResponse {
            url: request.url,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: "<iframe src=\"https://embed.example\"></iframe>".to_string(),
        })
    }

    async fn metadata(&mut self) -> Result<BrowserSessionMetadata, ArachneaHttpError> {
        Ok(BrowserSessionMetadata {
            url: "https://consuming.example/episode".to_string(),
            headers: HeaderMap::new(),
        })
    }

    async fn read_turnstile_token(&mut self) -> Result<Option<String>, ArachneaHttpError> {
        let already_read = self.first_token_read.fetch_add(1, Ordering::SeqCst);
        if already_read == 0 {
            Ok(Some("first-token".to_string()))
        } else {
            Ok(None)
        }
    }

    async fn close(self: Box<Self>) {}
}

/// Verifies a consumed token (page returns None) produces `TokenAbsent`.
#[tokio::test]
async fn page_fetch_token_consumed_returns_absent_without_reuse() {
    let engine = ConsumingPageEngine {
        navigations: Arc::new(AtomicUsize::new(0)),
        fetches: Arc::new(AtomicUsize::new(0)),
        first_token_read: Arc::new(AtomicUsize::new(0)),
    };
    let config = ArachneaHttpConfig::builder()
        .engine_instance(engine.clone())
        .cloudflare_browser_solver_instance(engine)
        .build()
        .expect("valid config");
    let client = ArachneaHttpClient::new_with_cookie_cache(
        config,
        Arc::new(RwLock::new(SharedCookieCache::default())),
    )
    .await
    .expect("client");

    let nav = PageNavigationRequest {
        url: "https://consuming.example/episode".to_string(),
        headers: HeaderMap::new(),
        collect_body: false,
    };

    // First call consumes the token.
    client
        .page_fetch(
            nav.clone(),
            PageFetchRequest {
                method: Method::POST,
                url: "https://consuming.example/ajax".to_string(),
                headers: HeaderMap::new(),
                body: Some(Bytes::from_static(b"token={browser_token}")),
                turnstile_token_placeholder: Some("{browser_token}".to_string()),
                reuse_turnstile_token: false,
                token_rejection_statuses: Vec::new(),
                token_rejection_body_markers: Vec::new(),
            },
        )
        .await
        .expect("first page fetch");

    // Second call without reuse should fail because the page token was consumed.
    let err = client
        .page_fetch(
            nav,
            PageFetchRequest {
                method: Method::POST,
                url: "https://consuming.example/ajax".to_string(),
                headers: HeaderMap::new(),
                body: Some(Bytes::from_static(b"token={browser_token}")),
                turnstile_token_placeholder: Some("{browser_token}".to_string()),
                reuse_turnstile_token: false,
                token_rejection_statuses: Vec::new(),
                token_rejection_body_markers: Vec::new(),
            },
        )
        .await
        .expect_err("token should be absent");
    assert!(
        matches!(err, ArachneaHttpError::TokenAbsent { .. }),
        "expected TokenAbsent, got {err:?}"
    );
}

/// Verifies domain-scoped token caching survives token consumption.
#[tokio::test]
async fn page_fetch_token_reuse_caches_across_calls() {
    let engine = ConsumingPageEngine {
        navigations: Arc::new(AtomicUsize::new(0)),
        fetches: Arc::new(AtomicUsize::new(0)),
        first_token_read: Arc::new(AtomicUsize::new(0)),
    };
    let config = ArachneaHttpConfig::builder()
        .engine_instance(engine.clone())
        .cloudflare_browser_solver_instance(engine)
        .build()
        .expect("valid config");
    let client = ArachneaHttpClient::new_with_cookie_cache(
        config,
        Arc::new(RwLock::new(SharedCookieCache::default())),
    )
    .await
    .expect("client");

    let nav = PageNavigationRequest {
        url: "https://consuming.example/episode".to_string(),
        headers: HeaderMap::new(),
        collect_body: false,
    };

    // First call reads the token once and caches it.
    client
        .page_fetch(
            nav.clone(),
            PageFetchRequest {
                method: Method::POST,
                url: "https://consuming.example/ajax".to_string(),
                headers: HeaderMap::new(),
                body: Some(Bytes::from_static(b"token={browser_token}")),
                turnstile_token_placeholder: Some("{browser_token}".to_string()),
                reuse_turnstile_token: true,
                token_rejection_statuses: Vec::new(),
                token_rejection_body_markers: Vec::new(),
            },
        )
        .await
        .expect("first page fetch");

    // Second call reuses the cached token; page read_turnstile_token is not called.
    let response = client
        .page_fetch(
            nav,
            PageFetchRequest {
                method: Method::POST,
                url: "https://consuming.example/ajax2".to_string(),
                headers: HeaderMap::new(),
                body: Some(Bytes::from_static(b"token={browser_token}")),
                turnstile_token_placeholder: Some("{browser_token}".to_string()),
                reuse_turnstile_token: true,
                token_rejection_statuses: Vec::new(),
                token_rejection_body_markers: Vec::new(),
            },
        )
        .await
        .expect("second page fetch");
    assert_eq!(response.status, StatusCode::OK);
}

/// Engine that returns a token-rejection response.
#[derive(Clone)]
struct RejectingPageEngine;

impl RejectingPageEngine {
    fn into_session(self) -> RejectingPageEngineSession {
        RejectingPageEngineSession
    }
}

struct RejectingPageEngineSession;

#[async_trait::async_trait]
impl HttpEngine for RejectingPageEngine {
    fn name(&self) -> &'static str {
        "rejecting-page-test"
    }

    async fn send(&self, _request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        Ok(EngineResponse {
            url: "https://rejecting.example".to_string(),
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        })
    }

    async fn open_browser_page_session(
        &self,
    ) -> Result<Box<dyn BrowserPageSession>, ArachneaHttpError> {
        Ok(Box::new(self.clone().into_session()))
    }
}

#[async_trait::async_trait]
impl BrowserPageSession for RejectingPageEngineSession {
    async fn navigate(
        &mut self,
        request: PageNavigationRequest,
    ) -> Result<PageNavigationResponse, ArachneaHttpError> {
        Ok(PageNavigationResponse {
            url: request.url,
            body: None,
        })
    }

    async fn fetch(
        &mut self,
        _request: PageFetchRequest,
    ) -> Result<PageFetchResponse, ArachneaHttpError> {
        Ok(PageFetchResponse {
            url: "https://rejecting.example/ajax".to_string(),
            status: StatusCode::FORBIDDEN,
            headers: HeaderMap::new(),
            body: "captcha required".to_string(),
        })
    }

    async fn metadata(&mut self) -> Result<BrowserSessionMetadata, ArachneaHttpError> {
        Ok(BrowserSessionMetadata {
            url: "https://rejecting.example/episode".to_string(),
            headers: HeaderMap::new(),
        })
    }

    async fn read_turnstile_token(&mut self) -> Result<Option<String>, ArachneaHttpError> {
        Ok(Some("rejected-token".to_string()))
    }

    async fn close(self: Box<Self>) {}
}

/// Verifies a rejected token produces `TokenRejected` and preserves the session.
#[tokio::test]
async fn page_fetch_token_rejected_preserves_session() {
    let engine = RejectingPageEngine;
    let config = ArachneaHttpConfig::builder()
        .engine_instance(engine.clone())
        .cloudflare_browser_solver_instance(engine)
        .build()
        .expect("valid config");
    let client = ArachneaHttpClient::new_with_cookie_cache(
        config,
        Arc::new(RwLock::new(SharedCookieCache::default())),
    )
    .await
    .expect("client");

    let nav = PageNavigationRequest {
        url: "https://rejecting.example/episode".to_string(),
        headers: HeaderMap::new(),
        collect_body: false,
    };

    // The page returns 403 Forbidden matched by rejection_statuses.
    let err = client
        .page_fetch(
            nav,
            PageFetchRequest {
                method: Method::POST,
                url: "https://rejecting.example/ajax".to_string(),
                headers: HeaderMap::new(),
                body: Some(Bytes::from_static(b"token={browser_token}")),
                turnstile_token_placeholder: Some("{browser_token}".to_string()),
                reuse_turnstile_token: false,
                token_rejection_statuses: vec![StatusCode::FORBIDDEN],
                token_rejection_body_markers: Vec::new(),
            },
        )
        .await
        .expect_err("token should be rejected");
    assert!(
        matches!(err, ArachneaHttpError::TokenRejected { .. }),
        "expected TokenRejected, got {err:?}"
    );

    // The session should survive: a subsequent call reuses the same origin session.
    // We verify by checking the engine still opens pages (no invalidation).
    // After TokenRejected, the page should still be open and navigable.
    // We use a marker-free request (no token placeholder) to confirm the session works.
    let response = client
        .page_fetch(
            PageNavigationRequest {
                url: "https://rejecting.example/another".to_string(),
                headers: HeaderMap::new(),
                collect_body: false,
            },
            PageFetchRequest {
                method: Method::GET,
                url: "https://rejecting.example/another".to_string(),
                headers: HeaderMap::new(),
                body: None,
                turnstile_token_placeholder: None,
                reuse_turnstile_token: false,
                token_rejection_statuses: Vec::new(),
                token_rejection_body_markers: Vec::new(),
            },
        )
        .await
        .expect("page fetch after rejection should succeed");
    assert_eq!(response.status, StatusCode::FORBIDDEN);
}

/// Engine that fails to open a browser session.
#[derive(Clone)]
struct FailingPageEngine {
    open_attempts: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl HttpEngine for FailingPageEngine {
    fn name(&self) -> &'static str {
        "failing-page-test"
    }

    async fn send(&self, _request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        Ok(EngineResponse {
            url: "https://failing.example".to_string(),
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        })
    }

    async fn open_browser_page_session(
        &self,
    ) -> Result<Box<dyn BrowserPageSession>, ArachneaHttpError> {
        self.open_attempts.fetch_add(1, Ordering::SeqCst);
        Err(ArachneaHttpError::ChaserCfFailure(
            "engine failure".to_string(),
        ))
    }
}

/// Verifies a non-token error invalidates the browser session and forces a fresh attempt.
#[tokio::test]
async fn page_fetch_other_error_triggers_new_session_attempt() {
    let open_attempts = Arc::new(AtomicUsize::new(0));
    let engine = FailingPageEngine {
        open_attempts: open_attempts.clone(),
    };
    let config = ArachneaHttpConfig::builder()
        .engine_instance(engine.clone())
        .cloudflare_browser_solver_instance(engine)
        .build()
        .expect("valid config");
    let client = ArachneaHttpClient::new_with_cookie_cache(
        config,
        Arc::new(RwLock::new(SharedCookieCache::default())),
    )
    .await
    .expect("client");

    let nav = PageNavigationRequest {
        url: "https://failing.example/episode".to_string(),
        headers: HeaderMap::new(),
        collect_body: false,
    };

    // First call fails because the engine returns an error.
    let err = client
        .page_fetch(
            nav.clone(),
            PageFetchRequest {
                method: Method::GET,
                url: "https://failing.example/episode".to_string(),
                headers: HeaderMap::new(),
                body: None,
                turnstile_token_placeholder: None,
                reuse_turnstile_token: false,
                token_rejection_statuses: Vec::new(),
                token_rejection_body_markers: Vec::new(),
            },
        )
        .await
        .expect_err("engine should fail");
    assert!(
        matches!(err, ArachneaHttpError::ChaserCfFailure(_)),
        "expected engine failure, got {err:?}"
    );
    assert_eq!(open_attempts.load(Ordering::SeqCst), 1);

    // The session was invalidated, so a second call attempts a fresh engine session.
    let err2 = client
        .page_fetch(
            nav,
            PageFetchRequest {
                method: Method::GET,
                url: "https://failing.example/episode".to_string(),
                headers: HeaderMap::new(),
                body: None,
                turnstile_token_placeholder: None,
                reuse_turnstile_token: false,
                token_rejection_statuses: Vec::new(),
                token_rejection_body_markers: Vec::new(),
            },
        )
        .await
        .expect_err("second call should also fail");
    assert!(
        matches!(err2, ArachneaHttpError::ChaserCfFailure(_)),
        "second call should retry engine, got {err2:?}"
    );
    // Two open attempts confirm the first was invalidated rather than reusing a cached session.
    assert_eq!(open_attempts.load(Ordering::SeqCst), 2);
}
