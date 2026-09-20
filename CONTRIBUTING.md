# Contributing to Arachnea Stream

Thank you for considering a contribution! This document explains how to set
up your environment, what to work on, and the rules that keep the project
consistent.

## Licensing of contributions

Arachnea Stream is dual-licensed under the [MIT](LICENSE-MIT) and
[Apache-2.0](LICENSE-APACHE) licenses, at the user's option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the project — through pull requests, patches, or
discussions on the issue tracker — is licensed, at the project's option,
under both licenses, without any additional terms or conditions. Do not
submit code you are not allowed to license this way, and do not paste code
from incompatible sources.

## Getting started

1. Read the root [`README.md`](README.md) for a functional overview.
2. Follow [`docs/BUILDING.md`](docs/BUILDING.md) to install prerequisites and
   build the project (Rust + Tauri backend in `server/`, Vue.js frontend in
   `front/`).
3. Run the app once with `cargo run -p arachnea-stream --bin arachnea` (from
   `server/`) to confirm your setup works.

## How the repository is organized

- `front/public-app/` - the consultation interface (Vue.js): home, search,
  entry details, video player.
- `front/admin-app/` - the administration interface (Vue.js).
- `server/crates/` - the Rust workspace crates (`arachnea-stream` is the
  executable application; `arachnea-scrapyfy` is the YAML-driven scraper
  engine; the other crates provide shared primitives, HTTP, proxy, and DNS).
- `server/services/` - the YAML scraper collections that define streaming
  sources.
- `docs/` - specifications (`docs/specifications/`), build documentation
  (`docs/BUILDING.md`), and screenshots (`docs/screenshots/`).

Agent-facing repository rules live in `AGENTS.md` (root) and `server/AGENTS.md`.

## Adding or fixing a streaming source (YAML)

Most source work requires **no Rust changes**. Follow this order:

1. Read the relevant specification in `docs/specifications/` —
   `arachnea-stream-en.md` for streaming sources, `arachnea-scrapyfy-en.md`
   for the scraper YAML format itself.
2. Find the closest existing collection under `server/services/arachnea-stream/`
   and reuse its query structure and field conventions.
3. Implement the smallest useful subset first (`search` and `get_entry` are
   the baseline; add `load_home`, `get_category`, `get_season`, and the live
   queries only when the site exposes them cleanly).
4. Keep identifiers, comments, and added text in English.

If the source needs a scraper capability that does not exist yet (a new
action, a new post-processor, browser automation, etc.), stop and open an
issue describing the gap before touching the engine code.

## Code changes

- Keep shared crates independent from application-specific configuration;
  pass paths and contexts as parameters.
- Keep errors contextualized.
- Write code, identifiers, doc comments, and comments in English; keep
  developer-facing strings in English too.
- Preserve existing behavior unless the change requires otherwise; do not
  mix unrelated refactors into a focused change.
- Keep inline comments rare and only for non-obvious logic.
- When behavior changes, update the affected Markdown documentation and add
  an entry to `CHANGELOG.md` (append at the end of the `Unreleased` section;
  the file is append-only).

## Commit and pull request guidelines

- Keep commits focused; write commit messages in English.
- A pull request must state what changed and why, and link the issue it
  addresses when one exists.
- Before opening a PR, run:
  - `cargo check --workspace` (from `server/`);
  - `npm run type-check` and `npm run build` (from `front/`).
- Update or add screenshots under `docs/screenshots/` when your change
  affects the interface.

## Reporting bugs and requesting features

Use the issue templates under `.github/ISSUE_TEMPLATE/`. For security
matters, do **not** open a public issue — see [`SECURITY.md`](SECURITY.md).
