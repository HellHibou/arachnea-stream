# Arachnea Core

`arachnea-core` contains shared backend primitives used by the Arachnea stream executable and scraper engine.

## Responsibilities

- Define controller abstractions and the REST/Tauri controller backends under `src/controler/`.
- Provide a main-thread dispatcher abstraction for native UI/event-loop work shared by controller backends.
- Define credentials persistence and runtime resource helpers under `src/persistence/`. The generic `PersistenceStore` contract is a factory of namespace-bound `PersistenceTransaction` handles (`get`/`put`/`delete`/`find_by_fields`/`commit`); records are stored exclusively as named fields, and the file backend defers writes until `commit` (dirty flag per namespace, atomic tmp + fsync + rename).
- Provide small shared utilities that are useful across backend crates and do not belong to the scraper engine or stream service.

## Notes

`TauriControlerService` is application-agnostic. Application crates must provide the generated `tauri::Context`, embedded frontend asset provider, custom URI scheme, and API prefix. Keep Tauri application config files, capabilities, icons, and build scripts out of this crate.

By default, the first configured frontend window hosts independent child webviews using Tauri's `unstable` multiwebview API. Its first child renders a 44 logical-pixel tab strip: the application frontend must recognize the injected `window.__DESKTOP_TAB_SHELL__` flag and render that shell instead of its regular page. Native window titles remain application-configured; tab titles follow document titles and initially inherit the native title.

The shell invokes `desktop_tabs_snapshot`, `desktop_tabs_open`, `desktop_tabs_activate`, and `desktop_tabs_close` (the latter two take an `id`) and listens for `desktop-tabs-changed` snapshots containing `tabs: [{ id, title }]` and `active`. These commands are restricted to the shell webview. Managed page webviews additionally report document fullscreen through `desktop_tabs_set_fullscreen`. Opening activates the new tab; closing the last tab creates a fresh home tab. Hidden tabs retain state and may continue media playback until closed. Tabs are not persisted across application restarts. Application capabilities must cover the host window and its child webviews.

Internal native new-window requests become tabs; external HTTP(S), mail and telephone requests use the system handler. Administration remains a dedicated window. Frontend HTML uses an absolute mount base so deep routes can load directly. Native resize and scale-factor events update child bounds explicitly. Desktop validation must include platform-specific multiwebview layout and native context-menu behavior.

Select the presentation in source with `TauriControlerConfiguration::new(context).browsing_mode(TauriBrowsingMode::Windows)` or `TauriBrowsingMode::Tabs`. Both the enum and `DEFAULT_TAURI_BROWSING_MODE` are exported from `controler::tauri`; the default is `Tabs`. Window mode opens `frontend-*` native windows, inheriting the requesting window title, and requires matching application capabilities. It does not mount the tab shell.

In tab mode, the strip is hidden when there is only one tab or when the active page enters document fullscreen, and that page fills the content area. From two tabs onward outside fullscreen, 44 logical pixels are reserved at the top. All child bounds are reapplied before showing a page, after closing one, and when fullscreen changes, using physical coordinates derived from the current display scale to keep the strip and content aligned.

On macOS, tab layout reads AppKit `contentLayoutRect` on the main thread and converts it into the native parent view coordinates before placing child webviews. This accounts for native title bar and toolbar insets without a fixed decoration offset. Other desktop platforms continue to use the window inner size.

Binary routes on the desktop custom URI scheme execute in background async tasks and reply through Tauri’s asynchronous protocol responder. Response bodies remain fully buffered before delivery; network waits do not block the protocol callback. REST routes can deliver streaming bodies progressively.
