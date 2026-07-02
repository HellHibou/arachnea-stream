# arachnea-proxy

`arachnea-proxy` provides the proxy layer for Arachnea. It exposes a reusable routing core, optional client connectors, and optional local proxy server listeners.

This README is the design and usage home for the proxy crate. Remaining work is tracked in the root `TODO.md`.

## Design Contract

- Keep routing, chain selection, privacy policy, and outbound transport decisions in the core.
- Keep server code as protocol parsing, listener setup, ACLs, authentication checks, and adaptation to core calls.
- Keep connectors as client adapters with no independent routing logic.
- Do not require a local server port for core library usage.
- Preserve hostnames whenever the incoming and upstream protocols can carry them.
- Keep profiles inspectable. Profiles must expand into concrete configuration instead of hidden behavior.
- Keep Smart DNS, egress selection, country/region routing, and extra proxy chaining as composable features rather than profiles.
- Reject public open-proxy exposure by default.
- Do not represent proxying as anonymity. Document what still leaks: account identity, cookies, browser or TLS fingerprint, WebRTC, traffic size, timing, SNI, and upstream logging.

## Main Capabilities

- `ArachneaProxyCore` for in-process library usage without opening a local port.
- Direct TCP and connected UDP paths.
- Upstream HTTP proxy, HTTP CONNECT, HTTPS proxy, SOCKS4/SOCKS4a CONNECT, SOCKS5 CONNECT, and SOCKS5 UDP ASSOCIATE support.
- Multi-hop TCP chaining, proxy pools, route selection, fallback behavior, and typed route errors.
- `tower` and hyper-oriented client connector adapters.
- `rquest` loopback helper for clients that require a proxy URL.
- Controller HTTP proxy URL helpers can carry optional request-local country routing hints and redirect-time `RemoveHeader` rules through the existing `opts` header mechanism.
- Post-response text replacement actions can carry an optional per-action content-type allowlist for manifests that use non-default textual MIME types.
- Optional HTTP proxy, HTTP CONNECT, HTTPS proxy, SOCKS4/SOCKS4a, SOCKS5 CONNECT, and SOCKS5 UDP ASSOCIATE server listeners.
- Public-bind safety checks, ACLs, optional HTTP/SOCKS authentication, connection limits, and typed errors.
- Optional integration with `arachnea-dns` when local resolution or Smart DNS decisions are required.

## Profiles

| Profile | Intent |
| --- | --- |
| `system_relay` | Use process/system networking behavior and direct network fallback. |
| `single_forwarder` | Route through one configured HTTP/HTTPS/SOCKS proxy. |
| `multi_forwarder` | Route through a fixed proxy chain. |
| `privacy` | Minimize logs, isolate by destination, preserve hostnames, and avoid unauthorized direct fallback. |
| `advanced` | Apply only explicitly configured behavior. |

The retired design notes also reserved `censorship_resistance`; it remains tracked in the root `TODO.md` unless the public API intentionally drops it.

## Features

| Feature | Purpose |
| --- | --- |
| `core` | Enables the reusable proxy core. |
| `server` | Enables file configuration, CLI support, and proxy listeners. |
| `connectors` | Enables client-side connector adapters. |
| `tower` | Enables the `tower` service adapter. |
| `hyper` | Enables the hyper-oriented connector adapter. |
| `rquest` | Enables the loopback helper for `rquest` compatibility. |
| `arachnea-dns` | Enables integration with the DNS crate. |

## Routing and DNS

- `Destination` should keep either an IP address or a hostname without forcing early resolution.
- SOCKS4a, SOCKS5, and HTTP CONNECT can carry hostnames upstream.
- SOCKS4 uses local IPv4 resolution by default; `socks4a://` uses proxy-side hostname transmission.
- SOCKS5 supports `local`, `proxy_then_local_fallback`, and `proxy_only` DNS strategies; `proxy_then_local_fallback` is the default.
- `arachnea-dns` should be called only when the proxy truly needs local resolution or a precomputed Smart DNS route decision.
- The proxy should consume DNS decisions; it should not redefine DNS security, privacy, cache, DNSSEC, or anti-censorship policy.

## Security and Privacy

- Loopback listeners are the safe default.
- Non-loopback binds without ACLs are rejected unless `allow_unsafe_public_bind_without_acl = true`.
- SOCKS4 clients must be refused when server username/password authentication is required.
- Proxy-generated or relayed HTTP proxy headers should be stripped when configured.
- Sensitive client parameters must not be logged by default.
- Privacy mode reduces accidental leaks and direct fallback, but it does not protect against account identity, cookies, application fingerprinting, TLS/SNI leaks from the application, traffic correlation, or a logging upstream proxy.

## Known Gaps

The durable remaining work from the retired specification is tracked in the root `TODO.md`. Important proxy gaps include MASQUE CONNECT-UDP, ExternalTunnel orchestration, richer Tor integration, concrete `hyper-util` support, stronger auth tests and secret handling, true per-IP rate limiting, benchmarks, a possible `rquest` in-process connector, and the reserved `censorship_resistance` profile.

## Common Commands

Run connector examples:

```powershell
cargo run -p arachnea-proxy --example tower_http_get -- example.com /
cargo run -p arachnea-proxy --example hyper_service -- example.com /
```

Run forwarding and chaining examples:

```powershell
cargo run -p arachnea-proxy --example proxy_single_forwarder -- socks5://127.0.0.1:9050 example.com /
cargo run -p arachnea-proxy --example proxy_chain -- http://127.0.0.1:8080 socks5://127.0.0.1:9050 example.com /
```

Validate and inspect a proxy configuration:

```powershell
cargo run -p arachnea-proxy -- validate-config crates/arachnea-proxy/config-sample/direct.toml
cargo run -p arachnea-proxy -- show-effective-config --config crates/arachnea-proxy/config-sample/direct.toml
```

Run the proxy server:

```powershell
cargo run -p arachnea-proxy -- serve --config crates/arachnea-proxy/config-sample/direct.toml
```

Test local listeners:

```powershell
curl -x http://127.0.0.1:8080 http://example.com
curl -x http://127.0.0.1:8080 https://example.com
curl --socks5-hostname 127.0.0.1:1080 https://example.com
```

## Useful Paths

- `src/core/`: routing, proxy chains, destinations, policies, transports, proxy pools, and errors.
- `src/connectors/`: adapters for Rust HTTP client ecosystems.
- `src/server/`: CLI, file configuration, listeners, handlers, ACLs, authentication, and safety checks.
- `config-sample/`: TOML examples for direct, upstream, routing, and listener configurations.
- `examples/`: library and connector usage examples.

## References

Important standards and ecosystem references for this crate include RFC 9110, RFC 1928, RFC 1929, RFC 9298, RFC 9484, RFC 8446, TLS ECH references, Tor pluggable transports, Arti, Shadowsocks SIP003, and the `rquest` proxy and `ClientBuilder` extension APIs.
