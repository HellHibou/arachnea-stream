# arachnea-http

`arachnea-http` provides outbound HTTP client utilities for Arachnea. It exposes a single client facade over `rquest` and optional Cloudflare-oriented engines.

This README is the design and usage home for the HTTP crate. Remaining work is tracked in the root `docs/TODO.md`.

## Source organization

- `src/client.rs` owns the public client facade. Private modules under `src/client/` handle request/response types, execution, proxy runtime ownership, URLs, Cloudflare refresh and reusable browser sessions. Existing public paths under `arachnea_http::client` remain available through reexports.
- `src/config.rs` owns configuration types; `src/config/builder.rs` owns construction and validation.
- `src/engine/chaser_cf/` separates challenge observation, page sessions and the private `session_cache.rs` implementation of typed session persistence, expiration and cookie conversion. Its existing cache tests mirror that module under `tests/engine/chaser_cf/session_cache.rs`. `src/engine/tauri_cloudflare/` separates application-owned windows from the standalone event loop. Engine and platform feature gates remain in effect.
- `src/lib.rs` centralizes documented constants and relevant defaults, preserving their values and existing public constant paths. Mutable caches and registries stay with their owners.
- Crate-owned JavaScript assets and Rust format templates live under `resources/chaser_cf/` and `resources/tauri_cloudflare/`. They are included at compile time; deployment does not require copying these directories. Format templates retain Rust placeholder and brace escaping.
- Existing tests mirror source paths under crate-root `tests/`; shared mocks live under `tests/support/`. Private-access suites are loaded using conditional `#[path]` declarations. `autotests = false` prevents Cargo from also discovering these files as incompatible integration targets.

From `server/`, run `cargo test -p arachnea-http` for the default configuration or `cargo test -p arachnea-http --all-features` for all compiled engines and proxy coverage. The existing suites use mocks, persistence fixtures and local transport fixtures; they do not qualify live Cloudflare solving or native GUI behavior across platforms.

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

Cloudflare detection considers only `403`, `429`, and `503` responses, and requires supporting signals such as `server: cloudflare`, `cf-ray`, `cf-mitigated`, `cf-` or `__cf` headers/cookies, or known challenge markers. A plain `403` is not enough by itself.
When `Auto` reaches the browser fallback after an active Cloudflare block, the browser solver is asked for a fresh solve instead of reusing an engine-specific session cache.
The chaser-cf adapter implements challenge handling directly on its shared Chrome instance, then returns `cf_clearance` cookies and the browser-observed user-agent to the shared caches. In `CloudflareBrowser` mode, `rquest` performs the subsequent request, redirects, and response handling. When invoked directly as an HTTP engine, `GET` returns the rendered browser HTML, while other methods are rejected except `HEAD` (which returns no body). It also supports persistent browser page sessions through the same shared Chrome process.
If the rquest handoff remains blocked, the client returns `CloudflareBlocked` after its bounded refresh policy; it does not retrieve HTML through the browser as a fallback. The solver and rquest must use the same proxy route, and clearance portability is target-dependent.
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

Current upstream client APIs still expose proxy URLs rather than a stable public transport replacement hook. Built-in Rquest, Ghostwire, Chaser-CF and Tauri/Wry therefore use routing leases on a shared HTTP loopback server when `proxy_core(...)` is selected. Pass an `Arc<arachnea_proxy::server::LoopbackProxyRuntime>` through `.proxy_runtime(runtime)` to share one listening port across multiple HTTP clients. Without an injected runtime, a standalone HTTP client owns its own runtime. Pinned attempts register their selected core on that same server; no additional listening socket is created per attempt or browser profile.

Routing credentials distinguish concurrent client contexts even though the listener address is shared. Keep the HTTP client alive while its engines use their routing lease. Internal proxy URLs contain opaque credentials and must not be logged. Default routing parameters and `Proxy-Authorization` are consumed locally rather than forwarded to the destination. Custom parameter definitions retain their explicit `forward_header` policy.

Tauri/Wry supports the shared routing credentials in both standalone Wry and application-owned Tauri windows. Proxy windows use isolated, non-persistent data stores. Native proxy configuration and authentication are installed before the first remote navigation; proxy credentials are kept separate from origin requests. Automatic browser selection follows the normal feature priority, including for proxy requests. Custom injected engines remain responsible for their transport.

On macOS, proxy mode requires macOS 14 or later. The internal routing URL is adapted to authenticated SOCKS5 on the **same listening port**, so both HTTP and HTTPS pass through the selected core. Explicit network proxies must use SOCKS5 on macOS; an HTTP-only endpoint is rejected. Network.framework direct failover is disabled. The modern native API is loaded only when proxy mode is used, preserving direct-mode startup on older macOS. Windows uses WebView2 proxy authentication and Linux uses WebKitGTK proxy authentication; authenticated SOCKS5 is rejected on Windows, where an HTTP proxy is required instead.

The HTTP facade supplies the effective routing URL automatically. Direct users of `TauriCloudflareSolverEngine` can pass it through `new_with_proxy_url(config, proxy_url)`; `new(config)` accepts explicit network proxies but cannot prepare an in-process core itself.

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
- Cookie path matching is a simple textual prefix check, not the full RFC 6265 path-match algorithm.
- `Domain` attributes are accepted as supplied after removing a leading dot and lowercasing; they are not checked against the response host.
- Matching cookies are emitted in unspecified order because the cache is hash-map backed.
- Redirect responses are processed by the facade so intermediate `Set-Cookie` headers are stored before the next request is sent.

## Browser Page Sessions

Browser APIs provide generic navigation, DOM interactions, in-page requests and
session management. Episode and player identities, resolved embed URLs and their
cache policies belong to `arachnea-stream`. The unused `ResolvedIframeCache`
API has been removed from both `arachnea_http::browser` and the crate root; no
compatibility reexport is provided.

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

- The `chaser-cf` engine supports persistent page sessions through its process-wide shared Chrome instance.
- Use an explicit browser engine for page-scoped JavaScript work, callbacks, or DOM interactions.
- Engines without this capability return `UnsupportedEngineOperation` rather than silently falling back to direct HTTP.
- `BrowserPageSession::clear_turnstile_token()` has a default no-op implementation.
- Browser contexts are resource-intensive. Keep `max_sessions` small and invalidate an origin when its session is known to be invalid. Invalidated handles are marked unusable immediately; do not reuse clones.

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

`HttpEngineKind::Rquest` and `HttpEngineKind::Auto` currently select the same built-in path; only `HttpEngineKind::Engine` changes dispatch by injecting a direct engine. The configured `ArachneaHttpConfig::cloudflare` value is recorded by the builder to choose the initial default request mode, but runtime dispatch reads `default_request_mode`; mutating `cloudflare` after construction does not reconfigure an existing client.

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
their routing lease on the shared loopback server even if a concurrent cache replacement occurs.
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
search identity at DEBUG before sending. Recoverable failures log delivery
evidence and replay eligibility at DEBUG; terminal localized request failure is
WARN. Bounded search counters require `performance-diagnostics` and DEBUG.
These diagnostics distinguish selection/probing from the business request;
no request body, cookies or additional full URL is logged by these events.

With the Arachnea transport and non-empty `countries`, the facade creates one
`ProxySearchContext` across selection, redirects and retries. An explicitly scoped
context is reused. Defaults are a 120-second discovery/acceptance target, a
15-second grace period for an engaged origin attempt. There is no count limit on
TCP prechecks, full validations or distinct business endpoints: discovery continues
until acceptance, candidate exhaustion or the shared deadline. Concurrency remains
bounded. Waiting for resources consumes the same deadline. Retry layers never reset it.

New localized Rquest candidates cap complete connection establishment (including
the loopback proxy and destination TLS) at 15 seconds, or a shorter configured
request timeout. For an exact endpoint/origin with prior acceptance or followed
redirect evidence, application-created pinned clients use a 60-second connection
envelope. Their send/header timeout is the smaller of the configured
`request_timeout` and 60 seconds (25 seconds for Scrapyfy clients). This outer
timer includes connection establishment and can stop the native connection
envelope earlier; Rquest has no separate TLS-only timer here. The token carries
immutable provenance, so another request cannot change this profile. Discovery
keeps its configured send/header timeout. Accepted-body collection still uses
its existing configured `request_timeout` and inspection allowances. The shared
120/135-second search horizon is unchanged. A known proxy that would respond
after the configured header deadline can therefore be abandoned earlier, leaving
the remaining search time for other eligible candidates when replay is allowed.
Native connector timeouts preserve
`NotSent` evidence on the first hop;
the outer response-header timeout remains conservative after possible sending.
It emits a correlated `timeout` diagnostic with `phase=response_headers` and
`timeout_ms` reporting the effective per-attempt allowance.

Cold discovery is coordinated by exact origin and canonical allowed-country set;
waiting consumes each caller's own search budget. Shared success notifications
also wake exploration already waiting on probes/providers. Responses and bodies
remain request-specific, and warm reuse does not serialize transfers. Followed
redirects store separate routing evidence for intermediate origins; they never
attribute final-content acceptance to the starting domain. Ambiguous correlated
transport failures suspend the association for at most 30 seconds, including old
stored `Other` exclusions. Proven tunnel-establishment timeouts use a 120-second
exact-endpoint/origin suspension; a later generic adapter report does not downgrade
it. These rules preserve global endpoint availability and explicit origin rejection
cooldowns. Strictly newer same-origin acceptance clears older `Other`/`Timeout`
exclusions, including restored records. Engine timeout/TLS text heuristics alone
do not enable the longer suspension. Search and per-attempt budgets are unchanged.

Rquest header/prefix observation uses the remaining origin deadline. Final
acceptance is committed once per token before body collection and wakes waiting
callers; a later body-transfer error does not erase that acceptance. Once a final
response is accepted, its body collection uses a separate `request_timeout`
(default 30 seconds), without the search deadline cutting an accepted transfer.
Ordinary requests keep their existing total request timeout. A safe-to-retry
rejected response is released before collecting its body; responses that cannot
be replayed retain body collection within the remaining time. A body collection
error is `ResponseReceived` and never triggers a transport retry. It releases
cold-discovery ownership even if the caller retains the attributed error/token.

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

Completed, accepted Rquest attempts may retain an idle transport for subsequent
compatible calls. A fully consumed 301/302/303/307/308 response classified as
inconclusive can additionally retain its transport only when the active policy
explicitly permits redirect transport reuse and the redirect loop confirms a
valid HTTP(S) target within its quota. The default status classifier permits
this; custom `ProxyResponseClassifier` implementations opt out through the
default `allows_redirect_transport_reuse() == false`. A configured rejection,
classifier/inspection error, incomplete transfer, missing/invalid target or
unfollowed redirect does not qualify. Retaining a redirect transport never
accepts its content or creates a final-content preference. The unchanged TF1
content callback therefore retains its previous behavior, including rejection
of HTTP 200 geo-block responses.

A private, non-cloneable step envelope owns the pending transport until redirect
following is confirmed; the public `ArachneaResponse` remains cloneable without
owning it. Cancellation before admission drops that transport. After admission,
it follows the normal pool lifetime even if a later request to another origin
fails or is cancelled. Idle age starts at complete body collection; limits remain
32 idle entries and 60 seconds. DEBUG `retaining HTTP transport after followed
proxy redirect` records admission without signed URL paths.

Every call first selects and validates its route again, then
exclusively checks out and rebinds the transport to its fresh selection token and
search deadline. Existing tunnels can be reused; reconnects use the current core.
Concurrent calls never share a checked-out transport or wait for it: they can
create separate transports within the existing resource limits. Errors,
inconclusive responses outside the explicitly permitted followed-redirect case,
rejections and incomplete bodies are not returned to the pool.

The pool retains at most two idle transports per route key, with at most 32 idle
transports globally, and rejects entries idle for 60 seconds on access. Checkout
exclusively takes the newest compatible idle transport; a third concurrent borrower
creates its own transport immediately when both retained transports are checked out.
Returning a third idle transport for one route evicts that route's oldest transport.
Exceeding the global limit evicts the oldest individual idle transport across all routes,
keeping other entries for its route. No preconnections or waits for active borrowers
are introduced. Its identity includes the exact origin, endpoint, concrete route and
core policy, HTTP profile, routing parameters, configured timeout, connector
profile and shared listener. Changed TLS/DNS/core policy isolates old entries.
The connector profile remains distinct for discovery and prior-destination reuse;
the first preferred call may therefore establish a new connection before later
preferred calls reuse it. Cache reuse is not proof that the server kept a socket
open: a closed connection reconnects through the newly rebound route.

By default the pool belongs to one HTTP client family. Applications can share an
`Arc<arachnea_http::client::ProxyTransportPool>` using
`.proxy_transport_pool(pool)` alongside `.proxy_runtime(runtime)` across derived
clients. Cookies and request headers stay outside the transport pool. DEBUG
`reusing selected proxy HTTP transport` records a checkout without URL paths or
credentials; it does not claim physical socket reuse. The request-debug path
reports body length rather than raw payloads and does not dump the shared cookie
cache; configured headers continue through the existing redaction helper.

With `performance-diagnostics`, targeted DEBUG qualification also reports `checked localized proxy HTTP transport
pool` for every checkout (`reused`, idle age, expired entries, remaining idle
entries and checkout duration), followed by `prepared localized proxy HTTP
transport` for rebinding or creation time. The Rquest adapter reports
`received localized proxy HTTP response headers`, `classified localized proxy
HTTP response` and `collected localized proxy HTTP response body`, correlated by
search and destination. Header time includes connection/tunnel/TLS establishment
when needed, request sending and waiting for headers; it does not isolate those
network stages. Classification can read a prefix, and collection includes any
replayed prefix; `body_bytes` is the materialized response size, not network
traffic. `body_skipped` identifies a retryable rejection whose body was skipped.
When a response-specific policy replaces its historical prefix requirement with
no inspection, classification no longer waits for those body bytes. Collection
preserves the skipped prefix's bounded reception window (byte limit, up to three
seconds within the original inspection allowance, or EOF), then starts the
ordinary transfer timeout. EOF completes immediately, and reaching the byte
limit starts the ordinary timer without granting unused initial time. This is
not a fixed three-second timeout increase. Non-accepted responses remain capped
by the search deadline; accepted bodies retain their independent transfer policy.
DEBUG `deferred_prefix_bytes` and `deferred_prefix_ms` describe the preserved
bounds; their reception time now belongs to collection rather than classification.
The response-specific hook defaults to historical behavior for existing callbacks.

Stage times exclude some observation/persistence work, so their sum is not a
complete API duration. These events contain no signed URL paths or response
contents. Compare checkout events with new CONNECT establishment events to
qualify physical reuse; checkout alone remains insufficient.


Browser/custom engines without early route correlation return
`UnsupportedEngineOperation` within this scoped search, rather than bypassing
its limits or inferring a proxy result. External API gateways must allow the
135-second search horizon plus the separate accepted-body transfer policy.


## Optional performance diagnostics

The `performance-diagnostics` feature is disabled by default. It gates localized
transport-checkout/preparation timers and response header/classification/body
phase measurements. It forwards to `arachnea-proxy` only when that optional
dependency is already enabled; it does not enable proxy transport by itself.
The real request/inspection/body deadlines, replay evidence, pool TTL/ownership
and callbacks remain active without it. Ordinary request-send and recoverable
per-attempt failure logs are DEBUG; the localized retry facade emits a terminal
WARN containing origin, method and delivery evidence. Normal startup and
persistence failure handling are unchanged. For the application command and
log policy, see the proxy crate README.
