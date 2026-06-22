# Changelog

All notable changes to the server workspace are recorded here. Add new entries at the end of the file so the history remains append-only.

## Unreleased

### Added
- **YAML `sub_queries` at entry level**: New `EntrySubQueryRaw` type (tagged by `scraper_type: html|json`) allows entries to declare follow-up HTTP requests seeded by the entry value. Supported in both `HtmlScraperEntryRaw` and `JsonScraperEntryRaw`. Parsed into `Box<dyn ScraperQuery>` via `EntrySubQueryRaw::into_boxed_query()`.
- **Unified polymorphic executor**: `scraper::query_executor::execute_query_items` dispatches on `ScraperType` (Html/Json/Static) using the common `ScraperQuery` trait. Single execution path for all query types.
- **`ScraperRequestMethod` and `ScraperRequestHeader` made public** — now visible through the `ScraperQuery` trait return types.

### Changed
- **Module layout**: `scraper_json/query.rs` decoupled into `config.rs`, `response_parser.rs`, `pointer.rs`, `row_extractor.rs`. Same for `scraper_html/query.rs` → `config.rs`, `response_parser.rs`, `row_extractor.rs`.
- **`ScraperManagerQuery` trait removed**: The legacy `async_trait`-based query trait and its single `impl` for `HtmlScraperQuery` are deleted. All query execution now goes through `ScraperQuery` (the unified trait).
- **Legacy `execute_query` methods removed**: `HtmlScraperQuery::execute_query`, `JsonScraperQuery::execute_query`, `StaticScraperQuery::execute_query` — all replaced by the unified executor.
- **`mod.rs` exports cleaned**: `ScraperManagerQuery` removed from public re-exports. `ScraperManager` trait retained for runtime integration.

### Removed
- Dead helpers: `process_root`, `parse_html_rows`, `collect_ordered_results`, `matches` (response_parser), `resolve_request_headers`/`resolve_request_body`/`execution_options` (JsonScraperQuery), `split_static_path`, `render_static_value`, `render_yaml_value`, `render_yaml_values` (StaticScraperQuery).
- `JsonScraperSubQuery::execute` / `execute_siblings` / `execute_context` / `execute_indexed_context` / `execute_indexed_sibling` / `build_row_node` — re-used through the new `JsonScraperSubQuery::execute_query_level` unified entry point (see regression fix below).

### Fixed
- **RTBF Auvio home banners**: Query-level sub-query fetches now execute child sub-queries on fetched responses and resolve nested request headers/bodies from the parent response, allowing `load_home` PROMOBOX banners to include the RedBee HLS `video` URL.
- **coflix.yaml**: Updated `get_entry` query to correctly extract season labels and links from the HTML entry page.
- **coflix.yaml**: Fixed `get_season` query to correctly iterate over all episodes in the `episodes` array.
- **Entry-level sub-query merge shape**: `execute_entry_sub_queries` now wraps each decoded sub-query value inside a named child node (`{ "embed-link": ["url"] }`) instead of pushing bare scalar items. This causes the serializer to emit an array of objects (`"players": [{"embed-link": ["url1"]}, …]`) instead of a flat string array (`"players": {"embed-link": ["url1", …]}`). Fixes the `players` output shape for `coflix.yaml` `get_entry`.
- **`rtbf-auvio-be.yaml` `list_lives` flattening**: the RTBF live catalog now declares `result_item_field: entries` and groups `/data/content/*` under `entries`, so the unified executor returns one live item per channel instead of merging the whole feed into a single result.
- **RTL Play login cookie handling**: `scrapyfy::HttpClient` now exposes materialized responses and a shared-cookie lookup for one URL, letting `rtlplay_resolver` reuse real redirect-set cookies from the HTTP jar instead of scraping `lfvp_device_id` and auth cookies from response bodies.
- **Scraper HTTP fetch diagnostics**: `scrapyfy::HttpClient` now includes the underlying HTTP/proxy error in `Fetch fail ...` messages, making proxy-dependent failures diagnosable from API logs.
- **TF1 mediainfo diagnostics**: TF1 playback now reports mediainfo HTTP status/body details and sends browser-like `Accept`, `Origin`, and `Referer` headers, improving proxy failure diagnostics without exposing auth tokens in header logs.
- **Proxy CONNECT diagnostics**: the rquest loopback proxy now logs failed CONNECT requests and proxy-hop handshake errors instead of silently dropping them, making upstream SOCKS failures visible.
- **Proxy pool transient failures**: tunnel proxy-pool selection no longer opens a destructive preflight tunnel before the real connection, compatibility checks retry a candidate once before marking it unusable, and an exhausted pool gets one fresh scan of KO members before failing, reducing false negatives from flaky SOCKS upstreams that intermittently return `network unreachable`.
- **6play front-auth login**: M6Play login token retrieval is now serialized to avoid concurrent Gigya/front-auth request storms, while still reporting HTTP status/body details when `front-auth.6cloud.fr` returns non-JSON.

## Unreleased — RTBF Auvio home banner RedBee auth simplification

### Fixed
- **`rtbf-auvio-be.yaml` `load_home` PROMOBOX banners**: RedBee anonymous auth for banner preview video no longer depends on scraping the embed page `deviceId`. The query now posts a stable anonymous WEB device payload after extracting only the `assetId`, which keeps `banners > video` populated even when the embed state shape changes.
- **Nested entry sub-queries on fetched responses**: `query_executor` now continues executing entry-level sub-queries after a fetched HTML/JSON sub-query populates its fields, and it preserves scalar results when a nested sub-query resolves back into the same field name. This restores RTBF Auvio PROMOBOX `video` extraction and makes `row_filters` apply on fetched JSON entry sub-queries as expected.

## Unreleased — query-level sub-query regression fix

### Added
- **`ScraperQuery::as_any()`** — returns `&dyn Any` so the unified executor can downcast back to the concrete query type when a query-level sub-query (legacy semantics) is encountered.
- **Query-level sub-query detection on the trait** — `ScraperQuery::context_pointer()`, `context_select()`, `context_entries()`, `target()` are now part of the trait (default `None`/empty). `JsonScraperSubQuery` and `HtmlScraperSubQuery` override them to expose their legacy fields.
- **`JsonScraperSubQuery::execute_query_level`** — unified entry point that reuses the legacy `execute` path for the `context_pointer`/`filters`/`row_filters`/`request_actions`/`target` semantics.
- **`query_executor` query-level dispatch** — when a sibling sub-query reports `context_pointer().is_some()`, the unified executor downcasts to `JsonScraperSubQuery` and calls `execute_query_level` against the parent row, instead of recursing into the empty-URL path.
- **Helpers in `query_executor`**: `item_to_parent_value` and `query_level_execution_options` to bridge the executor to the legacy `JsonScraperSubQuery` execution path.

### Fixed
- **Regression on query-level sub-queries** introduced when the unified executor was wired in (Step 16). YAMLs using the legacy `sub_queries` block at query level with `context_pointer`/`target`/`request_pointer` (e.g. `coflix.yaml`'s `get_season`, `rtbf-auvio-be.yaml`, `m6play-fr.yaml`, `tf1-fr.yaml`) used to be executed by the old per-query `execute` method and were silently broken by the new engine. The unified executor now dispatches them to the legacy path, restoring the previous behaviour without requiring a YAML migration.
- **`m6play-fr.yaml` `load_home` follow-up** — the follow-up sub-query has no `context_pointer` (single-context mode). The unified executor now dispatches **every** `JsonScraperSubQuery` sibling through `execute_query_level` regardless of whether `context_pointer` is `Some` or `None`, not only when it is `Some`. Previously only the `coflix` pattern (`context_pointer: /episodes/*`) was correctly dispatched; the `m6play-fr` pattern (no `context_pointer`, `target: banners`) fell through to the broken entry-level recursion.
- **`as_html_entry` / `as_json_entry`** in `query_executor` now use `entry.as_any().downcast_ref::<…>()` (matching the `as_any` plumbing added to `ScraperEntrySpec` in Step 15). This unblocks the polymorphic dispatch of entries to their concrete types.

## Unreleased — static query regression fix

### Fixed
- **Silent regression on `StaticScraperQuery`**: the unified executor's `FetchedResponse::Static` branch in `extract_items` was downcasting `&dyn ScraperEntrySpec` to `&JsonScraperEntry`, which always failed for the actual `StaticScraperEntryRaw` instances. The result was an empty `ScraperDataNode` for every static query — observable on `m6play-fr.yaml`'s `service_stream_metadata` (`get_service` returning `{}` instead of `{ id, title, logo, description }`) and on every other YAML using `scraper_type: static`. Fixed by adding `StaticScraperEntryRaw::apply_to` (concrete method, mirrors the legacy `StaticScraperQuery::execute_query` template pipeline using the already-present `split_static_path` / `render_yaml_value` / `render_yaml_values` helpers) and a dedicated downcast branch in `extract_items` that calls it. `extract_items` now returns `Result<Vec<ScraperDataNode>>` to surface unresolvable template placeholders instead of silently producing empty items.

## Unreleased — TF1 FR proxy routing

### Added
- **Scraper HTTP proxy country hint**: `ScraperHttpConfig::proxy_country(...)` now normalizes country values and passes them through the HTTP proxy parameter path so resolvers can request country-specific proxy routing without replacing the existing `create_http_client(...)` flow.
- **Catchable HTTP proxy errors**: `arachnea-http` now exposes `ArachneaHttpError::Proxy` for configured proxy transport failures, allowing resolvers to distinguish proxy failures from upstream service failures.

### Changed
- **TF1 resolver FR routing**: TF1 resolver HTTP calls now request `proxy_country("FR")`, including license proxy calls. When that FR proxy path fails, TF1 logs a warning and retries without the FR country hint as a temporary workaround.
- **Stream proxy configuration**: `arachnea-stream` now configures the FR proxy with `ProxyNode::from_url("proxy-fr", "socks5://...")` and a `country_routing` handler instead of making the SOCKS proxy the default route for all HTTP traffic.

## Unreleased — chaser-cf request header forwarding fix

### Fixed
- **`chaser-cf` custom request headers**: browser-backed page fetches now propagate per-request headers to Chrome before navigation, and `Referer` now uses Chrome's native navigation referrer field instead of an injected extra header. The Cloudflare refresh path also reuses the blocked request headers, so solver refreshes keep request-specific headers such as `Referer`. The explicit `chaser-cf` feature build also no longer fails due to missing `custom_headers` arguments.
- **`chaser-cf` referer observability**: when a navigation expects a `Referer`, the engine now logs the `observed_referer` and Chrome referrer policy reported by the outbound CDP request event, making it easier to distinguish a missing header from an upstream 403.

## Unreleased — HTTP proxy transport configuration

### Added
- **Built-in HTTP engine proxy configuration**: `ArachneaHttpConfig` now exposes `HttpProxyConfig`, `proxy_url(...)`, and, with the `arachnea-proxy` feature enabled, `proxy_core(...)` so built-in HTTP engines can route traffic through an explicit network proxy or an in-process `arachnea-proxy` core.

### Changed
- **Built-in engine proxy handoff**: `rquest` and Ghostwire now accept the configured proxy transport from `ArachneaHttpConfig`; when `proxy_core(...)` is used, `arachnea-http` starts a managed `arachnea-proxy` loopback helper because the upstream client APIs used here still expect proxy URLs.
- **ScraperManager default proxy bootstrap**: `StreamScraper` now enables the shared scraper HTTP proxy by default, so lazily created `scrapyfy::HttpClient` instances inherit the in-process proxy transport without requiring per-service wiring.
- **Documentation and tracking**: `arachnea-http` README now documents proxy transport selection, and the completed HTTP proxy task has been removed from the root `TODO.md`.

## Unreleased — scrapyfy invalid JSON payload logging

### Fixed
- **Scraper JSON parse diagnostics**: `arachnea-scrapyfy::HttpClient` now logs the raw response content that failed JSON parsing, including `__NEXT_DATA__` payloads, with truncation metadata so API errors like `Invalid JSON payload returned by ...` can be diagnosed from backend logs without changing the error sent to the frontend.
