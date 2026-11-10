# arachnea-subtitles

`arachnea-subtitles` owns subtitle-specific document contracts on top of the
generic `arachnea-translation` layer. It has no dependency on
`arachnea-stream`, a provider API or a player.

## Responsibilities

- Source subtitle descriptors, WebVTT/SRT format identifiers and document
  statistics.
- Persistent subtitle translation descriptors, opaque tokens, expiration and
  access-scope authorization.
- Authorized retrieval and document cache ports injected by Stream.
- Complete subtitle document cache identity: source fingerprint, format,
  parser revision, subtitle pipeline revision and the generic translation
  identity.
- Explicit HTTP status mapping for file responses, including the documented
  `HEAD` behavior.

The WebVTT parser and serializer are implemented at lot 3. Lot 1 defines the
boundaries and contracts so cue and timestamp knowledge cannot leak into the
generic translation engine. Raw SRT is not exposed to the player as WebVTT; SRT
normalization is a separate explicit extension.

## Boundaries

`SubtitleRetrievalPort` receives already authorized retrieval behavior from
Stream: proxy-link resolution, destination and redirect checks, and access
context stay there. `SubtitleTranslationCachePort` receives atomic persistence
from Stream. The subtitle layer never performs unrestricted network access and
never depends on `arachnea-scrapyfy` or `arachnea-stream`.

## Validation

From `server/`:

```sh
cargo check -p arachnea-translation -p arachnea-subtitles -p arachnea-scrapyfy
```

No translation endpoint is public at lot 1.
