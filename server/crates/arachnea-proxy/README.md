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
An accepted final response is committed before body collection; token clones
share one awaited acceptance commit. An inconclusive response preserves the
existing durable history. Redirect responses are classified with the actual hop's method, effective URL and token;
a final accepted response never validates the original domain by inference.
A followed 301/302/303/307/308 also records separate, persistent routing evidence
for its actual origin. It can reuse that endpoint before exploration but never
creates a final-content acceptance preference or clears an origin rejection.

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
default. Existing explicit origin exclusions are never shortened. Precedence is code injection,
then an explicit known policy, then legacy `proxy_rejection_statuses`, then
conservative defaults; policies are not silently combined. Child YAML policies
override inherited policies. `{ type: default }` explicitly resets an inherited
rejection policy. The configuration travels with rewritten media redirect URLs.

The public HTTP facade still returns materialized bodies, and transfer timeouts
remain separate from prefix inspection. Localized search deadlines and replay safety are described below. Engines without
early observation/correlation return an explicit unsupported-operation error when
called inside a correlated localized search; they cannot infer proxy acceptance
or rejection.

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
- Controller HTTP proxy URL helpers can carry optional request-local country routing hints, opaque dynamic-proxy affinity keys, origin-rejection statuses, and redirect-time `RemoveHeader` rules through the existing `opts` mechanism. Related requests with the same affinity key reuse the same eligible proxy; a configured rejection clears that binding, records a persistent 24-hour cooldown for the exact destination origin, and permits another candidate only under the shared budget and safe replay policy.
- Post-response text replacement actions can carry an optional per-action content-type allowlist for manifests that use non-default textual MIME types.
- Scrapyfy `resolve_url` can attach declarative `proxy_replace_all` rules to proxied textual responses, for example to proxy storyboard WebVTT cue-image URLs.
- Optional HTTP proxy, HTTP CONNECT, HTTPS proxy, SOCKS4/SOCKS4a, SOCKS5 CONNECT, and SOCKS5 UDP ASSOCIATE server listeners.
- Public-bind safety checks, ACLs, optional HTTP/SOCKS authentication, connection limits, and typed errors.
- Optional integration with `arachnea-dns` when local resolution or Smart DNS decisions are required.
- Optional persistent cache for dynamic proxies (feature `persistence`): the inventory consults injected typed repositories before falling back to the provider. Available records use a composite host/port key in `proxy-inventory`, with country field queries. The 24-hour threshold is measured from the latest positive evidence, or initial discovery for never-validated records, not attempt dates; age alone never deletes a working endpoint. Accepted histories live in `proxy-preferences`, and bounded eviction protection in `proxy-failures`.
- Dynamic provider refresh policy belongs to the shared inventory rather than HTTP clients. Refreshes are single-flight per country and normally suppressed for 120 seconds after every provider attempt, including failures and empty lists. An observed connection or tunnel failure shortens that cooldown to the configurable 10-second minimum without loading immediately, allowing the next exhausted-cache selection to discover new endpoints while preserving the failed endpoint's cached exclusion. HTTP engine failures observed before response headers remain destination-scoped. Correlated HTTP orchestration retries only under the common deadline and replay policy; body errors after headers never rotate. Configured origin-response rejections do not clear the provider cooldown. Multi-country selection imports every allowed country's persistent cache before exploring unknown memory candidates or consulting providers, then overlaps slow exploration with eligible country refreshes and ranks incrementally published results. Refreshed provider lists are deduplicated by endpoint and compared with the inventory before network probes: cached protocol, HTTPS capability, health and failure metadata are reused while `last_checked` remains within `probe_ttl`; provider records are stored without eager batch probing, and automatic selection validates unknown or expired required capabilities progressively within shared concurrency limits until a candidate works. Accepted origin responses update positive evidence and the durable history for that exact scheme, host and port, independently of country; the newest eligible accepted endpoint wins within the allowed countries. Optional affinity bindings are retained for 15 minutes and are reused only while the bound proxy still passes country, protocol, freshness, global cooldown, and destination-cooldown checks. Statuses listed in `proxy_rejection_statuses` persist a 24-hour destination rejection, make the recorded destination association ineligible without deleting its last success, and rotate without bypassing those checks.

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
- Directly observed endpoint connection failures can suspend global availability. Tunnel and destination TLS failures remain scoped to the exact destination: ambiguous failures suspend reuse for at most 30 seconds, whereas configured origin rejections retain their explicit cooldown (24 hours by default).
- The proxy should consume DNS decisions; it should not redefine DNS security, privacy, cache, DNSSEC, or anti-censorship policy.

## Security and Privacy

- Loopback listeners are the safe default.
- Non-loopback binds without ACLs are rejected unless `allow_unsafe_public_bind_without_acl = true`.
- SOCKS4 clients must be refused when server username/password authentication is required.
- Proxy-generated or relayed HTTP proxy headers should be stripped when configured.
- Sensitive client parameters must not be logged by default.
- Privacy mode reduces accidental leaks and direct fallback, but it does not protect against account identity, cookies, application fingerprinting, TLS/SNI leaks from the application, traffic correlation, or a logging upstream proxy.

## Country Routing Parameters

Country routing accepts only the JSON `countries` parameter, interpreted as an unordered set of allowed origins. HTTP proxy clients must send it in the `Arachnea-Proxy-Countries` header, for example `Arachnea-Proxy-Countries: ["FR", "DE"]`; SOCKS5 clients can provide `countries=["FR","DE"]` through the parameterized username format. Country codes are normalized to uppercase, invalid codes are ignored, and duplicates are removed with a stable alphabetical representation for parameters, pool markers and media URL headers. This representation does not define a routing priority. Dynamic selection ranks the combined candidates across all allowed countries; restored accepted histories select the newest eligible success. Static country routes choose by endpoint then node name, independently of country-list order; unrelated static pools retain their own ordering. The legacy singular `country` parameter and `Arachnea-Proxy-Country` header are not supported.

Dynamic country routing emits INFO diagnostics for local-country bypass decisions
and selected candidate authority and country. Routine route evaluation,
local-country mismatch, selected route transport, and reuse of the proxy validated
for an origin are logged at DEBUG. Route selection logs include the destination
authority; unavailable geo-proxy warnings also include it.
A local country matching any requested country skips the dynamic geo-proxy.
When no usable country proxy is available, `ProxyConfig.fallback_direct` defaults
to `false` (return the selection error). Set it to `true`, or call
`ArachneaProxyCore::with_fallback_direct(true)`, to select a fully direct route
without any remote proxy. This policy applies equally with and without a
`ProxySearchContext`; it does not recover unrelated transport or origin errors.
`routing.allow_direct_fallback = false` and `privacy.prevent_direct_fallback = true`
always prohibit this fallback, including request overrides.

Library callers can override the default with the `fallback_direct` client-context
parameter (`ClientParameter::Bool`, or the strings `true` / `false`). HTTP proxy
requests can use `Arachnea-Proxy-Fallback-Direct: true` or `false`; the header is
consumed locally and never forwarded to the destination. With `arachnea-http`,
use the equivalent `proxy_parameter("fallback_direct", "true")` builder option.
The controller web proxy requires an explicit default when registering or invoking
its handler: `register_service(controller, core, name, fallback_direct)` and
`handle_proxy_http(core, input, fallback_direct)`. Its `opts` JSON accepts an optional
boolean `fallback_direct`; omission inherits the service default, and explicit
`false` disables fallback. Precedence is URL option > request header > service
(or core) default, within the privacy restrictions above. Redirect URL rewriting
preserves the URL option.

Direct fallback logs the selection error. `ProxyFallbackDiagnostics::scope`
collects deduplicated non-fatal fallback diagnostics for a logical source execution,
without mixing concurrent sources. These logs do not include proxy credentials
or affinity values.

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
to this timeout. SOCKS username parameter separators are recognized only outside
JSON arrays and quoted strings, preserving multi-country lists. Inventory-backed core connections share a request-owned establishment
budget; correlated HTTP clients and `/api/proxy` use the search deadline below.
Loopback CONNECT uses the caller's remaining deadline when supplied.

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

The HTTPS probe continues through the established tunnel with a `HEAD` request
by default. `ProbeConfig::http_policy` and `https_policy` configure `Head`/`Get`,
an optional `ProxyResponsePolicy`, a versioned policy `identity`, service-indication
statuses and an optional same-scheme fallback URL. The conservative default accepts
2xx; redirects are never followed. One fallback at most is attempted across the
entire validation, sharing its deadline. GET permits a decoded body prefix bounded
to 64 KiB and three seconds. Handler errors remain inconclusive. Change the policy
identity whenever custom code behavior changes.

Every syntactically valid HTTP response records transport evidence separately
from `ProbeTargetAccepted`; even a 5xx response preserves successful TLS/tunnel
facts. Neither phase creates a business preference. Strict mode requires actual
runtime `Available` forwarding/tunnel/TLS capabilities. Relaxed mode permits a
bounded business trial after an inconclusive complementary test when the endpoint
is reachable, a declared protocol is usable and any HTTPS proxy certificate is
valid. Provider HTTPS hints (or SOCKS declarations) remain required where the
runtime capability is unknown. Authentication requirements and explicit negative
security/capability facts still reject the candidate; no capability is fabricated.

Target diagnostics are runtime-only and keyed by full URL and configuration.
The persisted observation target omits queries; a hash distinguishes full targets.
Three distinct endpoint service indications (default 502/503/504) within 60 seconds
suspend only repetitive complementary work for 30 seconds. An on-demand recovery
is shared by target; unsuccessful recoveries back off to 60 then 120 seconds, and
a healthy response clears suspicion. `ProbeTargetSuspension` configures these
thresholds. Timeouts, TLS/tunnel errors, 403 and 451 alone never create a service
outage verdict. Endpoint checks and eligible business trials continue during the
suspension. `ProxyProbe::target_diagnostics()` exposes this state; there is no country
circuit, direct diagnostic request, periodic task or automatic external fallback.

`supports_https` remains provider metadata and is not overwritten by runtime
probing. HTTP/HTTPS unknown or expired capabilities are validated on demand.
Provider loading stores lists without eagerly testing every endpoint. Unknown
completed results remain throttled for `probe_ttl` (10 minutes by default).
TCP results and completed phase-attempt timestamps are persisted. A completed
8-second validation timeout remains inconclusive; cancellation because another
candidate won does not commit a capability failure or synthetic inconclusive
attempt. A process exit during an unfinished validation can therefore permit a
new attempt after restart. Actual successes already recorded remain valid.

Prechecks are consumed as they finish, with no whole-window collection. Full
validations begin progressively and return as soon as one candidate works.
`probe_batch_size` bounds the concurrent candidate pipelines in one selection
pass, clamped to 1..=8. Each pipeline advances from precheck to validation; all
in-flight stages keep being polled, including awaited persistence. Full
validations are limited separately to two per search. Shared inventory limits
are 16 TCP prechecks and four validations. Saturating validation slots must not
park a precheck holding a commit lock needed by a validator.
Endpoint/phase/configuration operations use weakly registered shared futures,
polled by their subscribers. Provider refresh futures own a `JoinSet` child task
so nested Scrapyfy/HTTP calls execute on a fresh polling stack. One caller's
timeout drops only its subscription; dropping the last subscriber cancels the
operation and aborts its owned child. Cancellation cleanup releases permits when
the runtime processes the abort. There is no detached discovery task. Completed failure/inconclusive results retain
their existing contextual attribution and throttling. Certificate and country
checks are retained before returning a shared result to each caller.
The TCP precheck does not validate protocol or authentication; these remain
part of full validation. Provider hints never become runtime capability facts.

Shared TCP prechecks and capability validations own abort-on-drop child tasks,
so TLS probing does not run on the media handler's native polling stack. Jobs
remain deduplicated by work key; caller deadlines and concurrency slots are
unchanged. Dropping the last subscriber aborts the child, and the runtime then
releases its resources asynchronously.

For HTTPS selection, an eligible proxy previously accepted by the exact
destination is returned before affinity selection or new endpoint prechecks.
Its general probe age alone does not trigger discovery or revalidation. Origin
preferences are shared by the runtime inventory and keyed by scheme, canonical
host and port, never by country. With an injected preference repository, up to
three distinct accepted endpoints and their millisecond UTC success dates are
restored lazily before discovery. The newest eligible success within the allowed
countries wins regardless of country-list order; a disallowed association is
ignored without deletion. Histories do not expire on age alone.
Health, authentication, capability failures, country restrictions and destination
cooldowns still apply. An accepted response clears strictly older `Other` failures
for the same endpoint, scheme, host and port. Preferred selection also reconciles
legacy exclusions against the stored acceptance date and persists repaired records,
so existing caches do not need to be deleted. Equal-date or newer failures, explicit
origin rejections and unrelated exclusions remain intact. Other observed destination
failures temporarily make the association ineligible without deleting its last success;
configured origin rejections remain destination-scoped.
Other candidates from all allowed countries are ranked together for HTTP and HTTPS:
fully validated fresh capabilities first, complete capabilities requiring revalidation
next, then partial relevant capabilities and unknown candidates. Latency, failure
count and stable endpoint identity break ties within a category. Fresh candidates
are tried before any precheck window. An eligible accepted destination history
retains its fast path before country cache imports. Followed-redirect evidence is checked after country cache imports;
affinity reuse without business evidence waits for cold-discovery ownership so
parallel requests cannot bypass coordination through a newly created binding.
The persistent cache is imported before unknown candidates are probed. Discovery
starts after cache exhaustion, 10 seconds of exploration or eight unsuccessful
completed validations, without dropping active cache exploration. Country loads
are single-flight with concurrency two per search and inventory, scheduled by
eligible cache size and last refresh rather than caller order. These thresholds
and the refresh floor are configurable through `InventoryConfig`.
`ProxyDataProvider::load_proxies_progressively` publishes awaited `ProxyBatchSink`
batches directly to selection; the default adapter preserves list-only providers.
A useful batch may win while other sources are still pending. Last-subscriber
cancellation drops unfinished source work. No search budget is reset by discovery.
Probe validation concerns the configured probe destination and does not guarantee
acceptance by every origin. Explicitly unavailable capabilities and invalid proxy
certificates remain ineligible, including on the preference fast path.

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
10-second overall limit for an explicit probe or destination validation
attempt. Inventory capability validation caps the whole probe at eight seconds,
including all protocol candidates and the sole optional fallback. A TCP connection failure for the endpoint itself stops the
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
- `src/core/config/`: one module per public configuration type with its defaults and associated methods, plus private validation and system-environment helpers. Existing configuration imports remain available through the core and crate reexports.
- `src/core/core/`: `ArachneaProxyCore` facade and private modules for request routing, pool selection and state, dynamic country pools, TCP/HTTP connections, proxy hops, UDP, and DNS resolution.
- `src/core/http/proxy_service/`: controller HTTP service facade and private modules for request handling, headers, options, URLs, and redirects, enabled by `controller-service`. The existing public service and URL-construction imports are preserved.
- `src/core/proxy_inventory/`: inventory facade and private modules for configuration, records, discovery, eligibility, selection, preferences, probing, observations, and persistence. Public inventory imports remain available through `core::proxy_inventory` and the existing crate reexports.
- `src/core/proxy_probe/`: `ProxyProbe` facade and private modules for configuration, protocol detection, capability attempts, destination validation, HTTP/SOCKS exchanges, TLS, and results. Existing public probe imports are preserved through reexports.
- `src/connectors/`: adapters for Rust HTTP client ecosystems.
- `src/server/`: CLI, file configuration, listeners, handlers, ACLs, authentication, and safety checks.
- `src/**/tests/`: unit tests loaded as child modules with `#[cfg(test)]` and explicit paths, preserving access to private implementation details. `src/core/tests/core/` and `src/core/tests/inventory/` group scenarios by domain with test-only support modules; inventory lifecycle scenarios are distributed across those inventory domains. Qualified test names now include the domain; filters using the test function name still work.
- `tests/`: integration tests using the public crate API.
- `config-sample/`: TOML examples for direct, upstream, routing, and listener configurations.
- `examples/`: library and connector usage examples.

## Controller HTTP Service

With `controller-service`, `core::http::proxy_service::register_service` registers
the HTTP stream handler under the supplied `name`. The request entry-point URL
comes from `ControlerStreamInput::entry_point`.

Request paths accept
`/<API_PREFIX>/[opts_<BASE64>]/<PROTOCOL>[:<PORT>]/<HOST>/<PATH>[?<QUERY>]`,
for example `/api/proxy/https:8080/example.test/resource`. The optional port in
the protocol segment uses `:`. The public URL constructors also produce paths
containing a full target URL, such as `/api/proxy/https://example.test/resource`.
Options use Base64url without padding. Redirect `Location` headers use a
root-relative path for HTTP/HTTPS entry points and preserve absolute custom-scheme
entry points.

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

Shared response classifiers, search budgets, replay safety and target-scoped
complementary probe suspension operate without background maintenance.

### Durable preferences and endpoint eviction (lot 2)

The application opens one physical `proxy-inventory` SQLite store with three
typed table views: `proxy-inventory`, `proxy-preferences` and `proxy-failures`.
They share a connection in `<application-data>/data/proxy-inventory.sqlite3`,
with adjacent SQLite WAL/SHM files when present. Table schemas and metadata
remain independent. Shared repositories are injected through Scrapyfy;
compatibility constructors retain memory-only stores. Old directory-based
databases are ignored without data import or automatic deletion. New-format
records survive restarts.
Custom `ProxyRepository` implementations must provide
`clear_superseded_destination_failures`: read the latest record and clear only
matching older `Other` failures under the same commit lock as ordinary writes.
This explicit operation prevents conservative `save_many` merges from reviving
an exclusion removed after acceptance. Accepted-history
commits are awaited in the success path; a failed write preserves the response
and memory history, emits a contextual error and increments
`ProxyInventory::preference_persistence_failures()`. No business replay or repair
worker is started. Later accepted responses can retry persistence.

Per-destination locks serialize history updates. Repository commit locks are
shared by wrappers of the same typed-store `Arc`, including across application
rebuilds, preventing file snapshots from committing out of order. Independent
store objects targeting the same file and multiple processes are not coordinated.
There is no multi-table business transaction; restored references are checked against the
inventory and orphans are cleaned on demand.

Only a proven failure to connect to the proxy endpoint can evict it globally.
The 24-hour threshold uses the latest positive evidence, or initial discovery
when no positive evidence exists, never `last_checked`. Before that threshold,
the endpoint is globally unavailable with a configurable `ko_cooldown` (15 minutes
by default), after which a demand-driven connection test may run again. No
timer triggers a test or deletes a still-working endpoint. Authentication, proxy
TLS security failures and remote failures do not independently trigger eviction.

Eviction removes the available memory/persistent record, preference references
and affinity bindings. The separate `proxy-failures` table retains at most 1,024 records
for 48 hours from eviction; cleanup runs on demand. Rediscovery preserves the
initial discovery, positive evidence, exclusions and active cooldown; it is not
a success and cannot immediately revive the endpoint. Explicit origin exclusions
(24 hours by default, configurable) survive technical successes and provider
refreshes, without an implicit early release when all candidates are excluded.
Ambiguous `Other` failures use at most 30 seconds from their latest failure,
preserving shorter configured limits. Eligibility applies this cap to old stored
`Other` exclusions too, without clearing the cache or shortening explicit origin
rejections. Historical expiry fields may retain their old value; selection uses
the capped effective expiry.
Within the new database, missing optional health fields remain readable, but
attempt dates do not invent positive evidence or historical accepted preferences.
Country selection is independent of input order.


### Shared localized search budget (lot 5)

`ProxySearchContext` scopes one logical HTTP call, including redirects and
rotations. It has a monotonic 120-second target and a 15-second extension only
for an already engaged business attempt. New probes, loads, endpoint selections
and retries cannot start during that extension. No operation resets the clock.
After an accepted final response, the search deadline is released: established
media streams continue under their existing transfer policy. Buffered localized
media bodies have a separate 30-second transfer timeout. Ordinary non-search
media behavior is unchanged. Plain tunnel establishment uses 120 seconds without
an HTTP acceptance inference or extension.

Automatic current-country detection is auxiliary work, not an accepted business
response. Its provider runs outside the caller's `ProxySearchContext`, under the
resolver's own timeout (five seconds by default). The enclosing route-selection
deadline and caller cancellation still bound the awaited detection. A successful
geo HTTP response cannot close the pending origin search or bind a cached helper
transport to that search context.

Discovery has no count limit on TCP prechecks, full validations or distinct
business proxy endpoints. It continues until a usable proxy is found, the eligible
inventory/provider candidates are exhausted, or the shared 120-second deadline
expires. Concurrency stays bounded by the per-search and inventory semaphores.
Reservations prevent duplicate technical work within a search; cancellation
releases unfinished reservations so a later selection can resume those candidates.
A shared operation uses one network execution. Reads and already validated
candidates consume no probe reservation.
Cold business discovery is coordinated for each exact destination and canonical
set of allowed countries. One caller owns selection and its initial business
attempt, including when selected through affinity; followers wait within their
own deadline and recheck shared evidence.
A success also wakes discovery already waiting on probes or providers, including
other allowed-country sets. Country sets with different permissions can explore
independently; responses, cookies, bodies and replay guarantees are never shared.
An accepted response or followed redirect releases ownership before transfer;
failure or cancellation releases it so another waiter can continue. Warm callers
reuse eligible accepted or redirect evidence without serializing their transfers.

The selection token records `Discovery`, `AcceptedDestination` or
`RedirectDestination` for its exact endpoint and origin. New candidates keep TCP
3 s / proxy handshake 5 s limits. Reused business evidence permits up to 30 s for
each phase, preserving shorter configured transport limits. The media client
allows 30 s for destination TLS, a 60 s establishment envelope and 60 s for
response headers. Rquest application-created localized clients use a 60 s native
connection envelope and a 60 s send/header timeout for reused evidence; its TLS
phase is covered by that envelope, without an independent TLS-only timer. New
Rquest candidates retain the 15 s connection cap. Accepted-body policies do not
change. These profiles do not extend the shared 120/135 s search horizon.
Selection provenance, preferred-endpoint ineligibility, followed redirects and
selection failures are logged with search IDs without full signed URLs.

Repeated endpoint/URL attempts are excluded locally without changing persistent
health; redirect URLs retain their own attempt identity. Business requests are
sequential and are never shared. SOCKS DNS fallback cannot retry the same dynamic
endpoint inside an active search; unrelated static routing retains its policy.

For endpoints engaged by a localized search, the core caps TCP connection at
three seconds and proxy handshakes (including upstream proxy TLS) at five seconds.
Shorter configured limits remain effective. Other endpoints, ordinary direct/static
traffic and idle/accepted-transfer policies retain their configured limits. These
phase caps do not reset or extend the shared search deadline.

`proxy_retry_allowed` permits retry before proven delivery, or for GET/HEAD after
a relevant transport failure or configured rejection. A modifying operation with
uncertain delivery, including a received rejection, is not retried by default.
An explicit `proxy_retry_replayable: true` media URL option authorizes replay when
the caller guarantees safe effects and a reproducible body. The option is a
boolean and survives redirect option rewriting. Inconclusive responses and body
collection errors do not rotate. Rejections are recorded even when replay is
unsafe. Search exhaustion is reported without silently switching to direct egress.

Rquest loopback adapters for contextual searches are owned by the in-flight
request and dropped on cancellation; their `JoinSet` also aborts active child
connections. They are not placed in the cross-request pinned-client cache, so
contextual calls currently reconnect even when the destination preference is
reused. Ordinary non-search pinned clients retain their keep-alive behavior.
This trades cross-call socket reuse for exact lifetime/deadline ownership with
the current loopback connector API. External reverse proxies must allow the
135-second search horizon; their production settings are deployment-owned.

### Localized search diagnostics

`ProxySearchContext::log_progress()` emits INFO snapshots at validation completion,
origin attempt start, retry and logical response/failure. `search_id` correlates
one call without exposing its URLs or body. The snapshots distinguish reserved
TCP/validation work, completed work, reachable TCP endpoints, eligible validation
results, distinct business endpoints, elapsed/remaining time and acceptance.
Reservations include queued and completed operations; cancelled reservations are
released. Completed shared results count
for each interested search. Cached phase results may be reused without new TCP
work. An eligible relaxed result can still have unknown capabilities: the probe
log's `capability_proven` field distinguishes this from a complete technical proof.

Provider `loaded_count`/`pending_probe_count` describe inventory contents, not how
many endpoints were tested. A search does not eagerly validate an entire list:
it advances progressively through candidates within the shared deadline, with
only two full validations in flight per search. TCP prechecks were already performed before the full-validation INFO
messages; two visible validation messages do not imply two TCP attempts.

### Localized proxy acceptance checks

The existing lifecycle, inventory and repository tests cover discovery beyond the
former 64/32/eight count limits (201 operations/endpoints), redirect budget identity,
per-search slots, subscriber cancellation, bounded deadlines and safe replay
eligibility. Short configurable deadlines verify target/grace behavior, including
accepted transfer completion after the search deadline; default 120/15-second
values are asserted separately. Content-policy fixtures check rejection,
insufficient prefixes, handler errors/timeouts and byte-preserving stream replay.
They complement local pinned HTTP/TLS, eviction and storage-failure fixtures.

```sh
cargo test --offline -p arachnea-proxy -p arachnea-http --lib --features arachnea-proxy/persistence,arachnea-proxy/controller-service,arachnea-http/arachnea-proxy -- --nocapture
cargo test --offline -p arachnea-proxy --test post_action_integration --features controller-service
```

Test-only `lot7` output reports local timing samples and existing fixture counters;
it is not a production metric export or a public-provider benchmark. The full
acceptance matrix, measurements and remaining production checks are recorded in
`docs/dev-tracking/analyse-optimisation-proxies-localises.md`, section 21 at the
repository root. No production thresholds were increased from these samples.
