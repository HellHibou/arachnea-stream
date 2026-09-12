//! Obscura embedded headless-browser engine.
//!
//! Phase 2 of `docs/dev-tracking/obscura-embedded-engine-implementation-plan.md`:
//! `send`, `refresh_cloudflare` and `refresh_cloudflare_fresh` are implemented
//! on an ephemeral stealth page with a bounded Cloudflare clearance protocol.
//! Phase 3 adds persistent page sessions: `open_browser_page_session` returns
//! an `ObscuraPageSession` that retains one embedded browser page on a
//! dedicated thread (the Obscura runtime is `!Send`) behind a command channel,
//! with `navigate`, `fetch` (in-page `window.fetch`), `click_and_wait`,
//! Turnstile token read/reset and cookie/user-agent handoff.
//! The `Auto` browser-solver selection is intentionally unchanged; Obscura
//! must be selected explicitly through `CloudflareBrowserSolverKind::Obscura`.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use arachnea_core::persistence::TypedEntityStore;
use async_trait::async_trait;
use bytes::Bytes;
use http::{
    header::{CONTENT_TYPE, SET_COOKIE},
    HeaderMap, HeaderName, HeaderValue, Method, StatusCode,
};
use obscura::{Browser, Cookie as ObscuraCookie, Page};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};
use url::Url;

use crate::{
    browser::{
        BrowserPageSession, BrowserSessionMetadata, PageClickRequest, PageClickResponse,
        PageFetchRequest, PageFetchResponse, PageNavigationRequest, PageNavigationResponse,
    },
    chaser_session::{
        cache_origin_key, cached_session_headers, clearance_expires_at, memory_session_store,
        set_cookie_header, unix_timestamp, CachedChaserSession, StructuredCookie,
    },
    config::{ArachneaHttpConfig, HttpProxyConfig},
    engine::{EngineRequest, EngineResponse, HttpEngine, SOLVER_USER_AGENT_HEADER},
    error::ArachneaHttpError,
};

/// Registry name for the Obscura embedded engine.
pub const ENGINE_NAME: &str = "obscura";

/// Obscura Git revision pinned in `Cargo.toml`.
///
/// Kept manually in sync with the `rev` entry of the dependency so runtime
/// logs can report the exact embedded browser revision. The pin is re-checked
/// after the Phase 2 PoC, as required by the integration plan.
pub const OBSCURA_PINNED_REVISION: &str = "eec047a188cc75b7a1a257397ad84493ee59c091";

/// Dedicated timeout for the Obscura Cloudflare clearance protocol, kept
/// independent from the HTTP request timeout because an embedded browser solve
/// can far outlast a regular request. Starting bound copied from the chaser-cf
/// solve timeout until Obscura measurements (Phase 5) provide better values.
const OBSCURA_CLEARANCE_TIMEOUT: Duration = Duration::from_secs(180);

/// Passive wait after navigation before the first clearance poll. Cloudflare
/// invisible challenges start their proof-of-work after the load event; this
/// lets that JavaScript run without aggressive polling.
const OBSCURA_CLEARANCE_PASSIVE_WAIT_MS: u64 = 2_000;

/// JavaScript event-loop budget driven per clearance poll. The embedded
/// browser only advances timers, fetches and microtasks while the owner drives
/// the loop, so the wait alternates `settle` slices with short sleeps.
const OBSCURA_CLEARANCE_POLL_MS: u64 = 1_000;

/// Minimum wall-clock interval between bounded Turnstile widget click
/// attempts during the clearance protocol.
const OBSCURA_CLEARANCE_ATTEMPT_INTERVAL_MS: u64 = 2_000;

/// Short stabilisation window after `cf_clearance` appears so the challenge
/// transition commits before cookies and DOM are read.
const OBSCURA_CLEARANCE_SETTLE_MS: u64 = 500;

/// How long a retained interactive page waits for the target selector after a
/// click before giving up. Starting bound copied from the chaser-cf adapter
/// until Obscura measurements (Phase 5) provide better values.
const OBSCURA_CLICK_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the automatically solved Cloudflare challenge may take after a
/// page action click before `click_and_wait` gives up.
const OBSCURA_CLICK_SOLVE_TIMEOUT: Duration = Duration::from_secs(90);

/// How long `read_turnstile_token` polls for the token produced by the solved
/// challenge before returning `None`.
const OBSCURA_TURNSTILE_WAIT_TIMEOUT: Duration = Duration::from_secs(300);

/// Event-loop slice driven per in-page `fetch` turn. The embedded runtime only
/// resolves the in-page promise while its owner drives the loop, so the fetch
/// alternates `settle` slices with short sleeps until the result global is set.
const OBSCURA_FETCH_SETTLE_MS: u64 = 250;

/// Total bound for one in-page `fetch` turn before `PageFetchFailed`.
const OBSCURA_FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// Post-navigation event-loop slice before the session HTML is observed.
/// `goto` already waited for the load event; this lets framework bootstrap
/// scripts and async DOM mutations run first.
const OBSCURA_NAVIGATE_SETTLE_MS: u64 = 2_000;

/// Token-probe JavaScript for the retained session page, following the three
/// sources declared by the integration plan: `window.turnstile.getResponse()`,
/// `input[name="cf-response"]` and `input[name="cf-turnstile-response"]`.
/// The script returns the token string or `null`; the value only crosses the
/// JS/Rust boundary and is never logged.
const TURNSTILE_TOKEN_JS: &str = r#"(function () {
    if (window.turnstile && typeof window.turnstile.getResponse === 'function') {
        var token = window.turnstile.getResponse();
        if (token && token.length > 10) return token;
    }
    var field = document.querySelector('[name="cf-response"]');
    if (field && field.value && field.value.length > 10) return field.value;
    field = document.querySelector('[name="cf-turnstile-response"]');
    if (field && field.value && field.value.length > 10) return field.value;
    return null;
})()"#;

/// Turnstile reset JavaScript for the retained session page: resets the widget
/// when the API is present and empties the response fields. Returns `true`, so
/// a `null` result means the evaluation itself failed.
const TURNSTILE_CLEAR_JS: &str = r#"(function () {
    if (window.turnstile && typeof window.turnstile.reset === 'function') {
        try { window.turnstile.reset(); } catch (error) {}
    }
    var fields = document.querySelectorAll('[name="cf-response"], [name="cf-turnstile-response"]');
    fields.forEach(function (field) { field.value = ''; });
    return true;
})()"#;

/// Serializable outcome of one in-page `fetch` turn, produced by
/// [`in_page_fetch_script`] as a JSON string on the session page.
#[derive(Debug, Deserialize, Default)]
struct InPageFetchOutcome {
    /// Final URL after redirects inside the page context.
    #[serde(default)]
    url: Option<String>,
    /// HTTP status returned by the in-page request.
    #[serde(default)]
    status: Option<u16>,
    /// Response headers observed by the in-page request.
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    /// Response body decoded as UTF-8 text by the page.
    #[serde(default)]
    body: Option<String>,
    /// Network-layer error message when the in-page fetch rejected the call.
    #[serde(default)]
    error: Option<String>,
}

/// Builds the in-page fetch launcher script for one request payload.
///
/// The embedded runtime resolves promises only while the owner drives the
/// event loop, so the script stores the settled result in a page global as a
/// JSON string and flags completion; the session worker pumps the loop with
/// `settle` slices until the flag flips.
fn in_page_fetch_script(payload: &Value) -> String {
    format!(
        r#"(function () {{
            const input = {payload};
            globalThis.__arachneaObscuraFetchDone = false;
            globalThis.__arachneaObscuraFetchResult = null;
            fetch(input.url, {{
                method: input.method,
                headers: input.headers,
                body: input.body,
                credentials: 'same-origin',
            }})
                .then(async (response) => {{
                    const headers = {{}};
                    response.headers.forEach((value, name) => {{ headers[name] = value; }});
                    globalThis.__arachneaObscuraFetchResult = JSON.stringify({{
                        url: response.url,
                        status: response.status,
                        headers: headers,
                        body: await response.text(),
                    }});
                }})
                .catch((error) => {{
                    globalThis.__arachneaObscuraFetchResult = JSON.stringify({{
                        error: String((error && error.message) || error),
                    }});
                }})
                .then(() => {{ globalThis.__arachneaObscuraFetchDone = true; }});
            return true;
        }})()"#
    )
}

/// Converts one in-page fetch outcome into the client-facing response shape.
fn page_fetch_response_from_outcome(
    outcome: InPageFetchOutcome,
    fallback_url: &str,
) -> Result<PageFetchResponse, ArachneaHttpError> {
    if let Some(error) = outcome.error {
        return Err(ArachneaHttpError::PageFetchFailed(format!(
            "the in-page fetch failed: {error}"
        )));
    }
    let status = outcome.status.ok_or_else(|| {
        ArachneaHttpError::PageFetchFailed("the in-page fetch produced no HTTP status".to_string())
    })?;
    let mut headers = HeaderMap::new();
    for (name, value) in outcome.headers.unwrap_or_default() {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?;
        let value = HeaderValue::from_str(&value)
            .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?;
        headers.append(name, value);
    }
    Ok(PageFetchResponse {
        url: outcome.url.unwrap_or_else(|| fallback_url.to_string()),
        status: StatusCode::from_u16(status).map_err(|err| {
            ArachneaHttpError::PageFetchFailed(format!("invalid in-page fetch status: {err}"))
        })?,
        headers,
        body: outcome.body.unwrap_or_default(),
    })
}

/// Marker-lookup JavaScript run in the retained document. Only booleans cross
/// the JS/Rust boundary: document values never leave the page, so the signal
/// summary can never leak cookie values or signed URLs.
const CHALLENGE_SIGNAL_JS: &str = r#"(function () {
    const has = (selector) => !!document.querySelector(selector);
    return {
        cfChallengeFrame: Array.from(document.querySelectorAll('iframe'))
            .some((frame) => (frame.src || frame.getAttribute('src') || '')
                .includes('challenges.cloudflare.com')),
        turnstileResponse: has('input[name="cf-turnstile-response"]'),
        cfResponse: has('input[name="cf-response"]'),
        interstitialMarker: has('#challenge-form, #cf-please-wait, .cf-browser-verification'),
        challengeTitle: /just a moment|attention required|checking your browser|verify you are human/i
            .test(document.title || ''),
    };
})()"#;

/// Non-secret snapshot of generic Cloudflare challenge markers sampled from
/// the retained document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChallengeSignals {
    /// An iframe pointing at `challenges.cloudflare.com` is present.
    has_cf_challenge_frame: bool,
    /// `input[name="cf-turnstile-response"]` is present.
    has_turnstile_response: bool,
    /// `input[name="cf-response"]` is present.
    has_cf_response: bool,
    /// A generic Cloudflare interstitial marker is present.
    has_interstitial_marker: bool,
    /// The document title matches known challenge phrasing.
    has_challenge_title: bool,
}

impl ChallengeSignals {
    /// Decodes a `CHALLENGE_SIGNAL_JS` result payload.
    fn from_value(value: &Value) -> Self {
        Self {
            has_cf_challenge_frame: value
                .get("cfChallengeFrame")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            has_turnstile_response: value
                .get("turnstileResponse")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            has_cf_response: value
                .get("cfResponse")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            has_interstitial_marker: value
                .get("interstitialMarker")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            has_challenge_title: value
                .get("challengeTitle")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
        }
    }

    /// Whether the sampled markers look like a Cloudflare challenge.
    fn suggests_challenge(&self) -> bool {
        self.has_cf_challenge_frame
            || self.has_turnstile_response
            || self.has_cf_response
            || self.has_interstitial_marker
            || self.has_challenge_title
    }

    /// Formats a label-only summary for errors and logs. Never contains
    /// cookie values, tokens, URLs or document text.
    fn summarize(&self) -> String {
        let mut labels: Vec<&str> = Vec::new();
        if self.has_cf_challenge_frame {
            labels.push("cf-challenge-frame");
        }
        if self.has_turnstile_response {
            labels.push("turnstile-response-field");
        }
        if self.has_cf_response {
            labels.push("cf-response-field");
        }
        if self.has_interstitial_marker {
            labels.push("interstitial-marker");
        }
        if self.has_challenge_title {
            labels.push("challenge-title");
        }
        if labels.is_empty() {
            "no challenge markers".to_string()
        } else {
            labels.join(", ")
        }
    }
}

/// Stable Arachnea-side settings for the embedded Obscura browser.
///
/// This type intentionally keeps Obscura types out of the Arachnea
/// configuration surface; only the embedded engine consumes it.
///
/// Phase 1 carries configuration and construction only; the settings are
/// consumed by the Phase 2 browser build and solve paths, so the dead-code
/// allowance is scoped to this skeleton type.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ObscuraEngineConfig {
    /// Enable the Obscura stealth profile. The flag reaches
    /// `obscura::BrowserConfig::stealth` for fingerprint alignment. The
    /// dependency `stealth` feature is enabled since the workspace HTTP stack
    /// migrated from `newwreq` to `wreq` (option A1, integration plan Phase
    /// 1b), providing the impersonated TLS/HTTP stack.
    stealth: bool,
    /// Enable the Obscura render layer used by real click flows and element
    /// geometry.
    render: bool,
    /// User-Agent forced into the embedded browser context.
    user_agent: Option<String>,
    /// Timeout applied to normalized requests executed by this engine.
    request_timeout: Duration,
    /// Timeout of the initial Cloudflare clearance protocol.
    clearance_timeout: Duration,
    /// Margin before `cf_clearance` expiry where cached sessions refresh.
    cookie_refresh_margin: Duration,
    /// Proxy transport selection carried from `ArachneaHttpConfig`.
    proxy: HttpProxyConfig,
}

impl ObscuraEngineConfig {
    /// Derives the embedded engine settings from the client configuration.
    fn from_arachnea(config: &ArachneaHttpConfig) -> Self {
        Self {
            stealth: true,
            render: true,
            user_agent: Some(config.user_agent_profile.user_agent().to_string()),
            request_timeout: config.request_timeout,
            clearance_timeout: OBSCURA_CLEARANCE_TIMEOUT,
            cookie_refresh_margin: config.cookie_refresh_margin,
            proxy: config.proxy.clone(),
        }
    }
}

/// Transport mode selected for the embedded browser network stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObscuraTransportMode {
    /// The embedded browser's own network stack, optionally behind a network
    /// proxy URL.
    NetworkProxy(Option<String>),
    /// Every intercepted request is fulfilled through the in-process Arachnea
    /// proxy core. Selected for `HttpProxyConfig::Arachnea`; the interceptor
    /// adapter itself lands in Phase 4.
    #[allow(dead_code)]
    InterceptorFulfill,
}

impl ObscuraTransportMode {
    /// Selects the transport from the proxy configuration.
    fn select(proxy: &HttpProxyConfig) -> Self {
        match proxy {
            HttpProxyConfig::Disabled => Self::NetworkProxy(None),
            HttpProxyConfig::Network(url) => Self::NetworkProxy(Some(url.clone())),
            #[cfg(feature = "arachnea-proxy")]
            HttpProxyConfig::Arachnea(_) => Self::InterceptorFulfill,
        }
    }

    /// Returns the stable transport label used in logs; never carries secrets.
    fn label(&self) -> &'static str {
        match self {
            Self::NetworkProxy(_) => "network-proxy",
            Self::InterceptorFulfill => "interceptor-fulfill",
        }
    }
}

/// Runs the non-`Send` embedded browser solve on a dedicated thread.
///
/// `obscura::Page` pins its JavaScript runtime to its owner thread (the future
/// it drives is `!Send`), so the whole solve must stay on one thread that the
/// caller never shares. The closure builds its own single-threaded Tokio
/// runtime and blocks on the Obscura futures there; only `Send` inputs cross
/// into the thread and only the `Send`-safe result crosses back.
async fn run_blocking_solve<T>(
    operation: &'static str,
    solve: impl FnOnce() -> Result<T, ArachneaHttpError> + Send + 'static,
) -> Result<T, ArachneaHttpError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(solve).await.map_err(|err| {
        ArachneaHttpError::ObscuraFailure(format!(
            "the embedded browser {operation} task failed to join: {err}"
        ))
    })?
}

/// Commands sent from the `Send` session handle to its dedicated page thread.
///
/// Every payload is `Send`; the `!Send` page itself never crosses the channel.
#[derive(Debug)]
enum ObscuraSessionCommand {
    /// Navigate the retained page and optionally collect its stable HTML.
    Navigate {
        /// Absolute navigation URL.
        url: String,
        /// Whether `document.documentElement.outerHTML` should be returned.
        collect_body: bool,
        /// Completion responder.
        reply: oneshot::Sender<Result<PageNavigationResponse, ArachneaHttpError>>,
    },
    /// Execute one `window.fetch` call inside the retained page.
    Fetch {
        /// Serialized [`PageFetchRequest`] payload for the in-page script.
        payload: String,
        /// Fallback URL reported when the page does not expose `response.url`.
        fallback_url: String,
        /// Completion responder.
        reply: oneshot::Sender<Result<PageFetchResponse, ArachneaHttpError>>,
    },
    /// Click a selector, wait for the result selector, and return the HTML.
    ClickAndWait {
        /// CSS selector of the element to click.
        selector: String,
        /// CSS selector that must appear after the click.
        wait_for_selector: String,
        /// Completion responder.
        reply: oneshot::Sender<Result<PageClickResponse, ArachneaHttpError>>,
    },
    /// Read cookies and the observed user-agent for the HTTP handoff.
    Metadata {
        /// Completion responder.
        reply: oneshot::Sender<Result<BrowserSessionMetadata, ArachneaHttpError>>,
    },
    /// Probe the three Turnstile token sources on the retained page.
    ReadTurnstileToken {
        /// Completion responder.
        reply: oneshot::Sender<Result<Option<String>, ArachneaHttpError>>,
    },
    /// Reset the Turnstile widget and clear the response fields.
    ClearTurnstileToken {
        /// Completion responder.
        reply: oneshot::Sender<Result<(), ArachneaHttpError>>,
    },
    /// Terminate the page thread and release the embedded browser.
    Close {
        /// Completion responder.
        reply: oneshot::Sender<()>,
    },
}

/// Channel pair connecting a `Send` session handle to its page thread.
struct ObscuraSessionChannel {
    /// Command sender cloned into the session handle.
    sender: mpsc::UnboundedSender<ObscuraSessionCommand>,
    /// Handle to the dedicated page thread for orderly teardown.
    worker: Option<std::thread::JoinHandle<()>>,
}

impl ObscuraSessionChannel {
    /// Spawns the dedicated page thread and returns the connected channel.
    ///
    /// The thread owns the browser and its retained page for the whole session
    /// lifetime: the Obscura runtime is thread-affine (`!Send`), so neither
    /// handle may ever leave it. All interactions travel through the command
    /// channel and only `Send`-safe data crosses back.
    async fn spawn(
        config: ObscuraEngineConfig,
        transport: ObscuraTransportMode,
    ) -> Result<Self, ArachneaHttpError> {
        let (sender, receiver) = mpsc::unbounded_channel::<ObscuraSessionCommand>();
        let (init_sender, init_receiver) = oneshot::channel::<Result<(), ArachneaHttpError>>();
        let worker = std::thread::Builder::new()
            .name("arachnea-obscura-page".to_string())
            .spawn(move || {
                let init = ObscuraPageWorker::initialize(config, transport, receiver);
                match init {
                    Ok(None) => return,
                    Err(err) => {
                        let _ = init_sender.send(Err(err));
                        return;
                    }
                    Ok(Some(worker_parts)) => {
                        let (runtime, worker) = worker_parts;
                        let _ = init_sender.send(Ok(()));
                        runtime.block_on(worker.run_to_completion());
                    }
                }
            })
            .map_err(|err| {
                ArachneaHttpError::ObscuraFailure(format!(
                    "failed to spawn the obscura page thread: {err}"
                ))
            })?;
        // Fail fast when the embedded browser or its page cannot start, so
        // the caller never receives a handle to a dead thread.
        let init = init_receiver.await.unwrap_or_else(|_| {
            Err(ArachneaHttpError::ObscuraFailure(
                "the obscura page thread stopped during initialization".to_string(),
            ))
        });
        if let Err(err) = init {
            let _ = worker.join();
            return Err(err);
        }
        Ok(Self {
            sender,
            worker: Some(worker),
        })
    }
}

/// Dedicated-thread worker that owns the retained page.
struct ObscuraPageWorker {
    /// Embedded browser owning the network stack and cookie jar.
    browser: Browser,
    /// Retained page carrying the DOM and JS runtime across commands.
    page: Page,
    /// Stable engine settings (timeouts, clearance protocol bounds).
    context: ObscuraPageContext,
    /// Command receiver drained until `Close` or channel disconnect.
    receiver: mpsc::UnboundedReceiver<ObscuraSessionCommand>,
}

/// Immutable session settings shared with the page thread.
struct ObscuraPageContext {
    /// Engine configuration derived from `ArachneaHttpConfig`.
    config: ObscuraEngineConfig,
    /// Transport mode selected at engine construction.
    transport: ObscuraTransportMode,
}

impl ObscuraPageWorker {
    /// Builds the thread runtime, the browser, and the session page.
    ///
    /// Returns `Ok(Some((runtime, worker)))` once the page is ready, or an
    /// error describing the failed initialization step. `Ok(None)` is never
    /// produced; the shape exists to keep the early returns uniform.
    fn initialize(
        config: ObscuraEngineConfig,
        transport: ObscuraTransportMode,
        receiver: mpsc::UnboundedReceiver<ObscuraSessionCommand>,
    ) -> Result<Option<(tokio::runtime::Runtime, Self)>, ArachneaHttpError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| {
                ArachneaHttpError::ObscuraFailure(format!(
                    "the obscura page-thread runtime is unavailable: {err}"
                ))
            })?;
        let context = ObscuraPageContext { config, transport };
        let (browser, page) = runtime.block_on(async {
            let browser = ObscuraEngine::new_browser_with(&context.config, &context.transport)?;
            let page = browser.new_page().await.map_err(|err| {
                ArachneaHttpError::ObscuraFailure(format!(
                    "failed to open the obscura session page: {err}"
                ))
            })?;
            Ok::<_, ArachneaHttpError>((browser, page))
        })?;
        Ok(Some((
            runtime,
            Self {
                browser,
                page,
                context,
                receiver,
            },
        )))
    }

    /// Runs the command loop until `Close`, then tears the page down.
    async fn run_to_completion(mut self) {
        info!(
            engine = ENGINE_NAME,
            revision = OBSCURA_PINNED_REVISION,
            transport = self.context.transport.label(),
            "obscura persistent page session started"
        );
        while let Some(command) = self.receiver.recv().await {
            match command {
                ObscuraSessionCommand::Navigate {
                    url,
                    collect_body,
                    reply,
                } => {
                    let _ = reply.send(self.navigate(&url, collect_body).await);
                }
                ObscuraSessionCommand::Fetch {
                    payload,
                    fallback_url,
                    reply,
                } => {
                    let _ = reply.send(self.fetch(&payload, &fallback_url).await);
                }
                ObscuraSessionCommand::ClickAndWait {
                    selector,
                    wait_for_selector,
                    reply,
                } => {
                    let _ = reply.send(self.click_and_wait(&selector, &wait_for_selector).await);
                }
                ObscuraSessionCommand::Metadata { reply } => {
                    let _ = reply.send(self.metadata());
                }
                ObscuraSessionCommand::ReadTurnstileToken { reply } => {
                    let _ = reply.send(self.read_turnstile_token().await);
                }
                ObscuraSessionCommand::ClearTurnstileToken { reply } => {
                    let _ = reply.send(self.clear_turnstile_token());
                }
                ObscuraSessionCommand::Close { reply } => {
                    let _ = reply.send(());
                    break;
                }
            }
        }
        // Drain the page and browser on their own thread; the embedded
        // runtime must never be dropped from another thread.
        info!(
            engine = ENGINE_NAME,
            "obscura persistent page session closed"
        );
    }

    /// Navigates the retained page and collects its stable HTML when asked.
    async fn navigate(
        &mut self,
        url: &str,
        collect_body: bool,
    ) -> Result<PageNavigationResponse, ArachneaHttpError> {
        self.page.goto(url).await.map_err(|err| {
            ArachneaHttpError::ObscuraFailure(format!("session navigation failed: {err}"))
        })?;
        self.page.settle(OBSCURA_NAVIGATE_SETTLE_MS).await;
        Ok(PageNavigationResponse {
            url: self.page.url(),
            body: if collect_body {
                Some(self.page.content())
            } else {
                None
            },
        })
    }

    /// Runs one in-page `window.fetch` call and waits for its promise.
    ///
    /// The facade `evaluate` does not await promises, so the launcher script
    /// parks its result in a page global; the worker then drives the event
    /// loop in `settle` slices until the completion flag flips, bounded by
    /// `OBSCURA_FETCH_TIMEOUT`.
    async fn fetch(
        &mut self,
        payload: &str,
        fallback_url: &str,
    ) -> Result<PageFetchResponse, ArachneaHttpError> {
        let payload = serde_json::from_str::<Value>(payload).map_err(|err| {
            ArachneaHttpError::PageFetchFailed(format!("invalid in-page fetch payload: {err}"))
        })?;
        let launcher = in_page_fetch_script(&payload);
        let launched = self.page.evaluate(&launcher).as_bool().unwrap_or(false);
        if !launched {
            return Err(ArachneaHttpError::PageFetchFailed(
                "the in-page fetch launcher could not be evaluated".to_string(),
            ));
        }
        let started = Instant::now();
        loop {
            let done = self
                .page
                .evaluate("globalThis.__arachneaObscuraFetchDone === true")
                .as_bool()
                .unwrap_or(false);
            if done {
                let value = self
                    .page
                    .evaluate("globalThis.__arachneaObscuraFetchResult");
                let outcome = value
                    .as_str()
                    .and_then(|raw| serde_json::from_str::<InPageFetchOutcome>(raw).ok())
                    .unwrap_or_default();
                return page_fetch_response_from_outcome(outcome, fallback_url);
            }
            if started.elapsed() >= OBSCURA_FETCH_TIMEOUT {
                return Err(ArachneaHttpError::PageFetchFailed(format!(
                    "the in-page fetch did not complete within {}s",
                    OBSCURA_FETCH_TIMEOUT.as_secs()
                )));
            }
            self.page.settle(OBSCURA_FETCH_SETTLE_MS).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Clicks a selector, then waits for the result selector on the same page.
    ///
    /// When the click injects a Cloudflare challenge, the bounded clearance
    /// protocol runs on the same retained page — its context is never
    /// destroyed — with an extended deadline, exactly like the chaser-cf
    /// adapter. The current HTML is returned once the selector appears.
    async fn click_and_wait(
        &mut self,
        selector: &str,
        wait_for_selector: &str,
    ) -> Result<PageClickResponse, ArachneaHttpError> {
        let click_js = format!(
            "(function () {{
                const element = document.querySelector({selector_json});
                if (!element) return false;
                element.scrollIntoView({{ block: 'center' }});
                element.click();
                return true;
            }})()",
            selector_json = serde_json::to_string(selector)
                .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?
        );
        let clicked = self.page.evaluate(&click_js).as_bool().unwrap_or(false);
        if !clicked {
            return Err(ArachneaHttpError::PageInteractionFailed(
                "the click target was not found".to_string(),
            ));
        }
        let started = Instant::now();
        let mut deadline = started + OBSCURA_CLICK_WAIT_TIMEOUT;
        let mut solve_started = false;
        // The first bounded Turnstile attempt waits out the passive window:
        // Cloudflare's invisible challenge JavaScript starts its proof-of-work
        // right after the load event and must not be polled aggressively.
        let mut last_attempt = started - Duration::from_secs(60)
            + Duration::from_millis(OBSCURA_CLEARANCE_PASSIVE_WAIT_MS);
        let wait_js = format!(
            "(function () {{ return document.querySelector({selector_json}) !== null; }})()",
            selector_json = serde_json::to_string(wait_for_selector)
                .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?
        );
        loop {
            if !solve_started && Self::challenge_is_present(&self.page) {
                solve_started = true;
                deadline = started + OBSCURA_CLICK_SOLVE_TIMEOUT + OBSCURA_CLICK_WAIT_TIMEOUT;
                info!(
                    selector = %wait_for_selector,
                    "obscura: challenge detected after page action click, solving"
                );
                self.solve_injected_challenge(&mut deadline, &mut last_attempt)
                    .await;
            }
            let found = self.page.evaluate(&wait_js).as_bool().unwrap_or(false);
            if found {
                return Ok(PageClickResponse {
                    body: self.page.content(),
                });
            }
            if Instant::now() >= deadline {
                return Err(ArachneaHttpError::PageInteractionFailed(format!(
                    "the result selector did not appear within {}s",
                    (OBSCURA_CLICK_WAIT_TIMEOUT + OBSCURA_CLICK_SOLVE_TIMEOUT).as_secs()
                )));
            }
            if !solve_started
                && last_attempt.elapsed().as_millis() as u64
                    >= OBSCURA_CLEARANCE_ATTEMPT_INTERVAL_MS
            {
                ObscuraEngine::try_click_turnstile(&self.page);
                last_attempt = Instant::now();
            }
            self.page.settle(OBSCURA_CLEARANCE_POLL_MS).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Bounded clearance wait for a challenge injected after a click.
    ///
    /// Runs on the same retained page; only `cf_clearance` appearance or the
    /// bounded deadline ends the wait, and Turnstile widgets are clicked at
    /// the bounded protocol rate.
    async fn solve_injected_challenge(
        &mut self,
        deadline: &mut Instant,
        last_attempt: &mut Instant,
    ) {
        while !ObscuraEngine::has_clearance_cookie(&self.browser) && Instant::now() < *deadline {
            if last_attempt.elapsed().as_millis() as u64 >= OBSCURA_CLEARANCE_ATTEMPT_INTERVAL_MS {
                ObscuraEngine::try_click_turnstile(&self.page);
                *last_attempt = Instant::now();
            }
            self.page.settle(OBSCURA_CLEARANCE_POLL_MS).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Detects generic Cloudflare challenge markers on the retained page.
    fn challenge_is_present(page: &Page) -> bool {
        ObscuraEngine::sample_challenge_signals(page).suggests_challenge()
    }

    /// Collects cookies and the observed user-agent for the HTTP handoff.
    fn metadata(&self) -> Result<BrowserSessionMetadata, ArachneaHttpError> {
        let mut headers = HeaderMap::new();
        for cookie in ObscuraEngine::structured_cookies(&self.browser) {
            headers.append(
                SET_COOKIE,
                HeaderValue::from_str(&set_cookie_header(&cookie))
                    .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
            );
        }
        if let Some(user_agent) = ObscuraEngine::observe_user_agent(&self.page) {
            headers.insert(
                SOLVER_USER_AGENT_HEADER,
                HeaderValue::from_str(&user_agent)
                    .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
            );
        }
        Ok(BrowserSessionMetadata {
            url: self.page.url(),
            headers,
        })
    }

    /// Polls the three Turnstile token sources until one produces a value.
    async fn read_turnstile_token(&mut self) -> Result<Option<String>, ArachneaHttpError> {
        let started = Instant::now();
        loop {
            let token = self
                .page
                .evaluate(TURNSTILE_TOKEN_JS)
                .as_str()
                .map(str::to_string);
            if let Some(token) = token {
                return Ok(Some(token));
            }
            if started.elapsed() >= OBSCURA_TURNSTILE_WAIT_TIMEOUT {
                return Ok(None);
            }
            self.page.settle(OBSCURA_CLEARANCE_POLL_MS).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Resets the Turnstile widget and empties the response fields.
    fn clear_turnstile_token(&self) -> Result<(), ArachneaHttpError> {
        let cleared = self
            .page
            .evaluate(TURNSTILE_CLEAR_JS)
            .as_bool()
            .unwrap_or(false);
        if !cleared {
            return Err(ArachneaHttpError::ObscuraFailure(
                "the Turnstile reset script could not be evaluated".to_string(),
            ));
        }
        Ok(())
    }
}

/// `Send` handle to a persistent Obscura page owned by a dedicated thread.
///
/// The Arachnea-facing side of the session: every trait method translates the
/// request into one [`ObscuraSessionCommand`] and awaits the worker's reply.
/// The `!Send` page never crosses the channel; only `Send`-safe request and
/// response payloads do.
struct ObscuraPageSession {
    /// Command channel to the dedicated page thread.
    channel: ObscuraSessionChannel,
}

impl ObscuraPageSession {
    /// Sends one command and awaits its reply.
    async fn command<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, ArachneaHttpError>>) -> ObscuraSessionCommand,
    ) -> Result<T, ArachneaHttpError> {
        let (reply, receiver) = oneshot::channel();
        self.channel.sender.send(build(reply)).map_err(|_| {
            ArachneaHttpError::ObscuraFailure(
                "the obscura page thread is no longer running".to_string(),
            )
        })?;
        receiver.await.map_err(|_| {
            ArachneaHttpError::ObscuraFailure(
                "the obscura page thread stopped while processing a command".to_string(),
            )
        })?
    }

    /// Builds the JSON payload consumed by the in-page fetch launcher.
    fn fetch_payload(request: &PageFetchRequest) -> Result<String, ArachneaHttpError> {
        let mut headers = HashMap::new();
        for (name, value) in &request.headers {
            let value = value
                .to_str()
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?;
            headers.insert(name.as_str().to_string(), value.to_string());
        }
        let body = request
            .body
            .as_ref()
            .map(|body| {
                String::from_utf8(body.to_vec()).map_err(|_| {
                    ArachneaHttpError::PageFetchFailed(
                        "in-page fetch currently requires a UTF-8 request body".to_string(),
                    )
                })
            })
            .transpose()?;
        let payload = serde_json::json!({
            "method": request.method.as_str(),
            "url": request.url,
            "headers": headers,
            "body": body,
        });
        serde_json::to_string(&payload)
            .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))
    }
}

#[async_trait]
impl BrowserPageSession for ObscuraPageSession {
    async fn navigate(
        &mut self,
        request: PageNavigationRequest,
    ) -> Result<PageNavigationResponse, ArachneaHttpError> {
        self.command(|reply| ObscuraSessionCommand::Navigate {
            url: request.url,
            collect_body: request.collect_body,
            reply,
        })
        .await
    }

    async fn fetch(
        &mut self,
        request: PageFetchRequest,
    ) -> Result<PageFetchResponse, ArachneaHttpError> {
        let fallback_url = request.url.clone();
        let payload = Self::fetch_payload(&request)?;
        self.command(|reply| ObscuraSessionCommand::Fetch {
            payload,
            fallback_url,
            reply,
        })
        .await
    }

    async fn click_and_wait(
        &mut self,
        request: PageClickRequest,
    ) -> Result<PageClickResponse, ArachneaHttpError> {
        self.command(|reply| ObscuraSessionCommand::ClickAndWait {
            selector: request.selector,
            wait_for_selector: request.wait_for_selector,
            reply,
        })
        .await
    }

    async fn metadata(&mut self) -> Result<BrowserSessionMetadata, ArachneaHttpError> {
        self.command(|reply| ObscuraSessionCommand::Metadata { reply })
            .await
    }

    async fn read_turnstile_token(&mut self) -> Result<Option<String>, ArachneaHttpError> {
        self.command(|reply| ObscuraSessionCommand::ReadTurnstileToken { reply })
            .await
    }

    async fn clear_turnstile_token(&mut self) -> Result<(), ArachneaHttpError> {
        self.command(|reply| ObscuraSessionCommand::ClearTurnstileToken { reply })
            .await
    }

    async fn close(mut self: Box<Self>) {
        let (reply, receiver) = oneshot::channel();
        if self
            .channel
            .sender
            .send(ObscuraSessionCommand::Close { reply })
            .is_ok()
        {
            let _ = receiver.await;
        }
        // Join the page thread so the embedded runtime is torn down on its
        // own thread before this handle returns.
        if let Some(worker) = self.channel.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Drives one `Send`-unsafe Obscura future on the current thread.
///
/// Must only be called inside [`run_blocking_solve`]: the runtime is built and
/// blocked on within the surrounding blocking thread, so non-`Send` page
/// handles never leave that thread even though the outer `HttpEngine` future
/// stays `Send`.
fn block_on_local<T>(
    operation: &'static str,
    future: impl std::future::Future<Output = Result<T, ArachneaHttpError>>,
) -> Result<T, ArachneaHttpError> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| {
            ArachneaHttpError::ObscuraFailure(format!(
                "the embedded browser {operation} runtime is unavailable: {err}"
            ))
        })?
        .block_on(future)?)
}
/// Persistent Cloudflare session cache reusing the existing
/// `CachedChaserSession` storage shape, per the integration plan.
struct ObscuraSessionCache {
    /// Shared typed session store.
    store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    /// Proactive refresh margin before `cf_clearance` expiry.
    refresh_margin: Duration,
}

impl ObscuraSessionCache {
    /// Returns cached headers when a still-valid clearance exists for `url`.
    async fn headers_for_url(&self, url: &str) -> Result<Option<HeaderMap>, ArachneaHttpError> {
        let origin = cache_origin_key(url)?;
        let session = self.store.get(&origin).await.map_err(|err| {
            ArachneaHttpError::ObscuraFailure(format!(
                "failed to read the obscura session cache: {err}"
            ))
        })?;
        let Some(session) = session else {
            return Ok(None);
        };
        if !session.is_usable(self.refresh_margin) {
            self.store.delete(&origin).await.map_err(|err| {
                ArachneaHttpError::ObscuraFailure(format!(
                    "failed to evict the stale obscura session: {err}"
                ))
            })?;
            return Ok(None);
        }
        cached_session_headers(&session).map(Some)
    }

    /// Stores a newly solved session when it carries `cf_clearance`.
    ///
    /// Sessions without a clearance are never persisted, matching the
    /// `CachedChaserSession` invariant shared with the chaser-cf adapter.
    async fn store_session(
        &self,
        url: &str,
        cookies: &[StructuredCookie],
        user_agent: Option<&str>,
    ) -> Result<(), ArachneaHttpError> {
        let origin = cache_origin_key(url)?;
        if !cookies.iter().any(|cookie| cookie.name == "cf_clearance") {
            return Ok(());
        }
        let session = CachedChaserSession {
            origin,
            cookies: cookies.iter().map(|cookie| cookie.clone()).collect(),
            user_agent: user_agent.map(|user_agent| user_agent.to_string()),
            clearance_expires_at: clearance_expires_at(cookies),
            stored_at: unix_timestamp(),
        };
        self.store.put(&session).await.map_err(|err| {
            ArachneaHttpError::ObscuraFailure(format!(
                "failed to persist the obscura session: {err}"
            ))
        })
    }
}

/// Obscura adapter that resolves Cloudflare sessions with the embedded
/// headless browser: no Chrome/Chromium, no separate Obscura/CDP process.
pub struct ObscuraEngine {
    /// Stable Arachnea-side engine settings.
    config: ObscuraEngineConfig,
    /// Transport mode selected from the effective proxy configuration.
    transport: ObscuraTransportMode,
    /// Optional persistent session cache reused across refreshes.
    session_cache: Option<ObscuraSessionCache>,
}

impl ObscuraEngine {
    /// Creates an engine from the client configuration with an in-memory
    /// session store.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration used to derive the engine settings.
    ///
    /// # Returns
    ///
    /// A configured Obscura engine.
    ///
    /// # Errors
    ///
    /// Returns `ObscuraFailure` when the in-memory session store cannot be
    /// created.
    pub fn new(config: &ArachneaHttpConfig) -> Result<Self, ArachneaHttpError> {
        let session_store = memory_session_store().map_err(|error| {
            ArachneaHttpError::ObscuraFailure(format!("session cache store unavailable: {error}"))
        })?;
        Self::new_with_proxy_url(config, config.proxy.network_url(), session_store)
    }

    /// Creates an engine using the effective proxy URL selected by the HTTP
    /// client runtime and a shared typed session store.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration used to derive the engine settings.
    /// - `proxy_url`: Effective network proxy URL, when the runtime selected
    ///   one.
    /// - `session_store`: Shared typed Cloudflare session store.
    ///
    /// # Returns
    ///
    /// A configured Obscura engine.
    ///
    /// # Errors
    ///
    /// Returns `ObscuraFailure` when the engine settings cannot be derived.
    pub(crate) fn new_with_proxy_url(
        config: &ArachneaHttpConfig,
        proxy_url: Option<&str>,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) -> Result<Self, ArachneaHttpError> {
        let engine_config = ObscuraEngineConfig::from_arachnea(config);
        // The runtime-provided proxy URL wins so the solver shares the rquest
        // proxy route, exactly like the chaser-cf adapter.
        let transport = match proxy_url {
            Some(url) => ObscuraTransportMode::NetworkProxy(Some(url.to_string())),
            None => ObscuraTransportMode::select(&engine_config.proxy),
        };
        debug!(
            engine = ENGINE_NAME,
            revision = OBSCURA_PINNED_REVISION,
            platform = std::env::consts::OS,
            transport = transport.label(),
            stealth = engine_config.stealth,
            render = engine_config.render,
            has_user_agent = engine_config.user_agent.is_some(),
            "obscura engine constructed"
        );
        Ok(Self {
            config: engine_config,
            transport,
            session_cache: Some(ObscuraSessionCache {
                store: session_store,
                refresh_margin: config.cookie_refresh_margin,
            }),
        })
    }

    /// Rejects the methods the browser-solver contract does not accept.
    fn reject_unsupported_methods(
        request: &EngineRequest,
        operation: &'static str,
    ) -> Result<(), ArachneaHttpError> {
        if request.method != Method::GET && request.method != Method::HEAD {
            return Err(ArachneaHttpError::UnsupportedEngineOperation {
                engine: ENGINE_NAME,
                operation,
            });
        }
        Ok(())
    }

    /// Returns the browser proxy URL for an explicit transport selection.
    ///
    /// Split from the previous `&self` helper so the `!Send` solve running on
    /// the dedicated blocking thread validates its transport without borrowing
    /// `self` across that thread boundary.
    fn browser_proxy_url_for(
        transport: &ObscuraTransportMode,
    ) -> Result<Option<String>, ArachneaHttpError> {
        match transport.clone() {
            ObscuraTransportMode::NetworkProxy(None) => Ok(None),
            ObscuraTransportMode::NetworkProxy(Some(url)) => {
                let parsed = Url::parse(&url).map_err(|err| {
                    ArachneaHttpError::InvalidConfiguration(format!(
                        "invalid Obscura proxy URL: {err}"
                    ))
                })?;
                match parsed.scheme() {
                    "http" | "https" => Ok(Some(url.clone())),
                    scheme => Err(ArachneaHttpError::InvalidConfiguration(format!(
                        "unsupported Obscura proxy scheme '{scheme}': the stealth transport only supports http/https proxies"
                    ))),
                }
            }
            ObscuraTransportMode::InterceptorFulfill => Err(ArachneaHttpError::ObscuraFailure(
                "the in-process Arachnea proxy transport requires the Phase 4 interceptor adapter; refusing to bypass the configured route"
                    .to_string(),
            )),
        }
    }

    /// Builds a fresh embedded browser for one solve.
    #[allow(dead_code)]
    fn new_browser(&self) -> Result<Browser, ArachneaHttpError> {
        Self::new_browser_with(&self.config, &self.transport)
    }

    /// Builds a fresh embedded browser from explicit settings.
    ///
    /// Split from [`Self::new_browser`] so the `!Send` solve running on the
    /// dedicated blocking thread can build its browser without borrowing
    /// `self` across that thread boundary.
    fn new_browser_with(
        config: &ObscuraEngineConfig,
        transport: &ObscuraTransportMode,
    ) -> Result<Browser, ArachneaHttpError> {
        let mut builder = Browser::builder().stealth(config.stealth);
        if let Some(user_agent) = &config.user_agent {
            builder = builder.user_agent(user_agent.clone());
        }
        if let Some(proxy_url) = Self::browser_proxy_url_for(transport)? {
            builder = builder.proxy(proxy_url);
        }
        Ok(builder.build().map_err(|err| {
            ArachneaHttpError::ObscuraFailure(format!(
                "failed to build the embedded browser: {err}"
            ))
        })?)
    }

    /// Solves a Cloudflare session on an ephemeral page and extracts the
    /// cleared state.
    ///
    /// The whole solve runs on one dedicated blocking thread (see
    /// [`run_blocking_solve`]): the browser and page are created, awaited and
    /// dropped there with a single-threaded runtime, so the non-`Send`
    /// JavaScript runtime never crosses the `HttpEngine` future boundary.
    /// Nothing is shared across origins; only `Send` inputs enter the thread
    /// and only the `Send`-safe result leaves it.
    async fn solve(
        &self,
        url: &str,
        collect_html: bool,
    ) -> Result<ObscuraSolveResult, ArachneaHttpError> {
        let origin = cache_origin_key(url)?;
        let engine_config = self.config.clone();
        let transport = self.transport.clone();
        let target_url = url.to_string();
        let operation_origin = origin.clone();
        run_blocking_solve("solve", move || {
            block_on_local("solve", async move {
                ObscuraEngine::solve_on_local_thread(
                    &engine_config,
                    &transport,
                    &operation_origin,
                    &target_url,
                    collect_html,
                )
                .await
            })
        })
        .await
    }

    /// Runs one ephemeral solve on the current thread.
    ///
    /// Must only run inside [`run_blocking_solve`]: every Obscura handle
    /// created here is `!Send` and must never leave this thread.
    async fn solve_on_local_thread(
        config: &ObscuraEngineConfig,
        transport: &ObscuraTransportMode,
        origin: &str,
        url: &str,
        collect_html: bool,
    ) -> Result<ObscuraSolveResult, ArachneaHttpError> {
        let browser = Self::new_browser_with(config, transport)?;
        let mut page = browser.new_page().await.map_err(|err| {
            ArachneaHttpError::ObscuraFailure(format!("failed to open an embedded page: {err}"))
        })?;
        info!(
            engine = ENGINE_NAME,
            origin,
            collect_html,
            revision = OBSCURA_PINNED_REVISION,
            transport = transport.label(),
            "obscura solve started"
        );
        let result = async {
            page.goto(url).await.map_err(|err| {
                ArachneaHttpError::ObscuraFailure(format!("navigation to {origin} failed: {err}"))
            })?;
            Self::wait_for_clearance_on_local_thread(config, &browser, &mut page, origin).await?;
            let cookies = Self::structured_cookies(&browser);
            let user_agent = Self::observe_user_agent(&page);
            let html = if collect_html {
                Some(page.content())
            } else {
                None
            };
            Ok(ObscuraSolveResult {
                origin: origin.to_string(),
                final_url: page.url(),
                cookies,
                user_agent,
                html,
            })
        }
        .await;
        // Cleanup: the page owns the JS runtime and the browser owns the
        // network client; both are released with the page/browser drop.
        drop(page);
        drop(browser);
        result
    }

    /// Waits for `cf_clearance` to appear in the browser cookie jar.
    ///
    /// The protocol is bounded and observable: a passive window lets the
    /// challenge JavaScript run, then the retained page's event loop is driven
    /// in `settle` slices (sleeping alone never advances the embedded
    /// runtime). Known same-origin Turnstile widgets are clicked at a bounded
    /// rate; cross-origin frames are never opened from here. On timeout the
    /// error carries only non-secret challenge signal labels.
    ///
    /// Must only run on the dedicated blocking thread (see
    /// [`run_blocking_solve`]): `page` is `!Send`.
    async fn wait_for_clearance_on_local_thread(
        config: &ObscuraEngineConfig,
        browser: &Browser,
        page: &mut Page,
        origin: &str,
    ) -> Result<(), ArachneaHttpError> {
        let started = Instant::now();
        page.settle(OBSCURA_CLEARANCE_PASSIVE_WAIT_MS).await;
        let mut last_attempt = started - Duration::from_secs(60);
        loop {
            if Self::has_clearance_cookie(browser) {
                page.settle(OBSCURA_CLEARANCE_SETTLE_MS).await;
                return Ok(());
            }
            if started.elapsed() >= config.clearance_timeout {
                let signals = Self::sample_challenge_signals(page);
                let summary = signals.summarize();
                debug!(
                    engine = ENGINE_NAME,
                    origin,
                    %summary,
                    suggests_challenge = signals.suggests_challenge(),
                    "obscura clearance wait timed out"
                );
                return Err(ArachneaHttpError::ObscuraFailure(format!(
                    "Cloudflare clearance was not issued for {origin} within {}s ({summary})",
                    config.clearance_timeout.as_secs(),
                )));
            }
            if last_attempt.elapsed().as_millis() as u64 >= OBSCURA_CLEARANCE_ATTEMPT_INTERVAL_MS {
                Self::try_click_turnstile(page);
                last_attempt = Instant::now();
            }
            page.settle(OBSCURA_CLEARANCE_POLL_MS).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Returns cached headers when a still-valid clearance is cached.
    async fn cached_session_headers(&self, url: &str) -> Option<HeaderMap> {
        let cache = self.session_cache.as_ref()?;
        match cache.headers_for_url(url).await {
            Ok(headers) => headers,
            Err(err) => {
                warn!(error = %err, "failed to read obscura session cache");
                None
            }
        }
    }

    /// Stores a newly solved session in the optional persistent cache.
    async fn store_session_cache(&self, url: &str, solution: &ObscuraSolveResult) {
        let Some(cache) = &self.session_cache else {
            return;
        };
        if let Err(err) = cache
            .store_session(url, &solution.cookies, solution.user_agent.as_deref())
            .await
        {
            warn!(error = %err, "failed to write obscura session cache");
        }
    }

    /// Refreshes Cloudflare cookies, optionally reusing a cached session.
    async fn refresh_cloudflare_with_cache_policy(
        &self,
        request: EngineRequest,
        use_session_cache: bool,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        Self::reject_unsupported_methods(&request, "non-GET Cloudflare refresh requests")?;
        if use_session_cache {
            if let Some(headers) = self.cached_session_headers(&request.url).await {
                return Ok(EngineResponse {
                    url: request.url,
                    status: StatusCode::OK,
                    headers,
                    body: Bytes::new(),
                });
            }
        }
        let solution = self.solve(&request.url, false).await?;
        let mut headers = Self::response_headers(&solution)?;
        Self::insert_solver_user_agent(&mut headers, &solution)?;
        self.store_session_cache(&request.url, &solution).await;
        Ok(EngineResponse {
            url: solution.final_url,
            status: StatusCode::OK,
            headers,
            body: Bytes::new(),
        })
    }

    /// Builds response headers from the cookies solved by the embedded
    /// browser.
    fn response_headers(solution: &ObscuraSolveResult) -> Result<HeaderMap, ArachneaHttpError> {
        if !solution
            .cookies
            .iter()
            .any(|cookie| cookie.name == "cf_clearance")
        {
            return Err(ArachneaHttpError::CookieAbsent {
                origin: solution.origin.clone(),
                name: "cf_clearance".to_string(),
            });
        }
        let mut headers = HeaderMap::new();
        for cookie in &solution.cookies {
            headers.append(
                SET_COOKIE,
                HeaderValue::from_str(&set_cookie_header(cookie))
                    .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
            );
        }
        Ok(headers)
    }

    /// Adds the browser user-agent observed on the solve page to solver
    /// metadata.
    fn insert_solver_user_agent(
        headers: &mut HeaderMap,
        solution: &ObscuraSolveResult,
    ) -> Result<(), ArachneaHttpError> {
        let Some(user_agent) = solution
            .user_agent
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(());
        };
        headers.insert(
            SOLVER_USER_AGENT_HEADER,
            HeaderValue::from_str(user_agent)
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
        );
        Ok(())
    }

    /// True when the browser cookie jar holds a `cf_clearance` cookie.
    fn has_clearance_cookie(browser: &Browser) -> bool {
        browser
            .cookies()
            .get_all()
            .iter()
            .any(|cookie| cookie.name == "cf_clearance")
    }

    /// Samples non-secret challenge markers from the retained document.
    fn sample_challenge_signals(page: &Page) -> ChallengeSignals {
        ChallengeSignals::from_value(&page.evaluate(CHALLENGE_SIGNAL_JS))
    }

    /// Attempts one bounded click on a known same-origin Turnstile widget.
    ///
    /// Only well-known widget containers are targeted; Cloudflare's checkbox
    /// usually lives in a cross-origin frame that the Rust API cannot reach,
    /// so this is a best-effort supplement for same-origin widgets.
    fn try_click_turnstile(page: &Page) {
        for selector in [
            ".cf-turnstile",
            "#turnstile-wrapper",
            "input[name=\"cf-turnstile-response\"]",
        ] {
            let Some(element) = page.query_selector(selector) else {
                continue;
            };
            if element.click().is_ok() {
                return;
            }
        }
    }

    /// Converts all browser cookies into the shared structured form.
    fn structured_cookies(browser: &Browser) -> Vec<StructuredCookie> {
        browser
            .cookies()
            .get_all()
            .into_iter()
            .map(|cookie| structured_cookie(&cookie))
            .collect()
    }

    /// Reads the effective user-agent reported by the page runtime.
    fn observe_user_agent(page: &Page) -> Option<String> {
        let value = page.evaluate("navigator.userAgent");
        let Some(user_agent) = value.as_str().filter(|value| !value.trim().is_empty()) else {
            return None;
        };
        Some(user_agent.to_string())
    }
}

#[async_trait]
impl HttpEngine for ObscuraEngine {
    fn name(&self) -> &'static str {
        ENGINE_NAME
    }

    /// Executes a normalized GET/HEAD request through the embedded browser.
    ///
    /// A fresh stealth page navigates to the target, waits for `cf_clearance`
    /// with the bounded clearance protocol, then returns the stable page HTML
    /// for GET (empty body for HEAD). Cookies and the observed user-agent are
    /// the only values exposed to the normalized response; the session is
    /// persisted in the shared cache when a clearance was issued.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for non-GET/HEAD methods, and
    /// `ObscuraFailure` when the embedded browser cannot navigate or solve.
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        Self::reject_unsupported_methods(&request, "non-GET/HEAD HTTP requests")?;
        let solution = self
            .solve(&request.url, request.method == Method::GET)
            .await?;
        let mut headers = Self::response_headers(&solution)?;
        Self::insert_solver_user_agent(&mut headers, &solution)?;
        self.store_session_cache(&request.url, &solution).await;

        let mut body = Bytes::new();
        if let Some(html) = &solution.html {
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
            body = Bytes::from(html.clone());
        }
        Ok(EngineResponse {
            url: solution.final_url,
            status: StatusCode::OK,
            headers,
            body,
        })
    }

    /// Refreshes Cloudflare state, reusing the persistent session cache when
    /// it holds a still-valid clearance.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for non-GET/HEAD methods, and
    /// `ObscuraFailure` when the embedded browser cannot solve a fresh
    /// clearance.
    async fn refresh_cloudflare(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.refresh_cloudflare_with_cache_policy(request, true)
            .await
    }

    /// Refreshes Cloudflare state while bypassing the persistent session
    /// cache.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for non-GET/HEAD methods, and
    /// `ObscuraFailure` when the embedded browser cannot solve a fresh
    /// clearance.
    async fn refresh_cloudflare_fresh(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.refresh_cloudflare_with_cache_policy(request, false)
            .await
    }

    /// Opens a persistent page session on the embedded browser.
    ///
    /// The page and its browser live on a dedicated thread for the whole
    /// session lifetime (the Obscura runtime is thread-affine, `!Send`), and
    /// every interaction travels through a command channel. The session is
    /// closed by [`BrowserPageSession::close`] or by the
    /// `BrowserSessionManager` eviction path, both of which join the thread.
    ///
    /// # Errors
    ///
    /// Returns `ObscuraFailure` when the page thread or the embedded browser
    /// cannot be started.
    async fn open_browser_page_session(
        &self,
    ) -> Result<Box<dyn BrowserPageSession>, ArachneaHttpError> {
        let channel =
            ObscuraSessionChannel::spawn(self.config.clone(), self.transport.clone()).await?;
        Ok(Box::new(ObscuraPageSession { channel }))
    }
}

/// One ephemeral solver run: cookies, user-agent, final URL and optional HTML.
struct ObscuraSolveResult {
    /// Normalized origin key the solution belongs to.
    origin: String,
    /// Final URL observed on the retained page.
    final_url: String,
    /// Structured cookies observed by the embedded browser.
    cookies: Vec<StructuredCookie>,
    /// User-Agent reported by the page's `navigator.userAgent`.
    user_agent: Option<String>,
    /// Page HTML when the caller asked for a body (GET sends).
    html: Option<String>,
}

/// Converts one facade cookie into the shared structured form.
///
/// The Obscura facade does not expose `Expires`/`SameSite`, so these fields
/// stay absent and the session cache relies on its no-expiry TTL bound.
fn structured_cookie(cookie: &ObscuraCookie) -> StructuredCookie {
    StructuredCookie {
        name: cookie.name.clone(),
        value: cookie.value.clone(),
        domain: Some(cookie.domain.clone()),
        path: Some(cookie.path.clone()),
        expires: None,
        http_only: Some(cookie.http_only),
        secure: Some(cookie.secure),
        same_site: None,
    }
}

#[cfg(test)]
mod tests {
    use super::ObscuraEngine as LocalObscuraEngine;
    use super::{
        in_page_fetch_script, page_fetch_response_from_outcome, structured_cookie,
        ChallengeSignals, InPageFetchOutcome, ObscuraEngineConfig, ObscuraTransportMode,
    };
    use crate::chaser_session::set_cookie_header;
    use crate::config::{ArachneaHttpConfig, HttpProxyConfig};
    use http::StatusCode;
    use obscura::Cookie as ObscuraCookie;
    use serde_json::Value;
    use std::collections::HashMap;

    /// `Disabled` proxy config selects the browser stack without proxy URL.
    #[test]
    fn disabled_proxy_selects_direct_network_transport() {
        assert_eq!(
            ObscuraTransportMode::select(&HttpProxyConfig::Disabled),
            ObscuraTransportMode::NetworkProxy(None)
        );
    }

    /// Explicit network proxy URLs flow into the transport selection as-is.
    #[test]
    fn network_proxy_url_is_carried_into_the_transport() {
        assert_eq!(
            ObscuraTransportMode::select(&HttpProxyConfig::Network(
                "socks5h://127.0.0.1:9050".to_string()
            )),
            ObscuraTransportMode::NetworkProxy(Some("socks5h://127.0.0.1:9050".to_string()))
        );
    }

    /// Transport labels stay generic so logs never embed proxy URLs or
    /// credentials.
    #[test]
    fn transport_labels_stay_generic() {
        assert_eq!(
            ObscuraTransportMode::NetworkProxy(Some("http://user:pass@host:1".to_string())).label(),
            "network-proxy"
        );
        assert_eq!(
            ObscuraTransportMode::InterceptorFulfill.label(),
            "interceptor-fulfill"
        );
    }

    /// The facade cookie conversion keeps the fields Obscura exposes and
    /// leaves `Expires`/`SameSite` absent, so the cache falls back to its
    /// no-expiry TTL bound.
    #[test]
    fn cookie_conversion_keeps_facade_fields_and_drops_unknown_attributes() {
        let cookie = ObscuraCookie::new("cf_clearance", "opaque-value", "example.com");
        let structured = structured_cookie(&cookie);
        assert_eq!(structured.name, "cf_clearance");
        assert_eq!(structured.value, "opaque-value");
        assert_eq!(structured.domain, Some("example.com".to_string()));
        assert_eq!(structured.path, Some("/".to_string()));
        assert!(structured.expires.is_none());
        assert!(structured.same_site.is_none());
        // The synthesized Set-Cookie header carries the cookie but never an
        // Expires/SameSite attribute for the facade-provided fields.
        let header = set_cookie_header(&structured);
        assert!(header.starts_with("cf_clearance=opaque-value"));
        assert!(!header.contains("Expires="));
        assert!(!header.contains("SameSite="));
    }

    /// Boolean signal markers drive challenge detection; the summary stays
    /// label-only and never echoes document values.
    #[test]
    fn challenge_signals_are_detected_and_summarized_without_values() {
        let value = serde_json::from_str::<Value>(
            r#"{"cfChallengeFrame":true,"turnstileResponse":false,"cfResponse":true,"interstitialMarker":false,"challengeTitle":false}"#,
        )
            .expect("signal payload decodes");
        let signals = ChallengeSignals::from_value(&value);
        assert!(signals.suggests_challenge());
        let summary = signals.summarize();
        assert!(summary.contains("cf-challenge-frame"));
        assert!(summary.contains("cf-response-field"));
        assert!(!summary.contains("true"));
    }

    /// Absent markers never suggest a challenge.
    #[test]
    fn empty_challenge_signals_do_not_suggest_a_challenge() {
        let value = serde_json::from_str::<Value>(r#"{}"#).expect("empty payload decodes");
        let signals = ChallengeSignals::from_value(&value);
        assert!(!signals.suggests_challenge());
        assert_eq!(signals.summarize(), "no challenge markers");
    }

    /// The stealth transport refuses SOCKS proxies instead of silently
    /// bypassing the route, and refuses the Phase 4 interceptor for now.
    #[test]
    fn proxy_url_validation_rejects_unsupported_transports() {
        let engine = test_engine(ObscuraTransportMode::NetworkProxy(Some(
            "socks5h://127.0.0.1:9050".to_string(),
        )));
        match LocalObscuraEngine::browser_proxy_url_for(&engine.transport) {
            Ok(_) => panic!("SOCKS proxies must be rejected"),
            Err(err) => assert!(err.to_string().contains("unsupported Obscura proxy scheme")),
        }

        let engine = test_engine(ObscuraTransportMode::InterceptorFulfill);
        match LocalObscuraEngine::browser_proxy_url_for(&engine.transport) {
            Ok(_) => panic!("the Phase 4 interceptor must not be selected yet"),
            Err(err) => assert!(err.to_string().contains("Phase 4")),
        }
    }

    /// HTTP(S) network proxy URLs flow into the browser configuration as-is.
    #[test]
    fn proxy_url_validation_accepts_http_proxies() {
        let engine = test_engine(ObscuraTransportMode::NetworkProxy(Some(
            "http://127.0.0.1:8080".to_string(),
        )));
        assert_eq!(
            LocalObscuraEngine::browser_proxy_url_for(&engine.transport)
                .expect("http proxy must validate"),
            Some("http://127.0.0.1:8080".to_string())
        );
    }

    /// A successful in-page fetch outcome converts into the client-facing
    /// response with headers, status, body, and the page-reported final URL.
    #[test]
    fn successful_in_page_fetch_outcome_builds_a_response() {
        let outcome = InPageFetchOutcome {
            url: Some("https://example.com/api/final".to_string()),
            status: Some(200),
            headers: Some(HashMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-multi".to_string(), "a".to_string()),
            ])),
            body: Some("{\"ok\":true}".to_string()),
            error: None,
        };
        let response = page_fetch_response_from_outcome(outcome, "https://example.com/api")
            .expect("outcome converts");
        assert_eq!(response.url, "https://example.com/api/final");
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.body, "{\"ok\":true}");
        assert_eq!(response.headers.get("x-multi").unwrap(), "a");
        assert_eq!(
            response.headers.get("content-type").unwrap(),
            "application/json"
        );
    }

    /// Network-layer errors surface as `PageFetchFailed`; a missing status is
    /// also rejected instead of silently becoming a response.
    #[test]
    fn failing_in_page_fetch_outcomes_are_rejected() {
        let outcome = InPageFetchOutcome {
            error: Some("NetworkError when attempting to fetch resource.".to_string()),
            ..InPageFetchOutcome::default()
        };
        let err = page_fetch_response_from_outcome(outcome, "https://example.com/api")
            .expect_err("network errors must surface");
        assert!(err.to_string().contains("in-page fetch failed"));

        let outcome = InPageFetchOutcome {
            body: Some("orphan".to_string()),
            ..InPageFetchOutcome::default()
        };
        page_fetch_response_from_outcome(outcome, "https://example.com/api")
            .expect_err("an outcome without status must be rejected");
    }

    /// The in-page fetch launcher embeds the payload once, parks the result in
    /// a page global, and flags completion for the session worker.
    #[test]
    fn in_page_fetch_script_parks_its_result_and_flags_completion() {
        let payload = serde_json::json!({
            "method": "POST",
            "url": "https://example.com/api",
            "headers": {},
            "body": "payload-body",
        });
        let script = in_page_fetch_script(&payload);
        assert!(script.contains("\"body\":\"payload-body\""));
        assert!(script.contains("\"method\":\"POST\""));
        assert!(script.contains("const input = "));
        assert!(script.contains("globalThis.__arachneaObscuraFetchDone = false"));
        assert!(script.contains("credentials: 'same-origin'"));
        assert!(script.contains("globalThis.__arachneaObscuraFetchDone = true"));
    }

    /// Builds an engine for transport tests without touching the network.
    fn test_engine(transport: ObscuraTransportMode) -> LocalObscuraEngine {
        let config = ArachneaHttpConfig::default();
        LocalObscuraEngine {
            config: ObscuraEngineConfig::from_arachnea(&config),
            transport,
            session_cache: None,
        }
    }
}
