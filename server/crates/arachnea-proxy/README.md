# arachnea-proxy

`arachnea-proxy` provides the proxy layer for Arachnea. It exposes a reusable routing core, optional client connectors, and optional local proxy server listeners.

This README is the design and usage home for the proxy crate. Remaining work is tracked in the root `docs/TODO.md`.

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
- Controller HTTP proxy URL helpers can carry optional request-local country routing hints, opaque dynamic-proxy affinity keys, origin-rejection statuses, and redirect-time `RemoveHeader` rules through the existing `opts` mechanism. Related requests with the same affinity key reuse the same eligible proxy; a configured rejection clears that binding, records a persistent 24-hour cooldown for the exact destination origin, and retries once with another candidate.
- Post-response text replacement actions can carry an optional per-action content-type allowlist for manifests that use non-default textual MIME types.
- Scrapyfy `resolve_url` can attach declarative `proxy_replace_all` rules to proxied textual responses, for example to proxy storyboard WebVTT cue-image URLs.
- Optional HTTP proxy, HTTP CONNECT, HTTPS proxy, SOCKS4/SOCKS4a, SOCKS5 CONNECT, and SOCKS5 UDP ASSOCIATE server listeners.
- Public-bind safety checks, ACLs, optional HTTP/SOCKS authentication, connection limits, and typed errors.
- Optional integration with `arachnea-dns` when local resolution or Smart DNS decisions are required.
- Optional persistent cache for dynamic proxies (feature `persistence`): the inventory consults a shared `PersistenceStore` per country before falling back to the provider and writes mutations back through namespace-bound transactions. Records are keyed by authority in the `proxy-inventory` namespace and filtered by country through field queries. Their fixed 24-hour validation age is not a hard expiration: stale records remain selectable and are deleted only when a later proxy connection or configured origin-rejection check fails.
- Dynamic provider refresh policy belongs to the shared inventory rather than HTTP clients. Refreshes are single-flight per country and normally suppressed for 120 seconds after every provider attempt, including failures and empty lists. An observed connection or tunnel failure clears that cooldown without loading immediately, allowing the next exhausted-cache selection to discover new endpoints while preserving the failed endpoint's cached exclusion. HTTP engine proxy failures observed after route establishment are recorded against the exact destination, clear the cached client and affinity, and retry once with another candidate; a failed replacement is recorded but is not retried a third time. Configured origin-response rejections do not clear the provider cooldown. Multi-country selection searches every requested country's memory and persistent cache before consulting providers, then refreshes eligible countries in caller order. Refreshed provider lists are deduplicated by endpoint and compared with the inventory before network probes: cached protocol, HTTPS capability, health and failure metadata are reused while `last_checked` remains within `probe_ttl`; only new, untested or expired endpoints are probed. Accepted origin responses renew the selected proxy's 24-hour validation age and make it the preferred candidate for that exact country, scheme, host and port while it remains eligible; independent destinations therefore keep independent preferred proxies. Optional affinity bindings are retained for 15 minutes and are reused only while the bound proxy still passes country, protocol, freshness, global cooldown, and destination-cooldown checks. Statuses listed in `proxy_rejection_statuses` persist a 24-hour destination rejection, clear the rejected destination preference and affinity binding, and rotate without bypassing those checks.

## Profiles

| Profile | Intent |
| --- | --- |
| `system_relay` | Use process/system networking behavior and direct network fallback. |
| `single_forwarder` | Route through one configured HTTP/HTTPS/SOCKS proxy. |
| `multi_forwarder` | Route through a fixed proxy chain. |
| `privacy` | Minimize logs, isolate by destination, preserve hostnames, and avoid unauthorized direct fallback. |
| `advanced` | Apply only explicitly configured behavior. |

The retired design notes also reserved `censorship_resistance`; it remains tracked in the root `docs/TODO.md` unless the public API intentionally drops it.

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
| `persistence` | Enables the persistent cache of dynamic proxies backed by `arachnea-core::persistence`. |

## Routing and DNS

- `Destination` should keep either an IP address or a hostname without forcing early resolution.
- SOCKS4a, SOCKS5, and HTTP CONNECT can carry hostnames upstream.
- SOCKS4 uses local IPv4 resolution by default; `socks4a://` uses proxy-side hostname transmission.
- SOCKS5 supports `local`, `proxy_then_local_fallback`, and `proxy_only` DNS strategies; `proxy_then_local_fallback` is the default and is used only after SOCKS5 reports a remote hostname-resolution failure.
- `arachnea-dns` should be called only when the proxy truly needs local resolution or a precomputed Smart DNS route decision.
- Dynamic proxies that time out or fail at transport/TLS setup are marked globally unavailable for their cooldown, whereas configured HTTP origin rejections remain scoped to the rejected destination.
- The proxy should consume DNS decisions; it should not redefine DNS security, privacy, cache, DNSSEC, or anti-censorship policy.

## Security and Privacy

- Loopback listeners are the safe default.
- Non-loopback binds without ACLs are rejected unless `allow_unsafe_public_bind_without_acl = true`.
- SOCKS4 clients must be refused when server username/password authentication is required.
- Proxy-generated or relayed HTTP proxy headers should be stripped when configured.
- Sensitive client parameters must not be logged by default.
- Privacy mode reduces accidental leaks and direct fallback, but it does not protect against account identity, cookies, application fingerprinting, TLS/SNI leaks from the application, traffic correlation, or a logging upstream proxy.

## Country Routing Parameters

Country routing accepts only the ordered JSON `countries` parameter. HTTP proxy clients must send it in the `Arachnea-Proxy-Countries` header, for example `Arachnea-Proxy-Countries: ["FR", "DE"]`; SOCKS5 clients can provide `countries=["FR","DE"]` through the parameterized username format. Country codes are normalized to uppercase, duplicates are removed while preserving order, and static or dynamic routing selects the first available country. The legacy singular `country` parameter and `Arachnea-Proxy-Country` header are not supported.

## Known Gaps

The durable remaining work from the retired specification is tracked in the root `docs/TODO.md`. Important proxy gaps include MASQUE CONNECT-UDP, ExternalTunnel orchestration, richer Tor integration, concrete `hyper-util` support, stronger auth tests and secret handling, true per-IP rate limiting, benchmarks, a possible `rquest` in-process connector, and the reserved `censorship_resistance` profile.

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

## Dynamic Proxy Capabilities

Dynamic proxy records separate endpoint health from runtime capabilities. The
global `ProxyRuntimeStatus::Ok` means that the endpoint and declared/detected
proxy protocol are reachable and at least one configured application capability
worked, or that no application capability was configured.

`ProxyRecord` stores independent tri-state runtime results through
`ProxyCapabilityStatus::{Unknown, Available, Unavailable}`:

- `http_forwarding`: reaching the configured HTTP probe destination;
- `https_tunnel`: opening a proxy tunnel to the configured HTTPS probe
  destination;
- `destination_tls`: completing destination HTTPS validation through that
  tunnel, including the TLS handshake, certificate validation and accepted HTTP
  response;
- `proxy_tls_certificate`: validating the certificate presented by an HTTPS
  proxy endpoint.

The HTTPS probe continues through the established tunnel, sends a lightweight
`HEAD` request to the configured URL, and accepts syntactically valid HTTP
responses with status 200 through 499. Redirects prove end-to-end HTTPS
transport without being followed; 5xx responses, invalid responses, TLS errors
and timeouts do not validate the destination. Selection always rejects an
explicitly unavailable tunnel or destination
validation result. `ProbeMode::Strict` requires runtime `Available` results for
HTTP forwarding and, for HTTPS destinations, both the tunnel and complete
destination TLS validation. `ProbeMode::Relaxed` may use a declared protocol
and the provider's positive `supports_https` hint when the corresponding
runtime result is absent or inconclusive. SOCKS declarations are sufficient as
the relaxed HTTPS hint because those protocols carry arbitrary TCP tunnels.

`supports_https` remains provider metadata and is not overwritten by runtime
probing. Selection uses the matching runtime capability when known and keeps the
provider/protocol fallback only in relaxed mode and only for records whose
capability is still `Unknown`, including records loaded from an older persistent
store. Provider hints are never copied into runtime capability fields, so an
inferred relaxed decision remains distinguishable from successful validation.

Provider rows reach inventory preparation without endpoint-level deduplication,
then merge into one runtime record per `host:port`. `ProxyRecord::declarations`
preserves each distinct source identifier, advertised protocol and HTTPS hint so
conflicting provider claims remain diagnosable. The persistent identity continues
to use only the endpoint; provenance is stored as record metadata.

When an endpoint has several declared protocols, probes try every distinct
declaration in configured priority order until one is validated. Strict mode does
not invent an undeclared transport. Relaxed mode may retain an inconclusive
declared-protocol fallback, and only when no declared variant is usable may it
probe undeclared protocols as a correction path. An undeclared protocol is
selected only after a concrete runtime capability succeeds.

Probe time limits are configured through `ProbeConfig::timeouts` and the public
`ProbeTimeoutConfig`. Its intentionally low defaults are 3 seconds to connect to
the proxy, 5 seconds for proxy protocol exchanges and probe requests, 5 seconds
for local target resolution or tunnel setup, 5 seconds for TLS handshakes, and a
10-second overall limit for each capability probe or destination validation
attempt. Protocol detection applies that overall limit independently to each
protocol candidate so a failed candidate does not consume the budget of the
following candidates. A TCP connection failure for the endpoint itself stops the
remaining capability and protocol variants because they all share the same
`host:port`; protocol-specific failures on a reachable endpoint still allow the
remaining declared variants to run.

SOCKS probes preserve their DNS semantics. SOCKS4 resolves hostnames locally to
IPv4, while SOCKS4a sends hostnames to the proxy without an implicit local-DNS
fallback. SOCKS5 first sends the hostname to the proxy and retries through a new
connection with a locally resolved address only when the proxy reply indicates a
likely remote-DNS failure. Dynamic proxy nodes retain these same resolution
modes at runtime. Runtime and probe local resolution share the same resolver:
when the `arachnea-dns` feature is compiled and an `ArachneaDnsCore` is
configured, both use that core; otherwise both fall back to
`tokio::net::lookup_host` and the system resolver.

## Useful Paths

- `src/core/`: routing, proxy chains, destinations, policies, transports, proxy pools, and errors.
- `src/connectors/`: adapters for Rust HTTP client ecosystems.
- `src/server/`: CLI, file configuration, listeners, handlers, ACLs, authentication, and safety checks.
- `config-sample/`: TOML examples for direct, upstream, routing, and listener configurations.
- `examples/`: library and connector usage examples.

## References

Important standards and ecosystem references for this crate include RFC 9110, RFC 1928, RFC 1929, RFC 9298, RFC 9484, RFC 8446, TLS ECH references, Tor pluggable transports, Arti, Shadowsocks SIP003, and the `rquest` proxy and `ClientBuilder` extension APIs.
