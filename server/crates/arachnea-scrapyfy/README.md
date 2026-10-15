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
request timeout. Unsupported engines have no correlated decision and do not
update preferences or exclusions by inference. Existing retry counts are not
expanded; the common search deadline and replay policy remain lot 5 work.

## Durable localized proxy histories

The application can inject inventory/failure and preference repositories through
`ScraperAgregator::set_proxy_repositories()` before `ensure_proxy_core()`.
Provider snapshots share those repositories, including their commit coordination.
Compatibility constructors use memory-only repositories. Accepted histories are
restored before provider discovery and retain three endpoints per scheme/host/port;
the current country set filters eligibility without deleting other associations.
Destination exclusions survive technical successes and provider refreshes.
Storage failures do not replay a successful business request. The remaining
non-preferred country ordering is unchanged until localized proxy lot 4; the YAML
schema and response classification policy are not changed by lot 2.

## Geo-proxy request timeout

HTTP clients using the Arachnea proxy transport with non-empty country hints
allow `PROXY_CONNECT_ESTABLISHMENT_TIMEOUT` plus 25 seconds for each HTTP request
(145 seconds with the current two-minute CONNECT budget). This leaves time for
the origin request after proxy selection and connection. Other requests retain
the 25-second timeout. The timeout is per attempt, not a deadline for a complete
resolver sequence or its retries; per-candidate probe limits remain unchanged.

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
retry counts remain unchanged and replay-safety budgets remain lot 5 work.
