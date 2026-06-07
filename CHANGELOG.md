# Changelog

All notable changes to the server workspace are recorded here. Add new entries at the end of the file so the history remains append-only.

## Unreleased

### Fixed
- **coflix.yaml**: Updated `get_entry` query to correctly extract season labels and links from the HTML entry page. Now uses radio input selectors (`input[name="seasons"] + span`) to extract the full season label text (e.g., "La Sorcière invincible tueuse de Slime depuis 300 ans - Season 1") and regex on response body to build season API URLs in the format `{base_url}/wp-json/apiflix/v1/series/{post_id}/{season_num}`.
- **coflix.yaml**: Fixed `get_season` query to correctly iterate over all episodes in the `episodes` array. Changed `row_pointer` from `/episodes/*` to `/` with nested `entries` pointer at `/episodes/*` to properly wrap results in the standard section shape with `source` and `entries` fields.
