# arachnea-scrapyfy

## Source organization and validation

Production code lives under `src/`; existing tests mirror the exact source paths under crate-root `tests/`. Shared helpers and feature-gated test methods live under `tests/support/`. Their existing public paths, including `scrapyfy::scraper_manager::tests::test_query`, remain available with `test-support`. Unit suites use explicit conditional `#[path]` declarations; `autotests = false` prevents accidental integration targets.

The unified executor in `scrapyfy/scraper/query_executor.rs` delegates fetching, extraction, entry sub-queries and merging to private modules. The aggregator separates source loading, manifest expansion, execution and feature-gated diagnostics. Query collections separate execution and parameter handling; HTML/JSON conversions and administration settings handlers have dedicated private modules. Public import paths remain available through reexports.

Private helpers borrow URL proxy policy, header-grouping configuration, fetch request/response settings and aggregation-pass options through named views. The fetch request owns only the HTTP method that was already moved into the request. Public APIs, YAML fields, parsing/preprocessing order, source selection, validators and cache interactions remain unchanged; these views introduce no new heap allocation or collection/string clones.

The HTTP facade delegates shared proxy routing to `scrapyfy/http_client/proxy.rs`
and lazy transport construction/versioned caching to `http_client/transport.rs`.
The node model delegates output type names and serde to
`scrapyfy/scraper_data_node/output_type.rs`, typed JSON rendering to `render.rs`,
and raw deserialization/conversion to `raw.rs`. These modules stay private;
existing public type paths, serialized values, routing and tracing targets are
preserved. Each Rust file stays within 1,000 lines.

`src/lib.rs` centralizes documented constants and relevant defaults for cache identities, administration, JavaScript, HTTP, concurrency and proxy providers. Values, feature gates, YAML contracts, cache-key encoding and retry policies are preserved. Mutable caches, routing hooks and registries remain with their owning implementations.

From `server/`, run `cargo test -p arachnea-scrapyfy` for the default configuration or `cargo test -p arachnea-scrapyfy --all-features` for all compiled features. Existing tests use temporary manifests, local persistence, checked-in service YAML and local proxy/HTTP fixtures. They do not validate live providers or native browser windows. The localized proxy command below also checks the SQLite-enabled configuration without requiring optional browser engines.


With `arachnea-proxy`, the crate also enables the proxy crate's
`controller-service` feature for the proxied-media URL helpers used by
`resolve_url`. Standalone builds do not require feature unification from the
application. The localized-proxy test suites run with:

```sh
cargo test --offline -p arachnea-scrapyfy --lib --features arachnea-proxy,sqlite-persistence
```

## Test-only source-check support

With `test-support`, `scrapyfy::scraper_manager::tests` retains the existing
`test_query` and `TestParams` imports and exposes `read_strict_yaml` and
`assertions::{Expectations, AssertionOutcome, AssertionReport}`. These helpers
have no Stream dependency. The strict YAML reader rejects duplicate/non-string
mapping keys, tags and scalar coercion; the target type supplies its schema.
`Expectations::validate` checks useful-data minima and field paths before
`evaluate` examines public JSON roots. Reports distinguish extraction/contract
failures, include invalid-item counts, paths and observed JSON kinds without
field values, and preserve shape errors even
when other branches contain enough data. `first_valid_string` discovers a
bounded input from valid upstream items without making requests.

The legacy `test_query` now rejects zero rows, blank-only extraction and results
containing only source identifiers. Explicit expectation files reject unknown
keys, duplicate queries/fields and required/optional conflicts. Optional fields
may occur on any subset of rows; new fields remain diagnostic. The existing
production-field fallback and `ignore_entry_not_mapped` compatibility remain
for remaining legacy callers; they never permit wholly empty useful data. Stream
scenarios now use the independent v1 loader and JSON assertions.
The repository no longer contains `server/data-test/` files or callers of the
legacy `test_query` helper. Its optional external expectation-file lookup remains
available for compatibility; Stream uses only its crate-owned v1 fixtures.
See [the service-test YAML guide](../../../docs/specifications/arachnea-stream-source-checks-en.md)
for the independent source-check format, which is separate from production YAML.

## Rust API contracts

### Proxy configuration migration (Unreleased)

Direct Rust construction of the reexported in-process proxy variant now uses
`HttpProxyConfig::Arachnea(Box::new(core))`. The HTTP builder's `proxy_core(core)`
argument is unchanged. Constructing or cloning this variant allocates one box;
cloning still clones the contained core with its existing shared handles and
copied scalar settings. This also applies when Scrapyfy clones HTTP configuration
for derived clients. Owned-core consumers clone `core.as_ref()` at the existing
clone points without allocating an intermediate box. YAML proxy fields, query
behavior, routing and loopback lifecycle are unchanged. See the HTTP README for
owned and borrowed pattern-match migration guidance.

### Execution method migration (Unreleased)

Two public execution methods now take borrowed parameter structures, exported
from both `arachnea_scrapyfy` and `arachnea_scrapyfy::scrapyfy`. External Rust
callers must migrate; the old long signatures are removed.

| Method | New arguments, in order, after `self` |
| --- | --- |
| `ScraperAction::apply_with_dynamic_variables` | `selected`, `texts`, `params`, `ActionResponse`, `dynamic_variables` |
| `ScraperAgregator::execute_query_async` | `context`, `query_parameters`, `group_name`, `query_name`, `params`, `ScraperAggregationOptions` |

Move the action's old response arguments into
`ActionResponse { request_url, response_body, response_json }`. Keep the same
optional response values and dynamic-variable store. The legacy `apply` method
retains its signature and fallback behavior. The fallible method still reports
duplicate-variable and extraction errors without rolling back earlier inserts.

Move the aggregator's last six arguments into
`ScraperAggregationOptions { source_params, scrapper_list,
query_media_type_filter, fields_filters, source_field_name, operation }`.
Keep all supplied values, including `None`, unchanged; these structures add no
defaults or validation. Vector and map references remain borrowed as before.
The owned `QueryParameters` argument stays separate.

These structures add no heap allocation or cloning. Source order, result shaping,
cache modes, ETags, conditional responses and error correlation remain unchanged.
YAML fields, serialized actions and HTTP/frontend contracts are unchanged.

### Constructor migration (Unreleased)

Direct Rust constructor calls now use three public parameter types, available
from both `arachnea_scrapyfy` and `arachnea_scrapyfy::scrapyfy`. This is a Rust
source API change; YAML, HTTP/frontend contracts and serialized formats remain
unchanged. Update calls as follows:

| Constructor | New arguments, in order |
| --- | --- |
| `JsonScraperQuery::try_new` | `QueryEndpoint`, `JsonQueryConcurrency`, `filters`, `row_pointer`, `scraper_entries`, `sub_queries` |
| `TextScraperQuery::try_new` | `QueryEndpoint`, `row_delimiter`, `field_delimiter`, `result_item_field`, `scraper_entries` |
| `ScraperQueryCollection::new` | `CollectionMetadata`, `parameters`, `proxy_insecure_tls_hosts`, `parameter_defaults`, `queries` |

Move the old `name`, `base_url`, `media_types` and `query_url` arguments into
`QueryEndpoint { name, base_url, media_types, query_url }`. The three strings stay
borrowed until construction copies them at the existing points; `media_types`
still moves into the query. Move the old sibling/context/fetch limits into
`JsonQueryConcurrency { sibling_sub_query_concurrency,
sub_query_context_concurrency, sub_query_fetch_concurrency }`. Supply the same
explicit limits; zero still fails, after row-pointer validation. These parameter
types introduce no new defaults or normalization.

For a collection, move `name`, `title`, `logo` and `description` into
`CollectionMetadata { name, title, logo, description }`. The strings remain
borrowed inputs and the existing multilingual map moves into the collection.
Query insertion order and all other constructor statements are preserved;
no additional string/collection clone or heap allocation is introduced.
The existing seven-argument `JsonScraperQuery::new` convenience API is unchanged;
its internal delegation still uses the same 4/4/8 concurrency defaults.

Proxy replacement configuration calls also migrate from `ReplaceAll::new` to
`ReplaceAll::config` with identical arguments and return values; see the Proxy
README. `ReplaceAll::ordered` is unchanged. The old long signatures and factory
name are removed, so external direct callers must migrate.

`ScraperAgregator::default()` delegates to `ScraperAgregator::new()`. It performs the same empty-aggregator construction, independent in-memory-store setup and feature-gated proxy initialization. Existing callers of `new()` retain their behavior and cost.

`HttpClient::new` keeps its base-URL argument for compatibility and does not use it. Derived clients share routing, local-country and session-store handles. Cookie isolation creates a separate cookie cache; it does not create separate routing configuration. Ordinary textual HTTP requests attach a body only for POST; an omitted POST body uses the URL query string. Binary request helpers attach any supplied body independently of the method. Browser-page fetches use their own request-body and token configuration.

Source-scoped controller parameters omit JSON nulls, preserve strings and convert numbers, booleans, arrays and objects into their JSON text. Later entries for the same source overwrite matching parameter names. Server-cache reads and writes are best-effort and degrade to misses or skipped writes, while explicitly requesting the underlying cache handle can return an initialization error. Source-record batch writes inherit transactional guarantees from their typed-store backend.

Text-entry collection loading currently traverses nested entries without rewriting or validating their action placeholders. Those placeholders resolve when actions execute; common query settings retain their load-time collection resolution. The collection parameter argument on the entry traversal is reserved for that shared interface.

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
