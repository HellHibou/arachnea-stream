# arachnea-scrapyfy

## Source organization and validation

Production code lives under `src/`; existing tests mirror the exact source paths under crate-root `tests/`. Shared helpers and feature-gated test methods live under `tests/support/`. Their existing public paths, including `scrapyfy::scraper_manager::tests::test_query`, remain available with `test-support`. Unit suites use explicit conditional `#[path]` declarations; `autotests = false` prevents accidental integration targets.

The unified executor in `scrapyfy/scraper/query_executor.rs` delegates fetching, extraction, entry sub-queries and merging to private modules. The aggregator separates source loading, manifest expansion, execution and feature-gated diagnostics. Query collections separate execution and parameter handling; HTML/JSON conversions and administration settings handlers have dedicated private modules. Public import paths remain available through reexports.

`src/lib.rs` centralizes documented constants and relevant defaults for cache identities, administration, JavaScript, HTTP, concurrency and proxy providers. Values, feature gates, YAML contracts, cache-key encoding and retry policies are preserved. Mutable caches, routing hooks and registries remain with their owning implementations. Every Rust file stays within 1,000 lines.

From `server/`, run `cargo test -p arachnea-scrapyfy` for the default configuration or `cargo test -p arachnea-scrapyfy --all-features` for all compiled features. Existing tests use temporary manifests, local persistence, checked-in service YAML and local proxy/HTTP fixtures. They do not validate live providers or native browser windows. The localized proxy command below also checks the SQLite-enabled configuration without requiring optional browser engines.


With `arachnea-proxy`, the crate also enables the proxy crate's
`controller-service` feature for the proxied-media URL helpers used by
`resolve_url`. Standalone builds do not require feature unification from the
application. The localized-proxy test suites run with:

```sh
cargo test --offline -p arachnea-scrapyfy --lib --features arachnea-proxy,sqlite-persistence
```

## Direct fallback diagnostics

The generic proxy defaults to returning an error when no country proxy is available.
Applications may opt into direct fallback on the injected proxy core with
`with_fallback_direct(true)`. Each source execution collects non-fatal fallback
errors and returns them in the existing `errors` envelope alongside any successfully
extracted `data`, including for `get_entry`. Concurrent sources have separate
collectors. Responses with errors do not expose a client ETag or short-circuit as
HTTP 304, so clients receive the diagnostic even when remote content is unchanged.
No scraper YAML configuration is added by this policy.

## Proxy response policies

With `arachnea-proxy`, YAML `http.proxy_response_classifier` selects a shared
known policy: `{ type: default }`, `{ type: http403 }`, or
`{ type: statuses, rejected_statuses: [403], accepted_statuses: [200, 206], cooldown_seconds: 86400 }`.
The optional policy inherits from the parent unless explicitly overridden.
An injected `HttpClient::with_proxy_response_classifier(ProxyResponsePolicy)`
takes precedence and is retained by configured/isolated-cookie clients. Otherwise
the explicit known policy replaces (does not combine with) the legacy
`proxy_rejection_statuses` list. Status lists and cooldowns are validated when
constructing the transport. Default acceptance is 2xx only; unconfigured 4xx,
5xx and redirects are inconclusive, without an automatic exclusion or new
preference. A 403 rejection remains opt-in, including FranceTV and TF1.

Scrapyfy applies the early Rquest decision to the exact response token, rather
than reclassifying a transformed/materialized body. A custom policy can inspect
a non-destructive prefix up to 64 KiB within three seconds and the remaining
search time. Unsupported engines return an explicit unsupported-operation error
inside correlated localized searches and do not update preferences or exclusions
by inference. The HTTP facade owns safe retries within the common budget.

## Durable localized proxy histories

The application can inject inventory/failure and preference repositories through
`ScraperAgregator::set_proxy_repositories()` before `ensure_proxy_core()`.
Provider snapshots share those repositories, including their commit coordination.
Compatibility constructors use memory-only repositories. Accepted histories are
restored before provider discovery and retain three endpoints per scheme/host/port;
the current country set filters eligibility without deleting other associations.
Destination exclusions survive technical successes and provider refreshes.
Storage failures do not replay a successful business request. Allowed countries
are normalized, deduplicated and sorted for stable request identities. Their order
has no routing priority. The inventory ranks all allowed countries together after
importing their persistent caches, preferring fresh validated capabilities before
revalidation or unknown candidates for both HTTP and HTTPS. Provider requests
also use canonical country sets. Country loads run with concurrency two and are single-flight per country.
Discovery sources publish batches in completion order, independently of ordinary
scraper aggregation, which keeps configuration ordering and its cache contract.
Cache exhaustion, 10 seconds of exploration or eight unsuccessful validations
start discovery without cancelling cache exploration. Country scheduling uses
eligible cache contents and last refresh; observed transport failures preserve a
10-second refresh floor (normal cooldown: 120 seconds).

Already localized rows are published immediately. Unknown countries are resolved
for at most 20 distinct IPs per country load, sharing four concurrent geo lookups
and subscriber-owned per-IP work. Positive/negative results are cached for 15
minutes/60 seconds in a bounded 1,024-entry runtime cache. Endpoint/declaration
deduplication precedes geo work; unresolved countries are never guessed. Dropping
the last load subscriber cancels pending sources and geo work. Probe methods,
response policies and target-scoped suspicion are configured through the injected
`ProxyProbe` (see the proxy crate README); they never suspend a country.

## Geo-proxy request timeout

Localized calls scope one `ProxySearchContext` before waiting for the shared HTTP
client. The target is 120 seconds through accepted response classification, with
15 additional seconds only for an already engaged business attempt. Probes,
loads, redirects and rotations share counters: 64 TCP precheck reservations,
32 full-validation reservations and eight distinct business endpoints. Queued
cancelled work conservatively consumes its reservation. Per-search/shared
concurrency is 8/16 TCP, 2/4 validations and 2/2 country loads.

The HTTP facade owns retries, so Scrapyfy does not create another retry loop or
add 120 seconds to each attempt. GET/HEAD may retry relevant transport failures
or configured rejections; modifying requests require proven absence of sending,
or explicit `http.proxy_retry_replayable: true` and a reproducible body. The
optional boolean inherits normally; `false` overrides a parent's authorization.
Inconclusive responses and errors collecting an already received body do not
rotate. After accepted classification, the ordinary 25-second body transfer
timeout applies separately. Ordinary requests retain their 25-second timeout.
The search is per logical HTTP call, not an entire multi-query resolver sequence.

Derived clients share a bounded pool of idle Rquest transports as well as the
application loopback listener, while their mutable routing handles remain independent.
Each call selects its route again and exclusively rebinds any compatible idle
transport to the current token and deadline before reuse. Up to two idle transports
per route and 32 transports globally are retained; entries idle for 60 seconds are rejected on access. Only complete,
accepted attempts or explicitly permitted followed redirects return to the pool. Connection reuse depends on server keep-alive;
closed connections reconnect under the current request's context. Cookies and
HTTP responses are not shared by this transport pool. Browser or
custom engines lacking correlated early observation reject this search mode;
explicit browser-page APIs remain separate from the correlated HTTP contract.

`arachnea-scrapyfy` contains the generic scraping engine used by Arachnea.

## Proxy Provider Lifetime

With `performance-diagnostics` and DEBUG enabled, each polled provider source
query logs its source identifier, query name, optional
requested country, elapsed time, output row count and termination outcome
(`completed`, `failed` or `cancelled`). Parameters, URLs, response bodies and
credentials are not added to these diagnostics. A completed source query is
separate from downstream country filtering, geolocation, inventory merge and
business acceptance. Other sources and already published batches remain available
after a source failure; the refresh-attempt cooldown is unchanged.

Proxy and IP-country data providers own independent snapshots of their source
manifests and effective activation settings. They do not borrow the application
aggregator, so replacing it cannot invalidate an in-flight provider request.
Provider runtimes share persistence and local-country state, but use a separate
system-proxy handle to avoid an ownership cycle with the dynamic proxy core.
These independent routing handles share the application loopback runtime, so
provider bootstrap does not open another listening port.
Applications must publish a newly built core on reload to apply changed sources;
proxy lists are still loaded lazily for country-based requests.

## Responsibilities

- Define scraper query models for HTML and JSON sources.
- Execute configured queries through the shared HTTP client.
- Apply scraper actions and post-processors.
- Load and aggregate query collections from source configuration files.

## Notes

Keep source-specific response shaping in YAML actions or generic post-processors whenever possible. Service-specific Rust should remain limited to behavior that cannot be expressed generically.

### Dynamic proxy response attribution

HTTP response acceptance and configured status rejection use the request-owned
selection from `ArachneaResponse`, not the most recent connection for a destination.
Redirects attribute the final response only to its own origin. Uncorrelated browser
responses do not update dynamic proxy preferences or exclusions. Transport errors
after response headers preserve the proxy and do not trigger transport rotation.
Legacy rejection-status configuration now adapts to the shared classifier above;
safe sequential retries use the shared deadline and replay policy described above.

## Shared local proxy server

With `arachnea-proxy`, `SrcapyfyApplicationOptions` accepts `--proxy-server-port`
(or `proxy-server-port` in the persisted application option map). Absent or zero
selects a port automatically; a nonzero value requires that exact port. Values
outside 0..65535 and explicit use without the proxy feature are rejected.

Create a `LoopbackProxyRuntime::new(port)` before building the aggregator and
inject it through `SharedProxyConfigHandle::with_runtime(runtime)` and
`ScraperAgregator::new_with_typed_stores_and_proxy_handle(...)`. After installing
the effective proxy core, call `proxy_handle.start_proxy_server().await` to
publish the default route and bind before external clients connect. Stream
performs these steps at startup. The effective `127.0.0.1:<port>` address is
logged once at info level. A fixed port bind failure stops startup; changing the
configured port requires restarting the application.

Every HTTP client, pinned Rquest attempt, browser fallback and provider snapshot
from the aggregator uses that runtime. Rebuilt aggregators must receive the same
runtime and publish their new default route only after successful construction;
Stream preserves the port across source reloads. Existing connections retain
their routing lease. The server stops when the final runtime owner is released,
terminating its accepted connections. An owner can call `runtime.shutdown()` to
stop acceptance and connections explicitly even while clients retain references;
the stopped runtime cannot restart.

Tauri/Wry fallbacks also use this port and retain their request's routing context.
On macOS 14 or later, native WebViews use authenticated SOCKS5 on the same socket;
other clients continue to use HTTP/CONNECT. No separate browser listener is created.
Proxy WebViews use isolated data stores and configure authentication before navigation.
See the HTTP crate README for native platform constraints.

For external applications, configure the logged address as an HTTP proxy
(including for HTTPS destinations). A browser receives a `407` challenge before
routing when it supplies neither a profile nor explicit proxy parameter headers.
Use username `countries=BE,FR&proxy_affinity=browser-session-1` and password
`local`, or `default` / `local` for the application's default route. Parameter
values inside the username use URL form encoding. These credentials select a
routing profile and do not authenticate a local user. The default route retains
the application's direct-fallback policy.

Only IPv4 loopback is bound; use `127.0.0.1`, not an assumed IPv6 `localhost`.
In a container, the address is local to that container's network namespace.
HTTPS tunnels remain opaque to the server, so external HTTPS response statuses
cannot drive the scraper's correlated response classification.


### Optional proxy domain groups

`http.proxy_domain_group` is an optional list of domain names whose accepted
proxies can supply defaults to each other, including their subdomains:

```yaml
http:
  proxy_countries: ["FR"]
  proxy_domain_group: ["m6.fr", "6play.fr", "6cloud.fr", "bedrock.tech"]
```

The list is passed through the generic proxy parameters. An empty child list
inherits its parent, as with `proxy_countries`; a nonempty child replaces it.
It affects proxy selection only for group members. Exact accepted preferences,
explicit affinity, allowed countries, target exclusions and content callbacks
remain effective. The list does not force proxy use for returned media URLs or
share cookies, tokens or connections between hosts. Without a dynamic proxy route
it has no routing effect.


## Optional performance diagnostics

`performance-diagnostics` is disabled by default and forwards to HTTP and to an
already enabled optional proxy dependency. Provider source-query observers are
compiled only with both proxy support and this feature. Disabling it removes
their timing/outcome state; progressive publication, failure handling and
cancellation continue unchanged. Country-hint forwarding is logged at DEBUG.
The Stream application's feature enables all relevant diagnostics together;
setting `RUST_LOG` alone does not compile them back in.
