# Arachnea Server Agent Profile

## Project Context
- This directory contains the Rust scraper backend workspace.
- `crates/arachnea-stream` is the stream executable crate. Shared stream/backend primitives live in `crates/arachnea-core`, and the generic scraper engine lives in `crates/arachnea-scrapyfy`.
- `crates/arachnea-dns` contains reusable DNS resolver logic and optional DNS server entry points.
- `crates/arachnea-http` contains outbound HTTP client helpers and optional Cloudflare-oriented engines.
- `crates/arachnea-proxy` contains proxy routing, outbound transports, client connectors, and optional proxy server listeners.
- The Tauri controller implementation lives in `arachnea-core`, but application-specific Tauri context, embedded assets, URI scheme, API prefix, icons, capabilities, and `tauri.conf.json` are owned and supplied by `crates/arachnea-stream`.
- Runtime behavior is driven by files under `services/*`.
- Scraper logic and YAML configuration are tightly coupled and must stay consistent.
- `docs/TODO.md` tracks DNS, HTTP, and proxy follow-up work imported into this workspace.
- `CHANGELOG.md` records meaningful server workspace changes.
- Prefer small, local changes over broad rewrites.

## Code and Config Consistency
- The YAML format may be changed when needed.
- If a code change affects parsing rules, placeholders, actions, expected fields, or configuration semantics, update the relevant files under `services/*` or `data-test/*` in the same change.
- If a config change introduces new semantics, update the Rust code to match.
- Do not leave code and YAML out of sync.
- Prefer extending configuration over hardcoding source-specific behavior when that keeps the design simpler.
- Avoid source-specific business logic in Rust service facades. Prefer YAML entries, generic actions, or generic post-processors for source-specific extraction, mapping, pagination, and response shaping. Source-specific Rust should be reserved for integrations that cannot be expressed generically, such as authenticated player resolvers or protocol/proxy handling.
- Do not group, flatten, or otherwise shape scraper response structures in `stream_scraper.rs`; organize fields such as `source`, `entries`, pagination metadata, and `source_params` in YAML/actions or generic post-processors.

## Documentation Rules
- When source code and a `README.md` or `docs/specifications/*.md` file disagree, update the Markdown documentation in the same change. Also add missing documentation when it is needed to describe a relevant behavior, contract, limitation, or configuration requirement discovered during the work.
- Add or maintain Rust doc comments with `///` on modified public or non-trivial items when behavior, inputs, outputs, or invariants are not obvious.
- Document public constants, enum types, enum variants, and struct fields with `///` doc comments to describe their purpose and invariants.
- For function and method documentation, use idiomatic Rust `rustdoc` format rather than JavaDoc style.
- When a documented function or method has parameters, describe them in a `# Arguments` section.
- Add `# Errors`, `# Panics`, `# Returns`, or other standard `rustdoc` sections when they provide useful behavior contracts.
- When modifying a documented item, keep its existing `rustdoc` accurate and update parameter, error, and panic descriptions if behavior changes.
- Use YAML comments only when a field or structure is not self-explanatory.
- Keep crate README files aligned with public behavior, commands, configuration, and known gaps.
- Keep `docs/TODO.md` current when DNS, HTTP, or proxy planned work is added, completed, renamed, moved, or made obsolete.
- Append to `CHANGELOG.md` when server workspace behavior, public APIs, configuration, documentation, or operational workflows change.
- The files `docs/specifications/arachnea-scrapyfy-*.md` document the complete YAML schema for arachnea-scrapyfy. Any modification to scraper types, actions, or post-processors in the crate must be reported there to keep them in sync.

## Test File Layout

- These rules apply to every crate under `server/crates/`.
- Keep production Rust sources under each crate's `src/`; keep all test implementations, test-only helpers, and fixtures under that crate's root `tests/` directory. Do not place test bodies inline in production files or create test directories under `src/`.
- Mirror the tested source file's exact relative path and filename, replacing only the `src/` prefix with `tests/`: `src/core/http/client.rs` is tested in `tests/core/http/client.rs`, and `src/core/proxy_inventory/mod.rs` in `tests/core/proxy_inventory/mod.rs`. Do not add `_tests` suffixes or rename mirrored `mod.rs` files.
- Split suites that cover several source files across the corresponding mirrored test files. Scenarios exercising a composed module belong in the mirrored file for that module.
- Store shared test-only helpers in `tests/support/` and test data in `tests/fixtures/`; these support files are exempt from source-path mirroring.
- For unit tests needing private implementation access, keep only the `#[cfg(test)]` module declaration and a `#[path = "..."]` reference in the source, pointing to the mirrored file under `tests/`. Preserve existing feature gates and do not expose production internals merely to relocate tests.
- Load nested test files explicitly when needed; Cargo does not automatically discover the mirrored subdirectories. Public-API integration scenarios must also follow source-path mirroring and remain runnable through the appropriate test target.
- Apply this layout to new or moved tests. Migrate existing crates incrementally within the requested scope; `arachnea-proxy` is the first migrated crate.

## Engineering Defaults
- Avoid making the scraper more aggressive or brittle without an explicit reason.
- Keep source-specific logic configurable when that keeps the design simpler.
- Keep DNS core decisions in `arachnea-dns/src/core/`; server paths should adapt incoming protocol traffic and call the core.
- Keep HTTP engine-specific behavior behind the `arachnea-http/src/engine/` abstraction, and keep Cloudflare behavior explicit in configuration.
- Keep proxy routing, chain selection, privacy policy, and outbound transport decisions in `arachnea-proxy/src/core/`.
- Preserve the proxy safety posture: loopback by default and no unsafe public open proxy unless configuration explicitly allows it.
- Do not add `arachnea-stream` path references, Tauri app build scripts, or generated Tauri contexts to `arachnea-core`; pass them from the application crate instead.
