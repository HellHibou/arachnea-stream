# arachnea-translation

`arachnea-translation` contains the generic translation contracts used by the
Arachnea backend. It deliberately knows nothing about WebVTT, cues, players or
video tracks: subtitle parsing and document orchestration live in the
`arachnea-stream` subtitle module, which depends on this crate.

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
  and Stream selection. Standalone document planning remains an estimate.
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

`bridge::ScrapyfyQueryPort` is the only outbound Scrapyfy dependency.
`bridge::ScrapyfyCollectionPort` adapts an already loaded
`ScraperQueryCollection`; Stream supplies authorization, catalogs and
persistence. This crate never depends on `arachnea-stream`.
The `arachnea-stream` subtitle module depends on this crate, never the
opposite.

The normalized metadata query is `get_translation_metadata`. Its schema version,
languages, pair rules, detection and limits are documented in
`docs/specifications/arachnea-translation-en.md` and
`docs/specifications/arachnea-translation-fr.md`.

## Validation

From `server/`:

```sh
cargo check -p arachnea-translation -p arachnea-scrapyfy -p arachnea-stream
```

No translation endpoint is public after lots 1 to 4. Stream loads the provider
catalog from the `arachnea-translation` Scrapyfy group and keeps Azure
Translator and LibreTranslate disabled until configured. Generic text
execution is implemented in the shared bridge, including automatic source
handling, retries, budgets and metrics; the Azure Translator and LibreTranslate
YAML definitions are exercised by local contract checks. Real provider
qualification still requires credentials, and retrieval, browser revalidation
and transport remain planned for later lots. Stream also owns the WebVTT
document pipeline in its `subtitles` module.

Each query attempt has a 60-second timeout. A `Retry-After` hint above the
retry-delay ceiling returns the failure immediately instead of retrying early.
HTTP failures are classified in the shared bridge, including collection-backed
use outside Stream. Attempt logs expose only service IDs, counters, durations and
statuses; raw query failures and provider error bodies are omitted.

The budget port supports an availability check before execution and atomic
per-attempt reservations. In-memory limits can be updated while preserving used
quota and active requests; quota windows are fixed, process-local and reset on
restart. They cannot track external consumers of the provider account.
