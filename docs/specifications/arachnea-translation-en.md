# Arachnea translation contracts specification

This document describes the generic translation contract owned by
`arachnea-translation`, the subtitle document pipeline owned by the
`arachnea-stream` subtitle module, and their integration with Stream. The
general YAML schema remains documented in
[arachnea-scrapyfy](arachnea-scrapyfy-en.md); this document only covers the translation-specific normalized queries and outputs.

Status: lots 1 to 3 implemented (contracts, crate and subtitle module, generic
bridge, effective catalog and administration, WebVTT parser and document
planning). Lots 4 to 10 add provider execution, orchestration, transport and
the user interface.

## 1. Layer split

| Layer | Responsibilities | Prohibitions |
|---|---|---|
| `arachnea-translation` | Two-letter languages, abstract blocks, capabilities and limits, metadata, selection, planning, cache identity, Scrapyfy bridge. | No knowledge of WebVTT, cues, players or video tracks. No provider-specific Rust. |
| `arachnea-stream` | Rights, proxy-link resolution, catalogs, REST/Tauri assembly and file transport, plus its `subtitles` module: source descriptors, WebVTT parsing/serialization, cue segmentation and document planning, persistent descriptor and token, authorized retrieval and document cache ports, document cache identity and HTTP statuses. | No provider mapping or provider-specific Rust; no unrestricted network access. |
| Public front | Preferences, translation request decision, Video.js lifecycle. | No credentials or provider protocol. |

Providers are described and executed exclusively by Scrapyfy. The generic
bridge calls `get_translation_metadata`, then the declared text or URL query.
No endpoint is called directly from Translation or Stream.

## 2. `get_translation_metadata` query

Every activated service exposes a query named `get_translation_metadata`, either
static or backed by the provider API, with media type
`application/arachnea-translation-metadata`. Its output contains a normalized
object, usually under the `translation` entry:

```yaml
- name: get_translation_metadata
  scraper_type: static
  media_types:
    - application/arachnea-translation-metadata
  entries:
    - name: translation
      type: object
      value:
        schema_version: 1
        languages: [en, fr, de]
        source_detection:
          automatic: true
          returns_detected_language: false
        text_translation:
          query: translate_texts
          accepts_multiple_blocks: true
          size_unit: unicode_code_points
          max_blocks_per_request: 50
          max_units_per_block: 5000
          max_units_per_request: 10000
          max_request_bytes: 65536
          context_support: false
          markup_support: false
        url_translation: null
```

### Fields

| Field | Required | Meaning |
|---|---|---|
| `schema_version` | yes | Normalized contract version. Only version `1` is accepted. |
| `languages` | yes | Common list of two-letter lowercase codes. An empty list allows no translation. |
| `source_languages` | no | Allowed sources. Absent: `languages`. Must be a subset of `languages`. |
| `target_languages` | no | Allowed targets. Absent: `languages`. Must be a subset of `languages`. |
| `supported_pairs` | no | Optional directional `{source, target}` restrictions. Absent: every source × target pair except the same language. Present and empty: no pair. |
| `source_detection` | no | `automatic`, `returns_detected_language` and optionally `query`. |
| `text_translation` | no | Text capability, query and limits. |
| `url_translation` | no | Optional URL capability. |

`supported_pairs` never implies the reverse direction. Every pair must belong to
the declared source and target lists and must not translate into the same
language. Duplicates, invalid codes, languages outside `languages` and zero
limits are rejected. An absent limit means "not declared by the provider", not
"unlimited": local bounds still apply.

### Capabilities

`text_translation` contains `query`, `accepts_multiple_blocks`, `size_unit`,
`max_blocks_per_request`, `max_units_per_block`, `max_units_per_request`,
`max_request_bytes`, `context_support`, `markup_support` and
`preserves_request_order`. `accepts_multiple_blocks: false` limits each request
to one block. `preserves_request_order: true` lets a provider return rows in
request order without block identifiers; the bridge then matches rows to
submitted blocks by position while still validating cardinality.

`url_translation` contains `query`, `accepted_formats`, `max_document_bytes`,
`max_document_units` and `result_mode`. Only `result_mode: inline_document` is
defined by the base contract. A plain HTML page is not subtitle translation; an
upload endpoint is not a URL capability.

`size_unit` accepts `utf8_bytes`, `unicode_code_points` or an unknown unit kept
verbatim. An unknown unit is measured conservatively as UTF-8 bytes.

### Source detection

`automatic: true` means translation accepts an omitted source language.
`returns_detected_language: true` means the response exposes the effective
language. `query` names an optional dedicated detection query.

A normalized `source_language: null` requests detection; `auto` is not a
language code and never appears in `languages`. The bridge passes
`translation_source_language = "auto"` to the YAML, which maps it to the API
representation (omission, `auto` value or another form). With restricted pairs
and an unknown source, a prior detection is required to establish eligibility.

## 3. Text query `translate_texts`

Normalized parameters supplied by the bridge:

| Parameter | Content |
|---|---|
| `translation_source_language` | Two-letter code or `auto`. |
| `translation_target_language` | Two-letter target code. |
| `translation_blocks_json` | JSON array `[{"id":"block-1","text":"Hello"}]`. |

The YAML query builds its body with the generic `build_json`,
`build_json_array` and `json_array_projection` primitives. A provider that
accepts an object body can project texts out of the normalized blocks:

```yaml
request_body_actions:
  - type: build_json
    fields:
      q:
        json_array_projection:
          parameter: translation_blocks_json
          path: /text
      source: {string_parameter: translation_source_language}
      target: {string_parameter: translation_target_language}
```

A provider that requires an array of objects can use `build_json_array`:

```yaml
request_body_actions:
  - type: build_json_array
    source: {json_parameter: translation_blocks_json}
    fields:
      Text: /text
```

The normalized output contains one row per block with `text`, and either an
`id` or positional order under `preserves_request_order`, plus optionally
`detected_source_language`. The bridge validates cardinality and missing,
duplicated or extra identifiers, then returns blocks in input order. An
incomplete response fails; no partial result is published.

A known source equal to the target reuses the original text without a provider
call or a fake translated track.

## 4. Optional URL query `translate_url`

Normalized parameters: `translation_document_url`,
`translation_source_language`, `translation_target_language`. The normalized
output contains `content` and `content_type` for `inline_document`. The complete
content is validated by the format adapter in the `arachnea-stream` subtitle
module.

Without a URL capability, the engine retrieves the document, splits it into
abstract blocks and translates them with `translate_texts`. Private URLs,
application URLs or proxy secrets are never sent to the provider. No implicit
billable fallback occurs after a failed URL translation.

## 5. Selection and exclusions

The policy is `auto` with an optional preferred service, or `constrained` with
one required service. Selection:

1. filters activated, configured services with fresh metadata and available
   budget;
2. checks the source/target pair, the useful capability and context/markup
   requirements;
3. simulates batches without a provider call;
4. ranks a compatible preferred provider, then administrator order, estimated
   batch count and stable identifier.

Normalized exclusion reasons: `unsupported_language_pair`,
`unsupported_capability`, `size_limit`, `budget_exhausted`,
`metadata_unavailable`, `source_language_required`.

The provider is fixed for the whole document before the first call. There is no
switch to another provider after a failed translation call.

## 6. Planning and limits

Planning simultaneously respects:

- `max_blocks_per_request` (or 1 when `accepts_multiple_blocks: false`);
- `max_units_per_block`;
- `max_units_per_request`;
- `max_request_bytes`, measured against the normalized JSON construction
  without a network call.

Splitting an oversized cue remains in the `arachnea-stream` subtitle module,
which must not cut Unicode or markup. Translation rejects an oversized block
instead of truncating it.

Default local bounds: 50 blocks, 10,000 units per block, 50,000 units per
request and 65,536 serialized bytes. Effective limits are the strictest between
provider and local configuration.

## 7. WebVTT document pipeline

The `arachnea-stream` subtitle module parses and serializes complete WebVTT
documents without any provider or player dependency.

- Input must be valid UTF-8. A leading BOM is accepted and CRLF/CR line endings
  are normalized to LF.
- The signature line, optional header metadata, cue identifiers, raw timing
  lines (including cue settings), payload lines, `NOTE`, `STYLE` and `REGION`
  blocks are preserved on output.
- Timestamps are validated as `mm:ss.mmm` or `hh:mm:ss.mmm`; the end time must
  be strictly after the start time. Timing data is never rewritten by
  translation.
- Only visible cue text is submitted. Inline WebVTT tags and internal
  timestamps remain in their cue and act as segment boundaries, so a provider
  cannot alter or drop markup. Entities are decoded for translation and the
  returned text is re-escaped; translated text cannot introduce new tags or
  terminate a cue block.
- A cue is the initial logical unit: its lines are translated together when no
  protected markup splits it. Long text is split at sentence boundaries, then
  word boundaries, then last-resort Unicode character boundaries. Whitespace
  between split pieces is recorded and restored during reconstruction, and no
  Unicode character or markup token is cut.
- Every split segment receives a stable abstract block identifier and a
  verifiable cue/segment mapping. Reconstruction requires exactly the planned
  identifiers, count and order; missing, extra, reordered or empty segments
  fail the document instead of publishing a partial file.
- Local bounds are applied before translation: 4 MiB decompressed, 10,000 cues
  and 500,000 translatable units by default. These are local defaults, distinct
  from provider limits.

SRT is still not part of the initial path and is never served as raw WebVTT.

## 8. Cache

The generic identity includes the source content fingerprint, effective source
and target languages, service, endpoint/model, language options, YAML/action
configuration revision, Translation pipeline revision and access scope. It never
contains credentials. An unknown source stays `auto`; no code is invented.

The `arachnea-stream` subtitle module adds the document fingerprint, format,
WebVTT parser revision and subtitle pipeline revision. The final document ETag
is a strong digest of its bytes, including quotes; the remote ETag only
validates the source.

The generic cache combines backend persistence with HTTP validation. An ETag
alone cannot prevent retranslation after all backend state is lost.

## 9. Planned backend contract

| Command | Purpose |
|---|---|
| `get_translation_services` | Public catalog of usable services, capabilities and languages; no secret. |
| `prepare_subtitle_translation` | Checks rights and candidates, then produces a descriptor and token without retrieving or translating the document. |
| `GET <public-path>/subtitle_translation/<token>.vtt` | Returns the complete WebVTT file, its ETag and cache policy. |
| `HEAD` on the same path | Serves metadata from an existing result only. Without a result, answers `404` without retrieval or translation. |

The token is opaque, stable for its lifetime, and contains no source URL or
credential. Access is checked again on download, and one user's token cannot
access another user's private subtitles. Errors are never served as a WebVTT
file.

### HTTP statuses

| Situation | Status |
|---|---|
| Invalid descriptor, format or document | `400 Bad Request` |
| Access denied | `403 Forbidden` |
| Expired token | `410 Gone` |
| Document, cue or unit bounds exceeded | `413 Payload Too Large` |
| `HEAD` without an existing result | `404 Not Found` |
| Source unavailable or retrieval failed | `502 Bad Gateway` |
| Unsupported pair, capability or size | `422 Unprocessable Entity` |
| Quota or budget unavailable | `429 Too Many Requests` |
| Metadata unavailable or temporary incident | `503 Service Unavailable` |
| Cache or internal state failure | `500 Internal Server Error` |

`Cache-Control: private, no-cache` is proposed to keep the body while
revalidating with `If-None-Match`. A `304` response has no body and keeps the
ETag.

## 10. Out of scope

SRT remains behind an explicit extension. Integrated HLS/DASH subtitles, live
streams, TTML/ASS, audio transcription, iframes without an external track, URL
modes with deferred tasks, document upload, additional providers and
cross-cue linguistic grouping require a separate analysis.
