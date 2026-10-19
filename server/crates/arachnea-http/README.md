# arachnea-http

`arachnea-http` provides outbound HTTP client utilities for Arachnea. It exposes a single client facade over `rquest` and optional Cloudflare-oriented engines.

This README is the design and usage home for the HTTP crate. Remaining work is tracked in the root `docs/TODO.md`.

## Design Contract

### Correlated proxy response policies

With `arachnea-proxy`, `ArachneaHttpConfig::builder().proxy_response_classifier(policy)`
accepts a `ProxyResponsePolicy` from `arachnea-proxy::core::http::response_classifier`.
Rquest evaluates it on original headers and an optional prefix before full body
materialization. The view includes the original URL, effective hop URL, redirect
targets, actual method and exact selection token. The default accepts only 2xx;
unconfigured errors/redirects are inconclusive. Callers inspect
`ArachneaResponse::proxy_response_decision()` and apply effects with the token;
classification does not itself retry or mutate inventory.

Custom synchronous policies must be nonblocking and side-effect-free. Optional
prefixes are capped at 64 KiB and three seconds or the remaining request timeout,
whichever is shorter. Every consumed byte is replayed into the materialized body.
Incomplete or unavailable prefixes are explicitly identified, and policy errors
are logged as inconclusive. The public response remains materialized; no promise
of an end-to-end search deadline or streaming public HTTP API is added here.
The existing Rquest dependency enables its `stream` feature for this boundary.
Browser/custom engines do not expose equivalent early correlated observations:
their decisions remain absent, with a warning when a policy is configured.

- Keep the public API centered on `ArachneaHttpClient`.
- Keep HTTP engine choice explicit and observable through configuration and logs.
- Keep Cloudflare behavior explicit through the default request mode or a per-request mode.
- `HttpRequestMode::Direct` must never launch a Cloudflare solver.
- `HttpRequestMode::Auto` starts with `rquest` and uses only solver engines that are compiled in or explicitly injected.
- Keep cookies in a shared in-memory cache for all client instances in the process.
- Keep engine-specific API differences isolated in `src/engine/`.
- Keep logs, errors, and traces from exposing cookie values by default.
- Do not promise universal Cloudflare bypass. Turnstile, visual captchas, WAF 1020, behavioral checks, and account-level controls may require external or human handling.

## Main Capabilities

When the application uses SQLite session persistence, the `cloudflare-session`
table lives in `<application-data>/data/cloudflare-session.sqlite3`, with adjacent
SQLite WAL/SHM files when present. Legacy `cloudflare-session/records.sqlite3*`
databases are ignored without importing their sessions or deleting the old files.
Sessions saved in the new layout survive subsequent restarts.

- `ArachneaHttpClient` as the main request facade.
- `ArachneaHttpClient::default().await?` as the default-configuration constructor.
- Common HTTP methods, custom methods, request options, and typed responses.
- Shared in-memory cookie cache with Cloudflare cookie-state helpers.
- Callers can inspect the current cookie view for one URL through the shared cache when a source-specific login flow needs to reuse redirect-set cookies explicitly.
- Configurable engine selection through `HttpEngineKind`.
- Configurable outbound proxy routing through `HttpProxyConfig`.
- Configurable default request behavior through `HttpRequestMode`.
- Facade-managed redirects with cookies stored after every response in the chain.
- `BrowserProfile::Custom` for caller-provided user-agent values.
- JSON response parsing through `ArachneaResponse::json<T>()`.
- Text header conversion through `header_map_from_strings`.
- Per-request mode overrides through `ArachneaRequestBuilder::mode`.
- Origin-scoped reusable browser page sessions through `ArachneaHttpClient::page_fetch` when the configured browser engine supports persistent pages.
- Optional Cloudflare-oriented flows using Ghostwire, chaser-cf, or a Tauri/Wry-based solver path.
- Typed errors for network, status, Cloudflare, cookie, and configuration failures.

This crate is intended for authorized traffic only. Callers must respect target service terms, rate limits, `robots.txt`, and applicable law.

## Engine Behavior

| Engine or mode | Behavior |
| --- | --- |
| `HttpRequestMode::Auto` | Default mode. Start with `rquest`; when Cloudflare is detected, try Ghostwire if the `ghostwire` feature or an injected smart solver is available, then try the configured browser solver if available. |
| `HttpRequestMode::Direct` | Use `rquest` only. No Cloudflare solver is launched. |
| `HttpRequestMode::CloudflareSmart` | Reuse a valid `cf_clearance` cookie when present. If missing or stale, refresh with Ghostwire when the feature is enabled or with an injected smart solver, then execute through `rquest`. |
| `HttpRequestMode::CloudflareBrowser` | Reuse a valid Cloudflare cookie when present. If missing or stale, refresh with chaser-cf, Tauri/Wry, or an injected browser solver, then execute through `rquest`. |
| `CloudflareBrowserSolverKind::Auto` | Use the default browser solver selected by `get_default_cloudflare_solver`: Tauri/Wry when `tauri-cloudflare-solver` is enabled, otherwise chaser-cf when `chaser-cf` is enabled. |
| `CloudflareBrowserSolverKind::ChaserCf` | Require the `chaser-cf` feature for the browser fallback. |
| `CloudflareBrowserSolverKind::TauriCloudflareSolver` | Require the `tauri-cloudflare-solver` feature for the interactive browser fallback. |

Cloudflare detection should require supporting signals such as `server: cloudflare`, `cf-ray`, `cf-mitigated`, `cf-` or `__cf` headers/cookies, or known challenge markers. A plain `403` is not enough by itself.
When `Auto` reaches the browser fallback after an active Cloudflare block, the browser solver is asked for a fresh solve instead of reusing an engine-specific session cache.
The chaser-cf integration is a session solver only: it delegates challenge handling to `ChaserCF::solve_waf_session`, then returns `cf_clearance` cookies and the browser-observed user-agent to the shared caches. `rquest` always performs the subsequent HTML request, redirects, and response handling.
If the rquest handoff remains blocked, the client returns `CloudflareBlocked` after its bounded refresh policy; it does not retrieve HTML through the browser as a fallback. The solver and rquest must use the same proxy route, and clearance portability is target-dependent.
If the browser solve succeeds but the `rquest` cookie handoff is still blocked, the client returns `CloudflareBlocked` after its bounded refresh attempts. Chaser-CF is a session solver only and never returns HTML.
Callers can force the browser solver with `ArachneaHttpConfig::builder().cloudflare_browser_solver(...)` or provide an engine instance with `cloudflare_browser_solver_instance(...)`.
Injected custom engines keep full control over their own transport setup; the facade-level proxy configuration applies to built-in engines only.

## Proxy Configuration

`ArachneaHttpConfig` can now route built-in engines through either an explicit network proxy URL or, when the `arachnea-proxy` feature is enabled, an in-process `arachnea-proxy` core:

```rust
use arachnea_http::ArachneaHttpConfig;

let config = ArachneaHttpConfig::builder()
    .proxy_url("socks5h://127.0.0.1:9050")
    .build()?;
```

With `--features arachnea-proxy`, the built-in engines can also reuse an in-process `arachnea-proxy` core:

```rust
use arachnea_http::ArachneaHttpConfig;
use arachnea_proxy::core::{ArachneaProxyCore, UsageProfile};

let proxy_core = ArachneaProxyCore::new(UsageProfile::SystemRelay.config())?;
let config = ArachneaHttpConfig::builder()
    .proxy_core(proxy_core)
    .build()?;
```

Current upstream client APIs still expose proxy URLs rather than a stable public transport replacement hook, so the built-in `rquest` and Ghostwire engines both use the managed loopback compatibility helper when `proxy_core(...)` is selected. Chaser-CF receives a separate parameter-bound loopback listener so Chrome CONNECT requests use the same Arachnea proxy routing context as `rquest`.

The built-in `rquest` engine applies `request_timeout` to every request,
including clients supplied by the loopback proxy helper. The deadline covers
connection setup and response body collection; an established proxy tunnel
does not allow a stalled upstream to wait indefinitely. Proxy-path timeouts
remain classified as proxy errors for the caller's retry handling.

Use `.mode(...)` on a request builder to override the configured default for one call:

```rust
let response = client
    .get("https://example.com/protected")
    .mode(HttpRequestMode::CloudflareSmart)
    .send()
    .await?;
```

## Cookie Cache

- The cache is global to the crate process and memory-only.
- Cookie entries track name, value, domain, path, expiration, `Secure`, `HttpOnly`, and SameSite data.
- `Max-Age` wins over `Expires`.
- Session cookies use configurable temporary cache lifetime.
- Cloudflare cookies of special interest include `cf_clearance`, `__cf_bm`, and `_cfuvid`.
- Cookies must be refreshed when absent, expired, close to expiration, explicitly forced, or after a configured Cloudflare block.
- Redirect responses are processed by the facade so intermediate `Set-Cookie` headers are stored before the next request is sent.

## Browser Page Sessions

`ArachneaHttpClient::page_navigate(PageNavigationRequest)` navigates a persistent
browser page and can return its stable HTML source. `ArachneaHttpClient::page_click(PageNavigationRequest, PageClickRequest)` navigates, clicks a CSS-selected element, then returns the rendered HTML when a result selector appears. `ArachneaHttpClient::page_fetch(PageNavigationRequest, PageFetchRequest)` navigates that same kind of page to a source URL, then executes one JavaScript `fetch()` inside it. Sessions are scoped to origin, user-agent profile, and proxy route; they are bounded by `BrowserSessionConfig` and evicted after inactivity.

`ArachneaHttpClient::close_browser_session(url)` invalidates and closes the
retained page for the URL origin when a query-scoped browser workflow finishes.

### Session lifecycle

1. **Creation**: On the first `page_fetch` call for an origin, the session manager calls the engine's `open_browser_page_session()`. The page handle is cached in a `BrowserSessionHandle` behind the `BrowserSessionManager` (in-memory, keyed by origin + profile + proxy route).
2. **Navigation**: Each browser operation navigates the retained page to `PageNavigationRequest.url`. `page_navigate` returns stable HTML when `collect_body` is enabled. A stable non-interstitial HTML page is usable for page interactions even when it does not issue a Cloudflare `cf_clearance` cookie. Browser cookies and the observed user-agent are handed back to the shared HTTP caches via `store_browser_session_metadata()`.
3. **Click workflow**: `page_click` invokes the selected page element and waits for its result selector. This lets a page own its CAPTCHA callback, form submission, and DOM update without exposing the token to the caller.
4. **Token capture**: If `PageFetchRequest.turnstile_token_placeholder` is set, `page_fetch` reads an application Turnstile callback token from the page. With `reuse_turnstile_token: true`, the token is cached on the handle for the session's lifetime and reused without re-reading the page.
5. **Fetch**: A JavaScript `fetch()` is executed within the page context with the configured method, URL, headers, and body. The token placeholder is replaced in memory (never logged).
6. **Token-rejection classification**: If the fetch response matches `token_rejection_statuses` or `token_rejection_body_markers`, the client returns `TokenRejected`. The session and its page are preserved — the caller may retry with a fresh token.
7. **Session invalidation**: Any non-token error (engine failure, network error, etc.) invalidates the entire session for that origin. The page is closed and the handle is removed. A subsequent call will create a fresh session.
8. **Eviction**: Idle sessions are evicted after `BrowserSessionConfig::idle_timeout`. When at capacity (`max_sessions`), the least-recently-used session is evicted first (after idle candidates).

### Security rules

- Turnstile callback tokens are held only in memory, never serialized, logged, or exposed in extracted data or diagnostics.
- Token `Debug` is redacted to `<redacted>`.
- Two distinct origins never share the same token cache.
- Cookies from browser sessions are handed back to the existing in-memory HTTP cache; raw cookie values are not logged.
- The `BrowserSessionKey` includes profile and proxy route to prevent token reuse across different browser fingerprints or network paths.

### Configuration

```rust
use arachnea_http::browser::BrowserSessionConfig;

let config = ArachneaHttpConfig::builder()
    .browser_session(BrowserSessionConfig {
        max_sessions: 8,
        idle_timeout: Duration::from_secs(300),
    })
    .build()?;
```

### Engine support

- The `chaser-cf` engine is a Cloudflare session solver and does not support persistent page sessions.
- Use another explicit browser engine for page-scoped JavaScript work, callbacks, or DOM interactions.
- Engines without this capability return `UnsupportedEngineOperation` rather than silently falling back to direct HTTP.
- `BrowserPageSession::clear_turnstile_token()` has a default no-op implementation.
- Browser contexts are resource-intensive. Keep `max_sessions` small and invalidate an origin when its session is known to be invalid.

### Error contract for callers

| Error variant | Meaning | Session state |
|---|---|---|
| `TokenAbsent` | The page callback did not produce a token | Invalidated |
| `TokenRejected` | The fetch response matched rejection signals | Preserved (caller may retry once) |
| `PageInteractionFailed` | Click target or result selector failed | Invalidated |
| `BrowserSessionUnavailable` | Session was invalidated or manager is disabled | — |
| `CloudflareSolverUnavailable` | No browser engine is configured | — |
| `ChaserCfFailure` / other engine errors | Engine-level failure | Invalidated |

## Redirects

- `ArachneaHttpConfig::max_redirects(None)` uses the crate default of 16 redirects.
- `Some(0)` disables redirect following and returns the first redirect response.
- `Some(n)` follows up to `n` redirects.
- `303`, `302`, and `301` responses rewrite non-GET and non-HEAD requests to `GET`; `307` and `308` preserve the original method and body.

## Features

| Feature | Purpose |
| --- | --- |
| `arachnea-proxy` | Enables `arachnea-proxy` integration through a managed loopback compatibility helper shared by built-in engines. |
| `ghostwire` | Enables the optional Ghostwire engine. |
| `chaser-cf` | Enables the chaser-cf Cloudflare solver integration. |
| `tauri-cloudflare-solver` | Enables the Tauri/Wry-based solver path. |
| `tauri-cloudflare-solver-devtools` | Enables Tauri solver support with Wry devtools. |

The default feature set is empty, so the crate builds as an `rquest`-only client unless a solver feature is enabled explicitly.

## Known Gaps

The durable remaining work from the retired specification is tracked in the root `docs/TODO.md`. Important HTTP gaps include deeper no-loopback engine integrations where upstream APIs allow it, Wry/Tauri and web-server challenge flows, dependency API verification, a possible `tower::Service` API, deeper browser-driven captcha flows, and expanded cookie/Cloudflare tests.

## Common Commands

Check the crate with the default engine set:

```powershell
cargo check -p arachnea-http
```

Check the `rquest`-only path:

```powershell
cargo check -p arachnea-http --no-default-features
```

Check the chaser-cf feature:

```powershell
cargo check -p arachnea-http --no-default-features --features chaser-cf
```

Run the hybrid example when all required optional engines are enabled:

```powershell
cargo run -p arachnea-http --features ghostwire,chaser-cf,tauri-cloudflare-solver --example cloudflare_hybrid
```

## Useful Paths

- `src/client.rs`: main client facade and request builder.
- `src/config.rs`: engine, browser profile, and Cloudflare configuration.
- `src/cookies.rs`: shared in-memory cookie cache.
- `src/cloudflare.rs`: Cloudflare detection and cookie state helpers.
- `src/engine/`: engine abstraction and concrete engine adapters.
- `examples/`: runnable usage examples.

### Correlated Arachnea proxy attempts

The built-in Rquest path resolves and pins the route before every attempt, including
each redirect hop. Reusable clients are isolated by origin, endpoint and resolved
route identity; the bounded cache holds at most 32 entries. Active attempts retain
their loopback helper even if a concurrent cache replacement occurs.
`ArachneaResponse::proxy_selection()` exposes the exact dynamic selection.
Rquest records response receipt before collecting the body, independently of the
HTTP status. Lot 3 adds the injectable response classifier at this boundary;
localized calls share the search deadline and replay policy below.

`ArachneaHttpError::ProxyAttempt` retains the token and delivery evidence:
preselection failures are `NotSent`, opaque Rquest send failures are `PossiblySent`,
and failures collecting a body after headers are `ResponseReceived`. Body collection
errors do not invalidate another endpoint or trigger Scrapyfy transport rotation.
Browser/custom-engine responses without reliable correlation expose no token and
must not create a dynamic preference or exclusion by destination-map inference.


## Localized proxy search and safe retries

Localized Rquest attempts log the selected proxy endpoint, destination origin and
search identity before sending. Failures log delivery evidence and replay
eligibility, followed by bounded search counters at retry/final completion.
These diagnostics distinguish selection/probing from the business request;
no request body, cookies or additional full URL is logged by these events.

With the Arachnea transport and non-empty `countries`, the facade creates one
`ProxySearchContext` across selection, redirects and retries. An explicitly scoped
context is reused. Defaults are a 120-second discovery/acceptance target, a
15-second grace period for an engaged origin attempt. There is no count limit on
TCP prechecks, full validations or distinct business endpoints: discovery continues
until acceptance, candidate exhaustion or the shared deadline. Concurrency remains
bounded. Waiting for resources consumes the same deadline. Retry layers never reset it.

Localized Rquest clients cap complete connection establishment (including the
loopback proxy and destination TLS) at 15 seconds, or a shorter configured request
timeout. Native connector timeouts preserve `NotSent` evidence on the first hop;
the outer response-header timeout remains conservative after possible sending.
It emits a correlated `timeout` diagnostic with `phase=response_headers`.

Rquest header/prefix observation uses the remaining origin deadline. Once a final
response is accepted, its body collection uses a separate `request_timeout`
(default 30 seconds), without the search deadline cutting an accepted transfer.
Ordinary requests keep their existing total request timeout. A safe-to-retry
rejected response is released before collecting its body; responses that cannot
be replayed retain body collection within the remaining time. A body collection
error is `ResponseReceived` and never triggers a transport retry.

GET/HEAD may retry relevant correlated transport errors or configured rejections.
Rquest connector errors on the first hop (TCP, proxy CONNECT or destination TLS
setup) prove `NotSent`, allowing a modifying request to select another endpoint
before any business bytes are sent. Outer header/deadline timeouts and failures
after sending remain `PossiblySent`. Connector errors on a redirect hop also
remain conservative for the original operation, which may already have been sent.
Certificate verification remains enabled. Diagnostic categories distinguish
proxy tunnel, TLS certificate/handshake, connection, timeout and request failures
without logging raw source chains or URLs; only `is_connect()` and redirect
history determine the new absence-of-sending evidence.

Other methods require `NotSent` evidence or explicit operation-level authorization
through `ArachneaHttpConfig::builder().proxy_retry_replayable(true)`. Bodies are
reproduced from owned bytes. Rejection status alone does not authorize replay.
Inconclusive results return without rotation. Destination observations remain
attached to the exact token and never establish global proxy failure.

Contextual loopbacks live only as long as the call; they are excluded from the
cross-call pinned-client cache, so these calls reconnect while still reusing
accepted destination preferences. Ordinary non-search keep-alive clients are
unchanged. Browser/custom engines without early route correlation return
`UnsupportedEngineOperation` within this scoped search, rather than bypassing
its limits or inferring a proxy result. External API gateways must allow the
135-second search horizon plus the separate accepted-body transfer policy.
