# TF1 FR proxy routing analysis

## Context

`server/crates/arachnea-stream/src/main.rs` currently builds the FR upstream proxy with `ProxyNode::socks5(...)`. That hardcodes the transport kind at the call site and only accepts an endpoint such as `62.133.62.3:1081`.

The proxy crate already exposes `ProxyNode::from_url(name, proxy_url)`, which selects the proxy protocol from the URL scheme. This is the better fit for a configurable proxy value such as:

```rust
ProxyNode::from_url("fr-proxy", "socks5://12.34.56.78:1080")?
```

Supported schemes are documented in `ProxyNode::from_url`: `http`, `https`, `socks`, `socks4`, `socks4a`, `socks5`, `socks5h`, `tor`, and `torsocks`. Missing ports receive scheme defaults, for example `socks5://127.0.0.1` becomes `127.0.0.1:1080`.

## Current issue

The current `main.rs` proxy setup has two separate concerns mixed together:

- defining which upstream proxy exists;
- deciding when HTTP requests should use that upstream.

Because the created proxy core is injected globally into the scraper manager, the FR proxy becomes the default route for every HTTP request. The desired behavior is narrower: keep normal traffic direct by default, and use the FR proxy only when a request asks for country `FR`.

## Proposed direction

1. Replace the direct `ProxyNode::socks5(...)` construction in `main.rs` with `ProxyNode::from_url(...)`.
2. Build the proxy core around a direct default chain plus a `country_routing` parameter handler.
3. Register a route where `country=FR` appends the node returned by `ProxyNode::from_url(...)`.
4. Extend `ScraperHttpConfig` with an optional proxy country setting exposed through a setter.
5. Update the TF1 resolver path so all resolver HTTP calls use `ScraperHttpConfig::default().proxy_country("FR")`.
6. If the FR-proxied `HttpClient` fails because the FR proxy setup/request path is unavailable, log a warning and retry with a non-FR-proxied `HttpClient` as a temporary workaround.

This uses the geolocation mechanism that already exists in `arachnea-proxy` instead of hardcoding TF1 to a SOCKS5-only helper.

## Decisions

- The FR proxy URL stays hardcoded in `main.rs`.
- The hardcoded proxy node name is `proxy-fr`.
- The expected URL form is `socks5://12.34.56.78:1080`.
- TF1 uses the FR proxy for every HTTP call made by `tf1_resolver.rs`.
- TF1 requests the FR proxy through `ScraperHttpConfig::default().proxy_country("FR")`, keeping the existing `create_http_client(...)` entry point.
- `proxy_country(...)` normalizes values immediately with trim + uppercase, so `"fr"` and `" FR "` become `"FR"`.
- An invalid or unavailable FR proxy path surfaces as `ArachneaHttpError::Proxy`. TF1 catches that error, writes a warning log entry, and retries without the FR proxy as a workaround.
- The fallback without FR proxy is temporary and tracked in `TODO.md`.

## API shape

`ScraperHttpConfig` should gain an optional proxy country field and a setter:

```rust
ScraperHttpConfig::default().proxy_country("FR")
```

Internally this should map to the existing generic proxy parameter name `country`, so the HTTP layer can send the route hint accepted by `arachnea-proxy`'s `country_routing` handler. The public API remains country-oriented because this is the only currently needed scraper-level proxy hint.

The setter should normalize the country value immediately:

```rust
ScraperHttpConfig::default().proxy_country(" fr ")
```

stores `FR`.

The existing call shape stays intact:

```rust
let http_client = scraper_agregator.create_http_client(tf1_http_config());
```

## Important detail

The URL form should use a supported scheme such as:

```text
socks5://12.34.56.78:1080
```

Not:

```text
sock5:12.34.56.78:1080
```

`sock5` is not listed as a supported scheme, and the existing parser expects the proxy URL to include a usable host after the scheme separator.

## Expected impact

- Main/default scraper traffic can remain direct.
- TF1 traffic can request the FR proxy explicitly.
- Changing the proxy protocol later becomes a configuration/string change: `socks5://...`, `http://...`, `https://...`, etc.
- Invalid proxy URLs fail early with contextual configuration errors.

## Points to clarify

- Do we need support for authenticated proxy URLs, or should credentials continue to use the existing `with_credentials_env` path since inline credentials are rejected?
