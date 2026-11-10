# arachnea-translation

`arachnea-translation` contains the generic translation contracts used by the
Arachnea backend. It deliberately knows nothing about WebVTT, cues, players or
video tracks: subtitle parsing and document orchestration live in
`arachnea-subtitles`, which depends on this crate.

## Responsibilities

- Normalized two-letter language codes and directional language pairs.
- Abstract, opaque text block identifiers and normalized text/URL requests.
- Validated service metadata: languages, optional source/target restrictions,
  optional directional pairs, source detection, text/URL capabilities and
  explicit size units and limits.
- Deterministic service filtering, normalized exclusion reasons and
  ranking, without any provider call.
- Batch planning against block-count, per-block, cumulative and serialized
  request limits.
- Generic result cache identity excluding credentials and including service
  configuration and pipeline revisions.
- Optional positional response matching for providers that preserve request
  order without returning block identifiers.
- A single generic Scrapyfy bridge. Provider endpoints, headers, bodies and
  response mappings stay in YAML; no provider is implemented in Rust here.

## Boundaries

`bridge::ScrapyfyQueryPort` is the only outbound Scrapyfy dependency.
`bridge::ScrapyfyCollectionPort` adapts an already loaded
`ScraperQueryCollection`; Stream supplies authorization, catalogs and
persistence. This crate never depends on `arachnea-stream`.
`arachnea-subtitles` depends on this crate, never the opposite.

The normalized metadata query is `get_translation_metadata`. Its schema version,
languages, pair rules, detection and limits are documented in
`docs/specifications/arachnea-translation-en.md` and
`docs/specifications/arachnea-translation-fr.md`.

## Validation

From `server/`:

```sh
cargo check -p arachnea-translation -p arachnea-subtitles -p arachnea-scrapyfy
```

No translation endpoint is public after lots 1 and 2. Stream loads the provider
catalog from the `arachnea-translation` Scrapyfy group and keeps Azure
Translator and LibreTranslate disabled until configured. Translation execution
and subtitle orchestration remain planned for later lots.
