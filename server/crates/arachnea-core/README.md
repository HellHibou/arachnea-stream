# Arachnea Core

`arachnea-core` contains shared backend primitives used by the Arachnea stream executable and scraper engine.

## Responsibilities

- Define controller abstractions and the REST/Tauri controller backends under `src/controler/`.
- Provide a main-thread dispatcher abstraction for native UI/event-loop work shared by controller backends.
- Define credentials persistence and runtime resource helpers under `src/persistence/`. The generic `PersistenceStore` contract is a factory of namespace-bound `PersistenceTransaction` handles (`get`/`put`/`delete`/`find_by_fields`/`commit`); records are stored exclusively as named fields, and the file backend defers writes until `commit` (dirty flag per namespace, atomic tmp + fsync + rename).
- Provide small shared utilities that are useful across backend crates and do not belong to the scraper engine or stream service.

## Notes

`TauriControlerService` is application-agnostic. Application crates must provide the generated `tauri::Context`, embedded frontend asset provider, custom URI scheme, and API prefix. Keep Tauri application config files, capabilities, icons, and build scripts out of this crate.

The first configured frontend window handles native new-window requests for internal URLs by opening independent `frontend-*` windows with the requesting window's current title. Secondary windows support the same behavior. Application capabilities must include these labels for frontend IPC permissions. External HTTP(S), mail and telephone links requested through this handler open with the system handler. Frontend HTML uses an absolute mount base so deep routes can load directly.
