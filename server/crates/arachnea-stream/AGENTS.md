# Arachnea Stream Agent Notes

- This crate owns the backend executable, stream service facade, and player resolvers.
- Keep the stream facade in `src/stream_scraper.rs`; keep player resolver implementations under `src/services/`.
- Keep source-specific response shaping in YAML/actions or generic scraper post-processors, not in `stream_scraper.rs`.
- Player resolver code may contain source-specific Rust only when the behavior cannot be expressed in scraper YAML.
- Tauri assets and configuration live in this crate: `tauri.conf.json`, `capabilities/`, and `icons/`.
- Supply Tauri context, embedded assets, URI scheme, and API prefix to `arachnea-core::controler::tauri::TauriControlerService` from this crate.
