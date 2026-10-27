use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use http::header::SET_COOKIE;

use crate::browser::{BrowserPageSession, PageNavigationResponse};
use crate::engine::{EngineResponse, SOLVER_USER_AGENT_HEADER};

use super::*;

/// Minimal injected engine used by client tests.
#[derive(Clone)]
pub(super) struct StaticEngine {
    /// Number of requests observed by the engine.
    pub(super) calls: Arc<AtomicUsize>,
}

/// Injected engine that distinguishes refresh calls from regular sends.
#[derive(Clone)]
pub(super) struct RefreshEngine {
    /// Number of regular sends observed by the engine.
    pub(super) send_calls: Arc<AtomicUsize>,
    /// Number of Cloudflare refresh calls observed by the engine.
    pub(super) refresh_calls: Arc<AtomicUsize>,
}

/// Browser-capable engine that exposes a deterministic in-page fetch session.
#[derive(Clone)]
pub(super) struct PageEngine {
    pub(super) opens: Arc<AtomicUsize>,
    pub(super) navigations: Arc<AtomicUsize>,
    pub(super) fetches: Arc<AtomicUsize>,
    pub(super) token_reads: Arc<AtomicUsize>,
    pub(super) bodies: Arc<Mutex<Vec<String>>>,
}

/// In-memory page used by `PageEngine`.
pub(super) struct PageEngineSession {
    pub(super) navigations: Arc<AtomicUsize>,
    pub(super) fetches: Arc<AtomicUsize>,
    pub(super) token_reads: Arc<AtomicUsize>,
    pub(super) bodies: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl HttpEngine for StaticEngine {
    /// Returns the test engine name.
    ///
    /// # Returns
    ///
    /// The static test engine name.
    fn name(&self) -> &'static str {
        "static-test"
    }

    /// Sends a deterministic in-memory response.
    ///
    /// # Parameters
    ///
    /// - `request`: Request passed by the client.
    ///
    /// # Returns
    ///
    /// A response containing the requested URL and a tiny body.
    ///
    /// # Errors
    ///
    /// This test engine never fails.
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(EngineResponse {
            url: request.url,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Bytes::from_static(b"injected"),
        })
    }
}

#[async_trait::async_trait]
impl HttpEngine for RefreshEngine {
    /// Returns the refresh test engine name.
    ///
    /// # Returns
    ///
    /// The static test engine name.
    fn name(&self) -> &'static str {
        "refresh-test"
    }

    /// Records an unexpected regular send.
    ///
    /// # Parameters
    ///
    /// - `request`: Request passed by the client.
    ///
    /// # Returns
    ///
    /// A tiny successful response.
    ///
    /// # Errors
    ///
    /// This test engine never fails.
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        self.send_calls.fetch_add(1, Ordering::SeqCst);
        Ok(EngineResponse {
            url: request.url,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        })
    }

    /// Records a Cloudflare refresh and returns cookie plus user-agent metadata.
    ///
    /// # Parameters
    ///
    /// - `request`: Refresh request passed by the client.
    ///
    /// # Returns
    ///
    /// A response containing a clearance cookie and solver user-agent.
    ///
    /// # Errors
    ///
    /// This test engine never fails.
    async fn refresh_cloudflare(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.refresh_calls.fetch_add(1, Ordering::SeqCst);
        let mut headers = HeaderMap::new();
        headers.insert(
            SET_COOKIE,
            "cf_clearance=ok; Max-Age=60; Path=/"
                .parse()
                .expect("valid Set-Cookie header"),
        );
        headers.insert(
            SOLVER_USER_AGENT_HEADER,
            "SolverUA/1".parse().expect("valid user-agent metadata"),
        );
        Ok(EngineResponse {
            url: request.url,
            status: StatusCode::OK,
            headers,
            body: Bytes::new(),
        })
    }
}

#[async_trait::async_trait]
impl HttpEngine for PageEngine {
    fn name(&self) -> &'static str {
        "page-test"
    }

    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        Ok(EngineResponse {
            url: request.url,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        })
    }

    async fn open_browser_page_session(
        &self,
    ) -> Result<Box<dyn BrowserPageSession>, ArachneaHttpError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(PageEngineSession {
            navigations: self.navigations.clone(),
            fetches: self.fetches.clone(),
            token_reads: self.token_reads.clone(),
            bodies: self.bodies.clone(),
        }))
    }
}

#[async_trait::async_trait]
impl BrowserPageSession for PageEngineSession {
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
        let body = request
            .body
            .map(|body| String::from_utf8_lossy(&body).into_owned())
            .unwrap_or_default();
        self.bodies.lock().expect("test bodies lock").push(body);
        Ok(PageFetchResponse {
            url: request.url,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: "<iframe src=\"https://embed.example\"></iframe>".to_string(),
        })
    }

    async fn metadata(&mut self) -> Result<BrowserSessionMetadata, ArachneaHttpError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            SET_COOKIE,
            "cf_clearance=browser; Max-Age=60; Path=/"
                .parse()
                .expect("valid Set-Cookie header"),
        );
        headers.insert(
            SOLVER_USER_AGENT_HEADER,
            "BrowserPageUA/1"
                .parse()
                .expect("valid user-agent metadata"),
        );
        Ok(BrowserSessionMetadata {
            url: "https://page-session.example/episode".to_string(),
            headers,
        })
    }

    async fn read_turnstile_token(&mut self) -> Result<Option<String>, ArachneaHttpError> {
        self.token_reads.fetch_add(1, Ordering::SeqCst);
        Ok(Some("browser-token".to_string()))
    }

    async fn close(self: Box<Self>) {}
}
