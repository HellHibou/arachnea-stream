# arachnea-scrapyfy

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

Contextual Rquest loopbacks are owned by the active request rather than cached
across requests, to preserve exact cancellation/deadline ownership. Destination
preferences still provide fast selection, but these calls reconnect. Browser or
custom engines lacking correlated early observation reject this search mode;
explicit browser-page APIs remain separate from the correlated HTTP contract.

`arachnea-scrapyfy` contains the generic scraping engine used by Arachnea.

## Proxy Provider Lifetime

Proxy and IP-country data providers own independent snapshots of their source
manifests and effective activation settings. They do not borrow the application
aggregator, so replacing it cannot invalidate an in-flight provider request.
Provider runtimes share persistence and local-country state, but use a separate
system-proxy handle to avoid an ownership cycle with the dynamic proxy core.
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
