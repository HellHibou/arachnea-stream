# Arachnea Core Agent Notes

- This crate owns shared backend primitives: controller backends, controller contracts, credentials stores, and resource path helpers.
- Keep the crate independent from `arachnea-stream` service behavior and crate-local paths.
- Avoid adding scraper-source-specific behavior here; use `arachnea-scrapyfy` or stream resolvers as appropriate.
- Keep `TauriControlerService` generic: application crates must provide the `tauri::Context`, embedded asset provider, URI scheme, and API prefix.
- Do not add `tauri::generate_context!`, `tauri-build`, or hardcoded application config paths to this crate.
