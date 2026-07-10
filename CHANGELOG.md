# Changelog

All notable changes to the server workspace are recorded here. Add new entries at the end of the file so the history remains append-only.

## Unreleased

### Added
- **Dynamic proxy loading analysis**: Added a French design analysis for country-based dynamic proxy loading, proxy probing, persistence, and the `arachnea-proxy`/`arachnea-scrapyfy` trait boundary.
- **YAML `sub_queries` at entry level**: New `EntrySubQueryRaw` type (tagged by `scraper_type: html|json`) allows entries to declare follow-up HTTP requests seeded by the entry value. Supported in both `HtmlScraperEntryRaw` and `JsonScraperEntryRaw`. Parsed into `Box<dyn ScraperQuery>` via `EntrySubQueryRaw::into_boxed_query()`.
- **Unified polymorphic executor**: `scraper::query_executor::execute_query_items` dispatches on `ScraperType` (Html/Json/Static) using the common `ScraperQuery` trait. Single execution path for all query types.
- **`ScraperRequestMethod` and `ScraperRequestHeader` made public** — now visible through the `ScraperQuery` trait return types.
- **Proxy `RemoveHeader` redirect action**: controller proxy URLs can now carry `Arachnea-Proxy-RemoveHeader` JSON action headers with `type` and `headers` fields that remove selected `opts.headers` entries when an HTTP `302` redirect is rewritten.

### Changed
- **Module layout**: `scraper_json/query.rs` decoupled into `config.rs`, `response_parser.rs`, `pointer.rs`, `row_extractor.rs`. Same for `scraper_html/query.rs` → `config.rs`, `response_parser.rs`, `row_extractor.rs`.
- **`ScraperManagerQuery` trait removed**: The legacy `async_trait`-based query trait and its single `impl` for `HtmlScraperQuery` are deleted. All query execution now goes through `ScraperQuery` (the unified trait).
- **Legacy `execute_query` methods removed**: `HtmlScraperQuery::execute_query`, `JsonScraperQuery::execute_query`, `StaticScraperQuery::execute_query` — all replaced by the unified executor.
- **`mod.rs` exports cleaned**: `ScraperManagerQuery` removed from public re-exports. `ScraperManager` trait retained for runtime integration.
- **Stream proxy endpoint ownership**: `StreamScraper` now registers the generic HTTP `proxy` stream route and passes the controller-computed public proxy path into player resolvers, so RTBF Auvio media URLs no longer depend on a hardcoded `/api/proxy/` path.
- **Server public API mount configuration**: `arachnea` now accepts `--entrypoint-root` and `--entrypoint-api` in server mode so generated stream URLs can match reverse-proxy mounts such as `/arachnea/api/proxy`.
- **YAML URL proxy option**: `resolve_url` and `resolve_url_from_parent` now accept optional `proxy: true` to wrap resolved HTTP(S) URLs through the controller-computed generic HTTP proxy path. Omitting `proxy` preserves direct URL output.
- **Static query actions**: `scraper_type: static` entries with scalar `value` now honor their `actions` pipeline, allowing metadata fields such as service logos to use `resolve_url proxy: true`.
- **M6Play manifest proxy country hint**: `proxied_url` now accepts an optional country hint. When provided, generated controller proxy URLs carry `Arachnea-Proxy-Country` through the existing `opts` header mechanism; when omitted, URL generation is unchanged.
- **M6Play manifest redirects**: M6 manifest proxy URLs now remove the `Arachnea-Proxy-Country` option after HTTP `302` redirects so the redirected manifest request no longer forces country routing.
- **M6Play MPD initialization URLs**: M6 proxied manifests now rewrite `/m6web/` initialization paths to the proxied Bedrock CDN URL so segment initialization requests stay on the controller proxy route.
- **Stream YAML output standardization**: Dark-stream home sections now emit `entries` instead of `items`, selected media metadata fields now use typed arrays/numbers, m6play category descriptions were removed from YAML and the frontend category model, and TF1 category request descriptors no longer carry the redundant `channel_label` parameter.

### Removed
- Dead helpers: `process_root`, `parse_html_rows`, `collect_ordered_results`, `matches` (response_parser), `resolve_request_headers`/`resolve_request_body`/`execution_options` (JsonScraperQuery), `split_static_path`, `render_static_value`, `render_yaml_value`, `render_yaml_values` (StaticScraperQuery).
- `JsonScraperSubQuery::execute` / `execute_siblings` / `execute_context` / `execute_indexed_context` / `execute_indexed_sibling` / `build_row_node` — re-used through the new `JsonScraperSubQuery::execute_query_level` unified entry point (see regression fix below).

### Fixed
- **Scrapyfy dynamic proxy provider loading**: the provider now passes the requested country into the proxy source query, normalizes returned country codes before filtering, deduplicates by normalized authority, reports source collection load failures, and falls back to the existing `archanea-proxies` service directory when the preferred `arachnea-proxies` directory is absent.
- **HTTP proxy `ReplaceAll` post-action responses**: proxy action headers are now parsed before lossy `HashMap` merging so repeated `opts.headers` actions are preserved, invalid action JSON returns an explicit proxy error, and `Accept-Encoding: identity` is enforced case-insensitively to avoid corrupting compressed upstream bodies during text replacement.
- **`francetv.yaml` `load_home` section pagination**: Added a default `page` parameter so shared section metadata serializes `current_page` as a number instead of leaking the unresolved `{page}` placeholder on non-paginated home responses.
- **`rtbf-auvio-be.yaml` `load_home` PROMOBOX banner video shape**: RedBee banner preview extraction now keeps `banners[].video` as a single string URL instead of serializing it as an array containing an `_` object.
- **`rtbf-auvio-be.yaml` `load_home` PROMOBOX banner video proxying**: RedBee HLS banner preview URLs are now wrapped through the configured generic HTTP proxy path.
- **Frontend banner video proxy URLs**: Native video source detection now preserves same-origin proxy paths such as `/api/proxy/https://...m3u8` instead of extracting and playing the embedded upstream URL directly.
- **`anime-sama.yaml` `load_home` banners**: Carousel clone slides marked with `aria-hidden="true"` are now ignored, keeping the home banners list to the four real featured items.
- **`m6play-fr.yaml` `search` result grouping**: Search now extracts one root response with hits nested under `entries`, preventing repeated root `source` values from breaking typed serialization.
- **`francetv.yaml` `get_entry` season grouping**: FranceTV program details now build seasons from `collections` filtered to `type: playlist_video`, keeping each season's episodes under that season instead of flattening every playlist video item into a top-level `episode` list.
- **RTBF Auvio playback CORS**: RTBF RedBee media manifest URLs returned by the player resolver now use the same-origin `/api/proxy/` route, so the DASH MPD and relative segment requests are fetched through Arachnea while the Widevine license proxy remains on `/api/get_stream/rtbf-auvio-be/...`.
- **RTBF Auvio home banners**: Query-level sub-query fetches now execute child sub-queries on fetched responses and resolve nested request headers/bodies from the parent response, allowing `load_home` PROMOBOX banners to include the RedBee HLS `video` URL.
- **coflix.yaml**: Updated `get_entry` query to correctly extract season labels and links from the HTML entry page.
- **coflix.yaml**: Fixed `get_season` query to correctly iterate over all episodes in the `episodes` array.
- **Coflix `get_season` context fields**: Query-level sub-queries now merge `context_entries` from each selected context row into the fetched sub-query result, restoring episode `id`, `title`, `episode-number`, and `img/preview > link` fields alongside player links.
- **Entry-level sub-query merge shape**: `execute_entry_sub_queries` now wraps each decoded sub-query value inside a named child node (`{ "embed-link": ["url"] }`) instead of pushing bare scalar items. This causes the serializer to emit an array of objects (`"players": [{"embed-link": ["url1"]}, …]`) instead of a flat string array (`"players": {"embed-link": ["url1", …]}`). Fixes the `players` output shape for `coflix.yaml` `get_entry`.
- **`rtbf-auvio-be.yaml` `list_lives` flattening**: the RTBF live catalog now declares `result_item_field: entries` and groups `/data/content/*` under `entries`, so the unified executor returns one live item per channel instead of merging the whole feed into a single result.
- **RTL Play login cookie handling**: `scrapyfy::HttpClient` now exposes materialized responses and a shared-cookie lookup for one URL, letting `rtlplay_resolver` reuse real redirect-set cookies from the HTTP jar instead of scraping `lfvp_device_id` and auth cookies from response bodies.
- **RTL Play SSO redirects via proxy HTTP**: `arachnea-http` responses now expose followed redirect target URLs, `scrapyfy::HttpClient` can run with an isolated cookie cache, and `rtlplay_resolver` uses both to recover RTL Play SSO `oidc_callback` candidates while keeping requests on the shared proxy-aware HTTP path. The rquest loopback proxy client now leaves redirects to `arachnea-http`, so intermediate SSO cookies are preserved. RTL Play logins are serialized so concurrent resolver calls do not mix SSO cookies, and the SSO flow now reuses the post-auth callback, keeps SSO same-origin headers, and completes the OIDC `authorize` redirect before reading the RTL Play auth cookie.
- **Scraper HTTP fetch diagnostics**: `scrapyfy::HttpClient` now includes the underlying HTTP/proxy error in `Fetch fail ...` messages, making proxy-dependent failures diagnosable from API logs.
- **TF1 mediainfo diagnostics**: TF1 playback now reports mediainfo HTTP status/body details and sends browser-like `Accept`, `Origin`, and `Referer` headers, improving proxy failure diagnostics without exposing auth tokens in header logs.
- **Proxy CONNECT diagnostics**: the rquest loopback proxy now logs failed CONNECT requests and proxy-hop handshake errors instead of silently dropping them, making upstream SOCKS failures visible.
- **Proxy pool transient failures**: tunnel proxy-pool selection no longer opens a destructive preflight tunnel before the real connection, compatibility checks retry a candidate once before marking it unusable, and an exhausted pool gets one fresh scan of KO members before failing, reducing false negatives from flaky SOCKS upstreams that intermittently return `network unreachable`.
- **6play front-auth login**: M6Play login token retrieval is now serialized to avoid concurrent Gigya/front-auth request storms, while still reporting HTTP status/body details when `front-auth.6cloud.fr` returns non-JSON.
- **`francetv.yaml` `list_lives` flattening**: the FranceTV live catalog now groups `/items/*` under `entries` with `result_item_field: entries`, so typed scalar fields are serialized per live item instead of receiving values from the whole feed.
- **`proxifly.yaml` proxy list flattening**: the dynamic proxy source now groups root array items under `proxies` with `result_item_field: proxies`, so one proxy row is returned per upstream JSON item instead of a single merged result.
- **Dynamic proxy selection diagnostics and probing**: dynamic proxy inventory now logs loaded/probed/eligible candidate counts, HTTP proxy probes use forward `GET` instead of `CONNECT :80`, SOCKS proxies are not rejected for HTTPS destinations solely because a source reports `supports_https=false`, and proxy country headers no longer produce unknown action-header debug noise.
- **Dynamic proxy candidate selection**: country proxy selection now follows the documented eligibility order more closely by rejecting stale probes and active destination cooldowns, preserving hard KO/auth exclusions during reloads, and using deterministic latency/failure/authority ordering for equivalent candidates.
- **Scrapyfy dynamic proxy HTTPS validation**: the default scrapyfy-backed proxy inventory now verifies HTTPS tunnelling with `https://example.com/`, and failed HTTPS probes explicitly clear source-claimed HTTPS support so HTTP-only public proxies are not selected for HTTPS sites.
- **Dynamic proxy HTTPS CONNECT routing**: HTTP `CONNECT` targets on port 443 are now treated as HTTPS destinations for country proxy selection, so public HTTP proxies without working tunnel support are excluded before TF1-style HTTPS requests.

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
- **Documentation and tracking**: `arachnea-http` README now documents proxy transport selection, and the completed HTTP proxy task has been removed from the root `docs/TODO.md`.

## Unreleased — scrapyfy invalid JSON payload logging

### Fixed
- **Scraper JSON parse diagnostics**: `arachnea-scrapyfy::HttpClient` now logs the raw response content that failed JSON parsing, including `__NEXT_DATA__` payloads, with truncation metadata so API errors like `Invalid JSON payload returned by ...` can be diagnosed from backend logs without changing the error sent to the frontend.

## Unreleased — scrapyfy typed output contract

### Added
- **YAML output typing**: scraper entries now declare their JSON output shape with `type` (`string`, `number`, `boolean`, `object`, and array variants). Missing declared fields serialize as `null`, and failed casts report contextual errors.
- **Typed `ScraperDataNode` serialization**: scraper output nodes now render directly to `serde_json::Value`, including object/object-array handling for explicit groups and aligned implicit groups.

### Changed
- **Service YAML migration**: service definitions under `server/services/**/*.yaml` now declare output types and use `select: first` for scalar fields where needed. Pagination/count/rating fields are numeric, booleans are boolean, and duration remains string.
- **Frontend response normalization**: `rustify.ts` now consumes typed numbers and booleans directly, reads service descriptions as typed objects, and expects `get_entry` to return one object instead of a single-element array.
- **Static metadata test expectations**: service metadata descriptions are now validated as object fields such as `description > fr`.

### Fixed
- **Regex/post-process generated output**: generated fields now carry explicit output types, and implicit nested groups with aligned repeated children serialize as arrays of objects.

## Unreleased — M6 Play home banner logo typing

### Fixed
- **Targeted sub-query row cardinality**: query-level sub-queries with a `target` now append each returned row as a separate object item instead of merging all rows into one object. This preserves `object[]` output shape for cases such as `m6play-fr.yaml` `load_home` banners.
- **`m6play-fr.yaml` `load_home` banner fields**: banner scalar extraction now keeps the first value per program, matching the declared `type: string`/`number` output contracts and avoiding typed serialization failures when 6play returns multiple banner values.
- **`m6play-fr.yaml` `load_home` banner request URL**: the banner follow-up query now uses one scalar request context before applying its constant URL action, avoiding repeated fetches of the same five banners.
- **`get_section` scalar pagination fields**: section rows are merged with first-scalar semantics and scalar leaves are collapsed after merge, so flattened entry rows no longer duplicate fields such as `current_page`.
- **`m6play-fr.yaml` `get_entry` year field**: detail pages now keep one numeric year value when both copyright and production years are present.
- **`m6play-fr.yaml` `get_season` preview images**: episode preview fallback image extraction now keeps one image URL per episode, matching the scalar `img/preview > link` output contract.
- **`rtlplay-be.yaml` `get_season` pagination fields**: season pagination fields are now emitted once per response while episodes are collected through the `episodes` object array, avoiding repeated scalar booleans such as `have_more`.
- **`rtlplay-be.yaml` `search` pagination fields**: search pagination fields are now emitted once per response while results are collected through the `entries` object array, avoiding repeated scalar booleans such as `have_more`.
- **`tf1-fr.yaml` `load_home` video tags**: video slider tags are now emitted as a string array, matching the multiple tag slugs returned by TF1.
- **`tf1-fr.yaml` `get_category` section link**: category section links now use the formatted GraphQL URL instead of serializing the whole response object through `get_request_url`.
- **`tf1-fr.yaml` category pagination**: category and section requests now send TF1 GraphQL item offsets, first-page links start at offset `0`, and the page size matches TF1's 10-item responses so `have_more` can be derived from the actual paged result count.

## Unreleased — Proxy post-response actions

### Added
- **Post-response HTTP action system**: `arachnea-proxy` now supports pluggable post-response actions that run after the upstream response is received. Actions can modify status, headers, and body before the response is returned to the caller.
- **`ReplaceAll` action**: Replaces all regex occurrences in textual responses (text/*, application/json, application/javascript, image/svg+xml, etc.). Text encoding is detected from `charset` parameter, then UTF-8, with fallback to Windows-1252/ISO-8859-1 via `encoding_rs`.
- **Action header transport**: Actions are carried via `Arachnea-Proxy-ReplaceAll` headers (JSON value with `order`, `pattern`, `replacement`). Multiple actions of the same type are supported via the array-of-pairs format in `opts/headers`. Action headers are consumed by the proxy and never forwarded upstream.
- **`Accept-Encoding: identity` enforcement**: When text-transforming actions are present, the proxy forces `identity` encoding to avoid dealing with compressed bodies.
- **Action ordering**: Each action supports an optional `order` field (lower runs first). Actions without `order` execute after ordered ones, preserving discovery order.
- **Dependencies**: `regex` and `encoding_rs` added to `arachnea-proxy`.

### Changed
- **`ProxiedHttpRequest`**: Added `post_actions: Vec<ProxyHttpPostActionConfig>` and `headers_only: bool` fields.
- **`parse_http_response`**: Now accepts `headers_only` and `post_actions` parameters; applies actions at the designated extension point after chunked decoding, then fixes `content-length` and removes `etag`/`content-md5`/`digest` when body changes.
- **`proxied_url`**: Added `actions: &[ProxyHttpPostActionConfig]` parameter. All existing callers updated with `&[]` — no behavioural change for existing usage.
- **`handle_proxy_http`**: Now parses action headers from the request headers and passes them through to the HTTP client.

## Unreleased — FranceTV HLS key proxying

### Fixed
- **FranceTV HLS AES-128 keys**: FranceTV proxied manifests now rewrite `cloudreplay.ftven.fr/keys/*.key` `#EXT-X-KEY` and `#EXT-X-SESSION-KEY` URIs through the generic HTTP proxy without a country hint, while keeping the manifest request itself routed through France.
- **Proxy text action filtering**: `ReplaceAll` now accepts an optional per-action `content_types` allowlist. FranceTV key rewrites use it for HLS MIME variants such as `application/x-mpegurl` without changing the default proxy text filter.
- **M6Play MPD URL rewrites**: M6Play manifest replacements now use the proxy action `{proxy}` placeholder instead of embedding the resolved proxy path in the response sent to the client.
- **Proxy action `{proxy}` placeholder**: `{proxy}` now resolves to the public proxy path such as `/api/proxy` instead of the absolute local entry-point URL.
- **Proxy action `{base_url}` placeholder**: `{base_url}` now resolves to the target origin such as `https://myhost.be:8080`, without the target path or query string.

## Unreleased — dynamic proxy loading analysis clarifications

### Changed
- **Dynamic proxy loading analysis**: Clarified the `ProxyAvailabilityHint` contract, static/dynamic pool coexistence, strict HTTP/HTTPS probe URL requirements, runtime destination validation with per-origin cooldowns, synchronous bounded IP-to-country refresh behaviour, and the recommended CLI exposure for the refresh command.
- **Dynamic proxy loading analysis**: Refined destination-failure retention so a proxy reaches global KO after 10 active per-origin failures and clears the persisted destination-failure list.

### Added
- **Step 1 — Dynamic proxy data contracts**: Added `ProxyRecord`, `ProxyProtocol`, `ProxyRuntimeStatus`, `ProxyAvailabilityHint`, `ProxyDestinationFailureReason`, `ProxyDestinationFailure`, `ProxyLoadRequest`, and the `ProxyDataProvider` trait in `arachnea-proxy/src/core/proxy_record.rs`. `ProxyRecord` supports serde serialisation with custom `SystemTime` epoch-millis encoding. A `try_to_node()` conversion produces a `ProxyNode` when the protocol is known, with automatic IPv6 bracketing.
- **Step 2 — Explicit proxy probe API**: Added `ProbeConfig`, `ProbeMode`, and `ProxyProbe` in `arachnea-proxy/src/core/proxy_probe.rs`. The probe measures TCP latency, tests HTTP CONNECT forwarding, SOCKS5/SOCKS4 handshakes, HTTPS tunnelling, and detects authentication requirements, updating `ProxyRecord` in place without a separate report structure. Protocol detection iterates through a configurable order for records without a known protocol. Runtime destination validation against real targets is supported separately from context-free probing.
- **Step 3 — Runtime proxy inventory**: Added `ProxyInventory`, `InventoryConfig`, and `CoexistencePolicy` in `arachnea-proxy/src/core/proxy_inventory.rs`. Stores dynamic proxy records with a country index, selects the best candidate by latency with cooldown/status/HTTPS filtering, supports lazy loading per country via `ProxyDataProvider` with a loading guard and negative cache, tracks per-destination failures with cooldown, and marks proxies globally KO when the destination-failure threshold (default 10) is reached.
- **Step 4 — Proxy persistence store**: Added `ProxyStore`, `ProxySerdeCodec`, `ProxySerdeStore`, and `JsonProxyCodec` in `arachnea-proxy/src/core/proxy_store.rs`. Proxy records can now be loaded from and saved to a serde-backed file store with JSON as the default codec and atomic `*.tmp` writes followed by rename. `ProxyInventory` can be created with a store, loaded from it, saved to it, and automatically persists after lazy loads and runtime state updates.
- **Step 5 — Scrapyfy proxy provider skeleton**: Added `ScrapyfyProxyDataProvider` behind the `arachnea-proxy` feature in `arachnea-scrapyfy`. The provider is currently a placeholder whose `load_proxies` returns an empty vector with a TODO, and `default_scrapyfy_proxy_inventory()` wires this provider with the default probe for future dynamic proxy loading.
- **Step 5.b — Dynamic country routing runtime wiring**: Added `DynamicCountryRoutingProxyHandler` in `arachnea-proxy/src/core/parameters.rs` that emits `ProxyPool` markers (`dynamic-country:<CODE>`) resolved at connect time through `ProxyInventory`. Added `ProxyInventory` support to `ArachneaProxyCore` with `from_resolved_with_proxy_inventory()` and `with_proxy_inventory()` constructors. Dynamic pool markers are resolved in `resolve_proxy_pools_in_chain()` via `ProxyInventory::select()`, respecting `CoexistencePolicy`. Added `default_scrapyfy_proxy_core()` in `arachnea-scrapyfy` that builds a pre-wired proxy core with the dynamic handler — `arachnea-stream` now calls this single function instead of assembling the configuration inline.
- **Step 5.c — Scrapyfy proxy data provider implementation**: Replaced the placeholder `ScrapyfyProxyDataProvider::load_proxies()` with working logic that loads proxy data from the YAML-defined `proxifly/free-proxy-list` source, filters by requested country, normalises country codes, and deduplicates by authority. Added the `arachnea-proxies` query collection service registry with `proxifly.yaml` as the initial source.
- **Step 6 — IP-to-country resolution**: Added `IpCountryRecord`, `IpCountryDataProvider`, `IpCountryStore`, `IpCountrySerdeCodec`, `IpCountrySerdeStore`, and `JsonIpCountryCodec` in `arachnea-proxy/src/core/ip_country.rs` for loading, persisting, and retrieving IP geolocation data. Added `ScrapyfyIpCountryDataProvider` in `arachnea-scrapyfy` that resolves IPs to country codes via the YAML-defined ip-api.com query. Added `resolve_ip_direct()` for direct HTTP-based single-IP lookups, `refresh_ip_country_store()` for YAML-based batch refresh, and `refresh_ip_country_store_direct()` for HTTP-based batch refresh. Created the `arachnea-ip-countries` query collection with `ip-api.yaml` as the initial IP geolocation source. Exposed the refresh via `--refresh-ip-countries` CLI flag in `arachnea-stream`.

## Unreleased — stream resolver contract migration

### Changed

- **Stream resolution API**: Replaced the JSON command `resolve_player_stream` with `get_stream({ resolver, target })`; legal and YAML-driven resolvers now share this public entry point.
- **DRM license proxy API**: Renamed the binary command and generated license paths from `get_stream` and `/api/get_stream/...` to `get_drm_license` and `/api/get_drm_license/...`, without a compatibility route.
- **Legal player resolvers**: M6+, RTL Play, RTBF Auvio, TF1+ and FranceTV are selected through globally unique resolver identifiers; redundant YAML `resolverStreamKind` values were removed and each resolver uses its internal default.
- **DRM license public path**: `PlayerResolverEndpoints` now receives the controller-generated `get_drm_license` public path, so generated license URLs honor a configurable API mount instead of hardcoding `/api/`.
