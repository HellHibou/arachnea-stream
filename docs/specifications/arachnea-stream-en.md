# arachnea-stream YAML format specification

## 1. Overview

An arachnea-stream collection declares a streaming source (legal platform or
alternative catalogue). Each source defines named queries that produce
structured catalogue data — home pages, categories, search results, entry
details, episodes, and live channels.

These queries produce the catalogue and player data exposed by the backend.
The YAML must shape the response into the structures described below.

This document only describes fields and conventions specific to the stream
format. The general structure of a YAML file (`id`, `parameters`, `http`,
`queries`, `entries`, `actions`, `shared`, etc.) is documented in
[the common YAML specification](arachnea-scrapyfy-en.md).

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

AnimeUltime embeds the "Top animes", "Top dramas" and "Top tokusatsu" sections
directly in `load_home`, with IDs `top-animes`, `top-dramas` and `top-tokusatsu`.
They do not require `get_section`. Its HTML selectors apply after `pre_process`
removes the statistics block, so their child positions refer to the processed DOM.

Implement only supported capabilities. The catalogue and player-discovery queries are:

| Query name | Type | Description |
|---|---|---|
| `service_stream_metadata` | `static` | Service metadata (name, logo, themes) |
| `load_home` | `json` / `html` | Home page (categories, sections, banners) |
| `get_category` | `json` / `html` | Content filtered by channel/category |
| `get_section` | `json` / `html` | Paginated section (list of entries) |
| `get_banners` | `json` / `html` | Promotional banner payload loaded from a home banner link |
| `search` | `json` / `html` | Search with filters |
| `get_entry` | `json` / `html` | Entry detail (programme) |
| `get_recommendations` | `json` / `html` | Deferred entry recommendations (see 5.7.2) |
| `get_season` | `json` / `html` | Season episodes |
| `get_players` | `json` / `html` | Deferred player list |
| `resolve_stream` | `json` / `html` / `text` | Optional source-owned extraction, selected through `scraper-query` |
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

The public `get_service` command adds `supported_queries: string[]` to each
metadata row, sorted from the loaded query registry and joined by the exact
`id`/collection name. This is not a manually maintained YAML field. An unmatched
metadata id receives an empty list; the source-home selector only lists rows
advertising `load_home`. Administratively disabled/unloaded sources are not
reactivated by discovery or request filters.

---

### 5.2 Home page — `load_home`

The public REST/Tauri command accepts an optional `sources: string[]` of exact
backend collection identifiers (not display names or category descriptors):

- `{}` keeps the existing all-loaded-sources behavior.
- `{"sources":[]}` returns `{"data":[],"errors":[]}` without provider calls or ETag.
- `{"sources":["arte-fr","rtlplay-be"]}` executes only those sources; duplicates
  are removed and the selection is sorted.
- Explicit `null`, wrong JSON types and blank identifiers are rejected during
  deserialization. Identifiers are not trimmed or corrected.
- Unknown, disabled/unloaded or unsupported sources produce contextual entries
  in `errors`, with no fallback to all sources. Mixed selections preserve valid
  results. Explicitly filtered responses containing errors do not publish an
  ETag; unfiltered requests preserve the existing cache behavior.

REST uses `GET /api/load_home?sources=%5B%22arte-fr%22%5D` (JSON-encoded array).
Tauri uses `invoke('load_home', { sources: ['arte-fr'] })`. Omitting the field
differs from passing an empty array. The response shape and source provenance
remain unchanged; `get_category.sources` retains its descriptor-based meaning.

The public frontend keeps `/` aggregated and exposes `/sources/:sourceId` for
isolated source homes. The selector discovers metadata, not every provider home.
Source-home bookmarks are filtered by source; section preference writes use
`source:<encoded-source-id>:<legacy-key>` in the existing additive IndexedDB
store, with legacy styles as read-only fallback. Existing global preferences
and bookmarks are not migrated or reset.

`load_home` is the lightweight page-discovery query. When the upstream API
provides separate endpoints for banner payloads, category content, or section
items, it should return their links rather than eagerly fetching and embedding
their full contents. The frontend then calls `get_banners`, `get_category`, or
`get_section` on demand.

**Do not use YAML `sub_queries` in `load_home` to enrich every banner, category,
or section when one of these standard queries can load the same payload.** That
pattern turns a single home request into N+1 requests, increases cache and
provider load, and delays first-page rendering. Use `sub_queries` only when no
dedicated stream query can represent the follow-up contract.

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
        - name: subsections   # Optional selectable rails
          type: object[]
          entries:
            - name: key
              type: string
            - name: label
              type: string
            - name: entries   # MediaItem objects specific to this subsection
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
| `request > source` | `string` | Source identifier for `get_category` |

For a lazy category, `request > query_url` is the category endpoint consumed by
`get_category`. `request > name` (or `request > source`) must identify the
source that owns that endpoint.

#### Section

| Field | Type | Description |
|---|---|---|
| `label` | `string` | Rail title (e.g. `Latest additions`, `Trending`) |
| `key` | `string` | Source-owned persistent rail identity, independent of its title |
| `entries` | `object[]` | `MediaItem` objects (see next section) |
| `link` | `string` | URL for `get_section` (pagination) |
| `subsections` | `object[]` | Optional selectable rails below the parent section |

For a lazy section, emit `link` even if `entries` is initially absent or empty.
`get_section` receives this link and the requested one-based page number. Do
not prefetch every section's entries in `load_home` with a `sub_query`.

New rail identity mappings use `key: rail:<provider-id>` for JSON and HTML.
Use the provider's rail/container id, never a media item id, title, array index,
or randomly generated slider id. One fixed category-content rail may use
`rail:category-content`, scoped to its category block. Existing published keys
remain unchanged to avoid invalidating references. Source scope comes from the
parent scraper row. Arte, TF1, France TV and RMC BFM Play now expose rail ids
with this format; labels, title-based aggregation and RMC's internal
`_section_id` deduplication are unchanged.

TeleMB featured rails and Canal Zoom/Télésambre recommendations do not expose
persistent container ids in the inspected HTML. Keep descriptor fallbacks when
available, otherwise treat label references as fragile rather than inventing
stable ids. Télésambre slider UIDs change between requests and must not be used.
Provider ids can change when a rail is replaced; missing references require
repair, not matching to a similar title.

A subsection contains a stable `key`, a `label`, `entries`, and may carry the
same pagination fields as a section (`link`, `current_page`, `have_more`).
Entries from distinct subsections are never flattened or combined: multi-source
aggregation merges them by `key` beneath their parent section. The frontend
renders one active rail at a time and composes its title from the section and
subsection labels.

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

For lazy banners, `load_home` may emit only `banners > link`. The frontend
passes that value to `get_banners`, which returns the populated `banners`
array. Do not fetch banner details through a `load_home` `sub_query` when
`get_banners` is available.

---

### 5.3 MediaItem object (catalogue entry)

Common structure used in `sections[].entries`, `search.entries`,
`get_category.sections[].entries` and `list_lives.entries`.

| YAML field | YAML type | Required | Description |
|---|---|---|---|
| `source` | `string` | yes for an openable card | Source identifier. It must be emitted by YAML on each `MediaItem`, or on its enclosing result row so the frontend can inherit it. It is not automatically injected for every response shape. |
| `title` | `string` | yes | Main title |
| `title/alt` | `string` | no | Alternative title |
| `link` | `string` | yes | Internal URL for `get_entry` |
| `web-link` | `string` | no | Public content URL |
| `description` | `string` | no | Synopsis / description |
| `media-type` | `string` or `string[]` | no | Media type (e.g. `video/show/serie`). Custom `video/other/<label>` and `video/show/other/<label>` forms display their label directly in the frontend. |
| `duration` | `number` or numeric `string` | no | Formatted duration |
| `release-date` | `string` | no | Publication date (`YYYY-MM-DD`) |
| `expire` | `string` | no | Expiration date |
| `year` | `number` | no | Production or copyright year |
| `theme` | `string[]` | no | Genres / themes |
| `lang` | `string[]` | no | Available languages |
| `audio` | `string[]` | no | Alias for `lang` |
| `language` | `string[]` | no | Alias for `lang` |
| `label` | `string` | no | Substitution label (title fallback) |
| `rating` | `number` | no | Rating / score |
| `price` | `string` | no | `free`, `premium` or `<amount> <CURRENCY>` such as `2.99 EUR`; unknown formats are ignored |
| `episode > label` | `string` | no | Episode label on cards |
| `season` | `string` | no | Season |
| `tags` | `string[]` | no | Tags |
| `key` | `string` | no | Identifier for live channels |
| `img/poster > link` | `string` or `string[]` | no | Poster URL(s) |
| `img/portrait > link` | `string` | no | Portrait URL |
| `img/landscape > link` | `string` | no | Landscape URL |
| `img/preview > link` | `string` | no | Preview URL |
| `img > link` | `string` | no | Generic image fallback |
| `img/logo > link` | `string` | no | Logo URL |

> **Note**: Runtime parameter `{source}` does not automatically create an output
> field. Emit `source` explicitly on cards or their enclosing result row,
> especially for `search`, so that the frontend can open the result.

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

`get_category` is the on-demand counterpart of a category emitted by
`load_home`. It receives the category descriptor through `request` fields,
including `request > query_url`, and should fetch the category payload itself.
Do not prefetch every category in `load_home` through `sub_queries`.

---

### 5.5 Paginated section — `get_section`

Returns a list of entries with pagination. It is the on-demand counterpart of
a section emitted by `load_home`: the backend receives the section `link` and
the requested one-based `page` value.

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

### 5.5.1 Promotional banners — `get_banners`

Returns the populated banner payload for a `banners > link` emitted by
`load_home`. The backend exposes the link to YAML as both `{query_url}` and
`{link}`.

```yaml
- name: get_banners
  scraper_type: json
  base_url: "{base_url}"
  query_url: "{query_url}"
  row_pointer: /
  entries:
    - name: banners
      type: object[]
      pointer: /items/*
      select: all
      entries:
        - name: key
          type: string
          pointer: /id
        - name: title
          type: string
          pointer: /title
        - name: image
          type: string
          pointer: /image
```

Use `get_banners` rather than a `load_home` `sub_query` whenever banner data
has its own endpoint or is expensive to assemble. This keeps initial home
navigation fast and lets banner data use its own client-cache lifetime.

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
| `duration` | `number` or numeric `string` | Duration |
| `count-season` | `number` | Number of seasons |
| `content-advisor` | `string` | Content advisory (e.g. parental rating) |
| `price` | `string` | Optional access indicator: `free`, `premium` or a decimal amount and uppercase three-letter currency, e.g. `2.99 EUR` |
| `lang/audio` | `string[]` | Audio language labels |
| `lang/subtitles` | `string[]` | Subtitle language labels; these are labels, not playback track URLs |
| `language` | `string[]` | Languages |
| `subtitles` | `string[]` | Subtitles |
| `theme` | `string[]` | Genres |
| `genre` | `string[]` | Genres (alias) |
| `director` | `string[]` | Directors |
| `casting` | `string[]` | Actors |
| `rating` | `number` | Score |
| `program-id` | `string` | Internal programme identifier |
| `seasons` | `object[]` | Seasons (see 5.7.1) |
| `recommendations` | `object` | Editorial recommendations (see 5.7.2, optional) |
| `players` | `object[]` | Players (see 5.9) |
| `img/poster > link` | `string` | Poster |
| `img/portrait > link` | `string` | Portrait |
| `img/landscape > link` | `string` | Landscape |
| `img/logo > link` | `string` | Logo |
| `img/preview > link` | `string` | Preview |

`duration` represents seconds, including when supplied as a numeric string. Use
`normalize_duration` for textual durations before rendering. Access `price` is a
string: use a decimal point, a space and uppercase currency letters; `2,99 €` is
not the canonical format. `free` and absent prices show no access warning.
Arbitrary provider fields (for example `morality`, `quality`, `hoster-infos` or
`server-index`) are not automatically normalized into public player features.
Keep temporary extraction fields local and remove them after post-processing.

#### 5.7.1 Season

| YAML field | Type | Description |
|---|---|---|
| `label` | `string` | Season name |
| `link` | `string` | URL for `get_season` |
| `episodes` | `object[]` | Pre-loaded episodes (optional) |

#### 5.7.2 Recommendations

`recommendations` is an optional section object: a source with no editorial
relation omits it without failing `get_entry`. Its `label` is optional and is
never required to render the recommendations.

| YAML field | Type | Description |
|---|---|---|
| `entries` | `object[]` | Immediate `MediaItem` objects |
| `link` | `string` | URL for `get_recommendations` (deferred or paginated data) |
| `current_page` | `number` | Page already returned (normally `1` for an immediate response) |
| `have_more` | `boolean` | Whether a next page is available through `get_recommendations` |
| `label` | `string` | Optional section label |

A source provides either the cards already present in the detail response
(`recommendations > entries`) or a recommendations `link`. In the latter case,
the frontend calls `get_recommendations` with the current `source`, that `link`
and the requested one-based page (default `1`). The response reuses the
`get_section` shape (`entries`, `current_page`, `have_more`), with no extra
section envelope. Do not reuse `get_section`: its contract covers generic
catalogue rails. Do not load external recommendations through a `get_entry`
`sub_query` and do not derive recommendations from a search or a category: the
relation must be explicitly provided by the source.

Execution parameters available to `get_recommendations`:

TV5MONDE+ emits the public detail page as `recommendations.link`. The dedicated
HTML query extracts its "Vous pourriez aimer" rail separately from seasons,
with navigable asset API links and no pagination. This query overrides the
inherited Chrome User-Agent with `curl/8.7.1`, since Chrome receives an Akamai
denial on some detail pages. These recommendations are
absent from the asset API; Akamai rejection of the HTML page can prevent their
loading without preventing the API detail response.

The M6+ YAML prototype emits the program ID in `recommendations.link` and obtains
an anonymous JWT in `get_recommendations`, without user credentials. Signing uses
`exec_js` and configurable public client parameters. It scans at most eight
layout pages and separately selects `feature.recommended_programs_by_program`
and `feature.programs_by_tags`, regardless of the editorial title (for example,
"Toujours plus de héros !"). The first block of each family on each page is
selected; their cards are merged and only `program` targets are retained.
Pagination uses the first non-empty `nextPage` value from the fetched blocks.
The selections use distinct internal field names so that each sub-query fetches
only its own family's URLs instead of duplicating cards from both families.
Program cards include `img/poster` and `img/portrait`; card pagination is separate
from layout pagination. Internal JWT fields are removed from public output, but
internal traces must not be shared. Network validation is pending; reaching the
eight-page bound without a rail does not confirm its absence.

TF1+ includes only the "Si vous aimez…" program rail. When its label appears
in the Smart TV detail editorial sections, `get_entry.recommendations.link`
points to the public detail page. `get_recommendations` extracts the rail's HTML
cards with portrait images in both `img/poster` and `img/portrait` for public
thumbnail compatibility, and Smart TV detail links. Cast-related rails,
editorial discovery, "Populaires cette semaine" and episodes are excluded.
There is no pagination (`current_page=1`, `have_more=false`); a missing or empty
rail yields no cards. Loading these recommendations depends on HTML page access.

For RTBF Auvio, `get_entry.recommendations.link` uses the `contentPath` of the
detail page's "A découvrir aussi" widget. `get_recommendations` extracts only
its `PROGRAM` cards with API detail links, public Auvio links and portrait
images in `img/poster` and `img/portrait`. Pagination follows
`/meta/page/current` and `/links/next`. Episodes, next/previous navigation and
general homepage recommendations are excluded; a page without this widget
does not expose a recommendations link.

| Parameter | Description |
|---|---|
| `{query_url}` / `{link}` | Recommendations URL emitted by `get_entry` |
| `{page}` | Requested one-based page number (default `1`) |
| `{page_index}` | `page - 1` |

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
| `duration` | `number` or numeric `string` | Duration |
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

### 5.8.1 Deferred players — `get_players`

Dedicated command for the deferred loading of the players of a single item
(typically an episode) whose original payload declares no `players`. The backend
runs it with the media URL of that item.

| YAML field | Type | Description |
|---|---|---|
| `query_url` | `string` | Item media URL provided by the backend |
| `link` | `string` | Alias for `query_url` |
| `players` | `object[]` | Players (see 5.9) |

The query produces the same `players` shape as `get_entry`. When a source does
not embed players in its episodes, it must declare `get_players`: the frontend
then publishes the episode media `link` as its `players` collection and loads
that collection through this command. Without this query, the API returns no
player and playback stays unavailable for those items.

```yaml
- name: get_players
  scraper_type: json
  base_url: "{base_url}"
  query_url: "{query_url}"
  row_pointer: /
  entries:
    - name: players
      type: object[]
      pointer: /uuid
      select: all
      entries: *entries_player
```

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

For `m6play-video` and `m6play-live`, optional Gigya API-key preparation has a
five-second total budget for the login page and JavaScript bundle, then uses the
bundled fallback key if needed. A shared memory cache retains an extracted key
for 15 minutes, or the fallback key for one minute before retrying discovery.
A Gigya JSONP response with a nonzero numeric `errorCode` invalidates the submitted
key's cache entry for the next attempt, without replaying the POST. This budget
does not bound the complete `get_stream` operation or change subsequent routing.

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
| `subtitles` | `object[]` | Optional ordered external WebVTT subtitle tracks |
| `embed-link` | `string` | Iframe fallback URL, exclusive with `stream_url`; fallback also includes `web-link` with the original URL |

`stream_url` and `embed-link` are mutually exclusive: the former creates a native video player,
while the latter creates an iframe. Resolver `stream_headers` are not part of the public response:
they are embedded in the generated proxy stream URLs.

When a resolver declares `storyboard_vtt_url` with the `resolve_url` action, `proxy: true`, and a
`proxy_replace_all` rule, `get_stream` returns a proxy URL. The proxy then rewrites each WebVTT cue
line that starts with `http://` or `https://` to a proxy URL, then the frontend converts
the `#xywh` coordinates into Video.js sprite configuration. WebVTT files using one sprite image
with uniform cells are supported. If loading or parsing fails, the frontend uses `storyboard` when
available.

`subtitles` contains browser-consumable external WebVTT tracks in resolver order:

| Field | Type | Description |
|---|---|---|
| `lang` | `string` | Optional language code declared by the source; preserved without conversion |
| `label` | `string` | Optional human-readable label displayed by the player |
| `link` | `string` | Required HTTP(S) or application-proxy WebVTT URL |

Tracks with a blank or invalid `link` are discarded. Blank `lang` and `label` values are omitted;
otherwise a track is retained even when both are absent. Empty subtitle lists are omitted from the
JSON response. Resolver implementations must proxy subtitle URLs that require request headers or
would otherwise be unreadable by the browser. The public player adds these tracks as Video.js remote
subtitle tracks after its media source is set, does not enable one by default, and removes its own
remote tracks when the source changes or the player is disposed. Embedded HLS/DASH text tracks remain
managed by their respective playback engines.

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

### 5.9.3 External host resolvers

For `resolver: stream-resolver`, the target is an external embed URL. Recognition,
`resolve_stream`, HTML reuse, proxy headers, terminal errors and complete hoster
examples are specified in [arachnea-stream-hoster](arachnea-stream-hoster-en.md).
Keep those service YAML files in the `arachnea-stream-hoster` group.

For `resolver: scraper-query`, keep `resolve_stream` in this catalogue source's
YAML and set `resolver > source` to its collection ID. This path runs only that
source's fixed `resolve_stream` query; it does not dispatch arbitrary query names.
The target must be an absolute HTTP(S) URL, exposed as both `{url}` and
`{query_url}`. `{proxy_countries}` is a serialized JSON array. A non-empty
`error_message` is terminal; a missing/empty stream is an error, with no iframe
fallback. Use the hoster specification's stream output fields. Returned stream
URLs must be HTTP(S). No host-recognition queries are needed for this path.

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

A player can request geo routing within an allowed country set for a
`scraper-query` resolution by declaring `resolver > proxy > countries` in YAML as a list of ISO alpha-2
codes, for example `countries: ["FR", "BE"]`. Empty, invalid and duplicate
values are discarded; valid values are uppercased while preserving declaration
order at this input boundary; routing does not assign priority to that order.
The frontend forwards the resulting list as `proxy_countries` to
`get_stream`.

The source's `resolve_stream` query can consume the structured JSON list with
`http.proxy_countries: ["{proxy_countries}"]`. Generated proxy URLs embed the
canonical JSON set in their `opts` headers as `Arachnea-Proxy-Countries`.
The proxy ranks all allowed countries together, independently of input order;
accepted destination history and fresh validated capabilities precede unknown
candidates, after importing the relevant persistent caches. A missing or
empty list requests no geo proxy; it does not fall back to a singular country.

#### Geo-routing migration for external integrations

The plural list is the only accepted geo-routing contract. A one-country
request still uses a JSON/YAML list with one item.

| Previous contract (not accepted) | Current contract |
|---|---|
| HTTP header `Arachnea-Proxy-Country: FR` | `Arachnea-Proxy-Countries: ["FR"]` |
| Proxy parameter `country=FR` | `countries=["FR"]` |
| Player YAML `resolver.proxy.country: FR` | `resolver.proxy.countries: ["FR"]` |
| `get_stream` JSON `{ "proxy_country": "FR" }` | `{ "proxy_countries": ["FR"] }` |
| Scrapyfy `http.proxy_country: "{proxy_country}"` | `http.proxy_countries: ["{proxy_countries}"]` |

`Arachnea-Proxy-Country`, `country`, `resolver.proxy.country`,
`proxy_country`, and `proxyCountry` are rejected and must not be emitted by
integrations.

The FranceTV resolver preserves every non-empty country list supplied by the
player and uses `[FR]` only when that list is empty. A resolution also creates
one opaque proxy affinity shared by the K7 calls, manifest signing, DRM token,
manifest loading through `/api/proxy`, and the deferred Widevine license
request. The proxy URL also encodes HTTP `403` as a rejection status: the
selected proxy is rejected for that destination, the affinity moves to another
eligible candidate, and the request is retried once. Rewritten HLS key URLs use
`{proxy_inherited}` so these options are preserved.

TF1+ YAML detail and season players also fetch public `mediainfocombo` metadata
per video and expose `media.geoList` as `resolver.proxy.countries`, including
when playback is denied. The temporary fetch URL is removed from the result;
no hardcoded territory list is substituted.

The TF1+ resolver reads the allowed territory list from `media.geoList` in the
`mediainfocombo` response. It normalizes and deduplicates these codes, then uses
them as proxy countries to retry a geo-blocked negotiation before evaluating
`delivery.code`. Because TF1 can report a geo-restriction with HTTP `200` and
`delivery.code >= 400`, the resolver also installs a mediainfo response
classifier that rejects the current proxy for the destination and lets the
localized proxy search retry another eligible candidate; HTTP `403` remains a
rejection. Content inspection is requested when the original or effective URL
identifies mediainfo, preserving checks across redirects. Non-mediainfo responses,
including CDN manifests, keep status-based classification without a preliminary
prefix read. The Rquest collector preserves the skipped inspection's bounded
reception window before starting its ordinary body-transfer timeout; it still
returns the complete manifest. Callback priority, content rejections and the
overall proxy-search budget remain unchanged. This routing also applies when
resolving the final manifest URL.
The `stream_url` returned by `get_stream` keeps the local HTTP proxy wrapper but
does not carry proxy countries or geo-proxy rejection statuses, so playback no
longer uses a geolocated proxy. The territory list is retained for the storyboard
and deferred Widevine license request, without a local retry or direct fallback.
The storyboard `/api/proxy` URL encodes HTTP `403` as a rejection status, causing
safe sequential retries within the shared 120-second search target, 15-second
engaged-attempt grace, without a fixed endpoint-attempt cap. Modifying operations are not
replayed after uncertain sending without an explicit `proxy_retry_replayable`
boolean in the proxy URL options. Accepted media streams retain their existing
transfer policy; buffered localized bodies use a separate 30-second transfer
timeout.

The TV5MONDE+ resolver uses the player-supplied list for anonymous
authentication and the initial entitlement request. It then extracts country
codes from the `materialProfile` string returned by the `/play` entitlement,
normalizes and deduplicates them, and preserves their order. This list becomes
an unordered set of allowed countries in the proxy context for the manifest, storyboard, and deferred Widevine license
request. If the field is absent or contains no valid alpha-2 code, the initial
player-supplied list is retained.

A player can also request the rewrite of absolute URLs found in proxied HLS
manifests by declaring `resolver > proxy > rewrite_manifest_urls: "true"`. The
frontend forwards it as `proxy_rewrite_manifest_urls` to `get_stream`, and the
backend then adds a proxy `ReplaceAll` post-response action (`{proxy_inherited}/$1`)
restricted to HLS manifest content types. Child playlists and segments therefore
reuse the proxy options of the current request (countries, headers) instead of
leaving the proxied session, which is required when the master manifest exposes
absolute URLs, as for the ARTE live channel.

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

## 8. Operation inputs and authoring procedure

### Runtime inputs

All queries can use collection defaults and `{service_id}`, `{service_title}`,
`{service_logo}`, `{service_description}` and `{source}`. The following inputs belong to specific operations; do not assume they exist in
another operation:

| Query | Runtime inputs |
|---|---|
| `service_stream_metadata`, `load_home`, `list_lives` | Collection metadata and defaults; no content URL is required |
| `search` | `search_query` (raw text), `search_query_json` (JSON string literal including quotes), `search_terms` (URL-encoded text), `media_types` and `themes` (comma-separated), `page` (at least 1), source-specific continuation parameters |
| `get_category` | Source descriptors containing `name` or `source`; remaining descriptor keys become per-source parameters, including `query_url` when declared; `page` and continuation parameters |
| `get_entry` | `query_url` |
| `get_season` | `query_url`, `page` (at least 1) |
| `get_section`, `get_recommendations` | Nonblank link as `query_url` and `link`, `page` (at least 1) |
| `get_banners`, `get_players` | Nonblank link as `query_url` and `link` |
| `get_live` | `channel` — use `{channel}`, not an assumed `{query_url}` |
| `resolve_stream` via `scraper-query` | `url`, `query_url`, `proxy_countries`; see 5.9.3 |

Pagination helpers are documented in the common specification. A runtime
`page_size` or collection default is necessary to derive `offset`. Source-specific
continuation parameters should be returned under `source_params`; do not compute
the next request from an unrelated source's cursor. Existing continuation
parameters take precedence over category descriptor parameters.

### Build and check a source

1. Register the YAML under `server/services/arachnea-stream/`, directly or through
   an imported manifest. Choose a unique `id`; use that ID in source descriptors.
2. Write `service_stream_metadata` as one static row. Declare media types for the
   actual catalogue queries; reuse an anchored list rather than copying it.
3. Add the supported navigation operations. A `load_home` category must carry the
   descriptor needed by `get_category`; a section link must carry the URL expected
   by `get_section`; an entry link must work as `get_entry.query_url`.
4. Emit stable provider identities for sections where available. Do not manufacture
   stable keys from a rail's position or translated title.
5. For shows, choose embedded episodes or season links. Each season link must work
   with `get_season`. For deferred playback, emit `players_link` and implement
   `get_players`. Do not prefetch all seasons or all player pages on the home page.
6. Choose a direct media link, a registered native resolver, `stream-resolver`
   for external host pages, or `scraper-query` for this source's YAML extraction.
   Make sure every descriptor includes the target and source information its path
   requires. A public webpage is not a direct playable media URL.
7. Use `search_terms` in URL queries; use `search_query_json` without adding extra
   quotes when inserting a JSON string literal into a POST body.
8. Check first page, continuation, empty results and unavailable media. Confirm
   that cards retain their source and navigation links lead to the right operation.
9. Use [the source-check specification](arachnea-stream-source-checks-en.md) for
   existing metadata and live checks. Loading a YAML file does not qualify the
   remote source; report network failures separately from contract failures.

### Existing service examples

These optional examples illustrate provider-specific patterns. The complete
authoring contract remains in this guide and the common specification; do not
copy a provider URL, identifier or selector without checking the target response.

- [dark-stream/anime-sama.yaml](../../server/services/arachnea-stream/dark-stream/anime-sama.yaml) — HTML catalogue and post-processing.
- [legal-stream/rtlplay-be.yaml](../../server/services/arachnea-stream/legal-stream/rtlplay-be.yaml) — Next.js data extraction.
- [legal-stream/rtbf-auvio-be.yaml](../../server/services/arachnea-stream/legal-stream/rtbf-auvio-be.yaml) — JSON APIs and follow-up requests.

### Additional queries and result envelopes

A collection can contain helper queries, but names do not create public API
commands. The current `get_episode` in Papadustream V2 is not a standard stream
facade operation. Standard episode navigation uses the documented entry, season
and player operations. Do not require consumers to call an undocumented command.

Field paths containing `>` become nested JSON. Slash-containing names such as
`img/poster` remain literal keys. The backend wraps query output in an aggregation
result carrying `data`, `errors` and validation metadata as applicable. Write the
payload fields described in section 5 in YAML; do not add a YAML `data` wrapper
unless the payload contract explicitly calls for it. A result may carry partial
errors; an empty payload does not by itself establish that an operation succeeded.

## 9. Administration API

The backend exposes an administration API independent from the browsing
frontend. Over HTTP it is mounted under `/api/admin/<operation>` (after the
optional entrypoint root); on desktop it is callable through the Tauri invoke
handler under the same `<operation>` command names.

### 9.1 Access control

- Desktop clients are always authorized.
- In server mode, loopback clients are authorized without authentication.
- Any other client must authenticate with the administrator password to
  obtain a session cookie (`Set-Cookie: arachnea_admin_session=...;
  Path=/; HttpOnly; SameSite=Strict`).
- Writes must be sent with the `POST` method; requests carrying an `Origin`
  or `Referer` header are refused when that origin does not match the `Host`
  header.
- Failed logins are rate-limited per client address.
- Every response carries `Cache-Control: no-store`.

### 9.2 Operations

| Operation | Method | Purpose |
|---|---|---|
| `status` | GET | Runtime mode, capabilities, whether authentication is required for the caller, and whether the caller passes access control. |
| `login` | POST | Validates the administrator password and issues a session cookie. |
| `logout` | POST | Invalidates the current session. |
| `services` | GET | Full service catalog (including disabled and unavailable sources), localized descriptions, logos resolved from known parameters, and effective activation state. |
| `set-service-enabled` | POST | Persists an activation override for one service. |
| `reset-service-enabled` | POST | Removes the override, falling back to the manifest default. |
| `credentials` | GET | Credential state per service (presence and masked login only). |
| `set-credentials` | POST | Stores service credentials in the encrypted store. |
| `clear-credentials` | POST | Removes stored service credentials. |
| `settings` | GET | Effective server port, network mode and root with their provenance. |
| `update-settings` | POST | Persists port/root/network, then applies them hot to the running server when possible (server mode): answers `applied: true` and `admin_url` when the port or the root changes; otherwise `restart_required: true`. Response detail: `{restart_required, applied, apply_error, admin_url}`. |
| `set-admin-password` | POST | Sets or changes the permanent administrator password (Argon2id). When a permanent password is already configured, the current password is required; a missing or incorrect value returns an unauthorized response. |
| `reload` | POST | Validates and reloads every administrable group; each applicable runtime is swapped atomically. Send the empty JSON object `{}` as its request body. The response retains `{applied, build_error}` for the first group and adds `groups: [{service_store_id, applied, build_error}]` for every group. |

Reloading applies updated proxy (`arachnea-proxies`) and IP-country
(`arachnea-ip-countries`) source activation, including when all proxy sources
were disabled at startup. The stable `/proxy` route uses the current runtime;
in-flight requests retain their original runtime. Proxy lists remain loaded on
demand for country routing, not eagerly by the reload action. A rebuilt inventory
resets its in-memory provider-refresh cooldown while retaining persistent records.

### 9.3 Error format

Errors use a stable shape: `{"error":{"code":"<code>","message":"<message>"}}`
with HTTP statuses such as `400 bad_request`, `401 unauthorized`,
`403 forbidden`, `404 not_found`, `405 method_not_allowed`, and
`429 rate_limited`.
