# Arachnea Stream

`arachnea-stream` is the executable Rust backend crate. It wires the stream service facade, player resolvers, configured scraper sources, credentials storage, and the selected controller backend.

## Responsibilities

- Own the `arachnea` backend executable entry point in `src/main.rs`.
- Own the stream facade in `src/stream_scraper.rs`.
- Own player resolver services in `src/services/`.
- Depend on `arachnea-core` for controller and persistence primitives.
- Supply the Tauri context, embedded frontend assets, custom URI scheme, and API prefix to `arachnea-core`.
- Depend on `arachnea-scrapyfy` for generic scraper query execution.

## Application Assets

Tauri application assets live in this crate:

- `tauri.conf.json`
- `capabilities/`
- `icons/`

The crate build script runs Tauri build helpers from this crate root so these files remain the source of truth. `src/main.rs` passes the generated Tauri context and embedded assets into the generic Tauri controller implemented by `arachnea-core`.

## Commands

- `cd server && cargo run -p arachnea-stream --bin arachnea` - Run the backend executable. Add `--server-public` to bind the server to `0.0.0.0` so the web interface and API are reachable through any local network hostname or IP (for example `http://localhost:8080/`).
- `cd server && cargo check -p arachnea-stream` - Type-check this crate.
- `cd server && cargo test -p arachnea-stream` - Run this crate's tests.
- `cd server/crates/arachnea-stream && cargo tauri build` - Build the Tauri desktop application and release bundles.
- `cd server/crates/arachnea-stream && cargo tauri build --target <target-triple>` - Build a platform-specific Tauri bundle once the target and native platform toolchain are installed. See the root and server README files for cross-platform setup notes.

## M6 Play Premium Metadata

The M6 Play YAML source emits `price: premium` only when `freemium_products`
include a product of type `content`. `contains_freemium` is not an access
restriction indicator: it can be true even for programmes with free episodes.
Programme details also inspect the first 100 published full videos with one
additional middleware request, since programme-level products can be empty
even for paid replays. A premium programme flag means that an explicit paid
content product exists on the programme or one of those videos, not that every
episode requires a subscription. Season episodes are evaluated individually.
Optional playback benefits and absent content-access products do not mark
content as premium. Catalogue cards use only their own explicit products;
paid videos beyond the detail lookup's first 100 items are not inferred.

Algolia search and layout recommendation cards currently omit this field
because their programme payloads do not provide the same explicit access
indicators. No additional per-card requests are made to infer access.

## TF1+ Programme Players

TF1+ detail extraction keeps the editorial-list lookup and also requests the
public programme cover watch action by exact programme slug. Only
`WatchButtonAction` items with a `REPLAY` video supply resolver targets; offer
and navigation actions are excluded. This covers films without editorial lists
without relying on search ranking or accidentally selecting another programme.
Watch-action video links use `program.initialChannel.slug`, and `price: premium`
is emitted only when the video's `decoration.showPayBadge` is true.

The persisted `ProgramCover_CallToAction` query ID in the source YAML must track
TF1+ client changes. Player discovery does not bypass authentication, geographic
availability, subscription requirements, or DRM enforced during resolution.

Detail and season players also load their allowed territories from the public
`mediainfocombo` response's `media.geoList`, even when playback is denied. The
ordered list is exposed as `resolver.proxy.countries` and forwarded through the
existing frontend geo-routing contract. This adds one metadata request per
extracted video player; no local territory list is substituted.

Gigya bootstrap transport failures produce a warning with the fixed endpoint,
elapsed milliseconds, and the underlying error chain. The public error remains
`Failed to bootstrap the TF1 Gigya web session.` No credentials, cookies,
tokens, or response bodies are added to this diagnostic. Routing, timeouts,
and retry behavior are unchanged.

## Configuration Reload

The administration reload action rebuilds the runtime for Stream, proxy sources
(`arachnea-proxies`), and IP-country sources (`arachnea-ip-countries`). Sources
enabled after startup are included even when every proxy source was initially
disabled. The public `/proxy` route resolves the current runtime for each request;
in-flight requests retain their original runtime safely.

Proxy and IP-country providers own independent source snapshots rather than
borrowing the application's aggregator. A successful rebuild replaces the
in-memory inventory and its provider-refresh cooldown, while preserving the
typed persistent proxy cache. Proxy lists remain loaded on demand when a request
requires country-based routing; reloading does not eagerly collect all lists.

## Resolved Stream Chapters

Player resolvers can expose an optional ordered `chapters` list on
`ResolvedPlayerStream`. Each chapter uses playback seconds and serializes with
the following shape:

```json
{
  "start": 246,
  "end": 253,
  "type": "intro"
}
```

- `start` is the inclusive chapter start time in seconds.
- `end` is the exclusive chapter end time in seconds and must be greater than
  `start`.
- `type` identifies the chapter behavior. It is serialized from the Rust
  `chapter_type` field.
- `title` is optional. When omitted, the public frontend resolves a localized
  label from `type` when it knows that type.

Resolvers should omit invalid or incomplete ranges instead of returning
negative, non-finite, empty, or reversed intervals. When the source provides a
finite media duration, generated chapter ends should not exceed it. Resolvers
should return chapters ordered by `start`; overlapping ranges are permitted
only when they represent distinct source concepts that cannot be normalized
without losing information.

### Frontend chapter types

The chapter contract accepts source-defined type strings, but the public
frontend currently gives the following types dedicated behavior:

| Type | Meaning | Dedicated skip button | Auto-skip preference |
| --- | --- | --- | --- |
| `chapter` | Generic named or unnamed content chapter | No | No |
| `previously` | Recap of previous events | Yes | Yes |
| `intro` | Opening or introduction | Yes | Yes |
| `coming_next` | Preview of the next episode or upcoming content | Yes | Yes |
| `outro` | Closing credits or end sequence | Yes | Yes |
| `ads` | Advertising period | Yes | Yes |

The non-advertising auto-skip preferences (`previously`, `intro`,
`coming_next`, and `outro`) are applied only while episode autoplay is enabled.
The `ads` preference is independent of episode autoplay. All auto-skip
preferences default to disabled.

### FranceTV marker mapping

The FranceTV resolver builds chapters from the K7 response's `video` object:

| K7 field | Chapter type | Start | End |
| --- | --- | --- | --- |
| `previously` | `previously` | `timecode` | `timecode + duration` |
| `skip_intro` | `intro` | `timecode` | `timecode + duration` |
| `coming_next` | `coming_next` | `timecode` | `timecode + duration` |
| `closing_credits` | `outro` | `timecode` | `timecode + duration`, or `video.duration` when the marker has no duration |

All FranceTV values are expressed in seconds. Calculated ends are bounded by
`video.duration` when that duration is finite and positive. Missing, `null`, or
invalid markers are ignored without preventing stream resolution.

FranceTV can report `coming_next` and `closing_credits` with the same start
time. In that case, the resolver keeps the next-content preview first and moves
the `outro` start to `coming_next.end`. If the adjusted start reaches or exceeds
the bounded outro end, the empty outro chapter is discarded. For example, a
2889-second video with `coming_next = { timecode: 2859, duration: 12 }` and
`closing_credits = { timecode: 2859 }` produces:

```json
[
  {
    "start": 2859,
    "end": 2871,
    "type": "coming_next"
  },
  {
    "start": 2871,
    "end": 2889,
    "type": "outro"
  }
]
```
