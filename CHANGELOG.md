# Changelog

All notable changes to the server workspace are recorded here. Add new entries at the end of the file so the history remains append-only.

## Unreleased

### Fixed
- **Unified runtime option loading**: application option providers now apply
  JSON configuration-file values before command-line arguments, preserving the
  common `default < configuration < command line` priority rule and setting
  provenance during parsing. Stream no longer performs its own post-parse
  persisted-settings merge. Configuration files now use option names without
  `--` (such as `server-port`); legacy underscore keys remain readable, while
  the administrator `password_hash` remains configuration-only.
- **Unified persisted core options**: `ApplicationConfiguration` was removed in
  favor of `arachnea-core::controler::options::CoreApplicationOptions`, which
  now owns JSON loading, validation, atomic saving, and owner-only file
  permissions for the persisted server port, network mode, entrypoint root,
  and administrator password hash. The Stream executable continues to provide
  its application-specific `data/config.json` path to Core.
- **Service reload counters removed from front and backend**: the admin
  reload flow no longer reports the erroneous `loaded`/`disabled`/`ignored`/
  `errors` counts. `ReloadResponse` and the internal `AdminGroupReload`
  report now only carry `applied` and `build_error`, validation failures are
  collapsed into the `build_error` message, and the admin UI shows a simple
  success/failure alert. The tray reload log line and `StreamReloadReport`
  were reduced to the same outcome fields.
- **Service management list letter fallback removed**: services without a
  configured logo in the admin catalog no longer render a first-letter or
  "Service" text placeholder in their logo slot; the logo box is omitted
  entirely when no icon is present. The horizontal spacing once applied
  around the logo now sits between the enable switch and the logo (20px)
  and between the logo and the description (10px), keeping the switch and
  the description readable even when no icon exists.
- **macOS Windows ARM64 cross-compilation (`build-release/`)**: the
  `aarch64-pc-windows-msvc` build now uses a macOS-only compiler wrapper.
  It preserves cargo-xwin's `clang-cl` backend for BoringSSL, while translating
  its `/imsvc` flags to Unix `clang` flags when `ring` invokes `clang`
  directly; all other hosts and targets retain their existing toolchain.

### Added
- **Admin web application (`front/admin-app/`)**: Independent Vite/Vue/Vuetify SPA served under `/admin`, built separately from the public frontend. Features include:
  - Conditional login screen driven by `/api/admin/status` (`auth_required`/`authenticated`).
  - Service catalog with localized descriptions, configured logos, and enable/disable toggles (override persistence via `arachnea-services` namespace).
  - Credentials dialog with masked login display and signup URL opener (`window.open`).
  - Settings page for port, network mode (local/private/public), entrypoint root, and administrator password change.
  - Reload button with simplified success/failure feedback.
  - Theme system (system/light/dark) with `system` default, persisted to localStorage.
  - i18n (en/fr) with browser detection and `en` fallback.
  - All API calls centralized in `useAdminApi` composable with normalized error handling (`AdminApiException`, `{error:{code,message}}`).
  - Build outputs to `dist/admin/` without conflicting with public frontend (`dist/`).
  - Left navigation drawer holding the Services and Settings links (moved from the app bar tabs).
  - Popup notification stack for backend errors (network failure, invalid response, HTTP error statuses including 401/403 access-denied) with dismiss, Escape handling, and copyable error code.
  - Dev proxy rewrites the `Origin` header to the backend target so admin write-origin checks (Origin vs Host) pass under `npm run dev` (admin-app and public-app Vite configs).
  - Credentials key button stacked below the status chip in the service list; credentials dialog title shows the service title with its source code in parentheses, and the signup link on its own line.
  - Enabling a service that requires credentials but has none configured now asks how to proceed: add credentials (activation resumes after save, cancelled dialog keeps the service disabled), activate without credentials, or leave the service disabled.
- **Linux installers (.deb/.rpm/.AppImage) from the Docker cross image (`build-release/`)**:
  On hosts that cannot natively bundle them (macOS/Windows), the `linux-x86_64`
  and `linux-arm64` platforms are now fully produced inside the cross image: a
  single in-image `cargo tauri build --target <triple>` pass (via the newly
  installed Tauri CLI) compiles the release binary AND generates the three
  installer formats, alongside — not instead of — the existing portable
  `.tar.gz`. No FUSE or privileged container is involved: the Tauri deb/rpm
  bundlers are pure Rust and its AppImage bundler runs linuxdeploy with
  `--appimage-extract-and-run` (downloads cached in
  `<workspace target>/.tauri-bundle-cache/<arch>/` across runs). Host-side,
  `capabilities.mjs` gained `dockerBundlesFor()` (targets x64/arm64 Linux ->
  `deb`/`rpm`/`appimage`, intersected with the config's declared bundles) and
  `needsDockerBuild()` now also triggers on bundle declarations, not just
  portable ones; `release.mjs` assembles/checksums the container-produced
  installers like native ones (discovered under the existing
  `docker-build/<arch>/` roots). Cross image bumped to
  `arachnea-cross-builder:1.0.0`: added `curl`, `ca-certificates`, `file`,
  `desktop-file-utils`, and tauri-cli 2.11.4 (prebuilt binary on the amd64
  port, compiled from crates.io on arm64 since upstream ships no
  aarch64-linux asset; the GNU target is forced explicitly because the base
  image's cargo config defaults its arm64 port to a static musl build that
  fails to link). Docker platforms now also probe that a local image variant
  exists for their container architecture before running, so a missing amd64
  or arm64 build yields an actionable error instead of a registry pull
  failure. Linux installers are produced by **one in-image `cargo tauri build`
  pass per bundle type** (`bundle.targets` limited to a single type each time):
  requesting `deb`+`rpm`+`appimage` together in one process made the Tauri
  bundler crash with "Could not read binary file" on the mounted volume between
  types. The image also gained `squashfs-tools` (linuxdeploy needs `mksquashfs`
  for the AppImage) and each bundle pass is attempted independently: when a
  bundle cannot be produced in the container (e.g. `.AppImage` under Docker
  Desktop, which has no FUSE for linuxdeploy's internal sub-processes), the
  other installers and the portable archive are still shipped and the platform
  is reported as a success with a warning. `--list` and `--dry-run` reflect
  the new production.
- **Release artifact naming and checksums (`build-release/`)**: All produced
  artifacts now follow the `<product>-<version>-<os>-<arch>.<ext>` scheme
  (e.g. `arachnea-0.1.0-darwin-x64.dmg`,
  `arachnea-0.1.0-linux-amd64-portable.tar.gz`, `-setup.exe` kept for NSIS) and
  macOS outputs are no longer placed under an `osx/` sub-folder — the family's
  own `darwin/` name is kept in `releases/release-<version>/`.
  A portable archive's `.sha256`/`.md5` checksum file lists the archive's own
  hash first, then the hash of every file staged inside the archive; hashes are
  computed on the original files during packaging without ever extracting the
  archive. `release-config.json` also accepts a top-level `portable.include` /
  `portable.exclude` block shared by every platform with a `portable` section,
  merged with each platform's own lists (common values first, platform values
  after, de-duplicated).
- **`--force-use-docker-builder` release flag**: New CLI option routing every
  Docker-capable platform (`"build": "docker"` + portable block + target listed
  in the cross-image matrix) through the Docker builder even when the current
  host could natively produce it (e.g. building macOS portables from a Mac
  inside the cross image instead of natively). Targets not managed by the
  image (Windows MSVC bundles, `darwin-universal`, ...) keep the native
  builder.
- **Cross-image aarch64-apple-darwin compiler fix (`build-release/docker/`)**:
  The generated `aarch64-apple-darwin22.4-clang`/`-clang++` wrappers pointed at
  the historical unversioned osxcross driver (`aarch64-apple-darwin-clang`),
  which exists in neither the amd64 nor the arm64 image port, so any build
  compiling Objective-C sources for `aarch64-apple-darwin` inside the
  container failed (`objc2-exception-helper`: "exec: ... not found"). Wrappers
  now exec the arch-versioned drivers (`aarch64-apple-darwin22.4-clang[-++]`,
  present in both ports) with their matching `-ld`; verified by a successful
  full `darwin-arm64` container build producing a Mach-O arm64 executable.
- **Accurate rustup target provisioning before native builds (`build-release/`)**: In addition to the up-front `install-tools` provisioning, each native platform now runs a per-platform pre-flight guard right before its `cargo tauri build`. `install-tools.mjs` factors Rust target detection into `missingRustTargets()` and exports `ensurePlatformRustupTargets()`, which probes the actual `rustc` that will build (its sysroot `lib/rustlib/<target>` directory — the exact source of `can't find crate for core/std`) and installs any missing target (e.g. `aarch64-pc-windows-msvc`) on the owning rustup toolchain after explicit confirmation. `release.mjs` calls it for every native build (unless `--skip-build`). Detection is sysroot-based instead of `rustup target list`-based because the two can disagree when a standalone Rust install precedes rustup's cargo on PATH; in that case the guard stops with an actionable message instead of letting the build fail cryptically.
- **Automatic Windows ARM64 C/C++ toolchain provisioning (`build-release/`)**: Native `aarch64-pc-windows-msvc` builds on Windows now verify and, when missing, install the two Visual Studio Build Tools components the crates require (`ring`, `aws-lc-sys`): the MSVC ARM64 build tools (compiler + CRT, used by rustc to link) and the C++ Clang Compiler for Windows (clang-cl). Detection is physical (`vswhere` + toolset/sdk inspection, exposed as `windowsNativeMsvcArm64State()` in `lib.mjs`); missing components are added to the existing VS installation through `vs_installer.exe modify` (elevated via a UAC prompt through `Start-Process -Verb RunAs` — completed by polling the elevated process, since `-Wait` can return early — and with `--noUpdateInstaller` to avoid cancellable feed checks; if the unelevated self-elevation is cancelled by the installer, the error message points to running the script from an Administrator terminal) after explicit confirmation (no VS installation at all → `winget` installs the Build Tools C++ workload first). `buildEnvWithLlvm()` additionally extends every `cargo tauri build` PATH with the VS-provided LLVM bin (clang-cl), the VS CMake Ninja directory (`ninja.exe`, shushing the former false "missing ninja" warning) and the MSVC toolchain bin dirs, so native Windows ARM64 builds find the C toolchain without manual environment setup.

- **Release builder configuration**: Renamed `build-release/release-targets.json` to `release-config.json` and added `frontendProject`, so the frontend build directory is configured rather than hardcoded.
- **Release builder failure handling**: Added `--continue-on-error` to continue after a platform-specific failure, with a final per-target success/failure summary and non-zero exit status when any target fails.
- **Release tool provisioning confirmation**: The release toolchain now requests explicit interactive confirmation before installing Rust targets, packages, or build tools, including when provisioning is started by `release.mjs`.
- **Server-side scraper result cache (foyer)**: `ScraperAgregator` now embeds a lazily-built foyer hybrid cache (`ScraperServerCache`, bounded to 100 MiB on disk / 32 MiB in memory by default, rooted at `<application data>/cache`, overridable through `set_cache_config(ScraperCacheConfig)`). Entries are keyed per source execution (group + source + query + merged parameters + media-type filter + fields filters + source YAML hash) and hold the parsed rows together with the validator fragment that produced them. Freshness is driven by ETag revalidation against the stored fragment; a one-year safety TTL (`SERVER_CACHE_ENTRY_TTL`) bounds entry age. Stale sources of the phase-2 catch-up are served from this cache before any remote re-fetch; failed executions invalidate their entry instead of caching partial data.
- **`QueryParameters::cache_type`**: replaced `enable_etag: bool` with a `CacheType` enum — `NoCache` (former `enable_etag = false`), `ClientCache` (former `enable_etag = true`, still the default), `ServerCache` (server cache only, no global ETag exposed to the client) and `FullCache` (server cache AND global ETag exposed to the client, keeping the full `304 Not Modified` short-circuit). Internal engine callers (proxy/IP-country providers, stream resolver) now pass `CacheType::NoCache`.
- **Public server binding**: Added the `--server-public` flag to the backend executable. When set, the REST server binds to `0.0.0.0` instead of `127.0.0.1`, making the web interface and API reachable through any local network hostname or IP (for example `http://pc-jeremy:8080/`). It is exposed through `CoreApplicationOptions::server_public` and propagated to the REST controller configuration.
- **Generic persistence store**: Added `PersistenceStore`, `PersistenceKey`, `PersistedRecord`, `MemoryPersistenceStore`, and `FilePersistenceStore` in `arachnea-core::persistence`. The file backend stores one document per namespace with atomic writes and expired-record pruning on read; JSON remains the default codec, while callers can supply a codec and extension.
- **Cloudflare session persistence**: `ChaserCfEngine` now persists structured Cloudflare sessions (cookies, user-agent, expiration) through the shared `PersistenceStore` instead of the temporary JSON file. The first browser-strategy refresh may consume a persisted session before requesting a fresh solve; retries after a block stay fresh.
- **Persistence store propagation**: `StreamScraper::new_with_persistence_store`, `ScraperAgregator::new_with_persistence_store`, `HttpClient::with_http_config_proxy_handle_and_local_country_and_persistence_store`, and `ArachneaHttpClient::new_with_cookie_cache_browser_session_manager_and_persistence_store` now accept a shared `Arc<dyn PersistenceStore>`. `StreamScraper::new` uses the durable file backend; lower-level compatibility constructors retain the in-memory backend.

- **Frontend video source allowlist**: Added a central allowlist at
  `front/public/video-sources-whitelist.json`, loaded at startup by
  `loadVideoSourceAllowlist()` in `front/src/services/videoSourceAllowlist.ts`.
  The allowlist now gates embedded players and trailers (iframe surfaces): in
  `confirmation` mode an allowlisted source (seeded with YouTube) renders
  directly while a non-allowlisted source keeps the confirmation placeholder; in
  `safe` mode only allowlisted sources render and everything else is blocked;
  `unsafe` renders every embed. Native video streams resolved by the backend
  players (HLS/DASH) remain always playable and are not subject to the
  allowlist.
- **Banner trailer videos as page background**: when the trailer usage
  parameter (`useTrailerAsBackground`) is enabled and at least one home or
  category banner exposes a playable video, the catalog background media
  candidates now carry up to five banner videos instead of background images.
  Banner trailers are collected from direct `videoUrl` values and, like the
  hero banner, from `player` resolvers asynchronously resolved through
  `get_stream`; deferred `get_banners` arrivals are picked up reactively,
  including when `load_home` initially exposes no video at all.
  Each candidate keeps its paired banner image as a security-mode fallback,
  videos are deduplicated by URL, and playback is sequential: the background
  layer waits for each native video to end before starting the next one
  (iframe embeds such as YouTube loop or advance on the fixed rotation delay).
  When no banner video is available, the previous image-based behavior applies;
  the app-level background gate no longer drops these candidates when
  `useCatalogBannersAsBackground` is disabled since each media kind now carries
  its own parameter upstream.
- **Server cache CLI sizing overrides**: the backend executable now accepts
  `--cache-max-disk-bytes <BYTES>` and `--cache-max-memory-bytes <BYTES>` to
  override the server-side scraper result cache bounds (defaults stay at 100 MiB
  on disk / 32 MiB in memory). The values replace the corresponding fields of
  `ScraperCacheConfig` before the aggregator builds its foyer hybrid cache.
- **Entry point root redirect**: when the REST server runs under a non-root
  `--entrypoint-root` prefix, a bare `GET /` now answers `302 Found` with a
  `Location` header pointing to the mounted application path (for example
  `/prefix/`) instead of `404 Not Found`; without a prefix the existing
  root-serving behavior is unchanged.
- **Removed `--entrypoint-api` CLI flag**: the backend executable no longer
  accepts `--entrypoint-api`; the server continues to use the built-in `api`
  path segment under the configured `--entrypoint-root` (the
  `CoreApplicationOptions::entrypoint_api` plumbing in `arachnea-core` is
  unchanged).
- **Root-aware public JSON fetches**: `loadThemes()` and
  `loadVideoSourceAllowlist()` now resolve `themes.json` and
  `video-sources-whitelist.json` through `resolveAppPath()` like the locale
  assets, so both files load correctly when the server serves the application
  under a non-root `--entrypoint-root` prefix instead of failing with root-
  absolute `/themes.json` / `/video-sources-whitelist.json` requests.

### Fixed

- **Release tool installer EAGAIN crash**: the interactive confirmation in
  `build-release/install-tools.mjs` read stdin with a raw synchronous
  `readSync(0, ...)`, which failed with `EAGAIN` when stdin sat in non-blocking
  mode (e.g. IDE terminals). Confirmations now go through a
  `node:readline/promises` question written explicitly to stdout (readline
  ignores its own prompt string without `terminal: true`, which left a silent
  wait); `installTools()` and its helpers are async and awaited by
  `release.mjs`. A closed stdin (EOF/Ctrl+D) counts as a refusal.
- **Docker Linux cross-builds now run container-arch-matched containers**:
  linking a GNU/Linux binary requires GTK/WebKitGTK/OpenSSL for the *target*
  architecture, but the cross image only ships those stacks for its own arch,
  so `linux-x86_64` builds failed — first at `openssl-sys`
  (`openssl/opensslconf.h` is per-architecture under Debian multiarch), then
  at the final link (`cannot find -lwebkit2gtk-4.1`, ...). The cross image
  (`build-release/docker/Dockerfile`) is rebuilt as an amd64 + arm64 manifest
  list, and `docker.mjs` pins `--platform linux/amd64|arm64` to the Linux
  target triple (macOS targets keep the host default through osxcross). Cargo
  artifacts split into `<workspace target>/docker-build/<amd64|arm64>/`
  via `CARGO_TARGET_DIR` so build scripts / proc-macros compiled for different
  container architectures cannot overwrite each other in the shared mounted
  workspace; `targetBaseDirs()` includes those directories when present so the
  portable-archive step still finds the raw executables. The GNU/Linux link
  also pins the system OpenSSL shared objects through per-target
  `CARGO_TARGET_*_RUSTFLAGS`: `boring-sys2`'s static `libssl.a`/`libcrypto.a`
  -L directories shadow the bare `-lssl`/`-lcrypto` names and left OpenSSL
  3-only symbols (`SSL_read_ex`, `SSL_get1_peer_certificate`,
  `ERR_get_error_all`, ...) undefined at link time.

### Changed
- **Cloudflare session store schema flattened**: `CachedChaserSession` no longer
  persists a JSON `session` blob in the typed `cloudflare-session` store. The
  schema now declares `origin` (primary key), `cookies` (the only JSON column),
  nullable `user_agent`, nullable `clearance_expires_at`, required `stored_at`,
  and the derived indexed `expires_at` (unchanged fallback: `stored_at +
  CACHE_TTL_NO_EXPIRY` when `cf_clearance` carries no expiration). Reads and
  writes use checked `u64`/`i64` conversions with contextualized errors that
  never include cookie values. `CACHE_TTL_NO_EXPIRY` now has a single
  definition in `chaser_session` shared with the chaser-cf engine, and the
  `StructuredCookie`/`ChaserCookie` conversions moved to local `From`
  implementations. **Operator action required**: the schema is incompatible
  with an existing SQLite cache — delete
  `<application-data>/data/persistence/cloudflare-session/` (including WAL/SHM
  files) before starting this version; legacy Cloudflare sessions are disposable
  cache data and are never migrated.
- **Background Ken Burns pans exactly to the real image borders**: the animated background image now measures each source's natural dimensions on load and derives its true rendered overflow under `object-fit: cover` (`--ken-burns-max-x` / `--ken-burns-max-y` CSS variables per image), so the pan sweeps from one image corner to the opposite one regardless of the picture's aspect ratio — portrait images finally use their full vertical headroom while landscape ones stay capped at their own edges. A zoom breathing from `1x` to `1.22x` mid-cycle keeps the effect clearly visible even when the picture closely matches the screen ratio; every frame stays fully covered since translations never exceed the measured overflow. Bounds are recomputed on viewport resize, pruned when the media list changes, and fall back to a static frame until an image has loaded.
- **Bookmarks persist the generic `img.url` snapshot**: `EntryBookmark` gains an `imageUrl` field (the un-oriented image from `get_entry`), saved alongside `imagePosterUrl`/`imageLandscapeUrl` in `ProgramEntryDetails`, carried through the bookmark lookup and sanitization in `storage.ts`, and exposed by the bookmarks home section so favorite cards fall back to it when no poster or landscape image is available. Existing bookmarks without the field load as `imageUrl: null` with no migration needed.
- **URL slugs are now centralized and accent-free**: added `buildUrlSlug()` in `front/src/services/textUtils.ts` which lower-cases text, strips diacritics through Unicode NFD normalization (`é` -> `e`, `Ü` -> `u`), replaces every remaining character outside `[0-9a-z]` with `_`, and strips trailing underscores. The category and entry route slugs use this helper; empty slugs fall back to a fixed segment (`category` / `entry`).
- **Entry route now carries an informative title slug**: `/entry/:encodedEntry` links are built as `<slug>-<base64url payload>` via the new `encodeEntryRouteParam({ source, entryUrl, title })`; `MediaSelectionTarget` gained an optional `title` so cards, hero banners, and catalog selections can feed the slug, and banners pass their title in the selection target. `decodeEntryRoutePayload` ignores the slug prefix while still accepting bare tokens. The encoded payload is unchanged: bookmark keys produced by `buildBookmarkKey`/`encodeEntryRoutePayload` remain byte-identical.
- **Category route now carries a self-contained base64url payload**: `/categorie/:categoryToken` links are built as `<slug>-<base64url>` where the slug is the lower-cased category label with every character outside `[0-9a-z]` replaced by `_` (informative only) and the token encodes `{ v: 1, label, sources }` through the existing `routePayloads` base64url helpers (`encodeCategoryRouteParam` / `decodeCategoryRoutePayload`). `CategoryView` rebuilds the `HomeCategory` synchronously from the decoded sources and no longer calls `load_home`, so opening a category issues a single `get_category` request. Legacy merge-key-only URLs are no longer supported and render the invalid-link error; the unused `category.loadingTitle`, `category.loadingMessage`, and `category.notFound` locale keys were removed.
- **Dev profile dependency optimizations**: dependencies are now compiled with `opt-level = 1` in the workspace `dev` profile (`[profile.dev.package."*"]`). This keeps the serde-heavy paths (server cache serialization, scraper result envelopes) fast during development at the cost of slightly longer cold builds; application code stays fully debuggable.
- **Global ETag format is now self-describing**: the aggregated ETag is a `;`-separated list of `name=fragment` pairs covering only the sources that produced a real validator (`E:`/`C:`). Sources without remote validation (static/fetch-less queries) and failed sources are excluded from the value, so a returned ETag never claims freshness it cannot back. The previous positional format (one fragment per selected service, with a deterministic `N:` fallback and an `S:` services-hash block) is no longer emitted; legacy tags decode to an empty fragment map and trigger a full re-fetch. `decode_client_fragments` now takes the raw tag and returns a map.
- **Merged controller result contract**: `register_result_function` / `register_result_function_with_state` are now the single header-aware registration path. Their closures receive a new `RequestControlerContext` (carrying the incoming HTTP headers) right after the state parameter and return `(payload, Option<String> global_etag)`; they answer `304 Not Modified` or attach the `ETag` response header like the former `register_etag_result_function[_with_state]`, which were removed. REST builds the context from Warp headers, Tauri from an empty context. New constants `HEADER_IF_NONE_MATCH` and `HEADER_ETAG` live in `arachnea-core::controler`.
- **Conditional ETag ownership moved into Scrapyfy**: `ScraperAgregator::execute_query_async` now takes `(&RequestControlerContext, QueryParameters)` as its first parameters (after `&self`) and internally runs the phase 1 (parallel conditional validation), stale-source phase 2 re-fetch, and global ETag rebuild previously implemented by `StreamScraper::execute_query_with_etag`. `QueryParameters` defaults to `enable_etag = true`; internal engine callers (proxy/IP-country providers, stream resolver) disable it explicitly. `ScraperAggregationResult` gains a serialized `global_etag` field (`with_global_etag` / `take_global_etag`). The global ETag helpers moved from `arachnea-stream::stream_etag` to `arachnea-scrapyfy::global_etag` (the stream module keeps re-exports).
- **Internal-only ETag fields in the envelope**: `ScraperAggregationResult::validations` and
  `ScraperAggregationResult::global_etag` are now annotated with `#[serde(skip_serializing)]`.
  Both stay available to the aggregation engine and the header-aware controller contract
  (`ETag` response / `If-None-Match` request) but no longer appear in JSON responses. The
  frontend never read either field.
- **Source params helpers moved into Scrapyfy**: `source_params_from_entries`, its private
  `request_param_value_to_string` helper, and the request entry type (renamed
  `ScraperSourceParamsRequestEntry`) moved from `arachnea-stream::stream_scraper` to the new
  `arachnea-scrapyfy::source_params` module so other controller facades can reuse them. The
  stream scraper now consumes the re-exported items through its existing glob import.


- **ETag-free stream request payloads**: The eleven scraper command request structs no longer carry `arachneaEtag` / `enableEtag` fields. The client global ETag now flows exclusively through the `If-None-Match` request header into `RequestControlerContext`, and conditional validation is enabled by default through `QueryParameters`. Frontend callers that still send `arachneaEtag` keep working because unknown query parameters are ignored.
- **Server launch URL display**: The REST server startup message now shows `localhost` when bound to a loopback address, and the machine's first non-loopback IPv4 address when bound to an unspecified address (`0.0.0.0`), instead of always printing the raw bound IP.
- **Optional sprite storyboard dimensions**: Resolver storyboards may now omit `width` and
  `height`. The frontend preloads the first sprite image and derives its cell dimensions from the
  image's natural size and declared grid. Uniform cell separators are removed before rendering,
  allowing LuluStream's variable-size sprites to display correctly.
- **Chaser-CF session cache**: Replaced the file-backed `ChaserSessionCache` with an asynchronous adapter over `PersistenceStore`. Removed `DEFAULT_SESSION_CACHE_FILE_NAME`, `default_session_cache_path`, the private JSON document, and `std::fs` I/O from `chaser_cf.rs`. Sessions now store structured cookies instead of reconstructed `Set-Cookie` strings and are scoped only by normalized origin, including when routed through the dynamic Arachnea proxy.
- **Player image fit and error fallback**: Native video posters, media confirmation posters, and media overlay logos in the frontend now use `object-fit: contain` so the whole image stays visible without distortion. Storyboard hover thumbnails now fit their 16:9 preview frame with a uniform scale that preserves the source cell aspect ratio (previously an independent x/y scale deformed off-ratio cells). When a poster or logo image cannot be loaded, its URL is recorded as failed so it behaves exactly like a `null` candidate: the poster resolvers skip it and advance to the next available fallback image (`resolvedPlayerPosterUrl` → episode preview → landscape → portrait for programs, equivalent chains for live), instead of showing a broken-image icon or hiding the surface outright.

### Fixed
- **Proxy redirect Location host**: The generic HTTP proxy now rewrites redirect
  `Location` headers as root-relative proxy paths (`/api/proxy/opts_.../https://host/...`)
  when the controller entry point is an http/https URL. Previously the rewritten
  Location embedded the server bind address, producing unusable URLs such as
  `http://0.0.0.0:8080/api/proxy/...` when the server was bound to an unspecified
  address and reached through another hostname or IP. Custom scheme entry points
  (Tauri `arachnea://api/proxy`) keep their absolute form.

### Added
- **Parallel isolated page clicks**: Entry-level HTML `page_click` sub-queries
  can now use `browser_context: isolated`. The scraper opens dedicated browser
  pages and executes up to four independent clicks concurrently while preserving
  input order. PapaDuStream v2 uses this mode when resolving player embeds.
- **Generic grouped-field copying**: Scrapyfy now provides `copy_item_fields`
  to clone fields within every item of a root group. PapaDuStream v2 uses it to
  expose the page-click result as both `embed-link` and `web-link` without a
  duplicate browser sub-query.
- **DoodStream HTML recognition**: The resolver now identifies DoodStream-compatible
  players from their `doodcdn.io` markup rather than from a Playmogo-specific URL.
- **DoodStream sprite storyboards**: The DoodStream resolver exposes Dood's single 6 × 6
  preview sprite through the common `storyboard` contract instead of treating
  the misleading `get_slides/*.jpg` WebVTT endpoint as an image.
- **Chaser-CF session handoff**: The chaser-cf engine now delegates Cloudflare challenge handling to its public `ChaserCF::solve_waf_session` API and returns only clearance cookies plus the browser-observed user-agent. The shared client cache hands those values to `rquest`, which performs the actual HTML request and redirect handling.
- **Chaser-CF parameter-bound loopback**: Browser Cloudflare solves now use a dedicated Arachnea loopback proxy with routing parameters bound server-side, allowing Chrome CONNECT requests and `rquest` to share country-based proxy routing without exposing custom proxy headers to Chrome.
- **Chaser-CF persistent page sessions**: `open_browser_page_session` is supported again and returns a `ChaserCfPageSession` that retains a dedicated Chrome page for navigation, same-page fetch, click-and-wait, and Turnstile token reads. The page never solves the challenge itself: each navigation resolves a fresh session through the public `ChaserCF::solve_waf_session` facade, then replays the browser user-agent and clearance cookies via CDP before navigating. Logs expose cookie names but never cookie values.
- **PapaDuStream v2 YAML service**: Added series home rails, search, entry metadata, season episodes, and player language listings for PapaDuStream v2.
- **Embed4me YAML hoster**: Added `embed4me.yaml` for `lpayer.embed4me.com/#id` embeds. The resolver derives the encrypted video API endpoint from the fragment and exposes HLS, title, poster, and thumbnail VTT metadata.
- **SaveFiles YAML hoster**: Added `savefiles.yaml` for `savefiles.com/e/{id}` embeds. The resolver POSTs to `/dl`, then extracts JWPlayer HLS, poster, and thumbnail VTT metadata from the player page.
- **Generic encrypted API support**: Added `request_url_actions` for transforming resolved root query URLs and `aes_cbc_decrypt` for deterministic hexadecimal AES-128-CBC payloads.
- **Vidsonic YAML hoster**: Added `vidsonic.yaml` for `/e/{id}` embed pages, including HLS URL extraction, title, poster, and thumbnail VTT metadata. Added the generic `hex_decode` Scrapyfy action used to decode its deterministic hexadecimal payload.
- **TF1 intro chapters**: TF1 player resolution now maps `inGD`/`outGD`
  metadata to an `intro` chapter. The frontend displays localized intro
  labels and provides a Skip Intro button during that chapter.
- **Generic scraper transformation pipeline**: Added `caesar_shift`, `regex_replace_all`, `bytes_shift`, `reverse`, and `json_extract_text` actions plus generic `apply_actions_to_field` and `fetch_actions_to_field` post-processes. Added the YAML VOE resolver using those primitives.
- **Backend `get_banners` and `get_players` commands**: Added `GetBannersRequest`/`GetPlayersRequest` input structs, `StreamScraper::get_banners`/`get_players` methods executing the respective YAML query for a single source with `source`/`link` parameters, and command registration in the controller. Added frontend `getBanners`/`getPlayers` API wrappers.
- **Hybrid collection contract for banners and players**: Added `Collection<T>` type (`front/src/types/media.ts`). `HomeCatalogData.banners`, `EntryDetails.players`, and `EntryPlayableItem.players` migrated from bare arrays to `Collection<T>` with `entries`, `source`, and optional `link`. All normalizers and consumers updated without backward compatibility layer.
- **Dynamic proxy loading analysis**: Added a French design analysis for country-based dynamic proxy loading, proxy probing, persistence, and the `arachnea-proxy`/`arachnea-scrapyfy` trait boundary.
- **Spys.one Belgium proxy source draft**: Added an inactive `spysone-be.yaml` proxy source definition using POST form submission and dynamic port decoding.
- **YAML `sub_queries` at entry level**: New `EntrySubQueryRaw` type (tagged by `scraper_type: html|json`) allows entries to declare follow-up HTTP requests seeded by the entry value. Supported in both `HtmlScraperEntryRaw` and `JsonScraperEntryRaw`. Parsed into `Box<dyn ScraperQuery>` via `EntrySubQueryRaw::into_boxed_query()`.
- **Unified polymorphic executor**: `scraper::query_executor::execute_query_items` dispatches on `ScraperType` (Html/Json/Static) using the common `ScraperQuery` trait. Single execution path for all query types.
- **`ScraperRequestMethod` and `ScraperRequestHeader` made public** — now visible through the `ScraperQuery` trait return types.
- **Proxy `RemoveHeader` redirect action**: controller proxy URLs can now carry `Arachnea-Proxy-RemoveHeader` JSON action headers with `type` and `headers` fields that remove selected `opts.headers` entries when an HTTP `302` redirect is rewritten.
- **HTML group `post_build`**: Added ordered `math_formula` and `remove_fields` transformations that run after a YAML object group builds its child fields.
- **Current country IP query**: Added the `resolve_current_country` query to `arachnea-ip-countries` for direct public outbound country detection.
- **Proxy-core current country resolution**: Extended `IpCountryDataProvider` and `IpCountryResolver` so `arachnea-proxy` can resolve or receive the current outbound country.
- **Explicit current country CLI option**: Added `--current-country <ISO_CODE>` to propagate an explicit local country from `arachnea-stream` into the proxy core country resolver.
- **Proxy-core geo-country bypass**: Dynamic country proxy routing now bypasses the `dynamic-country:<CODE>` proxy pool when the requested country matches the current outbound country.
- **Geo-country bypass diagnostics**: Added structured proxy-core logs for requested country, local country source, bypass decisions, and conservative fallback when current-country detection is unavailable.
- **Dynamic country proxy fallback**: Country-routed proxy requests now continue without a geo proxy when no usable dynamic proxy is available for the requested country.
- **`math_formula` scraper action**: New action that evaluates a math expression per value. The `{value}` placeholder is replaced by the current text value before evaluation. Supports standard arithmetic via the `evalexpr` crate.
- **Server tray icon (Tauri)**: In server mode, when a graphical environment is available, `arachnea` now shows a tray icon with an English context menu — `Open <server URL>` (opens the default browser), `Show log` (dedicated log window), and `Shutdown server` (graceful HTTP shutdown and exit). It adds an in-memory log cache (last 5,000 lines) fed by `tracing`, an embedded application icon, a `--no-tray` flag, and a cooperative `ShutdownSignal` wired to graceful `warp` shutdown.
- **Windows console attach**: On Windows, `arachnea_core::application::application_init()` now attaches the process to the parent console when launched from a terminal and redirects `stdin`/`stdout`/`stderr` to it. When launched by double-click or shortcut, no console window is created. The `arachnea` binary is linked with the `windows` subsystem in release builds.

### Changed

- **Nested JSON pointer filters**: Scraper JSON pointer tokens now preserve `[...]` filter blocks containing slashes, and filter conditions can match nested paths such as `/*[content/0/idType=8]`. Previously such filters were split on `/` and only direct fields were supported.
- **Chaser-CF single-browser solver**: The chaser-cf engine no longer uses the `ChaserCF` facade. Cloudflare session solving (`solve_waf_session`) and page-source fetching (`fetch_page_source`) are implemented directly on a single process-wide shared `BrowserManager`, which is also reused by persistent page sessions. This guarantees exactly one Chrome instance, eliminating the `user-data-dir` lock conflict between the solver and page-session browsers that produced `Browser process exited with status ExitStatus(ExitStatus(0))` when a page session followed a session solve (or vice versa). Challenge handling — passive `cf_clearance` wait and Turnstile shadow-root click — is replicated in `chaser_cf.rs` using `chaser-oxide` CDP APIs.
- **Cloudflare browser fallback contract**: Browser solvers no longer fetch HTML after a rejected cookie handoff. After bounded refresh attempts, the client returns `CloudflareBlocked`; the chaser-cf engine does not use its page session as an implicit HTML fallback on that path.
- **`img.url` frontend fallback**: Media cards, entry details, live details, and page backgrounds now read the un-oriented backend `img.url` and fall back to it when no poster/portrait/landscape image is available. Oriented images keep priority; appearance is unchanged for entries that already expose oriented images.
- **Tray logic moved into `arachnea-core`**: The Tauri-based server tray implementation (menu, dedicated log window, browser action, cooperative shutdown wiring) now lives in `controler::rest::tray`, exposed as `spawn_tauri_server_tray(context, icon, configuration)`. `arachnea-stream::tray_service::ServerTrayService` now only embeds the application icon, generates the Tauri context, and delegates tray creation to the core spawner. Core still avoids `tauri-build` so only one crate emits the Windows resource.
- **Concrete tray factory moved into `arachnea-core`**: The Tauri tray factory is now `controler::rest::tray::ServerTrayIconService` in `arachnea-core`, storing the generated Tauri context at construction. `create_application_controler!` keeps its original `($options)` signature: when `CoreApplicationOptions::tray_enabled` is set, the macro builds the factory from `tauri::generate_context!()` (resolved at the call site in `arachnea-stream`). The tray uses no explicit icon and falls back to the context's embedded default window icon (from `bundle.icon` in `tauri.conf.json`). `arachnea-stream::tray_service` and the manual factory wiring in `main.rs` were removed.
- **`tray_enabled` moved into `CoreApplicationOptions`**: The tray enable/disable flag (used by `--no-tray`) moved from `arachnea-stream::RuntimeOptions` into `CoreApplicationOptions` (default `true`, builder `with_tray_enabled`).
- **Frontend action buttons as hyperlinks**: The "Voir la fiche" banner button, the "Regarder" media-card action, the clickable category tiles, the home button, and the lives button now render as real `<a>` links pointing to their internal routes (entry details, live details, category, home, lives screen), while keeping the existing SPA navigation on click.
- **Module layout**: `scraper_json/query.rs` decoupled into `config.rs`, `response_parser.rs`, `pointer.rs`, `row_extractor.rs`. Same for `scraper_html/query.rs` → `config.rs`, `response_parser.rs`, `row_extractor.rs`.
- **`ScraperManagerQuery` trait removed**: The legacy `async_trait`-based query trait and its single `impl` for `HtmlScraperQuery` are deleted. All query execution now goes through `ScraperQuery` (the unified trait).
- **Log window message column**: The dedicated log window's Message column now preserves line breaks (`\n`) without automatic wrapping. Multi-line messages show only their first line with an inline toggle to expand/collapse the full text; the table container keeps its existing scrollbar for overflow.
- **Legacy `execute_query` methods removed**: `HtmlScraperQuery::execute_query`, `JsonScraperQuery::execute_query`, `StaticScraperQuery::execute_query` — all replaced by the unified executor.
- **`mod.rs` exports cleaned**: `ScraperManagerQuery` removed from public re-exports. `ScraperManager` trait retained for runtime integration.
- **VK resolver**: New `vk.yaml` hoster resolver for `vk.com/video` pages and `video_ext.php` embeds, registered in `services.json`.
- **Server public API mount configuration**: `arachnea` now accepts `--entrypoint-root` and `--entrypoint-api` in server mode so generated stream URLs can match reverse-proxy mounts such as `/arachnea/api/proxy`.
- **YAML URL proxy option**: `resolve_url` and `resolve_url_from_parent` now accept optional `proxy: true` to wrap resolved HTTP(S) URLs through the controller-computed generic HTTP proxy path. Omitting `proxy` preserves direct URL output.
- **Static query actions**: `scraper_type: static` entries with scalar `value` now honor their `actions` pipeline, allowing metadata fields such as service logos to use `resolve_url proxy: true`.
- **M6Play manifest proxy country hint**: `proxied_url` now accepts an optional country hint. When provided, generated controller proxy URLs carry `Arachnea-Proxy-Country` through the existing `opts` header mechanism; when omitted, URL generation is unchanged.
- **M6Play manifest redirects**: M6 manifest proxy URLs now remove the `Arachnea-Proxy-Country` option after HTTP `302` redirects so the redirected manifest request no longer forces country routing.
- **M6Play MPD initialization URLs**: M6 proxied manifests now rewrite `/m6web/` initialization paths to the proxied Bedrock CDN URL so segment initialization requests stay on the controller proxy route.
- **Stream YAML output standardization**: Dark-stream home sections now emit `entries` instead of `items`, selected media metadata fields now use typed arrays/numbers, m6play category descriptions were removed from YAML and the frontend category model, and TF1 category request descriptors no longer carry the redundant `channel_label` parameter.
- **YAML stream hoster group rename**: Renamed the YAML stream resolver service group and directory to `arachnea-stream-hoster`.
- **Search automatic load-more**: `MediaCardCollection` now supports an `autoLoadMore` prop that automatically loads the next page when the load-more button scrolls into view. The media search screen enables it so additional search results load without a manual click.
- **Single-row card size**: Cards in `single-row` collections are now 50% larger (width scaled by 1.5), so posters scale up automatically via their aspect-ratio.
- **Entry details automatic load-more**: The season episode list in entry details now loads the next page automatically when the load-more button scrolls into view.
- **Poster shade removed**: The dark bottom gradient overlay on media card posters is removed so thumbnails display their image without the dark shade.
- **IndexedDB section preferences and entry bookmarks**: Home section preferences (pinned sections, collection mode, thumbnail orientation/fit) and entry bookmarks now persist in IndexedDB via new `homeSections` and `entryBookmarks` services instead of `localStorage`. `indexDB.ts` gained `deleteRecordDB`. `useStorage` exposes `initStorage`, `toggleSectionPinned`, `movePinnedSection`, `updateSectionCollectionMode`, `updateSectionThumbnailOrientation`, and `updateSectionThumbnailImageFit`; the catalog composable and `HomeCatalog.vue` now delegate section writes to these methods. `showSectionEditingButtons` stays in `localStorage`. No data migration is performed. `initDB` now closes the previous IndexedDB connection before opening the upgraded version to avoid `onblocked` when multiple stores are initialized sequentially, and starts from the database's existing version (via `indexedDB.databases()`) so a page reload does not try to open a lower version than stored. The in-memory cache is resynchronized from IndexedDB whenever the tab becomes visible again (`visibilitychange`/`focus`), so bookmarks and section preferences written from another tab are reflected in every open tab.

- **Recursive services manifest imports**: `services.json` manifests may now import other JSON manifests through `{"import": "relative/manifest.json"}` entries in addition to regular YAML sources. Imports expand depth-first in declaration order, keep each source's `path` relative to its declaring manifest, and are deduplicated to prevent cycles. A missing YAML source is skipped with a warning while a missing, invalid, or ambiguous import fails with contextual paths. `arachnea-stream` migrates to a root manifest importing `dark-stream/services.json` and `legal-stream/services.json`.

### Removed
- Dead helpers: `process_root`, `parse_html_rows`, `collect_ordered_results`, `matches` (response_parser), `resolve_request_headers`/`resolve_request_body`/`execution_options` (JsonScraperQuery), `split_static_path`, `render_static_value`, `render_yaml_value`, `render_yaml_values` (StaticScraperQuery).
- `JsonScraperSubQuery::execute` / `execute_siblings` / `execute_context` / `execute_indexed_context` / `execute_indexed_sibling` / `build_row_node` — re-used through the new `JsonScraperSubQuery::execute_query_level` unified entry point (see regression fix below).

### Fixed
- **Playmogo DoodStream hoster**: The YAML resolver now turns the `pass_md5`
  CDN base URL into the required signed media URL by adding the random suffix,
  token, and expiry timestamp before proxying it. It also declares the direct
  stream as MP4; frontend player source resolution now honors that declared
  MP4 type even when the proxied URL has no file extension. The direct MP4 no
  longer configures a body-rewrite action, allowing the proxy to stream it.
- **Chunked response streaming corruption**: `ChunkedBodyReader::read_more`
  left its zero-filled read chunk in place when the underlying stream returned
  `Poll::Pending`, so the next poll served those zeros as body data, corrupting
  and truncating streamed responses (e.g. partially loaded images through the
  public `/api/proxy` route). The buffer is now truncated on the pending path.
- **Spys.one proxy ports**: The proxy scraper now unpacks the Dean Edwards
  variable script before executing it, allowing its existing port formula to
  resolve the values rendered by Spys.one.
- **PapaDuStream v2 deferred episode players**: Season episodes now expose deferred player links; selecting one loads its actual player rows and enables playback controls.
- **PapaDuStream v2 seasons**: `get_season` now follows season links when called with a series entry URL, returning its episode list.
- **VTT storyboard cue timing**: Video.js thumbnail previews now use each WebVTT cue's declared time range and crop geometry instead of an averaged interval, preserving irregular cue durations.
- **VTT storyboard cropping**: Video.js now preserves the sprite image's intrinsic height instead of stretching it to the VTT-declared row count, preventing progressive vertical drift when trailing VTT cues exceed the image bounds.
- **TF1+ trailer extraction**: `get_entry` now returns the TF1 video page for
  `video/trailer` instead of an image preview URL, allowing the frontend to
  render the trailer as an embedded player.
- **Deferred home banners**: `get_banners` now starts only after the home
  hero mounts, allowing the catalog shell and sections to render before
  deferred banner sources are requested.
- **RTBF Auvio deferred trailers and home section loading**: `get_banners`
  trailer entries now retain their RedBee resolver through frontend
  normalization and the hero banner resolves its protected stream with the
  existing `get_stream` contract. Home sections are no longer fetched eagerly
  after `load_home`; their first page is requested by the existing visibility
  observer instead.
- **VOE HLS manifests**: VOE proxied manifests now preserve the required User-Agent and rewrite absolute playlist URLs through the inherited proxy options.
- **VOE stream hoster metadata**: The VOE YAML hoster now exposes page title, preview image, and single-frame storyboard metadata when available from the embed HTML.
- **Scraper HTTP redirect limits**: `max_redirects` is now deserialized from YAML HTTP configuration, so query-specific redirect limits are applied to the outbound client.
- **Optional storyboard interval**: `get_stream` now allows an omitted storyboard interval; the frontend derives it from video duration divided by the sprite cell count.
- **Storyboard page indexing**: `first_index` is replaced with `first_page_index` for numbered sprite files.
- **Storyboard preview sizing**: Video.js now keeps hover previews at a fixed 160×90 px while using the `get_stream` storyboard `width` and `height` values solely to crop each source sprite cell.
- **Frontend merged home sections**: sections that combine direct `load_home`
  entries with deferred `get_section` sources now load every deferred first
  page before rendering, alternate items from each source, and preserve
  per-source pagination. The `load_home` rails themselves are also ordered
  round-robin by source before equivalent rails are merged. Aggregated search
  results now use the same per-source round-robin order.
- **HTTP proxy dechunking tolerance**: buffered proxy responses now accept
  non-compliant upstream proxies that remove chunk framing but retain
  `Transfer-Encoding: chunked`. This behavior is enabled by default on
  `SimpleHttpClient` and can be disabled per client instance.
- **FranceTV HLS key proxying**: HTTPS key URLs under `/keys/*.key` are now
  rewritten through the generic HTTP proxy regardless of their host.
- **TF1 DRM license proxy**: TF1 now accepts the shared default
  `widevine-license-proxy` stream kind when forwarding Widevine challenges,
  preventing valid generated license URLs from failing with HTTP 400.
- **Scrapyfy dynamic proxy provider loading**: the provider now passes the requested country into the proxy source query, normalizes returned country codes before filtering, deduplicates by normalized authority, reports source collection load failures, and falls back to the existing `archanea-proxies` service directory when the preferred `arachnea-proxies` directory is absent.
- **HTTP proxy `ReplaceAll` post-action responses**: proxy action headers are now parsed before lossy `HashMap` merging so repeated `opts.headers` actions are preserved, invalid action JSON returns an explicit proxy error, and `Accept-Encoding: identity` is enforced case-insensitively to avoid corrupting compressed upstream bodies during text replacement.
- **`francetv.yaml` `load_home` section pagination**: Added a default `page` parameter so shared section metadata serializes `current_page` as a number instead of leaking the unresolved `{page}` placeholder on non-paginated home responses.
- **`rtbf-auvio-be.yaml` `load_home` PROMOBOX banner video shape**: RedBee banner preview extraction now keeps `banners[].video` as a single string URL instead of serializing it as an array containing an `_` object.
- **`rtbf-auvio-be.yaml` `load_home` PROMOBOX banner video proxying**: RedBee HLS banner preview URLs are now wrapped through the configured generic HTTP proxy path.
- **Frontend banner video proxy URLs**: Native video source detection now preserves same-origin proxy paths such as `/api/proxy/https://...m3u8` instead of extracting and playing the embedded upstream URL directly.
- **`anime-sama.yaml` `load_home` banners**: Carousel clone slides marked with `aria-hidden="true"` are now ignored, keeping the home banners list to the four real featured items.
- **`anime-sama.yaml` `load_home` pre-processing**: the volatile Cloudflare `window.__CF$cv$params` script injected before `</body>` is now removed through `remove_text_blocks` before parsing and before the fallback content-hash ETag fragment, so repeated home fetches yield a stable `C:` validator while the page content is unchanged.
- **`m6play-fr.yaml` `search` result grouping**: Search now extracts one root response with hits nested under `entries`, preventing repeated root `source` values from breaking typed serialization.
- **`francetv.yaml` `get_entry` season grouping**: FranceTV program details now build seasons from `collections` filtered to `type: playlist_video`, keeping each season's episodes under that season instead of flattening every playlist video item into a top-level `episode` list.
- **RTBF Auvio playback CORS**: RTBF RedBee media manifest URLs returned by the player resolver now use the same-origin `/api/proxy/` route, so the DASH MPD and relative segment requests are fetched through Arachnea while the Widevine license proxy remains on `/api/get_stream/rtbf-auvio-be/...`.
- **RTBF Auvio storyboard VTT**: RedBee player entitlements now map the first `sprites[].vtt` URL to `storyboard_vtt_url`, using the same-origin HTTP proxy path when configured.
- **RTBF Auvio home banners**: Query-level sub-query fetches now execute child sub-queries on fetched responses and resolve nested request headers/bodies from the parent response, allowing `load_home` PROMOBOX banners to include the RedBee HLS `video` URL.
- **coflix.yaml**: Updated `get_entry` query to correctly extract season labels and links from the HTML entry page.
- **coflix.yaml**: Fixed `get_season` query to correctly iterate over all episodes in the `episodes` array.
- **Coflix `get_season` context fields**: Query-level sub-queries now merge `context_entries` from each selected context row into the fetched sub-query result, restoring episode `id`, `title`, `episode-number`, and `img/preview > link` fields alongside player links.
- **Entry-level sub-query merge shape**: `execute_entry_sub_queries` now wraps each decoded sub-query value inside a named child node (`{ "embed-link": ["url"] }`) instead of pushing bare scalar items. This causes the serializer to emit an array of objects (`"players": [{"embed-link": ["url1"]}, …]`) instead of a flat string array (`"players": {"embed-link": ["url1", …]}`). Fixes the `players` output shape for `coflix.yaml` `get_entry`.
- **`rtbf-auvio-be.yaml` `list_lives` flattening**: the RTBF live catalog now declares `result_item_field: entries` and groups `/data/content/*` under `entries`, so the unified executor returns one live item per channel instead of merging the whole feed into a single result.
- **RTL Play login cookie handling**: `scrapyfy::HttpClient` now exposes materialized responses and a shared-cookie lookup for one URL, letting `rtlplay_resolver` reuse real redirect-set cookies from the HTTP jar instead of scraping `lfvp_device_id` and auth cookies from response bodies.
- **RTL Play SSO redirects via proxy HTTP**: `arachnea-http` responses now expose followed redirect target URLs, `scrapyfy::HttpClient` can run with an isolated cookie cache, and `rtlplay_resolver` uses both to recover RTL Play SSO `oidc_callback` candidates while keeping requests on the shared proxy-aware HTTP path. The rquest loopback proxy client now leaves redirects to `arachnea-http`, so intermediate SSO cookies are preserved. RTL Play logins are serialized so concurrent resolver calls do not mix SSO cookies, and the SSO flow now reuses the post-auth callback, keeps SSO same-origin headers, and completes the OIDC `authorize` redirect before reading the RTL Play auth cookie.
- **Scraper HTTP fetch diagnostics**: `scrapyfy::HttpClient` now includes the underlying HTTP/proxy error in `Fetch fail ...` messages, making proxy-dependent failures diagnosable from API logs.
- **TF1 mediainfo diagnostics**: TF1 playback now reports mediainfo HTTP status/body details and sends browser-like `Accept`, `Origin`, and `Referer` headers, improving proxy failure diagnostics without exposing auth tokens in header logs.
- **Proxy CONNECT diagnostics**: the rquest loopback proxy now logs failed CONNECT requests and proxy-hop handshake errors instead of silently dropping them, making upstream SOCKS failures visible.
- **Proxy pool transient failures**: tunnel proxy-pool selection no longer opens a destructive preflight tunnel before the real connection, compatibility checks retry a candidate once before marking it unusable, and an exhausted pool gets one fresh scan of KO members before failing, reducing false negatives from flaky SOCKS upstreams that intermittently return `network unreachable`.
- **6play front-auth login**: M6Play login token retrieval is now serialized to avoid concurrent Gigya/front-auth request storms, while still reporting HTTP status/body details when `front-auth.6cloud.fr` returns non-JSON.
- **`francetv.yaml` `list_lives` flattening**: the FranceTV live catalog now groups `/items/*` under `entries` with `result_item_field: entries`, so typed scalar fields are serialized per live item instead of receiving values from the whole feed.
- **`proxifly.yaml` proxy list flattening**: the dynamic proxy source now groups root array items under `proxies` with `result_item_field: proxies`, so one proxy row is returned per upstream JSON item instead of a single merged result.
- **Dynamic proxy selection diagnostics and probing**: dynamic proxy inventory now logs loaded/probed/eligible candidate counts, HTTP proxy probes use forward `GET` instead of `CONNECT :80`, SOCKS proxies are not rejected for HTTPS destinations solely because a source reports `supports_https=false`, and proxy country headers no longer produce unknown action-header debug noise.
- **Dynamic proxy candidate selection**: country proxy selection now follows the documented eligibility order more closely by rejecting stale probes and active destination cooldowns, preserving hard KO/auth exclusions during reloads, and using deterministic latency/failure/authority ordering for equivalent candidates.
- **Scrapyfy dynamic proxy HTTPS validation**: the default scrapyfy-backed proxy inventory now verifies HTTPS tunnelling with `https://example.com/`, and failed HTTPS probes explicitly clear source-claimed HTTPS support so HTTP-only public proxies are not selected for HTTPS sites.
- **Dynamic proxy HTTPS CONNECT routing**: HTTP `CONNECT` targets on port 443 are now treated as HTTPS destinations for country proxy selection, so public HTTP proxies without working tunnel support are excluded before TF1-style HTTPS requests.
- **Stream resolver follows simple JS redirects**: the HTML-content detection phase (`fetch_embed_html` in `stream_resolver.rs`) now follows a simple `window.location.href = "…"` string-literal redirect before running `can_resolve_html`. This lets content-based hosters (e.g. VOE on mirror domains fronted by a redirecting shim page) be recognized on their final player page instead of the shim. JavaScript is never executed and hops are bounded/validated like existing fetches.
- **Multi-arch Docker cross image (`build-release/`)**: the cross image is now
  built for **both architecture ports** (`linux/amd64` + `linux/arm64`) under
  one tag, through `docker buildx build --platform linux/amd64,linux/arm64` —
  which requires the Docker containerd image store ("Use containerd for
  pulling and storing images"); previously the single-platform `docker build`
  silently replaced the tag, so fixing one Linux target broke the other. With
  the classic image store, the tooling falls back to a single-variant build
  with an explicit warning. A missing variant is no longer a hard stop:
  `assertCrossImageFor` now offers to build it on the spot (TTY confirmation,
  duration under QEMU emulation disclosed) and only fails with the updated,
  store-aware instructions when declined or non-interactive. `ensureCrossImage`
  (first `install-tools` run) produces both ports as well; the confirmation
  prompt discloses the emulated arm64 build time. No Dockerfile change: the
  image tag stays `arachnea-cross-builder:1.0.0`.

## Unreleased — RTBF Auvio home banner RedBee auth simplification

### Fixed
- **`rtbf-auvio-be.yaml` `load_home` PROMOBOX banners**: RedBee anonymous auth for banner preview video no longer depends on scraping the embed page `deviceId`. The query now posts a stable anonymous WEB device payload after extracting only the `assetId`, which keeps `banners > video` populated even when the embed state shape changes.
- **Nested entry sub-queries on fetched responses**: `query_executor` now continues executing entry-level sub-queries after a fetched HTML/JSON sub-query populates its fields, and it preserves scalar results when a nested sub-query resolves back into the same field name. This restores RTBF Auvio PROMOBOX `video` extraction and makes `row_filters` apply on fetched JSON entry sub-queries as expected.

## Unreleased — query-level sub-query regression fix

### Added
- **`ScraperQuery::as_any()`** — returns `&dyn Any` so the unified executor can downcast back to the concrete query type when a query-level sub-query (legacy semantics) is encountered.
- **Query-level sub-query detection on the trait** — `ScraperQuery::context_pointer()`, `context_select()`, `context_entries()`, `target()` are now part of the trait (default `None`/empty). `JsonScraperSubQuery` and `HtmlScraperSubQuery` override them to expose their legacy fields.
- **`JsonScraperSubQuery::execute_query_level`** — unified entry point that reuses the legacy `execute` path for the `context_pointer`/`filters`/`row_filters`/`request_actions`/`target` semantics.
- **`query_executor` query-level dispatch** — when a sibling sub-query reports `context_pointer().is_some()`, the unified executor downcasts to `JsonScraperSubQuery` and calls `execute_query_level` against the parent row, instead of recursing into the empty-URL path.
- **Helpers in `query_executor`**: `item_to_parent_value` and `query_level_execution_options` to bridge the executor to the legacy `JsonScraperSubQuery` execution path.

### Fixed
- **Regression on query-level sub-queries** introduced when the unified executor was wired in (Step 16). YAMLs using the legacy `sub_queries` block at query level with `context_pointer`/`target`/`request_pointer` (e.g. `coflix.yaml`'s `get_season`, `rtbf-auvio-be.yaml`, `m6play-fr.yaml`, `tf1-fr.yaml`) used to be executed by the old per-query `execute` method and were silently broken by the new engine. The unified executor now dispatches them to the legacy path, restoring the previous behaviour without requiring a YAML migration.
- **`m6play-fr.yaml` `load_home` follow-up** — the follow-up sub-query has no `context_pointer` (single-context mode). The unified executor now dispatches **every** `JsonScraperSubQuery` sibling through `execute_query_level` regardless of whether `context_pointer` is `Some` or `None`, not only when it is `Some`. Previously only the `coflix` pattern (`context_pointer: /episodes/*`) was correctly dispatched; the `m6play-fr` pattern (no `context_pointer`, `target: banners`) fell through to the broken entry-level recursion.
- **`as_html_entry` / `as_json_entry`** in `query_executor` now use `entry.as_any().downcast_ref::<…>()` (matching the `as_any` plumbing added to `ScraperEntrySpec` in Step 15). This unblocks the polymorphic dispatch of entries to their concrete types.

## Unreleased — static query regression fix

### Fixed
- **Silent regression on `StaticScraperQuery`**: the unified executor's `FetchedResponse::Static` branch in `extract_items` was downcasting `&dyn ScraperEntrySpec` to `&JsonScraperEntry`, which always failed for the actual `StaticScraperEntryRaw` instances. The result was an empty `ScraperDataNode` for every static query — observable on `m6play-fr.yaml`'s `service_stream_metadata` (`get_service` returning `{}` instead of `{ id, title, logo, description }`) and on every other YAML using `scraper_type: static`. Fixed by adding `StaticScraperEntryRaw::apply_to` (concrete method, mirrors the legacy `StaticScraperQuery::execute_query` template pipeline using the already-present `split_static_path` / `render_yaml_value` / `render_yaml_values` helpers) and a dedicated downcast branch in `extract_items` that calls it. `extract_items` now returns `Result<Vec<ScraperDataNode>>` to surface unresolvable template placeholders instead of silently producing empty items.

## Unreleased — TF1 FR proxy routing

### Added
- **Scraper HTTP proxy country hint**: `ScraperHttpConfig::proxy_country(...)` now normalizes country values and passes them through the HTTP proxy parameter path so resolvers can request country-specific proxy routing without replacing the existing `create_http_client(...)` flow.
- **Catchable HTTP proxy errors**: `arachnea-http` now exposes `ArachneaHttpError::Proxy` for configured proxy transport failures, allowing resolvers to distinguish proxy failures from upstream service failures.

### Changed
- **TF1 resolver FR routing**: TF1 resolver HTTP calls now request `proxy_country("FR")`, including license proxy calls. When that FR proxy path fails, TF1 logs a warning and retries without the FR country hint as a temporary workaround.
- **Stream proxy configuration**: `arachnea-stream` now configures the FR proxy with `ProxyNode::from_url("proxy-fr", "socks5://...")` and a `country_routing` handler instead of making the SOCKS proxy the default route for all HTTP traffic.

## Unreleased — chaser-cf request header forwarding fix

### Fixed
- **`chaser-cf` custom request headers**: browser-backed page fetches now propagate per-request headers to Chrome before navigation, and `Referer` now uses Chrome's native navigation referrer field instead of an injected extra header. The Cloudflare refresh path also reuses the blocked request headers, so solver refreshes keep request-specific headers such as `Referer`. The explicit `chaser-cf` feature build also no longer fails due to missing `custom_headers` arguments.
- **`chaser-cf` referer observability**: when a navigation expects a `Referer`, the engine now logs the `observed_referer` and Chrome referrer policy reported by the outbound CDP request event, making it easier to distinguish a missing header from an upstream 403.

## Unreleased — HTTP proxy transport configuration

### Added
- **Built-in HTTP engine proxy configuration**: `ArachneaHttpConfig` now exposes `HttpProxyConfig`, `proxy_url(...)`, and, with the `arachnea-proxy` feature enabled, `proxy_core(...)` so built-in HTTP engines can route traffic through an explicit network proxy or an in-process `arachnea-proxy` core.

### Changed
- **Built-in engine proxy handoff**: `rquest` and Ghostwire now accept the configured proxy transport from `ArachneaHttpConfig`; when `proxy_core(...)` is used, `arachnea-http` starts a managed `arachnea-proxy` loopback helper because the upstream client APIs used here still expect proxy URLs.
- **ScraperManager default proxy bootstrap**: `StreamScraper` now enables the shared scraper HTTP proxy by default, so lazily created `scrapyfy::HttpClient` instances inherit the in-process proxy transport without requiring per-service wiring.
- **Documentation and tracking**: `arachnea-http` README now documents proxy transport selection, and the completed HTTP proxy task has been removed from the root `docs/TODO.md`.

## Unreleased — scrapyfy invalid JSON payload logging

### Fixed
- **Scraper JSON parse diagnostics**: `arachnea-scrapyfy::HttpClient` now logs the raw response content that failed JSON parsing, including `__NEXT_DATA__` payloads, with truncation metadata so API errors like `Invalid JSON payload returned by ...` can be diagnosed from backend logs without changing the error sent to the frontend.

## Unreleased — scrapyfy typed output contract

### Added
- **YAML output typing**: scraper entries now declare their JSON output shape with `type` (`string`, `number`, `boolean`, `object`, and array variants). Missing declared fields serialize as `null`, and failed casts report contextual errors.
- **Typed `ScraperDataNode` serialization**: scraper output nodes now render directly to `serde_json::Value`, including object/object-array handling for explicit groups and aligned implicit groups.

### Changed
- **Service YAML migration**: service definitions under `server/services/**/*.yaml` now declare output types and use `select: first` for scalar fields where needed. Pagination/count/rating fields are numeric, booleans are boolean, and duration remains string.
- **Frontend response normalization**: `rustify.ts` now consumes typed numbers and booleans directly, reads service descriptions as typed objects, and expects `get_entry` to return one object instead of a single-element array.
- **Static metadata test expectations**: service metadata descriptions are now validated as object fields such as `description > fr`.

### Fixed
- **Regex/post-process generated output**: generated fields now carry explicit output types, and implicit nested groups with aligned repeated children serialize as arrays of objects.

## Unreleased — M6 Play home banner logo typing

### Fixed
- **Targeted sub-query row cardinality**: query-level sub-queries with a `target` now append each returned row as a separate object item instead of merging all rows into one object. This preserves `object[]` output shape for cases such as `m6play-fr.yaml` `load_home` banners.
- **`m6play-fr.yaml` `load_home` banner fields**: banner scalar extraction now keeps the first value per program, matching the declared `type: string`/`number` output contracts and avoiding typed serialization failures when 6play returns multiple banner values.
- **`m6play-fr.yaml` `load_home` banner request URL**: the banner follow-up query now uses one scalar request context before applying its constant URL action, avoiding repeated fetches of the same five banners.
- **`get_section` scalar pagination fields**: section rows are merged with first-scalar semantics and scalar leaves are collapsed after merge, so flattened entry rows no longer duplicate fields such as `current_page`.
- **`m6play-fr.yaml` `get_entry` year field**: detail pages now keep one numeric year value when both copyright and production years are present.
- **`m6play-fr.yaml` `get_season` preview images**: episode preview fallback image extraction now keeps one image URL per episode, matching the scalar `img/preview > link` output contract.
- **`rtlplay-be.yaml` `get_season` pagination fields**: season pagination fields are now emitted once per response while episodes are collected through the `episodes` object array, avoiding repeated scalar booleans such as `have_more`.
- **`rtlplay-be.yaml` `search` pagination fields**: search pagination fields are now emitted once per response while results are collected through the `entries` object array, avoiding repeated scalar booleans such as `have_more`.
- **`tf1-fr.yaml` `load_home` video tags**: video slider tags are now emitted as a string array, matching the multiple tag slugs returned by TF1.
- **`tf1-fr.yaml` `get_category` section link**: category section links now use the formatted GraphQL URL instead of serializing the whole response object through `get_request_url`.
- **`tf1-fr.yaml` category pagination**: category and section requests now send TF1 GraphQL item offsets, first-page links start at offset `0`, and the page size matches TF1's 10-item responses so `have_more` can be derived from the actual paged result count.

## Unreleased — Proxy post-response actions

### Added
- **Post-response HTTP action system**: `arachnea-proxy` now supports pluggable post-response actions that run after the upstream response is received. Actions can modify status, headers, and body before the response is returned to the caller.
- **`ReplaceAll` action**: Replaces all regex occurrences in textual responses (text/*, application/json, application/javascript, image/svg+xml, etc.). Text encoding is detected from `charset` parameter, then UTF-8, with fallback to Windows-1252/ISO-8859-1 via `encoding_rs`.
- **Action header transport**: Actions are carried via `Arachnea-Proxy-ReplaceAll` headers (JSON value with `order`, `pattern`, `replacement`). Multiple actions of the same type are supported via the array-of-pairs format in `opts/headers`. Action headers are consumed by the proxy and never forwarded upstream.
- **`Accept-Encoding: identity` enforcement**: When text-transforming actions are present, the proxy forces `identity` encoding to avoid dealing with compressed bodies.
- **Action ordering**: Each action supports an optional `order` field (lower runs first). Actions without `order` execute after ordered ones, preserving discovery order.
- **Dependencies**: `regex` and `encoding_rs` added to `arachnea-proxy`.

### Changed
- **`ProxiedHttpRequest`**: Added `post_actions: Vec<ProxyHttpPostActionConfig>` and `headers_only: bool` fields.
- **`parse_http_response`**: Now accepts `headers_only` and `post_actions` parameters; applies actions at the designated extension point after chunked decoding, then fixes `content-length` and removes `etag`/`content-md5`/`digest` when body changes.
- **`proxied_url`**: Added `actions: &[ProxyHttpPostActionConfig]` parameter. All existing callers updated with `&[]` — no behavioural change for existing usage.
- **`handle_proxy_http`**: Now parses action headers from the request headers and passes them through to the HTTP client.

## Unreleased — FranceTV HLS key proxying

### Fixed
- **FranceTV HLS AES-128 keys**: FranceTV proxied manifests now rewrite `cloudreplay.ftven.fr/keys/*.key` `#EXT-X-KEY` and `#EXT-X-SESSION-KEY` URIs through the generic HTTP proxy without a country hint, while keeping the manifest request itself routed through France.
- **Proxy text action filtering**: `ReplaceAll` now accepts an optional per-action `content_types` allowlist. FranceTV key rewrites use it for HLS MIME variants such as `application/x-mpegurl` without changing the default proxy text filter.
- **M6Play MPD URL rewrites**: M6Play manifest replacements now use the proxy action `{proxy}` placeholder instead of embedding the resolved proxy path in the response sent to the client.
- **Proxy action `{proxy}` placeholder**: `{proxy}` now resolves to the public proxy path such as `/api/proxy` instead of the absolute local entry-point URL.
- **Proxy action `{base_url}` placeholder**: `{base_url}` now resolves to the target origin such as `https://myhost.be:8080`, without the target path or query string.

## Unreleased — dynamic proxy loading analysis clarifications

### Changed
- **Dynamic proxy loading analysis**: Clarified the `ProxyAvailabilityHint` contract, static/dynamic pool coexistence, strict HTTP/HTTPS probe URL requirements, runtime destination validation with per-origin cooldowns, synchronous bounded IP-to-country refresh behaviour, and the recommended CLI exposure for the refresh command.
- **Dynamic proxy loading analysis**: Refined destination-failure retention so a proxy reaches global KO after 10 active per-origin failures and clears the persisted destination-failure list.

### Added
- **Step 1 — Dynamic proxy data contracts**: Added `ProxyRecord`, `ProxyProtocol`, `ProxyRuntimeStatus`, `ProxyAvailabilityHint`, `ProxyDestinationFailureReason`, `ProxyDestinationFailure`, `ProxyLoadRequest`, and the `ProxyDataProvider` trait in `arachnea-proxy/src/core/proxy_record.rs`. `ProxyRecord` supports serde serialisation with custom `SystemTime` epoch-millis encoding. A `try_to_node()` conversion produces a `ProxyNode` when the protocol is known, with automatic IPv6 bracketing.
- **Step 2 — Explicit proxy probe API**: Added `ProbeConfig`, `ProbeMode`, and `ProxyProbe` in `arachnea-proxy/src/core/proxy_probe.rs`. The probe measures TCP latency, tests HTTP CONNECT forwarding, SOCKS5/SOCKS4 handshakes, HTTPS tunnelling, and detects authentication requirements, updating `ProxyRecord` in place without a separate report structure. Protocol detection iterates through a configurable order for records without a known protocol. Runtime destination validation against real targets is supported separately from context-free probing.
- **Step 3 — Runtime proxy inventory**: Added `ProxyInventory`, `InventoryConfig`, and `CoexistencePolicy` in `arachnea-proxy/src/core/proxy_inventory.rs`. Stores dynamic proxy records with a country index, selects the best candidate by latency with cooldown/status/HTTPS filtering, supports lazy loading per country via `ProxyDataProvider` with a loading guard and negative cache, tracks per-destination failures with cooldown, and marks proxies globally KO when the destination-failure threshold (default 10) is reached.
- **Step 4 — Proxy persistence store**: Added `ProxyStore`, `ProxySerdeCodec`, `ProxySerdeStore`, and `JsonProxyCodec` in `arachnea-proxy/src/core/proxy_store.rs`. Proxy records can now be loaded from and saved to a serde-backed file store with JSON as the default codec and atomic `*.tmp` writes followed by rename. `ProxyInventory` can be created with a store, loaded from it, saved to it, and automatically persists after lazy loads and runtime state updates.
- **Step 5 — Scrapyfy proxy provider skeleton**: Added `ScrapyfyProxyDataProvider` behind the `arachnea-proxy` feature in `arachnea-scrapyfy`. The provider is currently a placeholder whose `load_proxies` returns an empty vector with a TODO, and `default_scrapyfy_proxy_inventory()` wires this provider with the default probe for future dynamic proxy loading.
- **Step 5.b — Dynamic country routing runtime wiring**: Added `DynamicCountryRoutingProxyHandler` in `arachnea-proxy/src/core/parameters.rs` that emits `ProxyPool` markers (`dynamic-country:<CODE>`) resolved at connect time through `ProxyInventory`. Added `ProxyInventory` support to `ArachneaProxyCore` with `from_resolved_with_proxy_inventory()` and `with_proxy_inventory()` constructors. Dynamic pool markers are resolved in `resolve_proxy_pools_in_chain()` via `ProxyInventory::select()`, respecting `CoexistencePolicy`. Added `default_scrapyfy_proxy_core()` in `arachnea-scrapyfy` that builds a pre-wired proxy core with the dynamic handler — `arachnea-stream` now calls this single function instead of assembling the configuration inline.
- **Step 5.c — Scrapyfy proxy data provider implementation**: Replaced the placeholder `ScrapyfyProxyDataProvider::load_proxies()` with working logic that loads proxy data from the YAML-defined `proxifly/free-proxy-list` source, filters by requested country, normalises country codes, and deduplicates by authority. Added the `arachnea-proxies` query collection service registry with `proxifly.yaml` as the initial source.
- **Step 6 — IP-to-country resolution**: Added `IpCountryRecord`, `IpCountryDataProvider`, `IpCountryStore`, `IpCountrySerdeCodec`, `IpCountrySerdeStore`, and `JsonIpCountryCodec` in `arachnea-proxy/src/core/ip_country.rs` for loading, persisting, and retrieving IP geolocation data. Added `ScrapyfyIpCountryDataProvider` in `arachnea-scrapyfy` that resolves IPs to country codes via the YAML-defined ip-api.com query. Added `resolve_ip_direct()` for direct HTTP-based single-IP lookups, `refresh_ip_country_store()` for YAML-based batch refresh, and `refresh_ip_country_store_direct()` for HTTP-based batch refresh. Created the `arachnea-ip-countries` query collection with `ip-api.yaml` as the initial IP geolocation source. Exposed the refresh via `--refresh-ip-countries` CLI flag in `arachnea-stream`.

## Unreleased — stream resolver contract migration

### Changed

- **Stream resolution API**: Replaced the JSON command `resolve_player_stream` with `get_stream({ resolver, target })`; legal and YAML-driven resolvers now share this public entry point.
- **DRM license proxy API**: Renamed the binary command and generated license paths from `get_stream` and `/api/get_stream/...` to `get_drm_license` and `/api/get_drm_license/...`, without a compatibility route.
- **Legal player resolvers**: M6+, RTL Play, RTBF Auvio, TF1+ and FranceTV are selected through globally unique resolver identifiers; redundant YAML `resolverStreamKind` values were removed and each resolver uses its internal default.
- **DRM license public path**: `PlayerResolverEndpoints` now receives the controller-generated `get_drm_license` public path, so generated license URLs honor a configurable API mount instead of hardcoding `/api/`.
- **DarkStream player resolution**: Anime-Sama, Coflix and FrenchAnimes now describe external players with `stream-resolver` and `target`; the frontend resolves the `get_stream` video/iframe union, prefers VTT thumbnail metadata over a storyboard, and tries returned media URL alternatives after Video.js emits a source error.

## Unreleased — proxy streaming hardening

### Fixed

- **Proxy response streaming**: HTTP proxy responses without post-actions now remain streamed through the REST controller instead of being fully buffered or silently replaced with an empty body.
- **Truncated upstream responses**: Incomplete HTTP headers and truncated `Content-Length` or chunked response bodies now fail with protocol or I/O errors instead of being accepted or spinning indefinitely.
- **HTTP transfer encoding**: Composite `Transfer-Encoding` values containing `chunked` now select the chunk decoder correctly.
- **Tauri streamed responses**: The Tauri backend now collects streamed response bodies in memory via `block_on` + `StreamExt::next()`, and returns a `502 Bad Gateway` if the stream errors, instead of returning `501 Not Implemented`.
- **Streaming selector function**: `post_actions_require_identity_encoding` renamed to `post_actions_require_body_buffering` with clarified semantics — body buffering is required only when post-actions need to modify the body, and forces `Accept-Encoding: identity`; streaming mode preserves upstream compression.

## Unreleased — proxy buffering decision ownership

### Fixed

- **Proxy buffering decision**: `proxy_service` now decides whether post-actions require buffering and passes that explicit decision to the HTTP client. Any post-action conservatively selects buffered, identity-encoded handling until it explicitly supports streaming.

## Unreleased — FrenchAnimes player resolver shape

### Fixed

- **`frenchanimes.yaml` `get_entry` players**: Emit `web-link` and the structured `resolver.kind`/`resolver.target_id` fields expected by the stream resolver.
- **Nested aligned scraper fields**: Serialize nested values such as `players[].resolver` at the matching player index, preventing multi-value string serialization errors.

## Unreleased — generic stream resolver metadata

### Fixed

- **YAML stream resolver metadata**: Emit Sibnet storyboard metadata with a five-second interval, and expose its page title and title-image link.
- **Multi-image storyboards**: Resolved storyboard metadata now declares rows per sprite image. Sibnet uses its `{index}` JPEG template and six rows; M6Play explicitly declares one row.
- **Sibnet storyboard indexing**: Sibnet now declares `first_index: 1`; the frontend converts it to the Video.js `idxTag` callback so the first requested sprite is `…_1.jpg`.
- **Resolved stream response**: `stream_headers` is no longer serialized by `get_stream`; headers remain embedded in generated proxy stream URLs.
- **Resolved stream image metadata**: `get_stream` now returns the YAML-compatible `image/title > link` shape as the JSON key `image/title` with its `link` child.
- **Resolved player posters**: The frontend now uses `image/title.link` returned by `get_stream` as the selected player's poster, before falling back to episode or entry artwork.

## Unreleased — VidHide stream resolver

### Added

- **`unpack_packer` Scrapyfy action**: Safely unpacks literal Dean Edwards Packer blocks without executing JavaScript, supporting radices 2 through 62.
- **VidHide resolver**: Added the enabled YAML HLS resolver for `minochinos.com/embed/*`, including stream Referer, page title, proxied poster extraction, and `get_slides` VTT thumbnails.
- **Storyboard VTT contract**: Renamed the public `get_stream` thumbnail field from `vtt_url` to `storyboard_vtt_url`; the frontend continues to prefer it over sprite storyboard metadata.
- **Storyboard WebVTT previews**: The frontend now converts `#xywh` VTT cues into Video.js sprite thumbnail options instead of passing the VTT document as an image URL.
- **Storyboard WebVTT proxying**: `resolve_url` now supports declarative `proxy_replace_all` actions that rewrite proxied textual responses through the controller HTTP proxy, used by the VidHide storyboard VTT resolver.
- **LuluStream resolver**: Added the enabled YAML HLS resolver for `luluvdo.com` and `lulustream.com` (`.com`, `.to`). Handles Dean Edwards Packer-encoded JWPlayer setup via `unpack_packer`, stream Referer, proxied HLS source, title from `<title>`, and poster from `meta[name='og:image']`.
- **Proxy URL headers**: `resolve_url` and `resolve_url_from_parent` now accept declarative `proxy_headers`, embedding upstream HTTP headers in generated proxy URLs without exposing resolver-only stream metadata.

## Unreleased — UQLoad storyboard metadata

### Added

- **UQLoad resolver storyboard**: `uqload.yaml` now extracts the Packer-declared `/vtt/` thumbnail track through the HTTP proxy, rewriting its relative sprite references; the current sprite is a 5 × 5 grid of 200 × 112 px thumbnails.

## Unreleased — VidHide poster URL

### Fixed

- **VidHide resolver poster**: The proxied `image/title` URL now removes the `_xt` thumbnail suffix before `.jpg`.

### Added

- **`input_html` field for HTML scraper queries**: New optional `input_html` template on `scraper_type: html` queries that provides HTML content directly, bypassing the HTTP fetch. The template (typically `"{html}"`) is resolved with runtime parameters. When resolved to a non-empty string, the executor creates a synthetic `FetchedResponse::Html` from it instead of calling `fetch_responses`. Falls through to normal HTTP fetch when the template resolves empty. See `docs/specifications/arachnea-scrapyfy-en.md` and the design analysis at `docs/dev-tracking/vstream-resolver-content-detection-analysis.md`.
- **VOE HTML content detection**: Added optional `html` runtime input and `can_resolve_html` to `voe.yaml`, recognizing embedded VOE URLs on `voe.sx`, `voe.com`, `voe.ru`, `voesx.sx`, `voesx.com`, and `voesx.ru`. `resolve_stream` now accepts in-memory HTML via `input_html` while preserving the direct URL fetch path when `html` is empty.
- **VOE HTML marker detection**: Kept `can_resolve_html` restricted to explicit VOE or known VOE mirror domains in the URL or HTML to avoid false positives from generic encoded JSON script markers on unrelated hosts. The mirror whitelist now covers the observed redirect chain domains such as `jessicayeahcatch.com`, `pamelachangemission.com`, and `ellenpoliticalfollow.com`.

### Changed

- **`ScraperQuery` trait**: Added `fn input_html(&self) -> Option<&str>` with a default `None` implementation. `HtmlScraperQuery` overrides it to expose the configured template. All existing query types (JSON, Static, Text, sub-queries) are unaffected.
- **Stream resolver**: Replaced `aggregate_can_resolve` with sequential resolver selection in `services.json` order. `can_resolve_url` is now an optional direct-resolution prefilter rather than a mandatory contract: services with a positive result get a direct `resolve_stream` attempt, while services without a positive URL match are left for the shared HTML-content phase. This avoids repeated GETs against the same embed URL for unrelated services. Added `source_names_in_group()` to `ScraperAgregator` to enumerate source names in insertion order.
- **Stream resolver HTML fallback**: After direct `resolve_stream` attempts fail, the resolver now fetches the embed page once with the central scraper HTTP client, rejects non-`text/html` responses and bodies above 1 MiB, runs optional `can_resolve_html` queries in `services.json` order with `{url, html}`, and calls the recognized service's `resolve_stream` with the same in-memory HTML.
- **Redirect handling**: Raised the HTTP client's default redirect limit to 16 and aligned VOE plus the shared stream-resolver HTML fallback fetch with the same 16-redirect limit.
- **Stream resolver tests**: Reworked resolver tests to use isolated YAML service fixtures instead of the full `StreamScraper` service path. Added coverage for missing `resolve_stream` queries, first valid resolver priority, one-shot HTML fallback fetching, in-memory VOE domain detection, and embed-link fallback when no YAML recognizes the fetched document.
- **Stream resolver integration validation**: Extended the HTML fallback test to validate local integration behavior for HLS proxying, encoded `Referer` proxy options, `hls` manifest metadata, single HTML fetch, and absence of raw HTML or resolver headers in the serialized public stream JSON.

- **TF1 deferred home covers**: TF1 load_home now returns the covers endpoint as a deferred banner link. A single get_banners query reads both program and video covers, replacing the two blocking cover subqueries.

- **Deferred RTBF and M6 banners**: M6 Play and RTBF home banners are now loaded through get_banners; RTBF promobox RedBee authorization is exposed through get_players.

### Fixed

- **Deferred banner sources**: The frontend now preserves every deferred `source`/`link` pair, displays initial banner entries immediately, and appends each successful `get_banners` response as it arrives.

## Unreleased — Source-scoped scraper errors

### Added

- **Structured scraper execution errors**: Added shared `ScraperAggregationResult<T>`, `ScraperExecutionError`, and `ScraperErrorOrigin` contracts plus thread-safe `ARACHNEA_E{millis}{sequence}` correlation codes.
- **Aggregator contract coverage**: Added tests for successful aggregation order, partial and complete source failures, pre-execution errors without a source, nullable sources, and correlation-code formatting.

### Changed

- **Source failures no longer discard successful rows**: `ScraperAgregator::execute_query_async` now returns successful data together with ordered per-source errors, logs each failure with its correlation code, and reports missing groups or unmatched source selections as source-less errors.
- **Stream JSON command responses**: All `StreamScraper` JSON commands now return `{ data, errors }`, preserving each command's former payload shape inside `data`; `get_section`, `get_banners`, and `get_players` merge only successful rows while retaining errors.
- **Frontend scraper response boundary**: `call_api` now validates and unwraps the shared envelope for REST and Tauri, logs and queues structured backend errors, and synthesizes browser-local correlation codes for transport or protocol failures.
- **Source error notifications**: Added a route-independent, non-modal notification stack with localized source-aware summaries, copyable diagnostic codes, collapsed technical details, and Escape dismissal.
- **Concurrent deferred loading**: Section pages now retain fulfilled source results when another concurrent source rejects; deferred banner workers continue after an independently notified technical failure.

## Unreleased — Browser page fetch sessions

### Added

- **Reusable browser page fetch primitive**: `arachnea-http` now exposes origin-scoped browser sessions and `ArachneaHttpClient::page_fetch`, allowing supported browser engines to navigate a source page, capture an application Turnstile callback token in memory, and execute an authenticated same-page JavaScript `fetch()`.
- **Browser page click primitive**: `ArachneaHttpClient::page_click` now invokes a CSS-selected page control and returns rendered HTML after a result selector appears, preserving site-owned CAPTCHA callbacks and same-page AJAX behavior.
- **Browser navigation without Cloudflare clearance**: Persistent page operations now accept stable, non-interstitial HTML that does not set `cf_clearance`, allowing application-level CAPTCHA flows to run after navigation.
- **Faster browser page interactions**: Browser operations that only need a navigable page now use a short source-stability window before clicking, while full HTML collection retains the longer stability check.
- **Retained Chaser-CF sessions**: A retained browser page now owns its `BrowserManager`, preventing temporary configured clients from closing Chrome and breaking subsequent player resolutions.
- **Shared configured browser clients**: Scrapyfy configurations that only differ in browser execution settings now reuse the same underlying HTTP client and Chaser-CF engine when their transport, browser profile, and proxy route match.
- **Query-scoped browser cleanup**: Root Scrapyfy queries now always close retained origin sessions after all sub-queries complete; no YAML lifecycle option is required.
- **chaser-cf persistent page support**: chaser-cf retains the browser page across navigation and fetch, hands browser cookies plus the observed user-agent back to the shared HTTP cache, and closes retained pages on invalidation or eviction.
- **Structured page-fetch contracts**: Added navigation/fetch request and response types, explicit unsupported-engine behavior, configurable token-rejection classification, and unit coverage for session reuse, token insertion, and cookie/UA handoff.

## Unreleased — PapaDuStream browser player resolution

### Changed

- **PapaDuStream v2 players**: `get_players` extracts delayed-player fields through direct HTTP, then resolves each player through the generic browser `page_click` sub-query. It invokes the site's player control, lets the site render and resolve its CAPTCHA plus AJAX request, and returns the resulting `iframe[src]` as `embed-link` while preserving player name and language.
- **HTML sub-query request bodies**: HTML entry sub-queries now support the existing `request_body_actions` pipeline, enabling generic form-body construction without source-specific Rust code.

### Added

- **Frontend recoverable error on player resolution failure**: `loadDeferredPlayers` catch now shows a localized `entry.playerResolutionFailed` message instead of raw `String(error)`. Episode selection and player/lang selectors are preserved on error.
- **getxfield HTML fixtures**: Created `server/mock_data/papadustream_v2-get_players.html` and `server/mock_data/papadustream_v2-getxfield_response.html` for future scrapyfy integration tests.
- **Browser page-fetch mock engine variants**: Added `ConsumingPageEngine`, `RejectingPageEngine`, and `FailingPageEngine` in `arachnea-http` client tests covering token consumption, token rejection with session preservation, and non-token error triggering fresh session attempts.
- **Browser session token-cache unit tests**: 5 new tests in `browser.rs` for the handle-level `cached_turnstile_token`, `cache_turnstile_token`, `clear_turnstile_token` lifecycle and origin isolation.

## Unreleased — Domain-scoped browser token cache

### Added

- **Opt-in browser callback-token reuse**: `browser_token.cache_scope: domain` now retains one opaque callback token in memory for the matching origin, browser profile, and proxy route. The token is evicted on rejection, session invalidation, eviction, or closure and is never persisted or logged.

### Fixed

- **PapaDuStream v2 `get_entry` season fallback**: Added two fallback `seasons` entries for season-specific variant pages: `.saisontab a.th-hover` for linked seasons, and `.saisontab :not(a) > .thumb` for the current non-linked season (rendered as a plain `<div.thumb>` without an `<a>` wrapper).

### Added (2026-07-26)

- **`exec_js` Scrapyfy action**: Added `boa_engine` 0.21 dependency and a new `exec_js` action that executes JavaScript in a sandboxed engine. Designed for Spys.one port decoding: after execution, the action returns new global numeric variables as `Name=Value` lines. Controlled by `timeout_ms` (default 500ms) with a `loop_iteration_limit` on the Boa runtime to prevent infinite loops. All 8 unit tests pass.

## Unreleased — Sub-path frontend deployment

### Changed

- **`<base href>` marker replacement**: `front/index.html` now ships a `<base href="{base}">` marker. The shared `replace_html_base` helper in `arachnea-core` substitutes the marker with the runtime mount base before serving: `/{entrypoint_root}/` (or `/`) in the REST controller and `./` in the Tauri controller. The replacement is applied only to HTML assets that actually contain the exact `href="{base}"` marker, so no other asset is rewritten.
- **Frontend base resolution**: `front/src/services/baseUrl.ts` now derives the app base from `document.baseURI` (the injected `<base>`), so the Vue Router base, REST API base, and locale paths stay consistent with asset loading on deep History-API routes and under arbitrary entry-point sub-paths. Falls back to resolving Vite's configured base when no servable `<base>` is present.

## Unreleased — Structured log window

### Changed

- **Structured log cache**: `arachnea-core` log capture no longer mirrors free-form ANSI text into the shared cache. `LogCache` now stores structured `LogRecord`s (`timestamp`, `level`, `target`, `message`) built by a dedicated `LogCaptureLayer`; the console `fmt` layer keeps its original colored text output on `stdout`. The old `LogCacheWriter`/`LogCacheMakeWriter` ANSI duplication was removed.
- **`get_logs` structured contract**: The `get_logs` Tauri command now returns `Vec<LogRecord>` instead of `Vec<String>`.
- **Structured log window rendering**: The dedicated `Show log` window now renders the records as a table (`Date`, `Niveau`, `Chemin`, `Message`) with CSS-colored level badges (ERROR/WARN/INFO/DEBUG/TRACE) and no visible ANSI codes. The page reads the snapshot once, then receives live records through the new `subscribe_logs` Tauri command (a `tauri::ipc::Channel`), replacing the 1 s polling. Subscribers are cleared when the log window closes (`on_window_event`).

## Unreleased — Frontend player language persistence and error notification

### Changed

- **Player language preserved across video changes**: The entry video player now restores the preferred language (and the preferred player within that language) when switching playable items, matching the existing player-selection persistence. The language falls back to the preferred player's language when no explicit language preference exists.
- **Error notification code moved into technical details**: The correlation-code copy button now lives inside the collapsed "Détail technique" disclosure instead of the notification body.
- **Error notification width**: The error notification popup is now wider (`480px` instead of `390px`).

## Unreleased — Bookmark keys use the entry route token

### Changed

- **Entry bookmark keys now use the base64url route token**: `buildBookmarkKey` in `front/src/services/entryBookmarks.ts` now derives the IndexedDB primary key from `encodeEntryRoutePayload({ source, entryUrl })` instead of `JSON.stringify([source, entry])`. The bookmark key is therefore identical to the `encodedEntry` segment of the `/entry/:encodedEntry` route, so the card click token matches the stored record key. No migration is performed for previously persisted bookmarks.

## Unreleased — Uniform thumbnail sizing across grid and single-row modes

### Added

- **Configurable thumbnail size multiplier**: Added a persisted `thumbnailSizeMultiplier` parameter (default `1`, range `0.25`–`1.5`, step `0.05`) exposed via a new slider control in the display settings panel.

### Changed

- **Grid and single-row widths now share a common base**: Media card collections apply `--media-card-collection-size-multiplier` to the base column width (`176px`) for portrait cards and the landscape width (`370px`, derived from two grid columns plus the gap). The single-row layout no longer inflates card width by a hardcoded `×1.5` factor, so portrait and landscape cards match the grid mode at the same multiplier.

## Unreleased — Vidzy YAML hoster

### Added

- **Vidzy YAML hoster**: Added `vidzy.yaml` for `vidzy.cc/embed-<id>.html` pages. The resolver recognizes the Vidzy URL, decodes its deterministic inline HLS payload, and exposes the proxied stream with its title and thumbnail.

## Unreleased — Generic Vidara hoster

### Changed

- **Generic Vidara resolver**: `vidara.yaml` now recognizes the Vidara player markup independently of its hosting domain and calls `/api/stream` on `{origine}`, the final redirected embed origin. The redundant Merivo resolver was removed; Merivo and Kokoflix redirect targets are now handled by Vidara.
- **Deferred scraper placeholders**: Scrapyfy now preserves undeclared placeholders while loading YAML collections and reports missing request parameters only when the relevant query executes. This permits resolver context such as `{origine}` without declaring a dummy collection parameter.

## Unreleased — LuluStream mirror detection

### Changed

- **LuluStream resolver detection**: Identify LuluStream-compatible embeds from their
  player-page title rather than a fixed domain allowlist, supporting mirror domains
  such as `livavid.sbs`.
- **LuluStream HLS proxy headers**: Send the compatible Firefox user-agent and
  origin-root referer when proxying manifests, avoiding the CDN's 403 response.
- **LuluStream player request headers**: Match the manifest's Firefox request profile
  while loading the player page so signed HLS URLs are not issued to a mismatched
  client fingerprint.
- **LuluStream signed manifests**: Reload the player page while resolving a stream so
  the Firefox-profile request obtains a fresh, query-signed HLS URL instead of
  reusing the discovery HTML.

## Unreleased — Terminal YAML hoster errors

### Added

- **Terminal resolver error messages**: YAML `resolve_stream` responses can now return a non-empty
  `error_message`. The generic resolver stops immediately and returns that business error instead
  of probing additional hosters or falling back to an iframe, including when the player page itself
  answered HTTP 200.
- **DoodStream encoding-status errors**: The DoodStream resolver now follows the player page's
  `encStatus` request and returns its terminal `#enc_pp` message, such as `Video encoding error.`,
  through `error_message`.
- **VK video error messages**: The VK resolver now returns the `#video_ext_msg` player error. The
  known Russian message for a missing video file is normalized to `Video file not found`; other
  messages are preserved as returned by VK.

## Unreleased — Proxy persistence through the generic persistence store

### Added

- **Transaction-based persistence contract**: `PersistenceStore` is now a factory of namespace-bound
  `PersistenceTransaction` handles (`get`/`put`/`delete`/`find_by_fields`/`commit`). Keys are plain
  domain strings; the namespace is carried by the transaction. A crate-private `PersistenceBackend`
  trait isolates storage engines (in-memory and file today, database later). `find_by_fields`
  supports equality filters on named fields, including nested dot paths, with shared filtering
  logic used by both backends.
- **Field-only records**: `PersistedRecord` no longer carries a binary `payload`; records are stored
  exclusively as named fields (`fields`, sub-objects allowed), keeping documents inspectable and
  field-queryable. The legacy `payload` read fallback in `chaser_cf.rs` was removed.
- **Deferred file writes**: `FilePersistenceStore` now keeps an in-memory cache per namespace with a
  dirty flag. `get`/`put`/`delete`/`find_by_fields` only touch memory; `commit` flushes a dirty
  namespace atomically (temporary file + fsync + rename) and is a no-op for clean namespaces or the
  in-memory backend. Committing one namespace never touches other namespaces.
- **Proxy persistent cache**: New `persistence` feature in `arachnea-proxy`. `ProxyInventory` accepts
  a shared `Arc<dyn PersistenceStore>` via `with_persistence_store`: selection falls back to the
  `proxy-inventory` namespace filtered by country before triggering the provider, loaded batches are
  written through one transaction committed at the end of processing, status mutations persist the
  updated record, and removals delete the persisted record. Records are keyed by authority, expire
  through cooldown/probe freshness or a 24 h global TTL (`PROXY_CACHE_TTL`), and conversions between
  `ProxyRecord` and `PersistedRecord` live in the new `proxy_persistence` module.
- **Scrapyfy proxy cache wiring**: `default_scrapyfy_proxy_inventory` and
  `default_scrapyfy_proxy_core` accept the shared persistence store; `ScraperAgregator::ensure_proxy_core`
  passes its store so cookies/sessions and proxies share one backend with distinct namespaces.

### Changed

- **Removed `ProxyStore`/`ProxySerdeStore`**: The snapshot-based proxy file store, its codecs, and
  the `ProxyInventory::with_store`/`load_from_store`/`save_to_store` conveniences were removed;
  their fsync-before-rename behavior moved into `FilePersistenceStore::persist_document`.
  `IpCountrySerdeStore` is unaffected.

## Unreleased — Conditional validation (ETag fragments) and parallel catch-up

### Added

- **Base62 encoding helper**: New `arachnea-core::crypt::base62` module providing reusable
  base62 encoding of unsigned integers (digits + uppercase + lowercase alphabet), with unit
  coverage including round-trip validation.
- **Conditional request primitives**: New `arachnea-scrapyfy::conditional` module with validator
  fragments (`E:<etag>` for HTTP ETags, `C:<last_modified>-<hash>` for content hashes, `N:` for
  sources without remote validators), full XXH3-128 digests encoded in base62 without truncation
  (`xxhash-rust`, feature `xxh3`, plus the shared base62 encoder), fragment build/parse helpers,
  and a `ValidationSlot` carrying the incoming client fragment plus the recorded root-fetch
  outcome.
- **Root conditional fetch**: The unified query executor applies `If-None-Match` /
  `If-Modified-Since` headers to each source's root request when a client fragment is provided,
  detects `304 Not Modified` or an identical content hash (including browser-driven
  `page_navigate` queries via hash comparison), records the up-to-date fragment, and skips parsing
  entirely on unchanged content. Sub-queries never receive conditional headers.
- **Per-source validations in the envelope**: `ScraperAggregationResult` gained a `validations`
  map (`ScraperSourceValidation { status: fresh|stale, etag }`) populated by
  `execute_query_async`, which now accepts optional per-source client fragments.
- **Global ETag layer**: New `arachnea-stream::stream_etag` module building the deterministic
  global ETag (`[S:<hash services>;]<fragment>;...` over alphabetically sorted service names,
  the `S:` hash using the same full XXH3-128/base62 helper) and decoding client ETags back into
  per-source fragments by position, rejecting stale or mismatched service lists through the
  `S:` block.
- **Parallel validation + catch-up orchestration**: `StreamScraper::execute_query_with_etag`
  runs phase 1 (parallel conditional validation of every involved source), short-circuits to an
  empty payload when every source is stale and the rebuilt global ETag matches the client's
  `If-None-Match`, then runs phase 2 (forced full GETs restricted to stale sources) merging rows
  into the aggregate while keeping phase-1 fragments for the response ETag.
- **Header-aware JSON commands**: New controller contract (`register_json_function`,
  `ControlerJsonInput`/`ControlerJsonOutput`, `register_etag_result_function[_with_state]`)
  exposing request headers to JSON commands and letting them answer `304 Not Modified` (empty
  body) or attach an `ETag` response header. Implemented by both the REST (Warp) and Tauri
  backends; Tauri passes empty headers since IPC carries no conditional validation.
- **ETag-enabled catalog commands**: `search`, `load_home`, `get_service`, `list_lives`,
  `get_category`, `get_section`, `get_banners`, `get_players`, `get_entry`, `get_season`, and
  `get_live` accept optional `arachneaEtag` / `enableEtag` parameters (validation enabled by
  default) and are registered through the header-aware contract.

### Changed

- **REST bridge switched to GET**: The frontend `call_api` REST path now issues GET requests with
  every parameter JSON-encoded into the query string; the backend query-string deserializer
  JSON-decodes values starting with `[`, `{`, or `"` so complex parameters round-trip unchanged.
  Repeated keys collapse into arrays. The Tauri invoke path is unchanged.
- **Frontend ETag cache**: `call_api` caches the backend `ETag` per command + serialized
  parameters, replays it as `If-None-Match`, and resolves `304 Not Modified` responses from the
  cached envelope without touching the network payload.

### Fixed

- **Deterministic aggregation fragments**: Sources without a recorded validation outcome (static
  or fetch-less queries) contribute a stable `N:` fallback fragment derived from the source name,
  keeping the global ETag identical across identical requests.
- **Parameterless REST GET commands**: REST command routes now accept a missing query string as
  an empty payload. ETag-enabled calls such as `load_home`, `get_service`, and `list_lives` no
  longer fail with `400 Bad Request - Invalid query string` when invoked without parameters.
- **Typed REST GET scalar parameters**: Query-string values that are valid JSON scalars are now
  deserialized as their native JSON types. Numeric parameters such as `get_section.page` no
  longer reach Rust as strings and fail `usize` deserialization.
- **Scrapyfy response pre-processing**: Queries and sub-queries may now declare ordered
  `pre_process` actions. The initial `remove_text_blocks` action removes literal volatile blocks
  before parsing and before fallback content-hash ETag validation. The resulting `C:` validator
  is calculated only from transformed content and can stop dependent sub-queries. Remote ETags
  and Last-Modified validators retain their existing priority. PapaDuStream v2 removes its
  changing Cloudflare blocks from `get_players` responses.
- **REST ETag propagation to Scrapyfy**: Cached REST ETags are sent as the `arachneaEtag`
  request parameter, outside the cache key. `get_players` also consumes `If-None-Match` directly
  when that parameter is absent. Scrapyfy can therefore validate the root response and skip
  dependent sub-queries before the controller returns `304 Not Modified`. Header quotes and weak
  validator prefixes are normalized before comparing the global ETag, preventing a false catch-up
  pass that previously executed stale sources' sub-queries.
- **Static `get_service` validation**: Restored the header-aware ETag response for
  `get_service`. Its static metadata uses the existing deterministic `N:` source fragments and
  never hashes the final serialized response.
- **Multi-source ETag decoding**: Removed the duplicate separator between the `S:` block and the
  first source fragment, allowing aggregated client ETags to be decoded and validated again.
- **Quoted `304` ETags**: Conditional REST responses now use the same quoted ETag wire format as
  successful responses.
- **PapaDuStream entry and season validation**: `get_entry` and `get_season` now each declare
  the same local `pre_process` rules as `get_players`, removing volatile Cloudflare blocks before
  their fallback content ETag is calculated.
- **Parameterized pre-processing delimiters**: `remove_text_blocks.start` and `.end` now resolve
  `{base_url}` from the runtime query parameter (or the query base URL), allowing source domains
  such as PapaDuStream to remain configurable. Browser `page_fetch` sub-queries now apply the
  same pre-processing pipeline as other HTTP response paths.
- **YAML-hash-aware ETag fragments**: Per-service validator fragments now embed a compact base62
  hash of the source YAML document (`E:<yamlhash>-<etag>`, `C:<yamlhash>-<last_modified>-<hash>`,
  `N:<yamlhash>-<namehash>`). The hash is computed once from the raw YAML bytes at load time
  (`arachnea_core::crypt::hash62_64`) and a client fragment carrying a divergent hash is dropped,
  so editing a scraper forces a full re-fetch even when the remote content is unchanged. Legacy
  client ETags without a YAML-hash segment are treated as divergent and converge after one full
  re-fetch; the global ETag stays opaque to the frontend.

- **Local cross-platform release builder (`release/`)**: Added a zero-dependency Node toolchain that builds Tauri bundles for every platform the current host can produce locally (no Docker). `release/release-targets.json` lists the platforms; `release/capabilities.mjs` encodes the cross-compilation capability matrix per host (macOS: `.dmg` native and Windows NSIS via `cargo-xwin`; Windows: NSIS + MSI native; Linux: `.deb`/`.rpm`/`.AppImage` native and Windows NSIS via `cargo-xwin`). `node release/install-tools.mjs` installs the required rustup targets, `cargo-xwin`, and Linux system packages; `node release/release.mjs` (default: all locally available platforms, `--platform <id>` for one) runs `cargo tauri build`, assembles `release-<VERSION>/<platform>/` with version-named installers, builds a portable Windows `.zip` containing the release executable plus the runtime `services/` and `data/` folders (excluding `credentials.json`, the cache, and the runtime store files), copies `CHANGELOG.md` into the release folder and every platform subfolder, and archives the whole tree as `release-<VERSION>.zip`. `--skip-build` re-assembles from existing `target/` artifacts and `--list` shows which platforms the host can build.

- **Release builder layout rework (`build-release/`)**: Renamed the release tooling folder from `release/` to `build-release/`, moved generated outputs into `releases/release-<VERSION>/` at the repository root, and kept the project `CHANGELOG.md` only at the release folder root (no longer duplicated per platform subfolder). Removed the flattened `release-<VERSION>.zip`; the only generated archive is now the Windows portable zip (executable plus `services/`, without the `data/` runtime folder, which is created on first launch). `-p/--platform` now accepts several selectors, repeated or comma-separated, each either an exact platform id or a wildcard pattern such as `darwin-*`, `linux-*`, or `windows-*`.

- **Family-based release subfolders and selector aliases**: Platform output subfolders now use the family part before the first dash (`darwin-*` platforms land in `osx/`, `windows-*` in `windows/`, `linux-*` in `linux/`), with the mapping centralized in `build-release/capabilities.mjs`. `-p/--platform` (and `install-tools`) additionally accept a bare family name (`darwin`, `windows`, `linux`) selecting every platform of that family, and the alias `osx` mapping to `darwin`; `*` patterns and multiple selectors keep working.
- **Single frontend build and checksums (`build-release/`)**: The frontend is now built once up front and shared by every target; each `cargo tauri build` invocation receives a `--config` override disabling `beforeBuildCommand` (which stays active for manual `cargo tauri build` runs outside this tooling), plus a `--no-frontend-build` flag to reuse an existing `front/dist`. Every installer (`.dmg`, `.exe`, `.msi`, `.deb`, `.rpm`, `.AppImage`) and the portable Windows `.zip` now gets companion `*.sha256` and `*.md5` checksum files in `sha256sum`/`md5sum` format.
- **Targeted release cleanup (`build-release/`)**: The release run no longer wipes the whole `releases/release-<VERSION>/` folder. Cleaning is now scoped to what the run rebuilds: selecting a full family (e.g. `-p osx`) clears that family folder, while selecting a single platform (e.g. `-p darwin-x86_64`) removes only that build's artifacts and their `.sha256`/`.md5` checksums, leaving other platforms' files untouched (including unrelated files in the family folder).
- **Cross-platform bundle selection fix (`build-release/`)**: Bundle types are now passed to `cargo tauri build` through a `--config` merge (`bundle.targets`) instead of the `--bundles` flag, which the Tauri CLI validates against a host-dependent value list that rejects cross builds (e.g. a macOS host only accepts `app`/`dmg`/`ios`, so `--bundles nsis` failed for `x86_64-pc-windows-msvc`). `install-tools` additionally installs LLVM through Homebrew when `llvm-rc` is missing on non-Windows hosts building Windows targets (required by cargo-xwin), warns about a missing `cmake`, and every spawned `cargo tauri build` augments PATH with the common Homebrew LLVM directories.
- **Windows cross-build toolchain provisioning (`build-release/`)**: The first real `x86_64-pc-windows-msvc` cross build surfaced three missing native tools, now provisioned automatically by `install-tools`: Ninja and NASM through Homebrew/apt (BoringSSL's `boring-sys2` CMake build requires both), and `makensis` — for which a `makensis.exe` shell shim wrapping the native compiler is created under `~/.arachnea-cross-tools/bin/` because the Tauri NSIS bundler invokes the Windows-style executable name; spawned builds augment PATH with that directory. The full macOS → Windows NSIS pipeline is verified end to end: `arachnea.exe` cross-compiled, `*-setup.exe` installer generated, portable zip assembled, checksums written.
- **Portable Linux/macOS binaries via Docker (`build-release/`)**: The builder can now ship raw portable binaries for `linux-x86_64`, `linux-arm64`, `darwin-x86_64` and `darwin-arm64` from any host (e.g. Windows) by cross-compiling inside a dedicated Docker image. `build-release/docker.mjs` builds/runs the image and `build-release/docker/Dockerfile` derives it from `joseluisq/rust-linux-darwin-builder` (Rust + osxcross for macOS, WebKitGTK 4.1 stack added for the Linux GNU targets). A platform is produced this way (method `docker`) when `release-config.json` declares `"build": "docker"` with a `portable` block and the host cannot bundle it natively, packaging the executable as `arachnea_<ver>_<arch>-portable.tar.gz` (with `.sha256`/`.md5`). The generic portable step now supports both `.zip` (Windows) and `.tar.gz` (Linux/macOS). Added `--dry-run` to preview the planned production including the exact `docker run` commands; `--list` reports the production method (`native`/`docker`). The `install-tools` frontend build also works on Node ≥ 20 under Windows (`npm.cmd` is now spawned with `shell: true` to avoid an `EINVAL`). The cross image (`arachnea-cross-builder:1.0.0`) updates rustup to the latest stable so its rustc can compile the dependency graph (e.g. `boa`, `cookie_store`, `time` require rustc >= 1.88, reinstalling the darwin/linux targets on the new toolchain) and points cc-rs at osxcross compilers named with the full triple prefix (`x86_64-apple-darwin22.4-clang`, using the dash-substituted `CC_<target-with-underscores>` form cargo actually forwards) via wrappers that bind clang to the osxcross ld64 linker (`-fuse-ld`). Naming them with the triple prefix makes cmake infer and locate the osxcross binutils it needs (`install_name_tool`, `dsymutil`, `nm`, `ar`, ...), so C/ObjC sources and CMake builds (objc2, ring, BoringSSL) compile and link instead of falling back to the host GCC/`/usr/bin/ld`. No installer (`.dmg`/`.deb`/...) is created by this path — the `.dmg` and MSI remain host-bound; macOS portable binaries are unsigned (see `docs/dev-tracking/docker-build.txt`).

- **Installers now bundle the `services/` runtime folder**: added
  `bundle.resources` (`"../../../server/services": "services/"`) to
  `server/crates/arachnea-stream/tauri.conf.json`, so NSIS/MSI installers ship
  the scraper runtime data next to the executable (the release-mode application
  root) instead of only installing `arachnea.exe`. The directory-key resource
  form preserves the `services/` subtree (a glob key would have flattened it,
  per `tauri-utils` resource resolution). Linux/macOS installers also embed the
  folder, but into their platform resource directory, which the
  exe-directory application root does not read — the portable archives remain
  the self-contained option there.

## Unreleased — macOS `-app.tar.gz` archives embed a `.app` bundle (`build-release/`)

macOS targets (`darwin-x86_64`, `darwin-arm64`) now get a **second** archive
alongside the unchanged portable one: `arachnea-<ver>-darwin-<arch>-app.tar.gz`
holds only a launchable `<productName>.app` bundle with the tauri-bundler
`app` layout (`Contents/Info.plist` — identifier, version, icon — `PkgInfo`,
`Contents/MacOS/arachnea` marked executable, `Contents/Resources/icon.icns`,
all read from the crate's `tauri.conf.json`). The runtime `services/` folder
is staged in `Contents/Resources/`, the Tauri `bundle.resources` location,
probed by the application resource root resolution (see the resource/data
entry below). The classic
`arachnea-<ver>-darwin-<arch>-portable.tar.gz` (raw binary + `services/`) is
unchanged; both archives get `.sha256`/`.md5` checksums and are cleaned on
partial rebuilds. The `.app` cannot be produced by the Tauri CLI inside the
cross image — it ignores macOS bundle types on a Linux host (`Wrong package
type app for platform Linux`) — hence the host-side assembly.


## Unreleased — application resource and data root resolution (`server`)

`arachnea-core::application` gains two dedicated, process-wide resolvers and
the legacy `get_application_path`/`get_application_root` are kept only for
compatibility:

- **`get_application_resource_path`/`_root`** (read-only `services/` scraper
  manifests and YAML sources): probed once at first call, first candidate
  containing a `services` directory wins — executable directory (portable
  archives, NSIS), `Contents/Resources/` inside a macOS bundle (Tauri bundler
  layout), and `../lib/<product name>/` for Linux system packages
  (deb/rpm/AppImage, verified against the produced `.deb`:
  `/usr/lib/Arachnéa/services`). Scraper manifests were previously opened
  relative to the process working directory, which broke every packaged
  layout (`.app` via Finder/LaunchServices, `.dmg`, `.deb`, `.rpm`,
  `.AppImage`); `add_query_collection_from_config` now resolves relative
  manifest paths through this resolver.
- **`get_application_data_path`/`_root`** (writable `data/` and credentials):
  executable directory for portable layouts (unchanged behavior) when it is
  writable and the install is not packaged; otherwise the per-OS standard
  directory, named after the application identifier following the Tauri
  `app_data_dir` convention — `~/Library/Application Support/` on macOS,
  `$XDG_DATA_HOME` on Linux, `%APPDATA%` on Windows
  (`hell-hibou.arachnea`). Packaged-install detection covers macOS
  bundles, `/usr|/opt|/snap` prefixes and AppImage mount points; the
  writability check is a create-and-remove probe.

`build-release/release.mjs` stages the macOS `.app` runtime data in
`Contents/Resources/` (Tauri `bundle.resources` layout) accordingly, and the
portable assembly for Docker-produced platforms prefers the container-built
executable (`dockerBuildExecutable`, `docker-build/<arch>/…`) so a stale
host-built binary left in the plain target dir cannot shadow the fresh
cross-compiled one.

## Unreleased — macOS Finder launch arguments (`server`)

The `arachnea` executable now ignores the macOS Finder process serial number
argument (`-psn_...`). Launching the `.app` from Finder or the Dock therefore
reaches the desktop startup path instead of exiting before the Tauri window is
created.

## Unreleased — macOS server tray startup (`server`)

The release default remains server mode. The application controller now routes
the generated Tauri context to the server tray service in that mode, so the
server launched from the `.app` exposes its TrayIcon.

## Unreleased — macOS server shutdown (`server`)

Selecting **Shutdown server** from the server TrayIcon now lets the Tauri event
loop exit after the HTTP server has stopped. The tray still prevents accidental
application exits while the server is running.

## Unreleased — server tray menu (`server`)

The server TrayIcon now opens its menu with either a left or right click.

## Unreleased — server tray status (`server`)

The server TrayIcon menu now displays the full server URL and network access
mode as disabled status items. The browser action is labelled **Open in
browser**.

## Unreleased — application data path logging (`server`)

Application startup now logs the resolved application data directory at the
`info` level.

## Unreleased — macOS application data directory (`server`)

macOS `.app` bundles are now detected from their actual bundle directory name,
so writable application data is stored under `~/Library/Application Support/`
instead of inside an unpacked bundle.

## Unreleased — application data identifier configuration (`server`)

The shared core no longer hardcodes the application data directory name.
`arachnea-stream` now reads its Tauri `identifier` from `tauri.conf.json` at
build time and configures the core before any writable data path is resolved.

## Unreleased — Docker daemon detection in release tooling (`build-release`)

`assertDocker()` in `build-release/docker.mjs` now probes the Docker daemon
(`docker version`) after checking the CLI is on the PATH: when the daemon does
not respond (e.g. Docker Desktop is closed on Windows), the contextualized
"Docker is probably not running" error (still carrying the original daemon
error) is shown and the user is offered to retry — so a starting-up daemon can
be waited for without restarting the whole flow. A refusal (or a
non-interactive terminal) throws the same error, unchanged. The release and
install-tools flows no longer prompt for a cross-image build that would crash
afterwards on a raw named-pipe connection error.

## Unreleased — persistent Stream configuration (server)

Arachnéa Stream now loads its persistent application configuration from
data/config.json. It validates the port, public root, network mode and
administrator password-hash field, supports atomic owner-only writes on Unix,
and applies these values only when the equivalent CLI option is absent.
Server starts without a persisted administrator hash now print a new temporary
remote-administration password to the console.

## Unreleased — persistent service activation (server)

Scrapyfy now supports a persistence-backed source-activation policy. Arachnéa
Stream synchronizes every declared stream service into the arachnea-services
namespace, preserves administrator overrides, and applies them while loading
the YAML collections. Service YAML files can now declare optional credentials
metadata for required and signup_url.

## Unreleased - reloadable stream scraper and encrypted service credentials (server)

Arachnéa Stream now registers all of its routes against a new
`ReloadableStreamScraper` facade: every request resolves the active scraper
instance at call time, so future admin reloads can swap the instance without
re-registering routes or interrupting in-flight requests. The facade builds
and validates a replacement off the request path, synchronizes missing service
states in `arachnea-services` without overwriting existing overrides, and swaps
atomically only after validation succeeds, returning a detailed report of
loaded, disabled, ignored, and failing services. The registration-time proxy
core and public proxy/DRM paths are shared with rebuilt instances so the
generic `proxy` command and DRM licenses keep working across reloads.

Service credentials used by player resolvers are now persisted in the encrypted
`data/credentials` store instead of the clear JSON store. Scrapyfy also gained
`load_service_catalog_detailed`, a tolerant catalog loader that reports
per-source missing, invalid, or duplicate entries instead of aborting on the
first broken YAML file.
## Unreleased - administration API and security (server)

A new administration API is now served under `api/admin/<operation>` over HTTP
and through the Tauri invoke handler under the same command names. It exposes
status, login/logout with opaque sessions, the full service catalog (including
disabled and unavailable sources), persistent service activation and reset,
service credentials state (never the secrets themselves) with set/clear, server
settings with provenance and restart hints, permanent administrator password
management, and an atomic configuration reload reporting loaded, disabled,
ignored, and failing services.

Access control follows the local/remote rule: desktop and loopback clients
do not need authentication; any other client must hold a session obtained with
the administrator password. Passwords are hashed with Argon2id; a temporary
password is generated, printed once, and kept in memory when no permanent hash
is configured. Sessions use an HttpOnly SameSite=Strict cookie, admin
responses are `Cache-Control: no-store`, all writes require POST and reject
cross-site origins, and failed logins are rate-limited per client address.
The REST controller context now carries the remote TCP peer address, and the
latest response describes the provenance (command line, configuration file, or
default) of every effective setting.

## Unreleased — administration refinements (frontend)

The public desktop settings now expose an **Open administration** action that
opens or focuses the dedicated Tauri administration window. The admin reload
request now includes its required empty JSON object, preventing the previous
HTTP 400 EOF deserialization error. The shell refreshes its authentication
state after login so its app bar and navigation drawer appear immediately;
the Local network option is shown only on loopback hosts; and the credentials
dialog warns that provider accounts must use a username/email and password,
not OAuth sign-in.

## Unreleased — validation and scraper reliability

The targeted Core, Scrapyfy and Stream test suites now run without warnings or
failures in their deterministic mode. Scrapyfy restores the documented
`exec_js` output of numeric global variables, validates and resolves the
reserved `@` dynamic-variable namespace, and isolates its test data directory.
The HTTP rquest engine now ignores implicit system proxy settings unless an
Arachnéa proxy is explicitly configured, avoiding a macOS system-configuration
panic. Stream tests initialize their data-directory identifier and keep the
full live-provider sweep as an explicit ignored test because third-party
catalogues are inherently volatile.

## Unreleased — public settings order

The desktop-only **Administration** section is now the final section in the
public settings screen.

## Unreleased — SQLite typed persistence backend

`arachnea-core` gains an optional `sqlite-persistence` feature (rusqlite 0.40.2
bundled) exposing `SqliteEntityStore<E>`/`SqlitePersistenceStore<E>`. Each named
store owns its SQLite database under `<data-root>/<store-name>/records.sqlite3`
with WAL durability, blocking operations off the async runtime, and a single
transaction for `put_all`. The physical `STRICT` table, its columns and its
unit indexes are reconciled at open time against internal `arachnea_columns` /
`arachnea_indexes` metadata tables: missing declared columns are added as
nullable, managed indexes are created or dropped, and any type, primary-key or
unmanaged-object conflict fails with a contextualized error. `EntityReader`
additionally supports optional field reads for nullable schema fields.

## Unreleased — SQLite application wiring and legacy cleanup

`arachnea-stream` now composes the application around explicitly named typed
stores (`proxy-inventory`, `cloudflare-session`, `arachnea-services`) backed by
SQLite by default (`sqlite_application_stores`, under
`data/persistence/<store>/records.sqlite3`) or by in-memory stores for tests
(`memory_application_stores`). `ScraperAgregator`, `StreamScraper` and the admin
API inject repositories directly instead of the transitional legacy adapter,
and `CachedChaserSession` moved to `arachnea-http::chaser_session` so it stays
available without the `chaser-cf` feature. The deprecated map-based contract was
removed entirely: `PersistedRecord`, `PersistenceKey`, `PersistenceTransaction`,
`PersistenceBackend`, `record_matches_filters`, `LegacyTypedEntityStore`,
`LegacyMemoryPersistenceStore` and `LegacyFilePersistenceStore` are gone, along
with their modules. No existing JSON data is migrated; SQLite starts empty.

## Unreleased — typed persistence foundation

`arachnea-core::persistence` now exposes the schema-declared typed-store
foundation: `PersistentEntity`, schema fields and roles, typed readers/writers,
typed equality queries, and explicit per-store configuration. The new
`MemoryPersistenceStore<E>` keeps entities directly in memory and
`FilePersistenceStore<E>` atomically writes validated entity documents. The
pre-existing map-based stores remain temporarily available as
`LegacyMemoryPersistenceStore` and `LegacyFilePersistenceStore` until their
phase-2 domain consumers are migrated.

## Unreleased — typed persistence repositories

Dynamic proxies, Cloudflare sessions and source activation overrides now use
typed repositories. `ProxyRecord`, `CachedChaserSession`, and
`SourceEnabledOverride` declare explicit schemas and keys; the former
`ProxyRecord`/`PersistedRecord` conversion bridge has been removed. Existing
namespace stores are isolated behind `LegacyTypedEntityStore` during the
transition to SQLite, so no domain code manipulates persistence maps.

## Unreleased — typed persistence validation

Phase-5 validation coverage now exercises the three backends and the schema
evolution contract: direct entity roundtrips, single- and multi-field typed
queries, expiration filtering and pruning, restart persistence and batch
atomicity for the SQLite, file and memory stores. SQLite tests additionally
cover managed-index removal at reopen and preservation of externally created
indexes. The validation surfaced and fixed a file-backend defect: the untagged
JSON encoding lost the logical type of `DateTime` values (and of scalar `Json`
values), so reloaded documents were rejected on read; the file store now
restores the declared types from its schema when loading a document.

## Unreleased — composite typed persistence keys

`arachnea-core` typed persistence schemas and SQLite stores now support ordered
composite primary keys while retaining the existing scalar key APIs. SQLite
creates and reconciles table-level primary keys, binds every component for
lookups and deletion, and rejects incompatible primary-key shapes without
attempting a migration.

## Unreleased — flattened proxy inventory persistence

Dynamic proxy records now persist by native `(host, port)` identity rather than
a derived authority. The typed proxy store declares direct columns for all
scalar record fields, retains only destination failures as JSON, and validates
enum and integer conversions while decoding persisted records.

The incompatible legacy `proxy-inventory` SQLite cache is disposable: it is
never migrated or removed automatically. Operators must manually delete
`<application-data>/data/persistence/proxy-inventory/` before starting this
schema version.

Provider reloads now persist the canonical proxy records retained after the
inventory merge, preventing a fresh source batch from overwriting retained
runtime exclusion, cooldown, authentication, or destination-failure state.

Validation now covers proxy composite-key persistence across every backend,
legacy-cache rejection and manual recreation, expiration and atomic batches,
and post-merge provider reload persistence. The Scrapyfy no-proxy constructors
also compile without the optional proxy feature.

## Unreleased — service credentials in the `arachnea-services` store

Service login/password pairs no longer live in the standalone encrypted file
`data/credentials`; they are stored, encrypted per field, in the typed
`arachnea-services` SQLite store. `SourceEnabledOverride` is renamed to
`SourceServiceRecord` and gains two nullable string columns (`login`,
`password`) holding the encrypted values; the existing database is extended
automatically at open time through schema reconciliation, with no manual
migration.

A new `TypedServiceCredentialsStore` adapter (`arachnea-stream`) implements the
unchanged `CredentialsStore` contract: AES-256-GCM with a fresh random 12-byte
nonce per field, stored as `base64(nonce || ciphertext)`; writes are
read-modify-write so the activation override recorded on the same entity is
never clobbered. The encryption key stays application-owned and is passed in
from the executable. The admin HTTP contract, the admin frontend, and the
service resolvers are unchanged; the legacy `data/credentials` file is no
longer read or written.
## Unreleased — Hot application of server settings

Les réglages « Port du serveur », « Mode réseau » et « Racine d'entrée »
sont désormais appliqués à chaud au serveur REST en mode serveur, sans
redémarrage du processus ni du systray :

- Nouveau supervisor REST dans `arachnea-core` (`controler::rest::supervisor`) :
  il possède le listener TCP et la boucle d'acceptation, rejoue les
  enregistrements des services lors d'un re-bind, pré-vérifie le nouveau bind
  et se replie sur l'ancienne configuration en cas d'échec.
- Application conditionnelle : si le port et l'adresse de bind sont inchangés,
  les routes sont reconstruites sur le même listener (aucune coupure HTTP),
  même quand le mode réseau ou la racine change ; sinon cycle complet (arrêt
  gracieux borné, rejeu, re-bind).
- `update-settings` répond `{restart_required, applied, apply_error, admin_url}` ;
  en mode serveur `applied: true`, avec redirection de l'UI admin vers la
  nouvelle URL quand le port ou la racine change réellement. En mode desktop
  la réponse reste `restart_required: true`.
- Le systray est mis à jour sur place après application (`Server: url`,
  `Network: mode`, actions « Open in browser » / « Open administration »),
  sans recréation du tray.
- Frontend admin : message « appliqué à chaud », affichage des erreurs
  d'application et redirection vers `admin_url` le cas échéant.

## Unreleased — Admin settings follow-up

- **Admin web application (`front/admin-app/`)**: Entrypoint roots now use a
  relative, non-empty-segment format (for example `arachnea` or
  `arachnea/admin`). After a port re-bind, navigation waits for the deferred
  application and bounded connection drain to finish before loading the new
  administration URL.
- **Web frontend bundles (`front/public-app/`, `front/admin-app/`)**: Production
  asset references are now relative so the runtime-injected entrypoint root
  applies to JavaScript and CSS assets as well as the HTML document.
- **Admin settings**: Clearing the entrypoint-root field now sends an explicit
  empty value so the persisted root is removed rather than preserved.
- **Hot server settings**: The delayed application now uses the effective
  settings snapshot resolved with the update response, ensuring a simultaneous
  port and entrypoint-root change rebuilds the admin mount at its new path.
- **Hot server settings**: The active REST route snapshot is now selected for
  each HTTP request. Entrypoint-root updates therefore apply to keep-alive
  clients without stopping the Tokio runtime that owns the scraper cache.
- **Admin web application (`front/admin-app/`)**: Unknown URLs now render a
  localized 404 page instead of an empty route.
- **Public web application (`front/public-app/`)**: The fallback route now
  presents an explicit localized 404 error state above the decorative background.

## Unreleased — Namespaced service administration backend

- **Breaking operator action — `arachnea-services` persistence**: The service
  administration store now keys records by `(service_store_id, source_id)` to
  permit the same service identifier in independent groups. On first launch of
  this schema, `data/persistence/arachnea-services/` is deleted deliberately,
  including `records.sqlite3`, `records.sqlite3-wal`, and
  `records.sqlite3-shm`; existing service activation overrides and encrypted
  credentials are lost and defaults are recreated. No migration is provided.
- **Scrapyfy administration**: `admin/*` routes, authentication, sessions,
  DTOs, source activation and catalog orchestration now live in
  `arachnea-scrapyfy::admin`. Stream supplies configuration, encrypted
  credentials, REST hot application and runtime hooks through a narrow adapter.
- **Service groups**: The backend administration service declares and exposes
  `arachnea-stream`, `arachnea-stream-hoster`, `arachnea-proxies`, and
  `arachnea-ip-countries`; source API identities now include
  `service_store_id`.
## Unreleased — Frontend multi-group service administration

- **Admin web application — multi-group service administration**: the
  admin catalog now addresses every source by the composite pair
  `(service_store_id, service_id)`, and the frontend transports the group
  identifier in every source operation (catalog, activation overrides,
  credentials). A public `config.json` declares the app title and the ordered
  list of the four administrable groups; its absence or invalidity displays a
  localized error instead of an incomplete administration. The left navigation
  exposes one entry per group (en/fr titles/descriptions with the
  locale → `en` → identifier fallback chain), the app bar title comes from
  `config.json`, and the services view uses a single parametrized route
  `/services/:serviceStoreId` preserving direct links.

## Unreleased — Application option exports and shared file saving

- **Application options**: `ApplicationOptionsProvider` now requires an
  `export` method that emits persisted option names without their `--` prefix.
  Core and Scrapyfy option holders export their supported persisted values, and
  `CoreApplicationOptions::save` now obtains its JSON document through this
  contract.
- **File persistence**: `PersistenceFileCodec::save` centralizes document
  serialization, parent-directory creation, randomized temporary files,
  and synchronized atomic replacement. Owner-only Unix permissions are exposed
  separately by `persistence::set_owner_only_permissions`; core configuration
  and credential stores apply that policy explicitly, while typed file stores
  retain their existing permission behavior.

## Unreleased — Dynamic admin service groups

- **Dynamic admin service groups**: `ScraperAgregator` now retains each loaded
  group as a `ScraperQueryService` containing its name, query collections, and
  JSON manifest path. `register_admin_service` derives
  `AdminServiceGroupConfig` values from those loaded groups and builds the
  administration state itself, removing the Stream executable's hard-coded
  service-group manifest list.

## Unreleased — Generic reload validation foundation

- **Shared reload validation**: `arachnea-scrapyfy::admin::reload` now owns the
  generic service-group manifest and activation validation previously embedded
  in the admin request handler. `RuntimeReloadReport` replaces the duplicate
  internal `AdminGroupReload` and Stream-specific `StreamReloadReport` types;
  Stream uses the shared report through its runtime adapter while the existing
  flat `reload` API response remains unchanged. Runtime-specific Stream rebuild
  validation remains in place temporarily for the following migration phase.

## Unreleased — Validated Stream runtime rebuild

- **Reload validation boundary**: Scrapyfy now validates each manifest and its
  activation state before invoking an application runtime hook. The adapter hook
  receives `ValidatedReloadGroup` through `rebuild_validated_group`; Stream's
  `rebuild_validated` now only constructs and atomically swaps a replacement,
  restores proxy/DRM endpoints, and reapplies the current country. The temporary
  Stream tray wrapper delegates its validation to Scrapyfy pending coordinator
  unification.

## Unreleased — Shared reload coordinator

- **Unified reload entry points**: `ReloadCoordinator` in Scrapyfy now owns the
  group reload loop used by both `admin/reload` and the server tray. The tray no
  longer calls `ReloadableStreamScraper` directly; it logs every coordinated
  group outcome, while the existing HTTP response remains backward-compatible
  through its first-group projection.

## Unreleased — Multi-group reload reports and tray server application

- **Reload outcomes**: `admin/reload` now returns a `groups` array with the
  `{service_store_id, applied, build_error}` outcome of every administrable
  group, while retaining the historical first-group `applied` and `build_error`
  fields. The administration UI displays every group outcome after a reload.
- **Tray reload completion**: the server tray now applies pending REST port,
  network and root settings after coordinated group reloads whenever a REST
  target is available. Its log summary reports the server application result
  independently, so a rebind failure does not hide successful group reloads.
