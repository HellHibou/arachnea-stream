# Arachnea Stream Agent Notes

- This crate owns the backend executable, stream service facade, and player resolvers.
- Keep the stream facade in `src/stream_scraper.rs`; keep player resolver implementations under `src/services/`.
- Keep source-specific response shaping in YAML/actions or generic scraper post-processors, not in `stream_scraper.rs`.
- Player resolver code may contain source-specific Rust only when the behavior cannot be expressed in scraper YAML.
- Exception to crate-root constant centralization: keep constants used by only one service resolver at the beginning of its owning `src/services/*_resolver.rs` file, after imports and before runtime state/types. Private child modules count as part of that resolver and reuse its parent constants. Keep constants genuinely shared by multiple resolvers in `src/lib.rs` and import them instead of copying definitions. Keep unrelated provider TTLs, policies and protocol identifiers distinct even when their current values match. Application and generic facade constants also remain in `src/lib.rs`.
- Tauri assets and configuration live in this crate: `tauri.conf.json`, `capabilities/`, and `icons/`.
- Supply Tauri context, embedded assets, URI scheme, and API prefix to `arachnea-core::controler::tauri::TauriControlerService` from this crate.
- Follow the shared Rust source, constant, and test-layout rules in `../../AGENTS.md`. Keep Stream test implementations under crate-root `tests/` and preserve their use of Scrapyfy's existing `test-support` helpers through the development dependency; do not move application-specific test helpers into shared production code.
