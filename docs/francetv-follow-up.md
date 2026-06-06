# FranceTV Service Follow-up

Date: 2026-05-12

## Current State

- Source file analyzed: `plugin.video.catchuptvandmore-dev/resources/lib/channels/fr/francetv.py`.
- Arachnea config: `server/services/francetv.yaml`.
- Service registration: `server/services/services.json` includes `francetv.yaml`.
- Playback resolver: `server/src/service/francetv_resolver.rs`, registered through `stream_scraper.rs`.

## Implemented

- Catalog/search YAML:
  - `search`
  - `load_home`
  - `get_category`
  - `get_entry`
- Live YAML:
  - `list_lives` uses `/generic/directs?platform=apps`.
  - `get_live` returns a `francetv-live` resolver descriptor using the selected channel or partner `si_id`.
- Replay playback:
  - `francetv-video` calls the FranceTV K7 endpoint and signs the returned manifest URL.
- Live playback:
  - `francetv-live` uses the same K7/token flow with live channel `si_id` values.
- DRM support:
  - Non-DRM DASH/HLS manifests are returned directly.
  - DRM entries request a Widevine authorization token and expose a same-origin `francetv-license-proxy` license URL.

## Notes

- FranceTV K7 is queried with `country_code`, `capabilities=drm`, `os=androidtv`, `diffusion_mode=tunnel_first`, and `offline=false`.
- `country_code` is a YAML parameter and defaults to `FR`.
- The live listing currently returns all live and partner entries exposed by FranceTV, including France 2, France 3, France 4, France 5, Arte, LCP, France info, TV5 Monde, France 24, and FAST channels when present.

## Still To Revisit

- Unix timestamp normalization is not currently mapped for FranceTV live/program metadata.
- Season pagination by collection cursor is not modeled; the current YAML returns embedded episode collections.
- A dedicated `server/data-test/francetv.yaml` fixture could make the scraper helper tests stricter, but no new tests were added because this task did not request test infrastructure.

## Validation

- `cargo fmt`
- `cargo test --no-run`
- `ARACHNEA_TEST_QUERY_SOURCE=francetv cargo test test_query_load_home`
- Local REST checks on port `8091`:
  - `POST /api/list_lives` returned HTTP 200 and 15 live entries.
  - `POST /api/get_live` for France 2 returned HTTP 200 with a `francetv-live` player.
  - `POST /api/resolve_player_stream` for France 2 live returned HTTP 200 with a signed MPD URL.
  - `POST /api/resolve_player_stream` for replay id `0dd7aae9-5228-4954-a2f1-a3f6b531f364` returned HTTP 200 with a signed MPD URL.
