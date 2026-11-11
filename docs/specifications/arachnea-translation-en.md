# Arachnea translation contracts specification

This document describes the generic translation contract owned by
`arachnea-translation`, the subtitle document pipeline owned by the
`arachnea-stream` subtitle module, and their integration with Stream. The
general YAML schema remains documented in
[arachnea-scrapyfy](arachnea-scrapyfy-en.md); this document only covers the translation-specific normalized queries and outputs.

Status: lots 1 to 6 implemented (contracts, crate and subtitle module, generic
bridge, effective catalog and administration, WebVTT parser and document
planning, Azure Translator/LibreTranslate text execution with retries, budgets
and metrics), with authorized direct/proxy retrieval, conditional source
revalidation, complete text/URL document orchestration, and the public
REST/Tauri JSON and file transport. Local simulated-provider qualification
passes; real provider qualification still requires credentials. Lots 7 to 10
add browser storage and the user interface.

## 1. Layer split

| Layer | Responsibilities | Prohibitions |
|---|---|---|
| `arachnea-translation` | Two-letter languages, abstract blocks, capabilities and limits, metadata, selection, planning, translation revision identity, Scrapyfy bridge. | No knowledge of WebVTT, cues, players or video tracks. No provider-specific Rust. |
| `arachnea-stream` | Rights, proxy-link resolution, catalogs, REST/Tauri assembly and file transport, plus its `subtitles` module: source descriptors, WebVTT parsing/serialization, cue segmentation and document planning, persistent descriptor and token, authorized conditional retrieval, composite validator and HTTP statuses. | No provider mapping or provider-specific Rust; no unrestricted network access. |
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
          query: detect_language
        text_translation:
          query: translate_texts
          auto_query: translate_texts_auto
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

`text_translation` contains `query`, optional `auto_query`,
`accepts_multiple_blocks`, `size_unit`, `max_blocks_per_request`,
`max_units_per_block`, `max_units_per_request`, `max_request_bytes`,
`context_support`, `markup_support` and `preserves_request_order`.
`auto_query` is the query used when the source language is unknown and the
provider cannot express automatic translation through
`translation_source_language`; a known source always uses `query`.
`accepts_multiple_blocks: false` limits each request to one block.
`preserves_request_order: true` lets a provider return rows in request order
without block identifiers; the bridge then matches rows to submitted blocks by
position while still validating cardinality.

`url_translation` contains `query`, `accepted_formats`, `max_document_bytes`,
`max_document_units` and `result_mode`. Only `result_mode: inline_document` is
defined by the base contract. A plain HTML page is not subtitle translation; an
upload endpoint is not a URL capability.

`size_unit` accepts `utf8_bytes`, `unicode_code_points` or an unknown unit kept
verbatim. An unknown unit is measured conservatively as UTF-8 bytes.

### Source detection

`automatic: true` means translation accepts an omitted source language.
`returns_detected_language: true` means the translation response exposes the
effective language. `query` names an optional dedicated detection query; the
text `auto_query` also enables source-less translation when no dedicated query
is declared.

A dedicated detection query receives
`application/arachnea-translation-detection` and the normalized
`translation_blocks_json` parameter containing exactly one representative
sample block, bounded to 2,000 provider units. It returns one row with
`language` or `detected_source_language`; the bridge validates the code against
the declared metadata.

A normalized `source_language: null` requests detection; `auto` is not a
language code and never appears in `languages`. The bridge passes
`translation_source_language = "auto"` to a query that expects it, or omits the
parameter through `auto_query`, which maps the normalized request to the API
representation (omission, `auto` value or another form).

For a multi-batch request with an unknown source, the bridge first uses the
dedicated detection query when declared, then sends every batch with the
detected source. Without a dedicated query, the first batch must use
`auto_query` and return `detected_source_language`; later batches reuse that
language. A missing detection result fails the request instead of restarting
detection per batch. With restricted pairs and an unknown source, prior
detection is required to establish eligibility. If the detected source equals
the target, the original blocks are returned without a translated result.

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

### Response shaping

Scrapyfy merges root-query rows into one normalized row unless
`result_item_field` promotes a declared group to top-level rows. A translation
YAML must therefore build one group item per submitted block and flatten it:

```yaml
row_pointer: "/*"
result_item_field: translated_blocks
entries:
  - name: translated_blocks
    type: object[]
    pointer: /
    select: all
    entries:
      - name: text
        type: string
        pointer: /translations/0/text
      - name: detected_source_language
        type: string
        pointer: /detectedLanguage/language
```

For a provider returning parallel arrays, select each array element, for
example `pointer: /translatedText/*`. The bridge requires exactly one row per
submitted block and never accepts a partial batch.

### Retries, budgets and errors

Every provider attempt reserves one concurrency slot and its measured provider
units before the query is sent. The default policy is three attempts, a 500 ms
initial delay, an 8 s delay ceiling and bounded jitter; a provider
`Retry-After` delay is honored within the ceiling; a hint above that ceiling
returns the failure without an early retry. Each attempt times out after 60 s. Only temporary failures and
generic query execution failures are retried; authentication, request, language,
size, quota and invalid-response failures are returned immediately.

Scrapyfy HTTP statuses are classified as `authentication` (`401`, `403`),
`request` (`400`, `404`, `415`, `422`), `size` (`413`), `quota` (`429`) or
`temporary` (`408`, `425`, `500`, `502`, `503`, `504`). A rejected status is
returned as `ProviderRejected` with an optional `retry_after_seconds`, without
retrying the body or changing provider.

Budget parameters read by Stream from the service YAML are
`translation_max_concurrent_requests`,
`translation_max_units_per_window` and `translation_budget_window_seconds`.
A zero quota means no quota limit; concurrency and window values must be
positive. The in-memory budget is process-local and shared by service
identifier. Reloading limits preserves consumed units and active reservations.
Quota windows are fixed and process-local; restarting resets their counters.
Separate service definitions using the same provider account must split its
quota conservatively. Consumed units are not refunded
after a failed attempt because the provider may already have counted it.
URL translation reserves zero units because the document size is not known
before the provider call.

Execution returns `TranslationMetrics` with `batches`, `attempts`, `retries`,
`detection_attempts`, `blocks`, `units` and `elapsed_ms`. Metrics contain no
source text, translated text, credentials or provider secrets. Attempt logs
report counts, durations and statuses, including failures, without raw errors.

Dedicated detection also receives `translation_detection_text`, the same
representative sample as plain text. LibreTranslate uses this string for `q`;
Azure uses the single block in `translation_blocks_json`. LibreTranslate uses
dedicated detection even for one batch because its current YAML does not expose
the detected language from `/translate`. Empty translated text and inconsistent
detected languages within one batch are rejected as invalid responses.

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
- `max_request_bytes`, measured by the actual Scrapyfy POST body constructor
  after parameter and credential resolution, without a network call.

`plan_batches` remains a standalone normalized estimate for document planning.
`plan_batches_with_measurement` and `ScrapyfyTranslationBridge::plan_texts`
use actual body bytes. Stream's `select_text_translation_bridge` uses these
plans and live budget availability for selection, carries local limits into
execution, and keeps one provider for all batches.

Splitting an oversized cue remains in the `arachnea-stream` subtitle module,
which must not cut Unicode or markup. Translation rejects an oversized block
instead of truncating it.

Default local bounds: 50 blocks, 10,000 units per block, 50,000 units per
request and 65,536 serialized bytes. Effective limits are the strictest between
provider and local configuration.

Planning and budget checks are separate: planning rejects an oversized batch
without a provider call, while budget reservation applies to each actual
attempt, including retries. A provider-only quota remains external to Arachnea;
the configured budget should therefore be set below the full account quota
when other clients share the same provider account.

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

## 8. Browser cache and validators

The generic `TranslationRevision` records requested source and target languages,
service identity, YAML hash, configuration and pipeline revisions, and an opaque
fingerprint of output-affecting options. Stream's `SubtitleTranslationValidator`
adds the source validator, opaque source and access-scope fingerprints, format,
parser and subtitle pipeline revisions. The composite ETag is weak, versioned,
URL-safe base64-encoded JSON and limited to 4096 bytes. Its payload is visible:
never include credentials, signed URLs or other secret values. The catalog exposes
an optional `service_yaml_hash`; without it a reliable validator cannot be built.

The source validator contains either the upstream HTTP ETag or a hash of the
source bytes with an optional `Last-Modified` value. The retrieval port accepts
this validator and distinguishes an unchanged source from a complete document;
the caller must compare bytes when the source supplies no usable validator.
Client-supplied tags do not authorize access or establish freshness. They must be
checked against the authorized request and revalidated with the source. The
browser retains translated bytes; these contracts do not provide a server-side
translated-document store. If the browser loses its body, a new translation is
required. Public conditional HTTP transport and browser cache integration remain planned.

Stream revalidates an authorized descriptor before provider execution and can
translate a complete WebVTT document using the generic text or URL bridge. The
application facade supplies one catalog/configuration snapshot and fingerprints
source headers, options and local limits. Direct HTTP and application proxy
sources validate and pin public DNS addresses at each hop. Internal proxy URLs
are decoded without requesting a local controller endpoint; routing parameters,
cookies, actions, classifiers and allowlisted TLS options are preserved. The
logical hostname remains available for routing and TLS while CONNECT and direct
connections use the validated IP. Headers/cookies are cleared across origins.
Redirected and transformed sources use a content hash. Proxy responses must honor
identity content encoding; source bytes and post-action output are bounded.

Provider URL translation requires an explicitly approved public direct URL with
no query, fragment, credentials, required headers or redirect, and no provider
unit budget or unit limit lacking a declared unit. Otherwise the text route is
used when available. The source is checked again afterward and the result must
preserve metadata, cue structure, timing and markup. No provider fallback occurs
after execution starts. No source or translated file is retained; simultaneous
uncached requests can execute independently within provider concurrency limits.
The local simulated provider/proxy qualification is complete; real-provider
qualification still requires access. The public file transport is implemented
by lot 6 and documented in section 9.

## 9. Backend contract

| Command | Purpose |
|---|---|
| `get_translation_services` | Public catalog of usable services, capabilities and languages; no secret. |
| `prepare_subtitle_translation` | Checks rights and candidates, then produces a descriptor and token without retrieving or translating the document. |
| `GET <public-path>/subtitle_translation/<token>.vtt` | Returns a complete WebVTT file and composite ETag, or `304` after successful source and context revalidation. |
| `HEAD` on the same path | Returns `405` without retrieval or translation. |

The token is opaque, derived deterministically from the authorized access
scope, source descriptor, languages and selection policy, and contains no source
URL or credential. Repeated preparation of the same request keeps the same URL
while its descriptor is retained; expiry or descriptor eviction requires a new
preparation. Access is checked again on download, and one user's token cannot
access another user's private subtitles. The access scope is the trusted client
IP for REST (loopback shares the local scope) and the local scope for Tauri IPC.
Clients sharing one IP, such as behind a reverse proxy or NAT, share the same
scope; per-user isolation then requires an authenticated scope rather than this
initial transport. Errors are never served as a WebVTT file.

The preparation commands never retrieve the source, detect a language or call a
provider. Preparation accepts a direct HTTP(S) URL or a recognized application
proxy link and only rejects structurally invalid locations. It verifies that at
least one enabled text-capable service supports the requested pair (or source
detection when the source language is omitted), but it does not select the final
provider in automatic mode. Only WebVTT is accepted initially.

`If-None-Match` accepts lists and weak validators, evaluating each candidate as
a whole; components from different tags are never mixed. A wildcard `*` carries
no source validator, so it never proves source stability and the file is
translated again. Successful `200` and `304` responses carry the composite ETag
and `Cache-Control: private, no-cache`; the ETag is exposed through CORS for
cross-origin deployments. `HEAD` returns `405` with `Allow: GET` before any
descriptor read, retrieval or translation. Expired descriptors are normally
purged, so unknown and purged tokens answer `404`; a descriptor still readable
after its logical expiry answers `410`.

Both transports return one complete buffered file, never a partial WebVTT
document. Tauri collects that buffer before replying to the IPC call, so a slow
source retrieval or provider execution keeps the call pending until completion
or timeout and exposes no progress stream. REST returns the same buffered body;
the configured public path prefix is preserved across scraper reloads.

### HTTP statuses

| Situation | Status |
|---|---|
| Invalid descriptor, format or document | `400 Bad Request` |
| Access denied | `403 Forbidden` |
| Expired but still resolvable token | `410 Gone` |
| Unknown or purged token | `404 Not Found` |
| Document, cue or unit bounds exceeded | `413 Payload Too Large` |
| `HEAD` | `405 Method Not Allowed` |
| Source unavailable or retrieval failed | `502 Bad Gateway` |
| Unsupported pair, capability or size | `422 Unprocessable Entity` |
| Quota or budget unavailable | `429 Too Many Requests` |
| Metadata unavailable or temporary incident | `503 Service Unavailable` |
| Internal state failure | `500 Internal Server Error` |

`Cache-Control: private, no-cache` is proposed to keep the body while
revalidating with `If-None-Match`. A `304` response has no body and keeps the
ETag.

## 10. Out of scope

SRT remains behind an explicit extension. Integrated HLS/DASH subtitles, live
streams, TTML/ASS, audio transcription, iframes without an external track, URL
modes with deferred tasks, document upload, additional providers and
cross-cue linguistic grouping require a separate analysis.
