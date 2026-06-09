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
- **coflix.yaml**: Updated `get_entry` query to correctly extract season labels and links from the HTML entry page.
- **coflix.yaml**: Fixed `get_season` query to correctly iterate over all episodes in the `episodes` array.

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
