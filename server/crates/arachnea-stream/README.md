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
