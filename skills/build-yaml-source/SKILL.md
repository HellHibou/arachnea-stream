---
name: build-yaml-source
description: Create or update one Arachnea scraper YAML from one or more source URLs by analyzing the target website, reusing patterns from `server/services/*.yaml`, and checking supported scraper capabilities in `server/crates/arachnea-scrapyfy/src/scrapyfy/*`. Use when adding a new source, repairing or extending an existing source YAML, auditing whether a site can be supported without Rust changes, or when the user types `#build-yaml-source`.
---

# Build YAML Source

## Overview

Create or update `server/services/<source>.yaml` with a YAML-first approach. Analyze the target site from the provided URLs, reuse the closest existing source configs, and avoid Rust changes unless the current scraper primitives cannot express the source.

## Specifications

Formal YAML format specifications are in `docs/specifications/*-en.md`:

- **arachnea-scrapyfy-en.md** — scraper query YAML format: root fields, HTTP config, four scraper types (HTML, JSON, static, text), sub-queries, data transformation actions, post-processing, output structures.
- **arachnea-stream-en.md** — streaming source YAML format: nine standard queries (`search`, `get_entry`, `load_home`, `get_category`, `get_season`, etc.), `MediaItem` structure, player definitions, Rust/frontend contract.
- **arachnea-ip-countries-en.md** — IP geolocation source YAML format: `resolve_ip_country` query, `country_code` output, `IpCountryRecord` mapping.
- **arachnea-proxies-en.md** — proxy source YAML format: `application/arachnea-proxy` media type, output fields (protocol, host, port, country), scraper examples, `ProxyRecord` mapping.

## Workflow

1. Read `AGENTS.md` and `server/AGENTS.md`.
2. Read the relevant spec from `docs/specifications/*-en.md` for your source type, then skim the YAML capability reference below.
3. Inspect the provided URLs before touching files:
   - identify whether the source is HTML, direct JSON, Next.js `__NEXT_DATA__`, or JSON fetched through follow-up requests;
   - locate stable search, entry, home, category, season, and player URLs when they exist;
   - prefer network payloads or embedded JSON over brittle CSS scraping when both are available.
4. Reuse the closest existing examples instead of starting from scratch:
   - `server/services/papystreaming.yaml` for plain HTML pages;
   - `server/services/rtlplay-be.yaml` for Next.js `__NEXT_DATA__`;
   - `server/services/rtbf-auvio-be.yaml` for JSON APIs and `sub_queries`;
   - `server/services/anime-sama.yaml` for regex-heavy extraction or `post_process`.
5. Draft the smallest useful YAML first:
   - prefer `search` and `get_entry` as the baseline;
   - add `load_home`, `get_category`, and `get_season` only when the site exposes them cleanly;
   - keep identifiers, comments, and added text in English.
6. Before adding optional source-specific content, stop and report it. For each proposed addition, state:
   - what can be added;
   - which URLs or payloads prove it exists;
   - whether it is YAML-only or needs Rust changes.
7. If the site needs a capability not already supported by `server/crates/arachnea-scrapyfy/src/scrapyfy/*`, stop before editing Rust. Explain the missing capability, point to the closest code area, and ask for approval before modifying source code.
8. After the minimal scope is agreed, create or update the YAML and keep backend contracts aligned with the current query names and field conventions.

## Query Names Used by the Backend

Read `server/crates/arachnea-stream/src/main.rs` before changing query scope.

- `search` — used by `StreamScraper::search`, receives `search_terms`, supports runtime `media_types` filtering.
- `get_entry` — used by `StreamScraper::get_entry`, receives `query_url`, returns one detail payload for a single entry URL.
- `load_home` — used by `StreamScraper::load_home`, aggregates home content across configured sources.
- `get_category` — used by `StreamScraper::get_category`, receives the map stored under `category.source`, usually expects at least a source `name` and one request value such as `query_url`.
- `get_season` — used by `StreamScraper::get_season`, receives `query_url` and `page`, add only when the site exposes seasons or paginated episode payloads.

Unknown query names are not part of the current product contract. Treat any new top-level query name as a backend change request and ask before introducing it.

## Common Field Patterns

Use existing names whenever they fit the source:

- Search and section cards: `title`, `title/alt`, `description`, `link`, `web-link`, `media-type`, `img/poster > link`, `theme`, `channel`, `duration`, `release-date`, `expire`.
- Entry details: `img/logo > link`, `count-season`, `season > label`, `season > link`, `episode`, `players > embed-link`, `content-advisor`, `casting`, `director`, `language`, `lang/audio`, `lang/subtitles`, `rating`, `rating-count`, `video/trailer`.
- Home and category payloads: `banners`, `categories`, `sections`.
- Season payloads: `current_page`, `have_more`, `episodes`.

Treat these as reusable conventions, not mandatory targets.

## YAML Capability Reference

### Supported Query Shapes

- `scraper_type: html` — uses `row_selector`, entries use `selector`/`select`/`actions`/nested `entries`, supports `query_param_mappings` and `post_process`.
- `scraper_type: json` — uses `row_pointer`, entries use `pointer`/`select`/`actions`/nested `entries`, supports `extract_next_data`, `query_param_mappings`, `sub_queries`, and `post_process`.
- `extract_next_data: true` — existing path for Next.js pages exposing `__NEXT_DATA__`.

### Supported Action Families

- Extraction: `get_text`, `get_attribut`, `get_request_url`.
- Value transforms: `split`, `map`, `regex_find_all`, `suffix`, `format_text`, `ratio`, `normalize_duration`.
- URL handling: `resolve_url`, `resolve_url_from_parent` (both accept `proxy: true`), `get_url_host`, `build_nextjs_data_url`.
- Date normalization: `get_date` with `yyyy_mm_dd`, `dd_mm_yyyy`, `dd_mm_yy`, `dd_month_yyyy`, `days_from_today`.

### Supported Advanced Features

- `query_param_mappings` — build URL fragments from runtime params such as `media_types`.
- `sub_queries` (query-level) — fetch follow-up JSON payloads from values extracted in the parent payload, support context filtering, `request_actions`, and nested sub-queries.
- `sub_queries` (entry-level) — fetch follow-up HTML or JSON pages seeded by each extracted value, tagged by `scraper_type: html` or `scraper_type: json`, supports recursion. See `docs/SUB_QUERY_AT_ENTRY.md`.
- `post_process` — `extract_regex_items`, `filter_items`, `fetch_regex_items_from_items`, `pivot_items_by_index`.

### When Rust Changes Are Needed

Stop and ask before editing code when the source needs:

- custom HTTP methods or headers configured from YAML;
- authenticated flows, anti-bot challenges, or browser automation;
- response formats outside HTML, JSON, or Next.js payload model;
- a new extraction primitive beyond existing actions or post-process steps;
- a new controller or query contract not already used by `server/crates/arachnea-stream/src/main.rs`.

## Mandatory Report

Before any Rust change, and before any optional addition beyond the minimal YAML, present three short sections:

- `Supported in YAML now`
- `Optional additions`
- `Blocked or needs code`

Under `Optional additions` and `Blocked or needs code`, explicitly say whether code changes are required.

## Resource Use

Read the relevant spec first, then load only the specific config and code files that match the site being analyzed. Do not read every source YAML unless the site shape is still unclear after comparing the closest examples.
