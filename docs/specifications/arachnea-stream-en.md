# arachnea-stream YAML format specification

## 1. Overview

An arachnea-stream collection declares a streaming source (legal platform or
alternative catalogue). Each source defines up to 9 queries that produce
structured catalogue data — home pages, categories, search results, entry
details, episodes, and live channels.

These queries are consumed by `StreamScraper` (`stream_scraper.rs`) which
centralises calls to the `ScraperAgregator` engine and exposes results to
the controller.

This document only describes fields and conventions specific to the stream
format. The general structure of a YAML file (`id`, `parameters`, `http`,
`queries`, `entries`, `actions`, `shared`, etc.) is documented in
`arachnea-scrapyfy-yaml.md`.

---

## 2. Collection structure

YAML files are organised in thematic subdirectories:

```
services/arachnea-stream/
├── services.json                 # Activation and per-source parameters
├── legal-stream/                 # Legal platforms (TF1+, M6 Play, etc.)
│   ├── m6play-fr.yaml
│   ├── tf1-fr.yaml
│   ├── francetv.yaml
│   ├── rtlplay-be.yaml
│   └── rtbf-auvio-be.yaml
└── dark-stream/                  # Alternative catalogues
    ├── anime-sama.yaml
    ├── coflix.yaml
    ├── frenchanimes.yaml
    └── animeultime.yaml
```

### 2.1 Parameters and HTTP configuration

Each source defines its own parameters (`base_url`, API keys, channel
identifiers, pagination limits). HTTP configuration can be `direct`
(unprotected APIs) or `auto` (sites with Cloudflare).

```yaml
http:
  mode: direct
  user_agent_profile: firefox
```

---

## 3. Standard queries

Each source may implement any subset of the following 9 queries:

| Query name | Type | Description |
|---|---|---|
| `service_stream_metadata` | `static` | Service metadata (name, logo, themes) |
| `load_home` | `json` / `html` | Home page (categories, sections, banners) |
| `get_category` | `json` / `html` | Content filtered by channel/category |
| `get_section` | `json` / `html` | Paginated section (list of entries) |
| `search` | `json` / `html` | Search with filters |
| `get_entry` | `json` / `html` | Entry detail (programme) |
| `get_season` | `json` / `html` | Season episodes |
| `list_lives` | `json` / `html` | List of live channels |
| `get_live` | `json` / `html` | Live channel detail (player) |

---

## 4. Media types

Sources declare their media types via a `shared` section with a YAML anchor:

```yaml
shared:
  media_types_my_source: &media_types_my_source
    - video/movie
    - video/show/serie
    - video/show/anime
    - video/show/documentary
    - video/show/sport
    - video/show/other
    - video/news
    - video/live
    - images/manga
    - images/webtoon
```

Type hierarchy:

| Type | Description |
|---|---|
| `video/movie` | Movie |
| `video/show/serie` | TV series |
| `video/show/anime` | Anime / cartoon |
| `video/show/documentary` | Documentary |
| `video/show/sport` | Sports |
| `video/show/kids` | Children |
| `video/show/other` | Other show type |
| `video/news` | News / magazine |
| `video/live` | Live TV |
| `video/other` | Unclassified video |
| `images/manga` | Manga / scan |
| `images/webtoon` | Webtoon |

---

## 5. Output data structures

### 5.1 Service metadata — `service_stream_metadata`

Static query. Each source returns exactly one row.

```yaml
- name: service_stream_metadata
  scraper_type: static
  entries:
    - name: id
      type: string
      value: "{service_id}"
    - name: title
      type: string
      value: "{service_title}"
    - name: logo
      type: string
      value: "{service_logo}"
    - name: description
      type: object
      value: "{service_description}"
    - name: search_themes
      type: string[]
      value:
        - "Action"
        - "Comedy"
        - "Drama"
```

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | `string` | yes | Unique source identifier (e.g. `m6play-fr`, `tf1-fr`) |
| `title` | `string` | yes | Display name (e.g. `M6 Play`, `TF1+`) |
| `logo` | `string` | yes | Logo URL |
| `description` | `object` | yes | Multilingual description (e.g. `{ fr: "…", en: "…" }`) |
| `search_themes` | `string[]` | no | List of available search filter themes |
| `themes` | `string[]` | no | Alias for `search_themes` |

---

### 5.2 Home page — `load_home`

Structure produced by each source:

```yaml
- name: load_home
  scraper_type: json
  entries:
    - name: categories        # Channel filters
      type: object[]
      entries:
        - name: key
          type: string
        - name: label
          type: string
        - name: link
          type: string
        - name: request > query_url
          type: string

    - name: sections          # Content rails
      type: object[]
      entries:
        - name: label
          type: string
        - name: link
          type: string
        - name: entries       # MediaItem objects
          type: object[]
          entries: [ … ]

    - name: banners           # Promotional banners
      type: object[]
      entries:
        - name: key
          type: string
        - name: title
          type: string
        - name: description
          type: string
        - name: image
          type: string
        - name: logo
          type: string
        - name: link
          type: string
        - name: web-link
          type: string
```

#### Category

| Field | Type | Description |
|---|---|---|
| `key` | `string` | Category identifier (e.g. `m6`, `tf1`, `all`) |
| `label` | `string` | Display name (e.g. `M6`, `TF1`, `Full catalogue`) |
| `link` | `string` | URL or path for subsequent calls |
| `description` | `string` | Optional description |
| `request > query_url` | `string` | API URL for `get_category` |
| `request > channel` | `string` | Channel identifier for parameters |
| `request > channel_label` | `string` | Channel name |
| `request > page_size` | `number` | Page size |
| `request > source` | `string` | Source for sub-queries |

#### Section

| Field | Type | Description |
|---|---|---|
| `label` | `string` | Rail title (e.g. `Latest additions`, `Trending`) |
| `entries` | `object[]` | `MediaItem` objects (see next section) |
| `link` | `string` | URL for `get_section` (pagination) |

#### Banner

| Field | Type | Description |
|---|---|---|
| `key` | `string` | Internal identifier |
| `title` | `string` | Display title |
| `description` | `string` | Tagline / description |
| `image` | `string` | Background image URL |
| `logo` | `string` | Overlay logo URL |
| `video` | `string` | Background video URL |
| `link` | `string` | Internal URL to `get_entry` |
| `web-link` | `string` | Public URL |
| `subtitle` | `string` | Subtitle |
| `entryUrl` | `string` | Link variant |

---

### 5.3 MediaItem object (catalogue entry)

Common structure used in `sections[].entries`, `search.entries`,
`get_category.sections[].entries` and `list_lives.entries`.

| YAML field | YAML type | Required | Description |
|---|---|---|---|
| `source` | `string` | see note | Source identifier (automatically injected by the engine) |
| `title` | `string` | yes | Main title |
| `title/alt` | `string` | no | Alternative title |
| `link` | `string` | yes | Internal URL for `get_entry` |
| `web-link` | `string` | no | Public content URL |
| `description` | `string` | no | Synopsis / description |
| `media-type` | `string` or `string[]` | no | Media type (e.g. `video/show/serie`) |
| `duration` | `string` | no | Formatted duration |
| `release-date` | `string` | no | Publication date (`YYYY-MM-DD`) |
| `expire` | `string` | no | Expiration date |
| `year` | `number` | no | Production or copyright year |
| `theme` | `string[]` | no | Genres / themes |
| `lang` | `string[]` | no | Available languages |
| `audio` | `string[]` | no | Alias for `lang` |
| `language` | `string[]` | no | Alias for `lang` |
| `label` | `string` | no | Substitution label (title fallback) |
| `rating` | `number` | no | Rating / score |
| `episode > label` | `string` | no | Episode label on cards |
| `season` | `string` | no | Season |
| `tags` | `string[]` | no | Tags |
| `key` | `string` | no | Identifier for live channels |
| `img/poster > link` | `string` or `string[]` | no | Poster URL(s) |
| `img/portrait > link` | `string` | no | Portrait URL |
| `img/landscape > link` | `string` | no | Landscape URL |
| `img/preview > link` | `string` | no | Preview URL |
| `img/logo > link` | `string` | no | Logo URL |

> **Note**: the `source` field is automatically injected by
> `execute_query_async` when `source_field_name` is `Some("source")`.
> It must not be set manually.

---

### 5.4 Category page — `get_category`

Follows the structure of `load_home` (categories + sections). Sections
contain `entries` (MediaItem) and pagination metadata:

```yaml
- name: get_category
  entries:
    - name: categories   # Same categories as load_home
    - name: sections     # Paginated rails
      entries:
        - name: label
        - name: link
        - name: current_page
        - name: total_pages
        - name: page_size
        - name: total
        - name: entries   # MediaItem[]
    post_process:
      - type: compute_items_field
      - type: derive_pagination
```

---

### 5.5 Paginated section — `get_section`

Returns a list of entries with pagination.

| YAML field | Type | Description |
|---|---|---|
| `current_page` | `number` | Current page number |
| `have_more` | `boolean` | Whether more pages exist |
| `total_pages` | `number` | Total number of pages (consumed by `derive_pagination`) |
| `page_size` | `number` | Page size |
| `infer_have_more_from_full_page` | `boolean` | Infer `have_more` from a full page |
| `entries` | `object[]` | MediaItem objects |

Pagination is typically computed via the `derive_pagination` post-processor:

```yaml
post_process:
  - type: derive_pagination
    total_pages_field: total_pages
    page_size_field: page_size
    remove_fields:
      - total_pages
      - page_size
```

---

### 5.6 Search — `search`

Similar structure to `get_section`:

| YAML field | Type | Description |
|---|---|---|
| `source` | `string` | Source (injected) |
| `current_page` | `number` | Current page |
| `total_pages` | `number` | Total pages |
| `have_more` | `boolean` | Continuation indicator |
| `entries` | `object[]` | MediaItem objects |

Available execution parameters:

| Parameter | Description |
|---|---|
| `{search_query}` | Raw query string (encoded) |
| `{search_terms}` | Search terms (alias) |
| `{search_query_json}` | Query formatted for JSON |
| `{media_types}` | Filtered media types |
| `{media_type_filter}` | Filter formatted for URL |
| `{themes}` | Selected themes |
| `{themes_filter}` | Themes formatted for URL |
| `{page}` | Page number |
| `{page_index}` | `page - 1` |

Parameter mappings (`query_param_mappings`) translate internal media types
and themes to URL parameters:

```yaml
query_param_mappings:
  - target_param: media_type_filter
    source_param: media_types
    item_suffix: "&"
    values:
      video/show/anime: "type%5B%5D=Anime"
      images/manga: "type%5B%5D=Scans"
```

---

### 5.7 Entry detail — `get_entry`

Returns a single object with full programme metadata.

| YAML field | Type | Description |
|---|---|---|
| `title` | `string` | Title |
| `title/alt` | `string` | Alternative title |
| `description` | `string` | Synopsis |
| `media-type` | `string[]` | Media type |
| `link` | `string` | API link |
| `web-link` | `string` | Public link |
| `video/trailer` | `string` | Trailer URL |
| `year` | `number` | Year |
| `release-date` | `string` | Release date |
| `expire` | `string` | Expiration date |
| `duration` | `string` | Duration |
| `count-season` | `number` | Number of seasons |
| `content-advisor` | `string` | Content advisory (e.g. parental rating) |
| `language` | `string[]` | Languages |
| `subtitles` | `string[]` | Subtitles |
| `theme` | `string[]` | Genres |
| `genre` | `string[]` | Genres (alias) |
| `director` | `string[]` | Directors |
| `casting` | `string[]` | Actors |
| `rating` | `number` | Score |
| `program-id` | `string` | Internal programme identifier |
| `seasons` | `object[]` | Seasons (see 5.7.1) |
| `players` | `object[]` | Players (see 5.9) |
| `img/poster > link` | `string` | Poster |
| `img/portrait > link` | `string` | Portrait |
| `img/landscape > link` | `string` | Landscape |
| `img/logo > link` | `string` | Logo |
| `img/preview > link` | `string` | Preview |

#### 5.7.1 Season

| YAML field | Type | Description |
|---|---|---|
| `label` | `string` | Season name |
| `link` | `string` | URL for `get_season` |
| `episodes` | `object[]` | Pre-loaded episodes (optional) |

---

### 5.8 Season and episodes — `get_season`

| YAML field | Type | Description |
|---|---|---|
| `current_page` | `number` | Current page |
| `have_more` | `boolean` | Continuation |
| `episodes` | `object[]` | List of episodes |

#### Episode

| YAML field | Type | Description |
|---|---|---|
| `title` | `string` | Episode title |
| `title/alt` | `string` | Alternative title |
| `description` | `string` | Synopsis |
| `duration` | `string` | Duration |
| `release-date` | `string` | Broadcast date |
| `expire` | `string` | Expiration date |
| `link` | `string` | Detail link |
| `web-link` | `string` | Public link |
| `season-name` | `string` | Associated season name |
| `episode-number` | `string` | Episode number |
| `season` | `string` | Season |
| `players` | `object[]` | Players (see 5.9) |
| `img/preview > link` | `string` | Preview |
| `img/poster > link` | `string` | Preview fallback |

---

### 5.9 Player (`players[]`)

Structure used in `get_entry`, `get_season` and `get_live` to describe
playback options.

| YAML field | Type | Description |
|---|---|---|
| `name` | `string` | Player name |
| `lang` | `string` | Audio language |
| `direct-link` | `string` | Direct, immediately playable media link |
| `resolver` | `string` | Globally unique resolver identifier (flat form, e.g. `stream-resolver`, `m6play-video`) |
| `target` | `string` | Resolver target (URL or identifier, flat form) |
| `resolver > kind` | `string` | Resolver type (legacy object form, e.g. `tf1-video`, `m6play-video`) |
| `resolver > target_id` | `string` | Resolver target identifier (legacy object form) |
| `resolver > stream > kind` | `string` | Stream type (legacy, e.g. `tf1-license-proxy`, `widevine-license-proxy`) |

A player is either a `direct-link` or a `{ resolver, target }` descriptor resolved by the backend.
The `resolver` and `target` fields can use the flat form above or the legacy object form
(`resolver > kind`, `resolver > target_id`).

Example with the generic resolver (flat form):

```yaml
- name: players
  type: object[]
  entries:
    - name: name
      type: string
      actions:
        - type: format_text
          argument: "External host"
    - name: resolver
      type: string
      actions:
        - type: format_text
          argument: "stream-resolver"
    - name: target
      type: string
      actions:
        - type: build_url
          base: "https://player.example/embed/{id}"
          fields:
            id: /video_id
```

Example with a legal resolver (legacy object form):

```yaml
- name: players
  type: object[]
  entries:
    - name: name
      type: string
      actions:
        - type: format_text
          argument: "TF1+"
    - name: embed-link
      type: string
      actions:
        - type: build_url
          base: "{base_url}/{channel}/{program_slug}/videos/{slug}.html"
          fields:
            channel: /program/mainChannel/slug
            program_slug: /program/slug
            slug: /slug
    - name: resolver
      type: object
      entries:
        - name: kind
          type: string
          actions:
            - type: format_text
              argument: "tf1-video"
        - name: stream
          type: object
          entries:
            - name: kind
              type: string
              actions:
                - type: format_text
                  argument: "tf1-license-proxy"
        - name: target_id
          type: string
          pointer: /id
```

### 5.9.1 Stream resolution — `get_stream`

The `get_stream` backend command receives `{ resolver, target }` and returns an exclusive union:

| Field | Type | Description |
|---|---|---|
| `stream_url` | `string[]` | Media stream URLs (first is preferred) |
| `manifest_type` | `string` | Manifest type (`mpd`, `m3u8`, `mp4`, etc.) |
| `title` | `string` | Optional title extracted by the resolver |
| `image/title > link` | `string` | Optional title image; serialized as JSON `{ "image/title": { "link": "…" } }` |
| `license_url` | `string` | Optional DRM license proxy URL |
| `license_headers` | `object` | HTTP headers for the DRM license request |
| `storyboard_vtt_url` | `string` | Optional storyboard thumbnail WebVTT URL; preferred over `storyboard` |
| `storyboard` | `object` | Optional sprite storyboard metadata |
| `embed-link` | `string` | Iframe fallback URL, exclusive with `stream_url` |

`stream_url` and `embed-link` are mutually exclusive: the former creates a native video player,
while the latter creates an iframe. Resolver `stream_headers` are not part of the public response:
they are embedded in the generated proxy stream URLs.

When a resolver declares `storyboard_vtt_url` with the `resolve_url` action, `proxy: true`, and a
`proxy_replace_all` rule, `get_stream` returns a proxy URL. The proxy then rewrites each WebVTT cue
line that starts with `http://` or `https://` to a proxy URL, then the frontend converts
the `#xywh` coordinates into Video.js sprite configuration. WebVTT files using one sprite image
with uniform cells are supported. If loading or parsing fails, the frontend uses `storyboard` when
available.

`storyboard` contains:

| Field | Type | Description |
|---|---|---|
| `url` | `string` | Sprite image URL, optionally containing the literal `{index}` placeholder |
| `width` | `number` | Optional width of one thumbnail; inferred from the first sprite image when omitted |
| `height` | `number` | Optional height of one thumbnail; inferred from the first sprite image when omitted |
| `columns` | `number` | Thumbnail columns per sprite image |
| `rows` | `number` | Thumbnail rows per sprite image |
| `first_page_index` | `number` | Optional first image index for an `{index}` URL template; defaults to `0` |
| `interval` | `number` | Optional seconds between thumbnails; the frontend uses video duration divided by `rows × columns` when omitted |

For example, a resolver with twelve columns, six rows and `first_page_index: 1` produces the
first sprite URL by replacing `{index}` with `1`. A single-image sprite still declares `rows: 1`.
When dimensions are omitted, the frontend preloads this first image and calculates the cell size as
its natural width divided by `columns` and its natural height divided by `rows`. For a single-image
sprite, uniform separator lines between cells are removed before passing it to the thumbnail renderer.

### 5.9.2 DRM license proxy — `get_drm_license`

The `get_drm_license` binary command handles DRM license proxying. Its public path is
`/api/get_drm_license/{source}/{token}`.

### 5.9.3 YAML resolver group — `arachnea-stream-hoster`

The `arachnea-stream-hoster` group contains YAML-configurable resolvers for external hosts.
Each YAML file declares at least `resolve_stream`. Recognition queries are optional and are
used to avoid unnecessary network fetches or to identify a page that has already been loaded.

| Query | Type | Description |
|---|---|---|
| `can_resolve_url` | `static` | Optional prefilter before direct resolution; a positive result allows `resolve_stream` to fetch the URL |
| `can_resolve_html` | `static` / `html` | Optional recognition of pre-fetched HTML with `{url, origine, html}` runtime parameters; `{origine}` is the scheme, host, and optional port of the final URL after redirects |
| `resolve_stream` | `html` / `json` / `text` | Extracts the media stream and optional `title`, `image/title > link`, `stream_headers`, `storyboard_vtt_url`, and `storyboard` metadata |

`StreamResolver` walks active YAML services in `services.json` order. On the direct path, it
calls `can_resolve_url` when present; only services with a positive result then run
`resolve_stream` with a network fetch. If no direct stream is found, the resolver fetches the
page once, requires an HTML `Content-Type` and a body below 1 MiB, then calls `can_resolve_html`
with `{url, origine, html}`. The first recognized service runs `resolve_stream` with the same in-memory
HTML through `input_html`, without a second embed-page GET. When no service matches, `get_stream`
returns `{ "embed-link": "<url>" }`.

`stream_headers` are resolver-internal request metadata. They are used when constructing proxy
URLs and are therefore deliberately omitted from the `get_stream` JSON response.

---

### 5.10 Live channels — `list_lives` and `get_live`

#### `list_lives`

Returns a list of MediaItem objects with `key` and `media-type: video/live`:

| YAML field | Description |
|---|---|
| `key` | Channel identifier (e.g. `M6`, `tf1`) |
| `title` | Channel name |
| `link` | Identifier or URL for `get_live` |
| `web-link` | Public live URL |
| `release-date` | Current programme start time |
| `expire` | Current programme end time |
| `channel` | Channel name |

#### `get_live`

Returns playback information for a live channel:

| YAML field | Description |
|---|---|
| `players` | Array of players (see 5.9) with `resolver > kind: "tf1-live"` or `"m6play-live"` |

---

## 6. `shared` section and YAML aliases

Sources use the `shared` section with YAML anchors (`&`) and aliases (`*`)
to factor out repetitive configuration:

```yaml
shared:
  media_types_my_source: &media_types_my_source
    - video/movie
    - video/show/serie

  actions_image_to_url: &actions_image_to_url
    - type: format_text
      argument: "https://images.example.com/{}/raw"

  entries_catalog_cards: &entries_catalog_cards
    - name: title
      type: string
      selector: "h2.card-title"
      actions:
        - type: get_text
    - name: link
      type: string
      selector: "a"
      actions:
        - type: get_attribut
          argument: href
        - type: resolve_url
```

---

## 7. Minimal example

```yaml
id: my-stream-source
title: "My Streaming Source"
logo: "https://example.com/logo.png"
description:
  en: "Description of my streaming source."
http:
  mode: direct

parameters:
  - name: base_url
    value: https://api.example.com

queries:
  - name: service_stream_metadata
    scraper_type: static
    entries:
      - name: id
        type: string
        value: "{service_id}"
      - name: title
        type: string
        value: "{service_title}"
      - name: logo
        type: string
        value: "{service_logo}"
      - name: description
        type: object
        value: "{service_description}"

  - name: search
    scraper_type: json
    base_url: "{base_url}"
    query_url: "{base_url}/search?q={search_terms}&page={page}"
    media_types:
      - video/movie
      - video/show/serie
    row_pointer: /results
    entries:
      - name: current_page
        type: number
        actions:
          - type: format_text
            argument: "{page}"
      - name: total_pages
        type: number
        pointer: /total_pages
      - name: entries
        type: object[]
        pointer: /items/*
        select: all
        entries:
          - name: title
            type: string
            pointer: /title
          - name: description
            type: string
            pointer: /synopsis
          - name: media-type
            type: string[]
            pointer: /type
            actions:
              - type: map
                default: "video/show/other"
                argument:
                  movie: video/movie
                  serie: video/show/serie
          - name: link
            type: string
            pointer: /id
          - name: img/poster > link
            type: string
            pointer: /poster
    post_process:
      - type: derive_pagination
        total_pages_field: total_pages
        remove_fields:
          - total_pages
```

---

## 8. Rust-side correspondence

Results are returned as `ScraperDataNode` trees and serialised to JSON. YAML
naming conventions are preserved as-is (kebab-case, hierarchical paths with
`>`).

The backend entry point is `StreamScraper` in
`server/crates/arachnea-stream/src/stream_scraper.rs`. Each method:

1. Builds a `HashMap<String, String>` of parameters
2. Calls `agregator.execute_query_async("arachnea-stream", "<query_name>", …)`
3. Returns raw or merged rows

On the frontend, normalisers in `front/src/services/rustify.ts` transform
YAML fields (kebab-case, `img/poster > link` paths) into typed TypeScript
objects.
