# arachnea-translation

`arachnea-translation` contains the generic translation contracts used by the
Arachnea backend. The `document-vtt` feature provides WebVTT parsing, segmentation and reconstruction independently of players and retrieval. Translation owns document orchestration and optional authorized retrieval adapters; Stream supplies their runtime configuration. The `api` feature registers controller routes through an injected application runtime.

## Responsibilities

- Normalized two-letter language codes and directional language pairs.
- Abstract, opaque text block identifiers and normalized text/URL requests.
- Validated service metadata: languages, optional source/target restrictions,
  optional directional pairs, source detection, text/URL capabilities and
  explicit size units and limits.
- Deterministic service filtering, normalized exclusion reasons and
  ranking, without any provider call.
- Batch planning against block-count, per-block, cumulative and serialized
  request limits, using the actual Scrapyfy POST body constructor for execution
  and runtime selection. Standalone document planning remains an estimate.
- Optional dedicated source detection or provider-side automatic translation,
  with source stabilization across multi-batch requests.
- Bounded retries with exponential backoff, `Retry-After` support and
  normalized provider failure categories.
- Account-level budget and concurrency reservations before each provider
  attempt, plus non-sensitive execution metrics.
- Generic translation revision identity excluding credentials and including the
  service YAML hash, effective configuration and pipeline revisions.
- Optional positional response matching for providers that preserve request
  order without returning block identifiers.
- A single generic Scrapyfy bridge. Provider endpoints, headers, bodies and
  response mappings stay in YAML; no provider is implemented in Rust here.

## Boundaries

`bridge::ScrapyfyQueryPort` abstracts outbound Scrapyfy queries.
`bridge::ScrapyfyCollectionPort` adapts an already loaded
`ScraperQueryCollection`; Stream supplies authorization, configuration and persistence. This crate never depends on `arachnea-stream`.
Stream imports document and runtime contracts directly from this crate.

The normalized metadata query is `get_translation_metadata`. Its schema version,
languages, pair rules, detection and limits are documented in
`docs/specifications/arachnea-translation-en.md` and
`docs/specifications/arachnea-translation-fr.md`.

## Validation

From `server/`:

```sh
cargo check -p arachnea-translation -p arachnea-scrapyfy -p arachnea-stream
```

Stream registers `translation/get_services` and `translation/translate_url` and
loads the provider catalog from the `arachnea-translation` Scrapyfy group. Azure
Translator and LibreTranslate are disabled until configured. Generic text
execution is implemented in the shared bridge, including automatic source
handling, retries, budgets and metrics; the Azure Translator and LibreTranslate
YAML definitions are exercised by local contract checks. Real provider
qualification still requires access to actual providers. Translation supplies optional authorized
direct/proxy source retrieval; it also owns conditional revalidation,
transport handlers and text/URL orchestration. The document registry rejects
unsupported formats, including when `document-vtt` is disabled.

Each query attempt has a 60-second timeout. A `Retry-After` hint above the
retry-delay ceiling returns the failure immediately instead of retrying early.
HTTP failures are classified in the shared bridge, including collection-backed
use outside Stream. Attempt logs expose only service IDs, counters, durations and
statuses; raw query failures and provider error bodies are omitted.

The budget port supports an availability check before execution and atomic
per-attempt reservations. In-memory limits can be updated while preserving used
quota and active requests; quota windows are fixed, process-local and reset on
restart. They cannot track external consumers of the provider account.

Document consumers resolve `document::translator(content_type)` and call
`prepare` for each candidate capability before selecting a provider. The returned
`PreparedDocument` exposes the exact blocks to measure and retains segmentation
through `execute`. The shared runtime uses this path with its selection and budgets.
`document::translate` is a convenience for callers that already selected a service;
it does not perform multi-provider selection. Results include provider metrics and
an `original` flag when source bytes can be reused. URL result structure validation
is available in `document::vtt::same_structure`.

`aggregator_port::AggregatorTranslationPort` adapts a caller-supplied Scrapyfy
aggregator and service group for request measurement and query execution.

The `runtime` feature exposes the effective catalog and provider selection with
shared process-local budgets. Callers supply the aggregator, group, manifest and
credential store. Conditional retrieval contracts are exposed in
`document::retrieval`. `retrieval-http` exposes `document::http::SubtitleHttpRetrieval`;
combined with `document-vtt`, it assembles finite HLS WebVTT playlists.
`retrieval-proxy` adds the optional shared Proxy/Core integration and includes
`retrieval-http`. Prefixes, headers and the proxy runtime are supplied by the caller.
The HTTP adapter can be used without `runtime` or `api`.

The catalog retains validated metadata with its revision. Runtime bridges use
that snapshot without querying metadata again during selection or execution.
The selected bridge and document plan remain paired until execution.
`DocumentTranslator` validates source bytes and provider URL results; shared
orchestration and API handlers compile without any document format enabled.

The shared API handler borrows one catalog, aggregator and retrieval adapter for
the whole operation. Composite validators include document MIME type, URL,
languages, policy, access scope and provider configuration; legacy validators
are invalidated by the document pipeline revision.

LibreTranslate loads its directional language pairs from `/languages` when the catalog is loaded or reloaded. Only published lowercase two-letter codes are accepted; variants are excluded rather than truncated. HTTP failures, malformed responses and catalogs without usable pairs make the provider unavailable. Reload the service configuration after changing installed models. The effective metadata contributes to the configuration revision.

The public service catalog exposes URL translation media types and whether source detection identifies the language, so clients can check document and directional-pair eligibility.

`api::register_routes` accepts a snapshot factory and an optional subpath; without one the commands are
`get_services` and `translate_url`. Stream passes `translation`. The file request
uses `url`, `target` and `content-type`, plus optional `source`, `track_id` and
`policy`; no aliases for the former routes or parameter names are provided.
After a service reload, reload the frontend to replace its successfully cached
catalog. Real-provider, native Tauri WebView and end-to-end player reload
qualification remain open; see the refactor analysis for completed checks.

## Registering the complete API

`api::TranslationSnapshot` describes a retained application's dependencies:
catalog, Scrapyfy aggregator, service group, authorized retrieval factory and
optional trusted proxy prefix. The application implements these accessors; it
does not implement route handlers. Translation owns catalog serialization,
handler construction and document execution.

Call `api::register_routes(controller, snapshot_factory, Some("translation"))`
once. The factory returns `Arc<S>` where `S: TranslationSnapshot`. It is invoked
once for each request, and the returned snapshot stays alive until execution
completes. A reload therefore affects subsequent requests. Catalog requests
only read the snapshot catalog and never construct a source retriever.

This replaces the former `TranslationApi` callback interface. Applications must
supply a snapshot factory instead of `get_services` / `translate_url` delegates.
No dependency on `arachnea-stream` is introduced.
