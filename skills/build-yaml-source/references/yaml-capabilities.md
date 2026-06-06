# Arachnea YAML Capabilities

## Read These Files First

- `server/crates/arachnea-scrapyfy/src/scrapyfy/html_scraper_query.rs`
- `server/crates/arachnea-scrapyfy/src/scrapyfy/json_scraper_query.rs`
- `server/crates/arachnea-scrapyfy/src/scrapyfy/html_scraper_entry.rs`
- `server/crates/arachnea-scrapyfy/src/scrapyfy/json_scraper_entry.rs`
- `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_action.rs`
- `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_post_process.rs`
- `server/crates/arachnea-scrapyfy/src/scrapyfy/http_client.rs`

Use the code as the source of truth for what YAML can express.

## Supported Query Shapes

- `scraper_type: html`
  - Uses `row_selector`.
  - Entries use `selector`, `select`, `actions`, and nested `entries`.
  - Supports `query_param_mappings` and `post_process`.
- `scraper_type: json`
  - Uses `row_pointer`.
  - Entries use `pointer`, `select`, `actions`, and nested `entries`.
  - Supports `extract_next_data`, `query_param_mappings`, `sub_queries`, and `post_process`.

`extract_next_data: true` is the existing path for Next.js pages that expose `__NEXT_DATA__`.

## Supported Action Families

The current action set already covers many common cases:

- extraction:
  - `get_text`
  - `get_attribut`
  - `get_request_url`
- value transforms:
  - `split`
  - `map`
  - `regex_find_all`
  - `suffix`
  - `format_text`
  - `ratio`
  - `normalize_duration`
- URL handling:
  - `resolve_url`
  - `resolve_url_from_parent`
  - `get_url_host`
  - `build_nextjs_data_url`
- date normalization:
  - `get_date` with:
    - `yyyy_mm_dd`
    - `dd_mm_yyyy`
    - `dd_mm_yy`
    - `dd_month_yyyy`
    - `days_from_today`

If the extraction can be described with these primitives, prefer YAML over Rust.

## Supported Advanced YAML Features

- `query_param_mappings`
  - Build URL fragments from runtime params such as `media_types`.
- `sub_queries`
  - Fetch follow-up JSON payloads from values extracted in the parent payload.
  - Support context filtering, `request_actions`, and nested sub-queries.
- `post_process`
  - `extract_regex_items`
  - `filter_items`
  - `fetch_regex_items_from_items`
  - `pivot_items_by_index`

Use advanced features only when they reduce source-specific Rust or make the YAML more reliable than a brittle selector chain.

## Choose a Similar Example First

- Start from `server/services/papystreaming.yaml` for plain HTML detail and listing pages.
- Start from `server/services/rtlplay-be.yaml` for Next.js pages using `extract_next_data`.
- Start from `server/services/rtbf-auvio-be.yaml` for direct JSON APIs and `sub_queries`.
- Start from `server/services/anime-sama.yaml` for regex-driven extraction or `post_process`.

Copy patterns, then adapt field names and pointers/selectors to the new source.

## Usually Means a Rust Change Is Needed

Stop and ask before editing code when the source needs one of these:

- custom HTTP methods or headers configured from YAML;
- authenticated flows, anti-bot challenges, or browser automation not handled by the current client;
- response formats outside the current HTML, JSON, or Next.js payload model;
- a new extraction primitive beyond the existing actions or post-process steps;
- a new controller or query contract not already used by `server/crates/arachnea-stream/src/main.rs`.

The current HTTP client already gives you browser-like headers, cookies, plain GET requests, direct JSON fetches, and `__NEXT_DATA__` extraction. It does not expose YAML-level control for custom methods, headers, or scripted browser execution.
