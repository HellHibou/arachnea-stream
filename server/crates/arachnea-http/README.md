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
The chaser-cf browser solver keeps one browser tab alive while it clears Cloudflare, attempts Turnstile clicks through CDP, and samples the DOM until the HTML is stable.
When chaser-cf is used to fetch page content directly, it forwards per-request HTTP headers before navigation. The Cloudflare refresh path now reuses the blocked request headers as well. `Referer` is passed through Chrome's native navigation API, so normal browser referrer policy still applies.
When a `Referer` is expected, chaser-cf also emits one runtime log line with the `observed_referer` reported by Chrome for the outbound navigation request.
If the browser solve succeeds but the `rquest` cookie handoff is still blocked, the client falls back to the browser solver response for engines that can return page content.
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

Current upstream client APIs still expose proxy URLs rather than a stable public transport replacement hook, so the built-in `rquest` and Ghostwire engines both use the managed loopback compatibility helper when `proxy_core(...)` is selected.

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
