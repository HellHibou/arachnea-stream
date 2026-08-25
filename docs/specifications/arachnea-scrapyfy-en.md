# arachnea-scrapyfy YAML format specification

## 1. General file structure

A YAML file describes a **query collection** for a data source. The root structure is:

```yaml
id: my-source-identifier
title: "Optional title"
logo: "https://example.com/logo.png"
description:
  fr: "Description in French"
  en: "Description in English"
parameters:
  - name: base_url
    value: "https://example.com"
    description: "Base URL of the source"
http:
  mode: auto
  user_agent_profile: chrome
proxy_insecure_tls_hosts:
  - media.example.invalid
queries:
  - name: my_query
    scraper_type: html
    # ... type-specific fields
```
The optional proxy_insecure_tls_hosts collection field permits an explicit TLS exception for exact HTTPS hosts. Certificate validation remains enabled by default: the proxy normalizes the server-side list, checks it for every request and every redirect, and rejects all other hosts. Use it only for a known temporary provider failure and remove the entry once the certificate is renewed.

---

## 2. Root fields

### `id` (required)
Unique source identifier. Used as a search key.
YAML alias: `name` (interchangeable with `id`).

```yaml
id: m6play-fr
```

### `title` (optional)
Human-readable source title. If absent, the `id` value is used.

```yaml
title: "M6 Play France"
```

### `logo` (optional)
URL or path to the source logo.

```yaml
logo: "https://example.com/logo.png"
```

### `description` (optional)
Multilingual description map, key = language code, value = text.

```yaml
description:
  fr: "Service de streaming M6"
  en: "M6 streaming service"
```

### `parameters` (optional)
List of default collection parameters. Each parameter has:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Name used as `{placeholder}` in templates |
| `value` | string | yes | Default value (may contain `{placeholders}`) |
| `description` | string | no | Parameter description |
| `actions` | array | no | Actions applied to the resolved value |

Parameters are resolved sequentially in YAML declaration order. A parameter can reference previous parameters via `{placeholder}`.

Undeclared placeholders are preserved while loading the collection so callers can provide them at execution time. A required placeholder that remains absent then produces a contextual error while resolving the request, rather than while loading the YAML.

The `@` prefix is reserved for dynamic variables produced while a request is extracted. These variables are not collection parameters and are referenced with the `{@name}` syntax. Standard parameters must not use names starting with `@`.

```yaml
parameters:
  - name: base_url
    value: "https://www.m6play.fr"
  - name: query_url
    value: "{base_url}/api/search"
```

**Automatic injections**: The following parameters are automatically injected at runtime:
- `source` — collection identifier (`id`)
- `service_id` — collection identifier
- `service_title` — collection title
- `service_logo` — collection logo
- `service_description` — serialised JSON description
- `page_index` — derived from `page` (`page - 1`)
- `offset` — derived from `page * page_size`
- `query_separator` — `?`, `&` or `""` depending on the URL

---

## 3. HTTP configuration (`http`)

Applicable at collection, query and sub-query levels. Collection configuration is inherited by each query, and query configuration by its sub-queries.

```yaml
http:
  mode: auto                    # auto | direct | cloudflare_smart | cloudflare_browser
  user_agent_profile: chrome    # chrome | chrome_stable | firefox | firefox_stable
  user_agent: "Custom UA"       # optional: complete User-Agent override
  proxy_country: "US"           # optional: country hint for proxy
  max_redirects: 16             # optional: max redirect count (global default: 16)
```

### HTTP modes (`mode`)

| Value | Description |
|-------|-------------|
| `auto` | Default mode, automatic choice |
| `direct` | Direct request without bypass |
| `cloudflare_smart` | Attempts to bypass Cloudflare intelligently |
| `cloudflare_browser` | Uses a browser to bypass Cloudflare |

### User-Agent profiles (`user_agent_profile`)

| Value | Description |
|-------|-------------|
| `chrome` | Standard Chrome profile |
| `chrome_stable` | Stable Chrome profile |
| `firefox` | Standard Firefox profile (same as `firefox_stable` in practice) |
| `firefox_stable` | Stable Firefox profile |

---

## 4. Scraper type: `scraper_type`

Each query in `queries` must define `scraper_type` with one of the following values:
- `html` — extraction from HTML using CSS selectors
- `json` — extraction from JSON using JSON pointers
- `static` — data declared statically in YAML (no HTTP request)
- `text` — extraction from plain text split by delimiters

---

## 5. Common structure for all scrapers (except static)

The `html`, `json` and `text` scrapers share these fields via `ScraperQueryCommon`:

```yaml
name: "query-identifier"           # required
scraper_type: html|json|text        # required
base_url: "https://example.com"     # required: base URL, available as {base_url}
media_types:                        # optional: content types produced
  - movie
  - series
query_url: "/api/data"              # required: URL template
request_url_actions: []               # optional: actions after URL resolution
request_method: get                 # optional: get (default) | post
request_body_pointer: "/data/payload" # optional: JSON pointer for POST body
request_body_select: first          # optional: first | all (default: all)
request_body_actions: []            # optional: actions on body before sending
request_headers:                    # optional: HTTP headers
  - name: "Authorization"
    pointer: "/auth/token"
    select: first
    actions: []
http: {}                            # optional: HTTP configuration (override)
query_param_mappings: []            # optional: parameter mappings
result_item_field: "entries"        # optional: group field to flatten into rows
pre_process: []                     # optional: body transformations before parsing and fallback ETag hashing
post_process: []                    # optional: post-processing
```

### `pre_process`

Pre-processing applies to the final body returned by an HTTP request, after
redirects and before HTML, JSON, or text parsing. It also runs before the
content-hash fallback used for conditional validation. Remote `ETag` and
`Last-Modified` validators retain priority when present. Without them, the
transformed body produces the `C:` fragment; a matching fragment stops parsing
and all dependent sub-queries. Each root query and sub-query owns its own list;
it is not inherited by children. It does not apply to `input_html`. The `start`
and `end` values may use `{base_url}`, resolved from the runtime parameter when
present and otherwise from the query base URL.

```yaml
pre_process:
  - type: remove_text_blocks
    start: '<script data-volatile="true">'
    end: '</script>'
```

`remove_text_blocks` removes all literal blocks, including both delimiters.
An absent `start` is a no-op. A found `start` without a following `end` is a
configuration error at execution time. Both delimiters must be non-empty.

### `request_headers`

Each header is defined by:

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Header name (e.g. `Authorization`) |
| `pointer` | string (opt.) | JSON pointer to extract value from context |
| `select` | enum | `first` (default) or `all` (values joined with ", ") |
| `actions` | array | Actions applied to the resolved value |

```yaml
request_headers:
  - name: "Authorization"
    pointer: "/data/token"
    select: first
    actions:
      - type: format_text
        argument: "Bearer {}"
```

### `query_param_mappings`

Transforms source parameters into target parameters through a translation table:

| Field | Type | Description |
|-------|------|-------------|
| `target_param` | string | Output parameter name |
| `source_param` | string | Input parameter name (comma-separated values) |
| `values` | map | Mapping table: source key → target value. `{}` = URL-encoded value |
| `item_suffix` | string | Suffix added to each translated item |

```yaml
query_param_mappings:
  - target_param: "category_ids"
    source_param: "categories"
    values:
      movie: "cat_01"
      series: "cat_02"
    item_suffix: ","
```

### `result_item_field`

When a query produces a group whose items should become the result rows, this field indicates the group name to flatten:

```yaml
result_item_field: "entries"
```

With this setting, if the query produces `{ entries: [ { id: 1 }, { id: 2 } ] }`, the result will be `[ { id: 1 }, { id: 2 } ]`.

---

## 6. HTML scraper (`scraper_type: html`)

### Specific fields

```yaml
name: search
scraper_type: html
row_selector: "div.result-item"     # required: CSS selector for each row
row_concurrency: 4                  # optional: max parallel rows processed (default: 4)
input_html: "{html}"                # optional: template resolving to HTML content, bypasses HTTP fetch
entries: []                         # required: field extractors
sub_queries: []                     # optional: sub-queries (see section 10)
```

When `input_html` is set, the template is resolved with runtime parameters
(typically `{html}`) and the resulting HTML is used as the response body
without making an HTTP request. The `query_url` is still used as the
context URL for actions, headers, and error messages. When the template
resolves to an empty string or `{html}` is absent from the runtime
parameters, the normal HTTP fetch path is used.

`input_html` requires at least a `query_url` or a runtime `{url}` parameter
for context URL resolution.

### HTML entries (`entries`)

Each entry can be a **field** (leaf) or a **group** (containing sub-entries).

```yaml
entries:
  # Simple field
  - name: title
    type: string
    selector: "h2 a"                # optional CSS selector
    select: first                   # first (default) | all
    actions:                        # optional: action pipeline
      - type: get_text

  # Group
  - name: images
    type: object[]                   # object | object[] (default: object[])
    selector: "img.poster"
    select: all
    entries:                         # group sub-entries
      - name: url
        type: string
        actions:
          - type: get_attribut
            argument: src
          - type: resolve_url
            proxy: true
```

#### Group `post_build`

An HTML object group may declare ordered transformations that run after all its
child fields have been extracted.

##### `math_formula`

Computes a numeric child from numeric scalar siblings of the same object.

| Field | Type | Description |
|-------|------|-------------|
| `target` | string | `>`-separated child path receiving the computed number |
| `expression` | string | Math expression whose `{path}` placeholders read sibling scalar values |

Expressions are parsed by the built-in deterministic math engine. All
placeholders must resolve to numeric sibling fields at runtime; otherwise the
target is omitted. The expression is validated while loading the YAML.

##### `remove_fields`

Removes temporary sibling fields after a preceding post-build transformation
has consumed them.

| Field | Type | Description |
|-------|------|-------------|
| `fields` | string[] | `>`-separated child paths to remove |

```yaml
- name: storyboard
  type: object
  select: first
  entries:
    - name: count_per_image
      type: number
      actions: [ ... ]
    - name: count_per_row
      type: number
      actions: [ ... ]
  post_build:
    - type: math_formula
      target: rows
      expression: "{count_per_image} / {count_per_row}"
    - type: remove_fields
      fields: [count_per_image, count_per_row]
```

If a formula input is absent or non-numeric at runtime, its target is omitted.

#### HTML entry validation rules:

- A field must define `actions` or `sub_queries` (or both), but not `entries`.
- A group must define `entries` but not `actions`.
- `post_build` is supported only by HTML groups, never by leaf fields.
- `type` is required for both.
- For a group:
  - `type: object` with `select: first` → single object
  - `type: object` with `select: all` → array of objects
  - `type: object[]` → array of objects (incompatible with `select: first`)
- Names support hierarchical paths with `>`:
  ```yaml
  - name: parent>child>field
    type: string
  ```

### `select`

| Value | Description |
|-------|-------------|
| `first` (default for some contexts) | Only the first matching element |
| `all` (general default) | All matching elements |

---

## 7. JSON scraper (`scraper_type: json`)

### Specific fields

```yaml
name: api-search
scraper_type: json
row_pointer: "/data/results/*"           # required: JSON pointer for each row
extract_next_data: false                 # optional: parse HTML as Next.js __NEXT_DATA__
filters:                                 # optional: root filters
  status: ["active", "published"]
sibling_sub_query_concurrency: 4         # optional: sub-query parallelism (default: 4)
sub_query_context_concurrency: 4         # optional: context parallelism (default: 4)
sub_query_fetch_concurrency: 8           # optional: HTTP fetch parallelism (default: 8)
entries: []                              # required: field extractors
sub_queries: []                          # optional: sub-queries
```

### JSON pointer syntax

The JSON pointer system supports:

| Syntax | Description |
|--------|-------------|
| `/data/title` | Simple path in an object |
| `/data/*` | All elements of an array or all values of an object |
| `/data/0` | Numeric index in an array |
| `/data/*[role=mea]` | Filter: array elements where `role` = `"mea"` |
| `/data/*[content/0/idType=8]` | Filter with nested path: elements where `content[0].idType` = `8` |
| `/data/0[status=active]` | Filter on a specific index |
| `/data/*/external_key` | Descend into all sub-elements |

```yaml
# Example: filter by role=cover
row_pointer: "/images/*[role=cover]"

# Example: filter by a nested field in an array object
row_pointer: "/*[content/0/idType=8]"
```

### JSON entries (`entries`)

Each entry can be a **field** or a **group**.

```yaml
entries:
  # Simple field
  - name: title
    type: string
    pointer: "/title"                    # optional JSON pointer
    select: first                        # first | all
    actions:
      - type: get_text

  # Group
  - name: episodes
    type: object[]
    pointer: "/episodes/*"
    select: all
    entries:
      - name: id
        type: string
        pointer: "/id"
      - name: title
        type: string
        pointer: "/title"
```

**Constraint**: In JSON, the `type` entry is **required** for all fields and groups.

**Missing pointer**: If `pointer` is not defined, extraction operates on the row itself.

---

## 8. Static scraper (`scraper_type: static`)

Makes **no HTTP request**. Data is declared in YAML and `{placeholder}` templates are resolved at runtime.

### Specific fields

```yaml
name: metadata
scraper_type: static
media_types:                            # optional
  - metadata
entries: []                             # required: static entries
```

### Static entries (`entries`)

Each entry can have:

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Field name (supports `>` for hierarchical paths) |
| `type` | string | JSON output type (`string`, `number`, `boolean`, `object`, `object[]`) |
| `value` | YAML value | Scalar or structured value (`{placeholder}` template resolved) |
| `actions` | array | Actions applied after template resolution |
| `items` | array | List of static objects (`{placeholder}` in each value) |

**Rules**:
- `type` is required
- `value` and `items` are mutually exclusive
- If `items` is present, `type` must be `object` or `object[]`
- `value` fields and values within `items` are rendered via execution parameters

```yaml
entries:
  # Scalar value with template
  - name: logo
    type: string
    value: "{base_url}/logo.png"
    actions:
      - type: resolve_url
        proxy: true

  # Object value
  - name: platform
    type: object
    value:
      name: "M6 Play"
      url: "{base_url}"

  # List of static items
  - name: quick_links
    type: object[]
    items:
      - label: "Home"
        url: "{base_url}/home"
      - label: "Search"
        url: "{base_url}/search"
```

---

## 9. Text scraper (`scraper_type: text`)

Extracts data from a text response body by splitting into rows (`row_delimiter`) then fields (`field_delimiter`).

### Specific fields

```yaml
name: csv-files
scraper_type: text
row_delimiter: "\n"                    # required: row delimiter
field_delimiter: "|"                   # optional: field delimiter
entries: []                            # required: field extractors
```

### Text entries (`entries`)

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Field name (supports `>` for hierarchical paths) |
| `type` | string | JSON output type (required for fields) |
| `field` | int (opt.) | 1-based index of the field after splitting by `field_delimiter` |
| `actions` | array | Action pipeline applied to the extracted value |
| `entries` | array | Sub-entries (group) |

```yaml
row_delimiter: "\n"
field_delimiter: "|"
entries:
  - name: id
    type: number
    field: 1
  - name: title
    type: string
    field: 2
  - name: duration
    type: number
    field: 3
    actions:
      - type: normalize_duration
```

With a line like `42|Mon Film|1h30`, this produces:
```json
{ "id": 42, "title": "Mon Film", "duration": 5400 }
```

---

## 10. Sub-queries (`sub_queries`)

Sub-queries allow chaining additional HTTP requests from extracted values. They are available in `html` and `json` scrapers. A sub-query attached to a field inside an object group runs separately for each extracted object, so its templates use that object's fields and its result replaces only that field's source URL.

### Common structure (`SubQueryCommon`)

```yaml
sub_queries:
  - scraper_type: html|json            # required: sub-query type
    # Common fields:
    context_pointer: "/data/items/*"   # optional: pointer for context rows
    context_select: all                # optional: first | all
    filters:                           # optional: context filters
      type: ["movie", "series"]
    row_filters:                       # optional: filters on fetched rows
      lang: ["fr", "en"]
    target: "parent>details"           # optional: destination path in result
    request_pointer: "/url"            # optional: pointer for sub-query URL
    request_select: first              # optional: first | all
    request_actions: []                # optional: actions on URL before request
    request_method: get                # optional: get | post
    request_headers: []                # optional: sub-query headers
    http: {}                           # optional: HTTP config
    pre_process: []                    # optional: body transformations before parsing
    post_process: []                   # optional: post-processing
    # Scraper-type-specific fields
```

### `target`

Path where sub-query results are nested. Format: `"parent>child"` (separated by `>`).
If absent, results are merged at root level.

```yaml
target: "details>episodes"
```

### Browser page fetch

HTML queries and sub-queries can use a retained browser page instead of the
normal HTTP transport. `page_navigate` navigates a root HTML query and returns
its stable page source. `page_click` navigates, clicks an element, and returns
the rendered HTML once a configured selector appears. `page_fetch` navigates
then executes an authenticated JavaScript `fetch()` for an HTML sub-query.
`browser_context: origin` reuses the same browser session when origin, browser
profile, and proxy route match. `browser_context: isolated` is available only
to entry-level `page_click` sub-queries: it opens one page per input value and
runs up to four independent clicks concurrently. The page URL and all request
body/header templates use the current
sub-query parameter context; entry fields are also exposed with
non-alphanumeric characters normalized to `_`.

```yaml
http:
  execution: page_navigate
  browser_context: origin
```

`page_navigate` is HTML-only, supports only `GET`, and uses the query URL as
its navigation URL. It must be configured with `browser_context: origin`.

```yaml
http:
  execution: page_click
  browser_context: origin
  page_url: "{episode_url}"
  browser_click:
    selector: '.player-list .lien[onclick*="''{player_field}''"]'
    wait_for_selector: "#videoIframe iframe[src]"
```

`page_click` is HTML-only and available only to sub-queries. It invokes the
selected page element, so page JavaScript can own CAPTCHA rendering, callbacks,
and same-page requests. It then returns the page HTML after `wait_for_selector`
appears. `browser_click.selector` and `wait_for_selector` are required CSS
selectors and support sub-query templates. Use `browser_context: isolated` on
an entry-level sub-query when each click is independent and should use a
dedicated browser page; results retain their input order. Dynamic template
variables must not be used to communicate between concurrent isolated clicks.

```yaml
http:
  mode: auto
  execution: page_fetch
  browser_context: origin
  page_url: "{episode_url}"
  browser_token:
    source: turnstile_callback
    placeholder: "{browser_turnstile_token}"
    cache_scope: domain
    retry_on_rejection: once
    rejection_statuses: [403]
    rejection_body_markers: ["captcha invalid"]
```

`browser_context` must be `origin`. `page_fetch` is HTML-only and is available
only to sub-queries. The token is
kept in memory and is never exposed to extracted data or diagnostics.
`cache_scope: domain` explicitly reuses it only for the matching origin,
browser profile, and proxy route; omitting it retains one-shot behavior.
`retry_on_rejection: once` performs at most one fresh navigation and submission
after a configured rejection signal.

`http.execution` is optional and defaults to the normal direct HTTP path. When
set to `page_navigate`, `http.browser_context: origin` is required. When set to
`page_click`, `http.browser_context: origin` or `isolated`, `http.page_url`, and
`http.browser_click` are required. When set to `page_fetch`,
`http.browser_context: origin` and `http.page_url` are required.

Scrapyfy closes origin-matching retained browser pages after every root query,
once all of its sub-queries complete. Browser page sessions are therefore
query-scoped and require no YAML lifecycle option.
`page_url`, request headers, and request-body values are resolved as sub-query
templates before the browser request is sent.

`browser_token` is optional. When present, `source: turnstile_callback` and a
literal request-body `placeholder` are required; `turnstile_callback` is the
only supported source. `cache_scope` may be omitted or set to `domain`.
`retry_on_rejection` defaults to `never` and may be `never` or `once`.
`rejection_statuses` and `rejection_body_markers` both default to empty lists;
they classify the matching response as a token rejection only when a token was
submitted.

### HTML sub-query

```yaml
  - scraper_type: html
    row_selector: "div.episode"
    entries:
      - name: title
        type: string
        selector: "h3"
        actions:
          - type: get_text
    context_entries:                   # optional: also extract from parent context
      - name: season
        type: number
        pointer: "/season_number"
```

### JSON sub-query

```yaml
  - scraper_type: json
    row_pointer: "/data/*"
    entries:
      - name: id
        type: string
        pointer: "/id"
    request_body_pointer: "/payload"   # optional: POST body from context
    request_body_select: first
    request_body_actions: []
    extract_next_data: false           # optional: parse Next.js
```

### Entry-level sub-queries (`sub_queries` on an `entry`)

HTML and JSON entries can also carry sub-queries. The syntax is the same:

```yaml
entries:
  - name: url
    type: string
    selector: "a"
    actions:
      - type: get_attribut
        argument: href
    sub_queries:
      - scraper_type: html
        row_selector: "div.detail"
        entries:
          - name: description
            type: string
            selector: "p.desc"
            actions:
              - type: get_text
```

---

## 11. Actions (`actions`)

Actions are a transformation pipeline applied to extracted values. They are tagged by `type` in snake_case.

### Dynamic Variables `@...`
Some actions can produce dynamic variables shared during one request execution. These variables must use the reserved `@` prefix and are referenced with the `{@name}` syntax.

Rules:

- Standard collection parameters must not start with `@`.
- A dynamic variable added by an action must start with `@` and contain a name after the prefix.
- `extract_variables` rejects duplicates by default with `on_duplicate: error`.
- `on_duplicate: replace` can be used explicitly when the same request-scoped variable block is read multiple times.
- Dynamic variables are not resolved automatically by every action. Use `replace_variables` exactly where dynamic placeholders should be replaced.
- `replace_variables` resolves only known dynamic variables; standard and unknown placeholders are left unchanged.

### `get_text`
Reads the concatenated text content of the selected HTML element.

```yaml
- type: get_text
```

### `html_to_text`
Converts HTML content to plain text via `quick_html2md`.

```yaml
- type: html_to_text
```

### `get_html`
Adds the selected element HTML to the value list.

```yaml
- type: get_html
```

### `get_attribut`
Reads an HTML attribute (e.g. `href`, `src`, `content`).

```yaml
- type: get_attribut
  argument: "href"
```

### `split`
Divides each value by the separator and replaces the list with the fragments.

```yaml
- type: split
  argument: ","
```

### `map`
Rewrites values through a lookup table.

| Field | Type | Description |
|-------|------|-------------|
| `argument` | mapping | Key → value table. A `null` key matches a null entry |
| `default` | scalar (opt.) | Fallback value when the key is absent. `{}` = original value |

```yaml
- type: map
  argument:
    movie: "film"
    series: "série"
  default: "other"
```

Values mapped to `null` are removed from the output list.

### `regex_find_all`
Applies a regex and replaces the list with captured groups.

| Field | Type | Description |
|-------|------|-------------|
| `pattern` | string | Regular expression |
| `format` | string | Output template: `{1}` = group 1, `{request_url}`, `{base_url}`, etc. |

```yaml
- type: regex_find_all
  pattern: "id=(\\d+)"
  format: "{1}"
```

### `extract_variables`
Extracts dynamic variables from the current values and adds them to the request context. The current value list is preserved unchanged.

| Field | Type | Description |
|-------|------|-------------|
| `pattern` or `regex` | string | Regular expression applied to each value |
| `name` | string | Variable name template, with captures `{1}`, `{2}`, etc. The rendered result must start with `@` |
| `value` | string | Variable value template, with captures `{1}`, `{2}`, etc. |
| `on_duplicate` | string | Duplicate policy: `error` by default or `replace` |

Extracted variables are available as dynamic placeholders `{@name}` for actions that explicitly resolve dynamic variables. A duplicate `@...` variable is a runtime error.

```yaml
- type: extract_variables
  regex: "\\b([A-Za-z][A-Za-z0-9_]*)\\s*=\\s*(\\d+)\\s*;"
  name: "@{1}"
  value: "{2}"
  on_duplicate: replace
```

### `replace_variables`
Explicitly replaces dynamic placeholders in the current values. This action does not resolve standard parameters and does not modify unknown placeholders.

| Field | Type | Description |
|-------|------|-------------|
| `variable_prefix` | string | Variable prefix to resolve. Only `@` is currently supported. Default: `@` |

```yaml
- type: replace_variables
  variable_prefix: "@"
```

Example: when `@PortPart` is `80`, the value `"({@PortPart}+1)"` becomes `"(80+1)"`. Placeholders such as `{country}` or `{@Missing}` are left unchanged.

### `get_request_url`
Adds the current request URL to the list of values.

```yaml
- type: get_request_url
```

### `get_response_body`
Adds the raw response body to the list of values.

```yaml
- type: get_response_body
```

### `suffix`
Adds a constant suffix to each value.

```yaml
- type: suffix
  argument: ".jpg"
```

### `max`
Keeps only the largest positive integer value.

```yaml
- type: max
```

### `resolve_url`
Resolves each value as a relative URL against the page URL.

| Field | Type | Description |
|-------|------|-------------|
| `proxy` | bool | If true, wraps HTTP(S) URLs through the public proxy |
| `proxy_headers` | object | HTTP headers embedded in the proxy URL and sent upstream; requires `proxy: true`; values support query placeholders, `{request_url}`, and `{request_origin}` |
| `proxy_replace_all` | object[] | Ordered `ReplaceAll` rules attached to the proxy URL; requires `proxy: true` |

```yaml
- type: resolve_url
  proxy: true
  proxy_headers:
    Referer: "{request_origin}/"
  proxy_replace_all:
    - pattern: '(?m)^(https?://[^\r\n]+)'
      replacement: '{proxy}/$1'
      content_types: [text/vtt]
```

`proxy_headers` values support named query placeholders, `{request_url}`, and `{request_origin}`. Each replacement rule has a required regex `pattern`, a required `replacement` template supporting `$1`, `$2`,
and other captures plus proxy variables such as `{proxy}` and `{proxy_inherited}`, and an optional `content_types` MIME type
list. The proxy applies the rules to textual response bodies in declaration order.

`{proxy}` resolves to the public proxy path (e.g. `/api/proxy`). `{proxy_inherited}` resolves to the same proxy path but
includes the `opts_…` segment from the current request URL when present (e.g. `/api/proxy/opts_ABCD`), allowing
ReplaceAll rules to preserve inherited proxy options in rewritten URLs.

### `resolve_url_from_parent`
Resolves relative URLs against an ancestor of the page URL.

| Field | Type | Description |
|-------|------|-------------|
| `levels` | usize | Number of path segments to go up |
| `proxy` | bool | Wrap through the public proxy |
| `proxy_headers` | object | HTTP headers embedded in the proxy URL and sent upstream; requires `proxy: true`; values support query placeholders, `{request_url}`, and `{request_origin}` |
| `proxy_replace_all` | object[] | Ordered `ReplaceAll` rules attached to the proxy URL; requires `proxy: true` |

```yaml
- type: resolve_url_from_parent
  levels: 2
  proxy: false
```

### `get_url_host`
Replaces each value with the hostname of the parsed URL (removes `www.`).

```yaml
- type: get_url_host
```

### `build_nextjs_data_url`
Converts each value (page path) to a `/_next/data/...json` URL.

| Field | Type | Description |
|-------|------|-------------|
| `data_root` | string (opt.) | Prefix before `/_next/data` (e.g. `/rtlplay`) |
| `route_prefix` | string (opt.) | Prefix between build id and path |
| `page_path_prefix_to_strip` | string (opt.) | Prefix to strip from current path |

```yaml
- type: build_nextjs_data_url
  data_root: "/site"
  route_prefix: "detail"
```

### `ratio`
Multiplies numeric values by a factor.

```yaml
- type: ratio
  argument: 0.5
```

### `math_formula`
Evaluates a math expression for each value. The `{value}` placeholder is replaced by the current value before evaluation. The output list contains one result per input.

```yaml
- type: math_formula
  formula: "{value} * 2 + 5"
```

### `replace_text`
Replaces all occurrences of a string with another.

```yaml
- type: replace_text
  search: "&amp;"
  replace: "&"
```

### `format_text`
Formats each value via a template.

Available placeholders:
- `{}` = current value
- `{request_url}` = request URL
- `{base_url}`, `{locale}`, etc. = execution parameters

```yaml
- type: format_text
  argument: "{base_url}/image/{}.jpg"
```

### `build_url`
Builds a URL from multiple JSON response fields.

| Field | Type | Description |
|-------|------|-------------|
| `base` | string | URL template with named placeholders |
| `fields` | mapping | Mapping: placeholder → JSON pointer |

```yaml
- type: build_url
  base: "{base_url}/{program_code}-p_{program_id}/{clip_code}-c_{clip_id}"
  fields:
    program_code: "/program/code"
    program_id: "/program/id"
    clip_code: "/clip/code"
    clip_id: "/clip/id"
```

### `extract_field`
Extracts a field from the JSON response body by JSON pointer.

```yaml
- type: extract_field
  path: "/data/build_id"
```

### `get_date`
Normalises dates to `YYYY-MM-DD` format.

| Field | Type | Description |
|-------|------|-------------|
| `format` | string or array | Input format(s) tried in order |
| `months` | mapping (opt.) | Month name mapping table (needed for `dd_month_yyyy`) |

Supported formats (`GetDateSource`):

| YAML value | Description |
|------------|-------------|
| `yyyy_mm_dd` | `2026-03-25` or `2026-03-25T20:15:00+01:00` |
| `dd_mm_yyyy` | `25/03/2026` or `25.03.2026` |
| `dd_mm_yy` | `25/03/26` or `25.03.26` (70-year pivot: 70-99→19xx, 0-69→20xx) |
| `dd_month_yyyy` | `Mar 25, 2026` (needs `months`) |
| `days_from_today` | `"3"` → today + 3 days |
| `yyyy_mm_dd_hh_mm_ss` | `2026-03-25T20:15:00+01:00` or `2026-03-25 20:15:00` |

```yaml
- type: get_date
  format: yyyy_mm_dd

# Multiple formats tried in order
- type: get_date
  format:
    - yyyy_mm_dd
    - dd_mm_yyyy
    - dd_month_yyyy
  months:
    Jan: 1
    Feb: 2
    Mar: 3
    Apr: 4
    May: 5
    Jun: 6
    Jul: 7
    Aug: 8
    Sep: 9
    Oct: 10
    Nov: 11
    Dec: 12
```

### `normalize_duration`
Parses human-readable durations (e.g. `39 min`, `1 h 05 min`) into seconds.

```yaml
- type: normalize_duration
```

### `base64_decode`
Decodes each value from Base64 (standard alphabet). Non-decodable values are kept as-is.

```yaml
- type: base64_decode
```

### `hex_decode`
Decodes each hexadecimal value into UTF-8 text. Values containing non-hexadecimal characters, an
odd number of digits, or invalid UTF-8 bytes are discarded.

```yaml
- type: hex_decode
```

### `aes_cbc_decrypt`
Decrypts each hexadecimal value with AES-128-CBC, a 16-byte UTF-8 key, and a 16-byte UTF-8 IV.
Malformed or invalid values, and plaintext that is not UTF-8, are discarded.

```yaml
- type: aes_cbc_decrypt
  key: "sixteen-byte-key"
  iv: "sixteen-byte-iv!"
```

### `request_url_actions`
This action list transforms the URL produced after `query_url` interpolation and before the HTTP
request. It is useful when an input URL must be converted into an API endpoint.

```yaml
query_url: "{url}"
request_url_actions:
  - type: regex_find_all
    pattern: '#([A-Za-z0-9]+)$'
    format: "https://example.invalid/api?id={1}"
```

### `caesar_shift`, `regex_replace_all`, `bytes_shift`, `reverse`, and `json_extract_text`

Generic transformations for text values.

```yaml
- type: caesar_shift
  shift: 13
- type: regex_replace_all
  pattern: '[^A-Za-z0-9]'
  replacement: ''
- type: bytes_shift
  value: -3
- type: reverse
- type: json_extract_text
  path: /source/url
```

`caesar_shift` rotates ASCII letters by a signed offset. `regex_replace_all` performs a global regex replacement. `bytes_shift` applies a wrapping signed delta to every UTF-8 byte and preserves a value when the result is not valid UTF-8. `reverse` reverses Unicode scalar values. `json_extract_text` parses each current value as JSON and returns the scalar addressed by its JSON Pointer.

### `exec_js`
Executes JavaScript in an isolated sandbox (`boa_engine`) and returns new global
numeric variables as `Name=Value` lines.

The current pipeline value (`text`) is treated as JS code to execute.

Three JS sources can be injected, in this order:
1. **HTML** (`inject_html_scripts: true`) — extracts inline `<script>` blocks from
   the HTML `response_body` and executes them
2. **Hardcoded scripts** (`scripts`) — list of JS strings injected before the main code
3. **Pipeline text** (`text`) — main JS code from the previous action

| Field | Type | Default | Description |
|---|---|---|---|
| `timeout_ms` | u64 | 500 | Max instruction budget via `loop_iteration_limit` |
| `inject_html_scripts` | bool | false | When true, extracts scripts from the HTML response body and executes them first |
| `scripts` | string[] | [] | Hardcoded JS snippets injected before the main code |

```yaml
- type: exec_js
  timeout_ms: 500
  inject_html_scripts: false
  scripts:
    - "function helper(x) { return x ^ 42; }"
```

Typical usage with Spys.one:
```yaml
- type: get_response_body
- type: exec_js
  timeout_ms: 500
  inject_html_scripts: true
- type: extract_variables
  pattern: "^([A-Za-z_][A-Za-z0-9_]*)=(\\d+)$"
  name: "{1}"
  value: "{2}"
  on_duplicate: replace
```

### `unpack_packer`
Unpacks the deterministic Dean Edwards Packer format without executing JavaScript. The action
accepts only calls whose payload, radix, symbol count, dictionary, and `split` separator are
literals. A non-conforming or malformed value is discarded.

Radices from 2 through 62 are supported.

```yaml
- type: unpack_packer
```

---

## 12. Post-processing (`post_process`)

Transformations applied after initial field extraction. Available on `html` and `json` queries, and on sub-queries.

Tagged by `type` in snake_case.

### `apply_actions_to_field`

Applies a normal action pipeline to all scalar values from one root field and writes the result to another root field.

| Field | Type | Description |
|---|---|---|
| `source` | string | Existing root field to read |
| `target` | string | Root field to create or replace |
| `output_type` | string (opt.) | Output type assigned to the target |
| `actions` | array | Non-empty action pipeline |

```yaml
- type: apply_actions_to_field
  source: encoded
  target: stream_url
  output_type: string[]
  actions:
    - type: base64_decode
    - type: json_extract_text
      path: /source
```

### `fetch_actions_to_field`

Fetches every URL in a root scalar field, then applies an action pipeline to each response and writes the combined values to a target field.

| Field | Type | Description |
|---|---|---|
| `source` | string | Root field containing request URLs |
| `target` | string | Root field receiving extracted values |
| `output_type` | string (opt.) | Output type assigned to the target |
| `request_actions` | array (opt.) | Actions applied to URLs before fetching |
| `actions` | array | Non-empty action pipeline applied to response bodies |

### `copy_item_fields`

Copies fields between every item of a root group. Source nodes are cloned, so
their values, output types, and nested structure are preserved.

| Field | Type | Description |
|---|---|---|
| `source` | string | Root group containing the items to update |
| `fields` | array | Non-empty list of source/target field mappings |

```yaml
- type: copy_item_fields
  source: players
  fields:
    - source: embed-link
      target: web-link
```


### `extract_regex_items`
Builds group items from repeated regex matches in a text field.

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | Source field containing text to parse |
| `target` | string | Target field receiving extracted items |
| `pattern` | string | Regular expression with capturing groups |
| `entries` | array | Definition of fields extracted from each match |

```yaml
- type: extract_regex_items
  source: "raw_html"
  target: "episodes"
  pattern: '<a href="(.*?)">(.*?)</a>'
  entries:
    - name: url
      type: string
      capture_group: 1
      actions:
        - type: resolve_url
          proxy: true
    - name: title
      type: string
      capture_group: 2
```

Each `entry` in `entries`:

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Output field name (supports `>`) |
| `type` | string (opt.) | JSON output type |
| `capture_group` | int (opt.) | Capture group index (1-based) |
| `actions` | array | Actions applied to the extracted value |

### `filter_items`
Filters items in a group by testing a scalar field with a regex.

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | Source group field |
| `field` | string | Scalar field to test in each item |
| `pattern` | string | Regex: if `keep_matching: true`, keeps matching items |
| `keep_matching` | bool | `true` (default) = keep matches, `false` = keep non-matches |

```yaml
- type: filter_items
  source: "videos"
  field: "format"
  pattern: "mp4|hls"
  keep_matching: true
```

### `filter_fields`
Removes specific fields from items in a group when a condition field value matches (or does not match) a regex pattern.

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | Source group field |
| `field` | string | Scalar field to test in each item |
| `pattern` | string | Regex applied to the condition field value |
| `keep_matching` | bool | `true` (default) = remove fields from matching items, `false` = remove from non-matching items |
| `remove` | string[] | List of `>`-delimited paths of fields to remove |

```yaml
- type: filter_fields
  source: "entries"
  field: "resourceType"
  pattern: "^PROGRAM$"
  keep_matching: true
  remove:
    - img/landscape > link
    - _internal_field
```

### `fetch_regex_items_from_items`
For each item in a group, makes an HTTP request, applies a regex on the response, and adds results to the target path.

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | Source group field |
| `request_field` | string | Item field containing the URL to fetch |
| `request_actions` | array | Actions applied to the URL before the request |
| `request_method` | `get` or `post` (opt.) | HTTP method; defaults to `get` |
| `response_pointer` | string (opt.) | JSON pointer selecting response text before regex extraction |
| `target` | string | Target path for extracted items |
| `target_per_source` | boolean (opt.) | Append items below each source item instead of the root target |
| `pattern` | string | Regex applied to the response body |
| `entries` | array | Field definitions for extracted items |
| `copy_item_fields` | array | Fields to copy from source item to target item |

```yaml
- type: fetch_regex_items_from_items
  source: "episodes"
  request_field: "detail_url"
  request_actions:
    - type: resolve_url
  target: "episodes>sources"
  pattern: '<source src="(.*?)" type="(.*?)">'
  entries:
    - name: url
      type: string
      capture_group: 1
    - name: format
      type: string
      capture_group: 2
  copy_item_fields:
    - source: "id"
      target: "episode_id"
```

### `pivot_items_by_index`
Reorganises aligned value lists into indexed items.

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | Source group field |
| `target` | string | Target field |
| `values_field` | string | Field containing value lists |
| `nested_field` | string | Field within values_field to use as pivot |
| `nested_value_field` | string | Sub-field of the value to pivot |
| `sort_by` | string (opt.) | Sort field for resulting items |
| `copy_item_fields` | array | Fields to copy from source item |
| `copy_root_fields` | array | Root fields to copy |
| `promote_first_nested_fields` | array | Fields to promote from the first item |
| `copy_target_fields` | array | Fields to copy into the target item |
| `generated_fields` | array | Generated fields (1-based index) |

`ScraperFieldMapping` structure (for `copy_item_fields`, `copy_root_fields`, `copy_target_fields`):

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | Source path |
| `target` | string | Destination path |

`ScraperGeneratedField` structure:

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Field name |
| `type` | string (opt.) | JSON type |
| `format` | string | Template where `{}` = 1-based index |

```yaml
- type: pivot_items_by_index
  source: "qualities"
  target: "sources"
  values_field: "values"
  nested_field: "format"
  nested_value_field: "url"
  sort_by: "height"
  generated_fields:
    - name: "label"
      type: string
      format: "Source {}"
  copy_item_fields:
    - source: "id"
      target: "item_id"
```

### `compute_items_field`
Computes a scalar field via a mathematical expression per item.

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | Source group field |
| `nested_source` | string (opt.) | Group sub-field |
| `target` | string | Target field for the result |
| `expression` | string | Mathematical expression |
| `variables` | mapping | Variables available in the expression |

`ScraperComputedFieldVariable` structure:

| Field | Type | Description |
|-------|------|-------------|
| `path` | string (opt.) | Resolution path (default: variable name) |
| `scope` | enum | `auto` (default), `current`, `parent`, `root`, `params` |

```yaml
- type: compute_items_field
  source: "qualities"
  target: "height"
  expression: "{height} * 2"
  variables:
    height:
      path: "height"
      scope: current
```

Variable scopes:

| Value | Description |
|-------|-------------|
| `auto` (default) | Searches: current item → parent item → root → parameters |
| `current` | Currently processed item |
| `parent` | Parent item of `nested_source` |
| `root` | Query root node |
| `params` | Execution parameters |

### `derive_pagination`
Derives pagination metadata from extracted fields.

| Field | Type | Description |
|-------|------|-------------|
| `source` | string (opt.) | Source field (optional) |
| `entries_field` | string | Field containing entries (default: `"entries"`) |
| `current_page_field` | string | Field containing current page (default: `"current_page"`) |
| `total_pages_field` | string (opt.) | Field containing total page count |
| `have_more_field` | string | Boolean "has more" field (default: `"have_more"`) |
| `next_value_field` | string (opt.) | Field containing next page value |
| `next_param` | string | Parameter name for next page (default: `"page"`) |
| `source_params_target` | string | Target field for source parameters (default: `"source_params"`) |
| `page_size_field` | string (opt.) | Field containing page size |
| `infer_from_full_page_field` | string (opt.) | Field to infer from full page |
| `remove_fields` | array | Fields to remove after derivation |

```yaml
- type: derive_pagination
  current_page_field: "page"
  have_more_field: "has_next"
  next_param: "page"
  remove_fields:
    - "page"
    - "has_next"
```

### `append_static_items`
Adds static items to a group, with optional deduplication.

| Field | Type | Description |
|-------|------|-------------|
| `target` | string | Target group field |
| `items` | array | List of items (each item is a string→string map) |
| `unique_field` | string (opt.) | Field used for deduplication |

```yaml
- type: append_static_items
  target: "links"
  items:
    - label: "Home"
      url: "{base_url}"
    - label: "Contact"
      url: "{base_url}/contact"
  unique_field: "url"
```

### `set_nested_fields`
Sets one or more fields on every item of a sub-array, using either a static
value or a copy from a sibling field.

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | Source group field containing items |
| `nested_source` | string | Sub-array field within each source item |
| `fields` | array | Field definitions to apply |

`ScraperNestedFieldDefinition` structure:

| Field | Type | Description |
|-------|------|-------------|
| `target` | string | Destination path (supports `>` for hierarchical paths) |
| `value` | string (opt.) | Static value to write (mutually exclusive with `copy_from` and `host_from`) |
| `copy_from` | string (opt.) | Source field to copy from the same item (mutually exclusive with `value` and `host_from`) |
| `host_from` | string (opt.) | Extracts the hostname from the URL in this sibling field (mutually exclusive with `value` and `copy_from`) |

```yaml
- type: set_nested_fields
  source: "episodes"
  nested_source: "players"
  fields:
    - target: resolver > kind
      value: "stream-resolver"
    - target: resolver > target_id
      copy_from: web-link
    - target: name
      host_from: web-link
```

---

## 13. Output types (`type` / `ScraperOutputType`)

| YAML value | Description |
|------------|-------------|
| `string` | Character string |
| `number` | Number (integer or floating-point) |
| `boolean` | Boolean |
| `object` | JSON object |
| `string[]` | Array of strings |
| `number[]` | Array of numbers |
| `boolean[]` | Array of booleans |
| `object[]` | Array of objects |

---

## 14. Output data model (`ScraperDataNode`)

The result of a query is a tree of nodes. Each node can contain:

- `values` — scalar values
- `children` — named sub-nodes (hierarchical paths)
- `items` — explicit items (arrays of objects)
- `output_type` — declared JSON type

Hierarchical paths are expressed with `>` in names:
```yaml
name: "media>images>poster"
```

This creates the structure:
```json
{ "media": { "images": { "poster": "..." } } }
```

---

## 15. Complete example

```yaml
id: example-source
title: "Example Source"
description:
  fr: "Une source d'exemple complète"
parameters:
  - name: base_url
    value: "https://www.example.com"
  - name: api_url
    value: "{base_url}/api"

http:
  mode: auto
  user_agent_profile: chrome

queries:
  # Static query for metadata
  - name: service_metadata
    scraper_type: static
    media_types: [metadata]
    entries:
      - name: name
        type: string
        value: "Example Source"
      - name: logo
        type: string
        value: "{base_url}/logo.png"
        actions:
          - type: resolve_url
            proxy: true

  # JSON query for search
  - name: search
    scraper_type: json
    base_url: "{base_url}"
    media_types: [movie, series]
    query_url: "{api_url}/search?q={query}&page={page}"
    row_pointer: "/results/*"
    entries:
      - name: id
        type: string
        pointer: "/id"
      - name: title
        type: string
        pointer: "/title"
      - name: type
        type: string
        pointer: "/type"
      - name: year
        type: number
        pointer: "/year"
      - name: poster
        type: string
        pointer: "/poster"
        actions:
          - type: resolve_url
            proxy: true
    sub_queries:
      - scraper_type: json
        context_pointer: "/results/*"
        target: "details"
        request_pointer: "/detail_url"
        request_actions:
          - type: resolve_url
        row_pointer: "/data/*"
        entries:
          - name: synopsis
            type: string
            pointer: "/synopsis"
          - name: duration
            type: number
            pointer: "/duration"
            actions:
              - type: normalize_duration
          - name: genres
            type: string[]
            pointer: "/genres/*"
    post_process:
      - type: derive_pagination
        current_page_field: "page"
        have_more_field: "has_next"
        next_param: "page"

  # HTML query for details
  - name: details
    scraper_type: html
    base_url: "{base_url}"
    media_types: [movie, series]
    query_url: "/program/{id}"
    row_selector: "main.content"
    entries:
      - name: title
        type: string
        selector: "h1.title"
        actions:
          - type: get_text
      - name: description
        type: string
        selector: "div.description"
        actions:
          - type: html_to_text
      - name: duration
        type: number
        selector: "span.duration"
        actions:
          - type: get_text
          - type: normalize_duration
      - name: thumbnail
        type: string
        selector: "meta[property='og:image']"
        actions:
          - type: get_attribut
            argument: content
      - name: images
        type: object[]
        selector: "img.gallery"
        entries:
          - name: url
            type: string
            actions:
              - type: get_attribut
                argument: src
              - type: resolve_url
                proxy: true
          - name: alt
            type: string
            actions:
              - type: get_attribut
                argument: alt

  # Text query (CSV)
  - name: episodes_csv
    scraper_type: text
    base_url: "{base_url}"
    media_types: [episodes]
    query_url: "/episodes.csv"
    row_delimiter: "\n"
    field_delimiter: ","
    entries:
      - name: id
        type: string
        field: 1
      - name: season
        type: number
        field: 2
      - name: episode
        type: number
        field: 3
      - name: title
        type: string
        field: 4
      - name: duration_min
        type: number
        field: 5
        actions:
          - type: ratio
            argument: 60
```
