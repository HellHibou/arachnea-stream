# arachnea-stream-hoster YAML authoring specification

## 1. Responsibility and prerequisites

This group recognizes external video embed pages and extracts playable streams.
It does not provide catalogue pages, searches, seasons or episode lists. Catalogue
YAML emits a player descriptor; this group resolves its target.

Use [the common YAML specification](arachnea-scrapyfy-en.md) for collection fields,
HTTP settings, entries, actions, sub-queries, preprocessing and post-processing.
Use [the stream specification](arachnea-stream-en.md) for player descriptors and
public playback metadata. These documents contain the authoring contract; no
source lookup is needed.

Place one collection per hoster in `server/services/arachnea-stream-hoster/`.
Register its file in that directory's `services.json`:

```json
[{"path":"example-hoster.yaml","enabled":true}]
```

Append to the existing array; do not replace other hosters. Manifest order is
resolution priority. Collection IDs must be unique within the group. A collection
requires `resolve_stream` and at least one usable recognition query to be reached.
Recognition queries are individually optional; omitting both makes it unreachable
through the generic resolver.

## 2. Caller and operation inputs

A catalogue can emit this JSON player descriptor through YAML entries:

```json
{"name":"Example host","resolver":"stream-resolver",
 "target":"https://player.example.org/embed/123"}
```

The equivalent legacy object form uses `resolver > kind: stream-resolver` and
`resolver > target_id: <embed URL>`. The target must use HTTP(S).

| Query | Recommended scraper type | Inputs | Required useful output |
|---|---|---|---|
| `can_resolve_url` | `static` | `url`, `origine` | Nonblank scalar `resolver` when the URL matches |
| `can_resolve_html` | `static` or `html` | `url`, `origine`, `html` | Nonblank scalar `resolver` when the document matches |
| `resolve_stream` | `html`, `json` or `text` | `url`, `origine`; `html` only in the HTML fallback phase | One row with `stream_url`, or a terminal `error_message` |

`url` is the original target for direct recognition/extraction and the final
URL after redirects for HTML recognition/extraction. `origine` is deliberately
spelled this way: scheme, hostname and optional port, without a trailing path.
`html` is the fetched document, not a URL. Common `service_*` metadata parameters
and `{source}` are also available. Do not assume catalogue search or pagination
parameters are provided. The application supplies its public proxy path when
configured; do not hardcode the application hostname or port.

Recognition tests **only whether any row has a nonblank `resolver` value**. That
value does not dispatch another service; emit `{service_id}` for clarity. A regex
that does not match must produce no value, not the string `false`, which is nonblank
and would match. Match a precise hostname or a distinctive document signature.
Do not claim every HTML page with a generic `<video>` tag.

## 3. Resolution order and failures

1. Visit active hosters in manifest order. Run `can_resolve_url`; absent or
   nonmatching recognition skips direct extraction for that hoster.
2. For a positive match, run `resolve_stream` using a network request. Only the
   first output row is consumed. A usable stream returns immediately. No row or
   an empty stream continues to the next hoster. Technical direct-path failures
   do not by themselves stop the search.
3. If no direct stream succeeds, fetch the target page for shared HTML detection.
   Require a successful HTTP status, `Content-Type: text/html` (parameters such
   as charset are allowed), and at most 1 MiB (1,048,576 bytes) of body.
   HTTP redirect limit is 16. Simple literal `window.location.href = "..."`
   redirects are followed with bounded HTTP fetches; arbitrary JavaScript is
   not executed by this redirect detection.
4. Visit `can_resolve_html` queries in manifest order using the final URL and
   fetched HTML. The first positive match selects the extractor. For an HTML
   query, declare `input_html: "{html}"` in both recognition and extraction when
   they must reuse the body. Without it, an HTML query can fetch again. JSON/text
   queries do not support that HTML reuse field.
5. If the selected HTML extractor returns no stream or fails, return an error;
   do not continue to another recognized service. Recognition must be specific.
6. If the shared fetch succeeded but no HTML recognizer matched, return an iframe
   fallback with both `embed-link` and `web-link` equal to the original target.
   Shared fetch failures (status, type, size, redirect limit or transport) return
   errors; they do not produce this fallback.

A nonblank `error_message` on the first extraction row takes precedence over
`stream_url` in both phases and stops resolution. Use it for a confirmed business
failure such as a removed video. A blank or absent message has no terminal effect.
Do not emit a permanent error message merely because an optional selector failed.

## 4. Extracted stream schema

These are fields of the first `resolve_stream` row, before the application wraps
and proxies the result. Use the exact underscore spellings shown.

| Field | YAML output type | Required / meaning |
|---|---|---|
| `stream_url` | `string[]` | Required for success; ordered absolute media URLs, first preferred; trimmed blank values are removed |
| `manifest_type` | `string` | Optional; e.g. `m3u8`, `mpd`, `mp4` |
| `error_message` | `string` | Optional terminal business error; overrides a stream |
| `stream_headers` | `object` | Optional map of upstream header names to nonblank string values |
| `title` | `string` | Optional title |
| `image/title > link` | `string` | Optional title image URL |
| `license_url` | `string` | Optional license endpoint for an already supported DRM flow |
| `license_headers` | `object` | Optional string-valued DRM request headers |
| `storyboard_vtt_url` | `string` | Optional browser-readable thumbnail WebVTT URL |
| `storyboard` | `object` | Optional sprite metadata, below |
| `subtitles` | `object[]` | Optional ordered external WebVTT tracks, below |

`stream-url` and `manifest-type` are accepted compatibility aliases; new YAML
should use `stream_url` and `manifest_type`. Only declaring `embed-link` does not
produce an extracted stream. `chapters` is not populated by this YAML converter.

Use `resolve_url` to turn relative extraction results into absolute URLs. Stream
headers are internal: they are embedded in proxy requests and omitted from public
playback JSON. The converter supplies `Referer: <current source URL>` when the
exact `Referer` key is absent. Header names in this map retain their spelling;
write `Referer` consistently. `request_headers` configures the embed-page request;
`stream_headers` configures later media requests. They are different scopes.

The application proxies media URLs when its public proxy endpoint is configured.
Use `resolve_url` with `proxy: true` for auxiliary images, subtitles or thumbnail
resources that need proxy access, and provide their required `proxy_headers`.
For media-specific redirects or replacement rules, `resolve_url` also supports
`proxy_follow_redirects` and `proxy_replace_all`; their complete syntax is in the
common specification. Inspect the generated URLs rather than adding a second
manual proxy prefix.

### Sprite metadata

| Child of `storyboard` | Type | Rules |
|---|---|---|
| `url` | `string` | Required nonblank sprite URL; `link` is a compatibility alias; may contain literal `{index}` |
| `columns`, `rows` | `number` | Required positive integers |
| `width`, `height` | `number` | Optional positive integer thumbnail dimensions; inferred by the player when absent |
| `first_page_index` | `number` | Optional unsigned integer; player defaults to 0 |
| `interval` | `number` | Optional positive seconds between thumbnails |

Missing required grid data, blank URL or zero grid/dimensions causes the storyboard
to be ignored. Preserve literal `{index}` for the player; it is not a service
parameter. Without `interval`, the player derives it from duration and grid size.
`storyboard_vtt_url` is preferred; sprite metadata is the fallback if VTT loading or
parsing fails. The current VTT renderer supports a single sprite with uniform cells.

### External subtitles

Each `subtitles` item has required `link` and optional `lang` and `label`, all
strings. Use an HTTP(S) or application-proxy URL serving browser-consumable WebVTT.
Blank/invalid links are discarded. Blank optional values are omitted; language
codes are preserved. An empty track list is omitted from public JSON. Proxy tracks
that require headers or cannot otherwise be read by the browser. No external track
is enabled by default.

## 5. Complete URL-recognized HTML example

This complete file illustrates extraction from a hypothetical HTML host. Its
selectors and domain must be adapted to the real provider; it is not a live source.

```yaml
id: example-hoster
title: Example hoster
description:
  en: "Extracts HTML5 media from the example host."
http:
  mode: direct
queries:
  - name: can_resolve_url
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{url}"
        actions:
          - type: regex_find_all
            pattern: '^https?://player\.example\.org/embed/'
            format: "{service_id}"

  - name: resolve_stream
    scraper_type: html
    base_url: "{url}"
    query_url: "{url}"
    row_selector: html
    request_headers:
      - name: Referer
        actions:
          - type: format_text
            argument: "{url}"
    entries:
      - name: stream_url
        type: string[]
        selector: "video source[src]"
        actions:
          - type: get_attribut
            argument: src
          - type: resolve_url
      - name: manifest_type
        type: string
        select: first
        actions:
          - type: format_text
            argument: mp4
      - name: title
        type: string
        select: first
        selector: title
        actions:
          - type: get_text
      - name: stream_headers > Referer
        type: string
        select: first
        actions:
          - type: format_text
            argument: "{url}"
```

## 6. Add content recognition without another embed fetch

Add this query to the preceding file's `queries` list:

```yaml
  - name: can_resolve_html
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{html}"
        actions:
          - type: regex_find_all
            pattern: 'data-player-engine="example-hoster"'
            format: "{service_id}"
```

Also add `input_html: "{html}"` to the HTML `resolve_stream` query and this
collection default before `queries`:

```yaml
parameters:
  - name: html
    value: ""
```

In direct mode, no `html` input is provided; the empty default keeps the normal
GET available. Without that default, the missing `{html}` template is an error.
During HTML fallback, the supplied nonempty body overrides the default and
replaces the GET. To recognize HTML with selectors instead, use an HTML query
with `base_url: "{url}"`, `query_url: "{url}"`, `input_html: "{html}"`, a precise
`row_selector` and a `resolver` field built with `format_text`.

### Existing service examples

These optional examples illustrate provider-specific patterns. The complete
authoring contract remains in this guide and the common specification; do not
copy a provider URL, identifier or selector without checking the target response.

- [sibnet.yaml](../../server/services/arachnea-stream-hoster/sibnet.yaml) — URL recognition and HTML stream extraction.
- [voe.yaml](../../server/services/arachnea-stream-hoster/voe.yaml) — HTML-content recognition and inline HTML reuse.

## 7. Author checklist

1. Confirm the target is an embed page, not a catalogue or a manifest.
2. Register the hoster and check activation and priority.
3. Use specific recognition; exercise a nonmatching URL/document as well.
4. For content recognition, reuse HTML explicitly and handle final redirected URLs.
5. Extract a single result row, including all candidate media URLs in `stream_url`.
6. Keep embed request headers and media request headers separate. Validate access
   to the manifest, its segments, subtitles and sprite resources where applicable.
7. Check a removed video produces a meaningful error without hiding ordinary
   optional-field absence behind a terminal error.
8. Validate YAML loading, extracted JSON and actual playback independently.
   A syntax/schema pass cannot establish that an upstream host still works.
