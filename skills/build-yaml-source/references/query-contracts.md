# Arachnea Query Contracts

## File Naming

- Store each source in `server/services/<source>.yaml`.
- Keep the top-level `name` aligned with the filename stem and the source identifier used by the backend.
- Reuse existing field names and nesting patterns when possible instead of inventing new ones.

## Query Names Used by the Backend

Read `server/crates/arachnea-stream/src/main.rs` before changing query scope.

- `search`
  - Used by `StreamScraper::search`.
  - Receives `search_terms`.
  - Supports runtime `media_types` filtering.
- `get_entry`
  - Used by `StreamScraper::get_entry`.
  - Receives `query_url`.
  - Should return one detail payload for a single entry URL.
- `load_home`
  - Used by `StreamScraper::load_home`.
  - Aggregates home content across all configured sources.
- `get_category`
  - Used by `StreamScraper::get_category`.
  - Receives the map stored under `category.source`.
  - Usually expects at least a source `name` and one request value such as `query_url`.
- `get_season`
  - Used by `StreamScraper::get_season`.
  - Receives `query_url` and `page`.
  - Add only when the site exposes seasons or paginated episode payloads.

Unknown query names are not part of the current product contract. Treat any new top-level query name as a backend change request and ask before introducing it.

## Minimal Versus Optional Scope

- Minimal baseline:
  - `search`
  - `get_entry`
- Add when the site clearly supports them:
  - `load_home`
  - `get_category`
  - `get_season`

Prefer the minimal baseline first, then report optional additions separately for approval.

## Common Field Patterns

Use existing names whenever they fit the source:

- Search and section cards:
  - `title`
  - `title/alt`
  - `description`
  - `link`
  - `web-link`
  - `media-type`
  - `img/poster > link`
  - `theme`
  - `channel`
  - `duration`
  - `release-date`
  - `expire`
- Entry details:
  - `img/logo > link`
  - `count-season`
  - `season > label`
  - `season > link`
  - `episode`
  - `players > embed-link`
  - `content-advisor`
  - `casting`
  - `director`
  - `language`
  - `lang/audio`
  - `lang/subtitles`
  - `rating`
  - `rating-count`
  - `video/trailer`
- Home and category payloads:
  - `banners`
  - `categories`
  - `sections`
- Season payloads:
  - `current_page`
  - `have_more`
  - `episodes`

Treat these fields as reusable conventions, not mandatory targets.

## Optional Additions Checklist

When analyzing a new source, actively look for optional additions to report before implementing them:

- home banners or hero content;
- categories or curated collections;
- editorial sections or shelves;
- season navigation;
- paginated episode lists;
- trailer or preview video;
- player embed links;
- subtitles or audio language metadata;
- ratings, advisory labels, or casting credits;
- release and expiry dates;
- source-specific metadata such as channel, network, creators, or theme taxonomy.

For every optional addition, report:

- the feature;
- the evidence URL or payload;
- whether it is YAML-only;
- whether it needs Rust changes.
