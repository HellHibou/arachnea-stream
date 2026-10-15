# arachnea-proxy

## Shared response classification (localized proxy lot 3)

`core::http::response_classifier` exposes a client-independent response view,
`ProxyResponseClassifier` and `ProxyResponseDecision`: `Accepted`,
`RejectedForDestination { cooldown, reason }`, or `Inconclusive`. Receiving a
response is transport evidence, not application acceptance. Defaults accept only
2xx; unconfigured 403, other 4xx, 5xx and redirects remain inconclusive and do not
create preferences or exclusions. `HttpStatusProxyResponseClassifier::http_403()`
explicitly rejects 403 while preserving conservative acceptance for other codes.
FranceTV and TF1 retain their existing explicit `[403]` activation.

The same policy runs before media post-actions and Rquest body materialization.
Only the orchestrator applies its decision to the exact selection token. Handler
errors are logged and become inconclusive; they do not reject the endpoint.
An inconclusive response preserves the existing durable history. Redirect
responses are classified with the actual hop's method, effective URL and token;
a final accepted response never validates the original domain by inference.

Code injects `ProxyResponsePolicy(Arc<dyn ProxyResponseClassifier>)` through
`ProxiedHttpRequest::response_classifier` or the HTTP/Scrapyfy client builders.
The synchronous handler must not block, perform network I/O, mutate inventory or
retry. By default it reads no body. `Prefix(n)` exposes at most 64 KiB with an
inspection allowance capped at three seconds (and the remaining request timeout
in Rquest). The adapter replays every consumed byte and read error. Complete,
limited and unavailable observations are distinct; reaching the limit never
proves EOF. Missing content in an incomplete prefix cannot establish rejection.
Media bodies remain streamed unless existing post-actions/controller requirements
already need buffering. Content-prefix inspection uses the transfer-decoded
stream; it cannot use the buffered compatibility fallback for upstreams sending
already-dechunked data with a stale `Transfer-Encoding: chunked` header. This
malformed-response tolerance remains available for header-only classifiers.

YAML HTTP options and media URL JSON options accept `proxy_response_classifier`:

```yaml
proxy_response_classifier:
  type: statuses
  rejected_statuses: [403]
  accepted_statuses: [200, 206]
  cooldown_seconds: 86400
```

Other known types are `{ type: default }` and `{ type: http403 }`. Statuses must
be in 100..599, acceptance and rejection lists must not overlap, and an explicit
cooldown must be positive and representable. Omitted accepted statuses mean all
2xx; an explicit empty list accepts nothing. Omitted cooldown uses the inventory
default. Existing exclusions are never shortened. Precedence is code injection,
then an explicit known policy, then legacy `proxy_rejection_statuses`, then
conservative defaults; policies are not silently combined. Child YAML policies
override inherited policies. `{ type: default }` explicitly resets an inherited
rejection policy. The configuration travels with rewritten media redirect URLs.

The public HTTP facade still returns materialized bodies, and transfer timeouts
remain separate from prefix inspection. End-to-end search deadlines and retry
safety/budgets belong to lot 5; lot 3 does not extend retries. Engines without
early observation/correlation do not infer proxy acceptance or rejection and
report this limitation when a policy is configured.

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
- Optional persistent cache for dynamic proxies (feature `persistence`): the inventory consults injected typed repositories before falling back to the provider. Available records use a composite host/port key in `proxy-inventory`, with country field queries. The 24-hour threshold is measured from the latest positive evidence, or initial discovery for never-validated records, not attempt dates; age alone never deletes a working endpoint. Accepted histories live in `proxy-preferences`, and bounded eviction protection in `proxy-failures`.
- Dynamic provider refresh policy belongs to the shared inventory rather than HTTP clients. Refreshes are single-flight per country and normally suppressed for 120 seconds after every provider attempt, including failures and empty lists. An observed connection or tunnel failure clears that cooldown without loading immediately, allowing the next exhausted-cache selection to discover new endpoints while preserving the failed endpoint's cached exclusion. HTTP engine proxy failures observed after route establishment are recorded against the exact destination, clear the cached client, and retry once with another candidate; a failed replacement is recorded but is not retried a third time. Configured origin-response rejections do not clear the provider cooldown. Multi-country selection searches every requested country's memory and persistent cache before consulting providers, then refreshes eligible countries in caller order. Refreshed provider lists are deduplicated by endpoint and compared with the inventory before network probes: cached protocol, HTTPS capability, health and failure metadata are reused while `last_checked` remains within `probe_ttl`; provider records are stored without eager batch probing, and automatic selection tests unknown or expired required capabilities sequentially until the first usable candidate is found. Accepted origin responses update positive evidence and the durable history for that exact scheme, host and port, independently of country; the newest eligible accepted endpoint wins within the allowed countries. Optional affinity bindings are retained for 15 minutes and are reused only while the bound proxy still passes country, protocol, freshness, global cooldown, and destination-cooldown checks. Statuses listed in `proxy_rejection_statuses` persist a 24-hour destination rejection, make the recorded destination association ineligible without deleting its last success, and rotate without bypassing those checks.

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

Country routing accepts only the ordered JSON `countries` parameter. HTTP proxy clients must send it in the `Arachnea-Proxy-Countries` header, for example `Arachnea-Proxy-Countries: ["FR", "DE"]`; SOCKS5 clients can provide `countries=["FR","DE"]` through the parameterized username format. Country codes are normalized to uppercase, duplicates are removed while preserving order, and static routing and non-preferred dynamic selection currently select the first available country; restored accepted histories instead select the newest eligible success independently of country-list order (the remaining ordering is scheduled for lot 4). The legacy singular `country` parameter and `Arachnea-Proxy-Country` header are not supported.

Dynamic country routing emits INFO diagnostics for local-country bypass decisions
and selected candidate authority and country. Routine route evaluation,
local-country mismatch, selected route transport, and reuse of the proxy validated
for an origin are logged at DEBUG. Route selection logs include the destination
authority; unavailable geo-proxy warnings also include it.
A local country matching any requested country
skips the dynamic geo-proxy, and an unavailable dynamic pool continues along the
remaining route without that geo-proxy. These logs do not include proxy credentials
or affinity values and do not change those routing decisions.

TLS handshake failures emit a WARN diagnostic with the TLS server name,
certificate-verification flag, and underlying error. The shared helper uses
public WebPKI roots when verification is enabled, both for HTTPS upstream proxies
and for HTTPS destinations fetched by the built-in HTTP client. The server name
helps distinguish upstream-proxy certificate failures from destination failures;
the diagnostic does not disable certificate validation or log signed media URLs.

HTTP CONNECT diagnostics distinguish upstream tunnel establishment, client
acknowledgement failures, and relay failures after establishment. Normal relay
completion includes directional byte totals and elapsed time; relay failures
include the destination authority, elapsed time, and I/O error kind, but cannot
identify which peer caused the failure from the bidirectional copy error alone.
No tunnel payload or authentication headers are logged.

HTTP CONNECT establishment has a 120-second overall budget, defined by the public
`PROXY_CONNECT_ESTABLISHMENT_TIMEOUT` constant, covering route
selection, provider loading, probes, and upstream connection attempts. Client EOF
or read failure cancels establishment. Up to 16 KiB of early client data is
preserved and forwarded after acknowledgement. Established relays are not subject
to this timeout. The establishment budget does not apply to direct core calls or
the built-in `/api/proxy` HTTP client.

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
probing. With a configured probe, selection tests unknown or expired required
capabilities sequentially, including records from memory, persistent cache, and
affinity bindings. It persists results and stops at the first usable candidate.
Provider loading stores the list without probing every endpoint; remaining
records are tested only when needed. Unknown results after probing are skipped,
with inconclusive attempts throttled for `probe_ttl` (10 minutes by default).
TCP precheck results and per-phase attempt timestamps (`tcp`, `http`, `https`)
are persisted as `tcp_reachable` and `probe_attempts`. Recent failed TCP checks
and recent inconclusive capability attempts are skipped before prechecking.
Successful TCP checks are reused while fresh but never establish application
support. Capability attempts are saved before network validation, so the retry
delay survives a timeout, caller cancellation, or application restart. Timeouts
remain inconclusive rather than changing a capability to `Unavailable`. Provider
refreshes and cache imports preserve the newest attempt timestamps. Older records
without these optional fields remain readable and are checked once when needed.
Without a configured probe, existing strict/relaxed eligibility remains in effect.
Per-endpoint probe locks avoid duplicate automatic capability probes without
blocking unrelated endpoints. Automatic selection prechecks TCP reachability in
bounded parallel windows (`probe_batch_size`, at least one); it then validates
the requested HTTP or HTTPS capability sequentially and stops at the first usable
candidate. TCP prechecks do not validate proxy protocols or application support.
Each window completes before destination validation, preserving candidate order.
Unrelated capability results are retained. Complete capability probes are bounded
to 8 seconds and run inline so dropping their caller cancels them. No background
batch is started. The current lightweight precheck is TCP-only; TLS to an HTTPS
proxy and SOCKS authentication remain part of targeted capability validation.
Provider hints are never copied into runtime capability fields.

For HTTPS selection, an eligible proxy previously accepted by the exact
destination is returned before affinity selection or endpoint precheck windows.
Its general probe age alone does not trigger discovery or revalidation. Origin
preferences are shared by the runtime inventory and keyed by scheme, canonical
host and port, never by country. With an injected preference repository, up to
three distinct accepted endpoints and their millisecond UTC success dates are
restored lazily before discovery. The newest eligible success within the allowed
countries wins regardless of country-list order; a disallowed association is
ignored without deletion. Histories do not expire on age alone.
Health, authentication, capability failures, country restrictions and destination
cooldowns still apply. Observed destination failures temporarily make the association ineligible without
deleting its last success; configured origin rejections remain destination-scoped.
Other candidates are ranked by runtime validation
before latency: an available tunnel with validated destination TLS comes first,
then an available tunnel with unknown TLS, then relaxed protocol/provider hints
with an unknown tunnel. HTTP selection keeps its latency ordering, and existing
eligible affinity bindings are preserved. Probe validation concerns the configured
probe destination and does not guarantee acceptance by every origin. Explicitly
unavailable capabilities remain ineligible; certificate verification is unchanged.

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

## Contextual observations and request correlation

Dynamic records persist optional `health_evidence` and up to 64 latest observation
contexts. `first_discovered_at` is a retention date, not a success. Only a positively
observed phase updates `last_positive_at`; legacy `last_checked` is never converted
to positive evidence. Each context retains its last success separately from its
latest result. Provider merges preserve these facts.

Endpoint connection, proxy protocol/authentication, proxy TLS, tunnel, destination
TLS and HTTP response receipt are distinct phases. A destination timeout, tunnel
refusal, remote certificate error or HTTP 5xx does not promote the endpoint to
global KO or evict it. Certificate verification remains enforced for the relevant
exchange. A received HTTP status proves transport, not business acceptance.
Probe configuration identities invalidate legacy aggregate results and recent
http/https attempt timestamps when the URL, mode or timeout policy changes; TCP
attempts remain reusable. Historical contextual facts remain available.

`ArachneaProxyCore::select_for_request` returns a concrete pinned core and an opaque
`ProxySelectionToken`. Reconnects cannot silently select another endpoint. Responses
and failures must use their own token rather than a destination's last selection.
Pinned routes reject another authority; HTTP CONNECT may describe that same
authority as TCP instead of HTTPS. Tokens are not serialized or sent to origins.
Legacy destination-only observation APIs remain compatibility helpers and are not
safe for concurrent attribution; Scrapyfy and the media route use tokens instead.

Shared response classifiers are implemented in lot 3 above. End-to-end budgets,
retry safety and complementary probe circuits remain subsequent implementation
lots. No new background maintenance is introduced.

### Durable preferences and endpoint eviction (lot 2)

The application opens separate `proxy-preferences` and `proxy-failures` typed
stores next to `proxy-inventory`, then injects shared repositories through
Scrapyfy. Compatibility constructors retain memory-only stores. Accepted-history
commits are awaited in the success path; a failed write preserves the response
and memory history, emits a contextual error and increments
`ProxyInventory::preference_persistence_failures()`. No business replay or repair
worker is started. Later accepted responses can retry persistence.

Per-destination locks serialize history updates. Repository commit locks are
shared by wrappers of the same typed-store `Arc`, including across application
rebuilds, preventing file snapshots from committing out of order. Independent
store objects targeting the same file and multiple processes are not coordinated.
There is no cross-store transaction; restored references are checked against the
inventory and orphans are cleaned on demand.

Only a proven failure to connect to the proxy endpoint can evict it globally.
The 24-hour threshold uses the latest positive evidence, or initial discovery
when no positive evidence exists, never `last_checked`. Before that threshold,
the endpoint is globally unavailable with a configurable `ko_cooldown` (15 minutes
by default), after which a demand-driven connection test may run again. No
timer triggers a test or deletes a still-working endpoint. Authentication, proxy
TLS security failures and remote failures do not independently trigger eviction.

Eviction removes the available memory/persistent record, preference references
and affinity bindings. A separate failure store retains at most 1,024 records
for 48 hours from eviction; cleanup runs on demand. Rediscovery preserves the
initial discovery, positive evidence, exclusions and active cooldown; it is not
a success and cannot immediately revive the endpoint. Destination exclusions
(24 hours by default, configurable) survive technical successes and provider
refreshes, without an implicit early release when all candidates are excluded.
Legacy records remain readable, but their attempt dates do not invent evidence
or historical accepted preferences. The remaining non-preferred selection and
provider country ordering will be revised in lot 4.
