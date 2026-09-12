# arachnea-http

`arachnea-http` provides outbound HTTP client utilities for Arachnea. It exposes a single client facade over `rquest` and optional Cloudflare-oriented engines.

This README is the design and usage home for the HTTP crate. Remaining work is tracked in the root `docs/TODO.md`.

## Design Contract

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
- Optional Cloudflare-oriented flows using Ghostwire, chaser-cf, the embedded Obscura headless browser, or a Tauri/Wry-based solver path.
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
| `CloudflareBrowserSolverKind::Obscura` | Require the `obscura` feature for the embedded Obscura headless-browser fallback. Phase 2: the Cloudflare session solver (`send`, `refresh_cloudflare`, `refresh_cloudflare_fresh`) is implemented on an ephemeral stealth page with the shared `CachedChaserSession` cache; persistent page sessions land in Phase 3 (`UnsupportedEngineOperation`). `HttpProxyConfig::Arachnea` is refused until the Phase 4 in-process transport lands, and only `http`/`https` network proxies are accepted. |

Cloudflare detection should require supporting signals such as `server: cloudflare`, `cf-ray`, `cf-mitigated`, `cf-` or `__cf` headers/cookies, or known challenge markers. A plain `403` is not enough by itself.
When `Auto` reaches the browser fallback after an active Cloudflare block, the browser solver is asked for a fresh solve instead of reusing an engine-specific session cache.
The chaser-cf integration is a session solver only: it delegates challenge handling to `ChaserCF::solve_waf_session`, then returns `cf_clearance` cookies and the browser-observed user-agent to the shared caches. `rquest` always performs the subsequent HTML request, redirects, and response handling.
The Obscura integration (Phase 2) solves Cloudflare sessions with an embedded headless browser (no Chrome/Chromium, no external process): each solve runs on a dedicated thread because the Obscura page runtime is thread-affine, observes non-secret challenge markers, and returns the stable page HTML for `send` plus `cf_clearance` cookies and the observed user-agent to the shared caches.
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

- The `chaser-cf` engine is a Cloudflare session solver and supports persistent page sessions through a retained Chrome page (`ChaserCfPageSession`).
- The `obscura` engine is a Cloudflare session solver (Phase 2); persistent page sessions are not implemented yet (Phase 3).
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
| `ghostwire` | Enables the Ghostwire engine and is active by default. |
| `chaser-cf` | Enables the chaser-cf Cloudflare solver integration. |
| `tauri-cloudflare-solver` | Enables the Tauri/Wry-based solver path. |
| `tauri-cloudflare-solver-devtools` | Enables Tauri solver support with Wry devtools. |

Use `--no-default-features` to build the `rquest`-only client.

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
