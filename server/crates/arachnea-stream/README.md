# Arachnea Stream

## Direct fallback when a country proxy is unavailable

Stream scraper and resolver requests enable `fallback_direct` on their proxy core.
The `/proxy` web handler is also instantiated with `fallback_direct: true`.
If no usable proxy exists in the requested countries, routing falls back to a
connection without any remote proxy. Other transport and origin failures retain
their existing error behavior. A geo-restricted origin may still deny direct access.

The web proxy's `opts` JSON can override the default with `{"fallback_direct": false}`
or `true`. Alternatively, use `Arachnea-Proxy-Fallback-Direct: false` or `true`;
the URL option takes precedence over that header. Proxy privacy restrictions remain
applicable. An unavailable country proxy is returned as a source-attributed error
alongside successfully extracted data in `get_entry` and other scraper aggregation
responses. The existing frontend error adapter receives both through the usual
`data` / `errors` envelope. Responses containing errors are not converted to HTTP 304.

## Localized proxy persistence

The physical `proxy-inventory` store owns the `proxy-inventory`,
`proxy-preferences` and `proxy-failures` tables in the application data
directory's `data/proxy-inventory.sqlite3`, sharing one SQLite connection.
Each table has its own typed view and schema. Preferences retain up to three
accepted endpoints per exact destination without age expiration. Failure records
retain bounded rediscovery protection (1,024 endpoints, 48 hours from eviction,
cleaned on demand). These stores are shared across aggregator rebuilds; no worker
is started. Compatibility application constructors use memory-only stores.
The other physical stores use `data/cloudflare-session.sqlite3` and
`data/arachnea-services.sqlite3`. WAL/SHM files, when present, are adjacent
`<STORENAME>.sqlite3-wal` / `<STORENAME>.sqlite3-shm` files. Old
`<STORENAME>/records.sqlite3*` databases are ignored, not imported or deleted.
The first run with this layout starts with empty proxy records, accepted
histories, eviction protection and Cloudflare sessions. Existing service
activation overrides and stored credentials in the old services database must
be configured again. New-format data survives subsequent restarts; no legacy
reset directory or schema marker is created. Sharing one database does not
make endpoint eviction and preference cleanup a single transaction.

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

The `load_home` REST/Tauri command accepts optional exact `sources` identifiers.
Omission selects all loaded sources; `[]` returns an empty success without
provider calls or ETag. Null, invalid types and blank identifiers are rejected.
Unknown/unavailable/unsupported sources return per-source errors alongside any
valid results, never a fallback to all sources. Filtered error responses omit
the ETag; unfiltered requests preserve their existing cache behavior.
`get_service` advertises sorted `supported_queries` from the loaded registry,
joined by metadata id; filters never reactivate disabled sources. See the stream
FR/EN specifications for transport examples.

New YAML rail identity mappings use the `key` field with `rail:<provider-id>`
values (Arte, TF1, France TV, RMC BFM Play), independently from display titles.
LN24's single category rail uses `rail:category-content`. Existing keys are not
renamed. Local HTML rails without persistent container ids retain documented
descriptor/fragile-label fallbacks; random slider UIDs and array positions are
not stable identities. See the Section contract in the stream specifications.

- `cd server && cargo run -p arachnea-stream --bin arachnea` - Run the backend executable. Add `--server-public` to bind the server to `0.0.0.0` so the web interface and API are reachable through any local network hostname or IP (for example `http://localhost:8080/`).
- `cd server && cargo check -p arachnea-stream` - Type-check this crate.
- `cd server && cargo test -p arachnea-stream` - Run this crate's tests.
- `cd server/crates/arachnea-stream && cargo tauri build` - Build the Tauri desktop application and release bundles.
- `cd server/crates/arachnea-stream && cargo tauri build --target <target-triple>` - Build a platform-specific Tauri bundle once the target and native platform toolchain are installed. See the root and server README files for cross-platform setup notes.

## M6 Play API Key Preparation

Replay and live session setup share a five-second total budget for the optional
login-page and JavaScript-bundle lookup of the public Gigya API key. If lookup
fails or exceeds that budget, setup uses the existing bundled fallback key.
This deadline ends the awaited lookup; it does not add a detached refresh task.

The process keeps one API-key cache entry, shared across logins under the existing
login lock: discovered keys live for 15 minutes, and fallback keys for one minute
before another web lookup is allowed. Cache locks are never held during network
requests. A parsed Gigya JSONP response with a nonzero numeric `errorCode`
invalidates the submitted key's cache entry for the next login attempt, without
replaying the POST. Front-auth or transport failures do not invalidate this key.
The cache is memory-only and is separate from credentials and the session cache.
Diagnostics report lookup/cache use, duration and fallback without key values.

This bounds API-key preparation, not the complete `get_stream` operation. The
existing proxy routing, per-request search budgets, session handling and safe
replay rules continue to govern subsequent authentication and video requests.

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
`WatchButtonAction` items with a `REPLAY` video supply resolver targets; order
and navigation actions are excluded from player discovery. This covers films
without editorial lists without relying on search ranking or accidentally
selecting another programme.
Watch-action video links use `program.initialChannel.slug`, and `price: premium`
is emitted only when the video's `decoration.showPayBadge` is true.

TVOD `OrderAction` items supply the rental amount from `buttonDecoration.ctaLabel`
as a canonical monetary `price` (for example, `3.99 EUR`), not a subscription
marker or a playable resolver target. The euro amount is extracted dynamically;
missing or unrecognized monetary labels do not produce a price.

Search video cards extract monetary amounts only from `TVOD` offer prices.
They emit `premium` only when `video.rights` is exactly `["MAX"]`; optional
SVOD and ad-free offers also occur on free replays and trailers and are not
evidence of paid-only access. Search programme cards with `PURCHASE_REQUIRED`
fetch their exact programme call-to-action to obtain a TVOD amount, adding one
request per such card. No rental amount or subscription marker is inferred
from `isAVOD: false` or a missing free offer.

Detail `count-season` requires an explicit season label such as `5 saisons`.
A leading four-digit year in the programme sublabel is exposed as `year`, not
as a season count (for example, `2013 · 2h07`).

Recommendations remain limited to the contextual "Si vous aimez…" rail. An
"Aucune recommandation disponible" slider does not expose recommendations,
and "Populaires cette semaine" is not used as a substitute.

The persisted `ProgramCover_CallToAction` query ID in the source YAML must track
TF1+ client changes. Player discovery does not bypass authentication, geographic
availability, subscription requirements, or DRM enforced during resolution.

Detail and season players also load their allowed territories from the public
`mediainfocombo` response's `media.geoList`, even when playback is denied. The
ordered list is exposed as `resolver.proxy.countries` and forwarded through the
existing frontend geo-routing contract. This adds one metadata request per
extracted video player; no local territory list is substituted.

TF1 replay and live negotiation install a mediainfo response classifier that
inspects the JSON body before accepting a localized proxy. TF1 reports a
geo-restriction with HTTP `200` and `delivery.code >= 400`, so the classifier
rejects that proxy for the destination and the localized search retries another
eligible candidate. HTTP `403` remains a rejection as well. The same classifier
keeps the previous status behavior for non-mediainfo requests. Body inspection
is requested only when the original or effective URL identifies mediainfo,
preserving protection across redirects. CDN responses are classified from their
status without waiting for a prefix; the Rquest collector preserves the skipped
inspection's bounded reception window before its ordinary body timeout. This
advances classification without shortening the time allowed for reception;
the full manifest must still be downloaded before the resolver returns. The
callback remains opted out of additional redirect-transport retention.

TF1 replay and live negotiation emit INFO diagnostics with the requested and
configured proxy countries, mediainfo HTTP status, delivery code, delivery
country, remote territory list, and the existing country-change retry decision.
Scrapyfy logs at DEBUG whether a proxy transport is configured when it forwards
non-empty country hints. These diagnostics do not include credentials, bearer
tokens, headers, or response bodies.

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


## M6 default proxy domain group

M6 resolver HTTP clients supply the optional generic `proxy_domain_group`
containing `m6.fr`, `6play.fr`, `6cloud.fr` and `bedrock.tech`. Their subdomains may
try a proxy accepted by another member before ordinary hierarchical discovery,
when no exact eligible preference exists. This is a default candidate, subject to
country/capability checks, target exclusions and response classification. It does
not force a common IP or change the routing of the final returned stream URL.
The domain configuration belongs to the M6 resolver; the shared proxy crate has
no built-in source domains.


## Performance diagnostics build

The `performance-diagnostics` feature is disabled by default and forwards to
`arachnea-proxy`, `arachnea-http` and `arachnea-scrapyfy`. Normal builds retain
operational errors and all proxy validation, refresh, timeout and cancellation
behavior, without performance-only observers/counters or media body wrapping.

From the repository root on macOS, start normally with:

```sh
cd /Users/jdecker/Downloads/arachnea/server
RUST_LOG=info cargo run -p arachnea-stream --bin arachnea
```

To compile and enable detailed performance traces:

```sh
cd /Users/jdecker/Downloads/arachnea/server
RUST_LOG='info,arachnea_proxy=debug,arachnea_http=debug,arachnea_scrapyfy::scrapyfy::scraper_agregator=debug' cargo run -p arachnea-stream --bin arachnea --features performance-diagnostics
```

Routine proxy selections, probes, successful tunnels and recoverable attempts are
DEBUG. Terminal localized HTTP failures and degraded direct fallbacks remain WARN;
critical persistence/runtime failures remain visible. Cargo `--all-features`
includes the optional diagnostics, independently of the logging filter.
