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

- `cd server && cargo run -p arachnea-stream --bin arachnea` - Run the backend executable.
- `cd server && cargo check -p arachnea-stream` - Type-check this crate.
- `cd server && cargo test -p arachnea-stream` - Run this crate's tests.
- `cd server/crates/arachnea-stream && cargo tauri build` - Build the Tauri desktop application and release bundles.
- `cd server/crates/arachnea-stream && cargo tauri build --target <target-triple>` - Build a platform-specific Tauri bundle once the target and native platform toolchain are installed. See the root and server README files for cross-platform setup notes.
