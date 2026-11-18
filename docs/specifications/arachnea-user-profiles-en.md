# Server user accounts and profiles — implementation contract

Status (18 November 2026): steps 1–5 delivered. Target contract for the [implementation plan](../dev-tracking/server-user-profiles-analysis.md); stores, accounts/sessions, user administration and content access controls are implemented; profile/preferences APIs, embedded avatars and the live admin quota are implemented; streaming collections and frontend adapters are delivered; steps 6–9 remain pending. [French version](arachnea-user-profiles-fr.md).

## 1. Scope and ownership

Public accounts are administered accounts, independent of the administrator identity and provider credentials. Authentication is required before content access. There is no public registration, email recovery, local-data import, offline write queue or external-player authorization in this scope.

`arachnea-stream/src/profiles/` owns the application feature, without a new crate:

| Module | Responsibility |
| --- | --- |
| `common/models`, `common/repository` | Account/profile identity, common preferences, onboarding and local transactions in `profiles` |
| `accounts/service`, `accounts/sessions`, `accounts/credentials` | Public credentials, persistent sessions, administrator account mutations; common repositories retain identity/quota authority |
| `streaming/models`, `streaming/repository`, `streaming/operations`, `streaming/validation`, `streaming/state` | Streaming preferences, homes, section preferences and bookmarks |
| `coordination` | Durable operations, per-profile serialization, restart recovery and account deletion |
| `avatars` | Embedded manifest validation, catalogue, images and fallback |
| `accounts/dto`, `accounts/routes`, `accounts/guard`, `accounts/native` | Account DTOs and REST/Tauri adapters, content authorization, native secret broker; profile adapters follow in later steps |
| `main.rs` composition | Open stores, inject repositories/configuration/authentication, recover operations, register routes |

Core owns only generic persistence/controller extensions. Scrapyfy retains its administrator authentication; it does not acquire public-account business logic. YAML and scraper facades do not shape profile responses.

One `SqliteDatabase` per physical store, typed table views injected into consumers:

| Store | Tables and keys |
| --- | --- |
| `data/profiles.sqlite3` | `users(id)`; unique normalized login; `user_sessions(id)` indexed by `user_id`; `profiles(id)` indexed by `user_id` and unique `(user_id, normalized_name)`; `profile_settings(profile_id)`; `profile_onboarding(profile_id, scope)`; `profile_operations(operation_id)` |
| `data/arachnea-stream-profiles.sqlite3` | `stream_profile_settings(profile_id)`; `profile_homes(profile_id, home_id)`; `profile_home_sections(profile_id, section_key)`; `profile_bookmarks(profile_id, source_id, entry)`; `stream_profile_collections(profile_id, kind)`; `stream_profile_operations(operation_id)` |

`user_id`, `profile_id` and `home_id` are server-generated UUIDs, never reused. Clients generate stable retry UUIDs for `operation_id`. Only the common store creates `profile_id`; streaming reuses it unchanged, with no duplicated owner/name/avatar. There is no cross-database foreign key or atomic global transaction. Scalar identities, owner, state, normalized names, revisions and lookup fields remain explicit; nested definitions are typed JSON documents with `schema_version: 1`. Declare each table schema version through Core's existing schema mechanism. Existing services/Cloudflare/proxy stores remain untouched. Preserve in-memory constructor support.

Core now provides synchronous local IMMEDIATE transactions through `SqliteDatabase::transaction`, typed get/find/put/delete_where operations and verified caller-owned composite unique indexes through `SqliteTransaction::ensure_unique`. Transactional put uses primary-key upsert and refuses secondary uniqueness conflicts without deleting other rows. Table views from another physical database are rejected. These calls run on blocking threads and must not nest or invoke async methods on the locked database. REST registration currently accepts both GET and POST for JSON commands; step 3 must provide method-aware registration and trusted authentication context before using it for mutations.

New profiles have no custom homes, sections or bookmarks. Common preferences/onboarding exist on profile creation; streaming defaults are initialized only on first streaming use. No browser data is read to initialize a profile. Backups/restores must cover both stores at one quiescent application boundary; validate references and recover unfinished operations before accepting writes.

## 2. Complete browser-storage inventory and destination

This inventory describes the original local storage before implementation. Step 5 replaces homes/sections/bookmarks persistence; other local preference paths remain until steps 7–8.

Inventory covers `front/public-app` and `front/admin-app` source storage calls. Parameter keys below are literal suffixes of `arachnea.param.` in `services/storage/keys.ts` (`PARAMETERS_DEF`); other keys are fully qualified. Existing defaults and valid enums come from `keys.ts`, `sanitizers.ts`, `types/media.ts` and `services/languages.ts`; preserve them unless specified here.

| Current key(s) | Target owner / field |
| --- | --- |
| `language`, `theme` | Common profile `settings.values.language`, `settings.values.theme`; null language follows browser language |
| `defaultHomeId` | Streaming profile; null means aggregated landing page; non-null must reference this profile's home |
| `thumbnailOrientation`, `thumbnailImageFit`, `thumbnailSizeMultiplier`, `collectionMode` | Streaming display preferences |
| `useTrailerAsBackground`, `useCatalogBannersAsBackground`, `isBackgroundAnimated`, `backgroundImageFit` | Streaming background preferences |
| `isEpisodeAutoplayEnabled` | Streaming player preferences |
| `videoPlayer.autoskip.previously`, `videoPlayer.autoskip.intro`, `videoPlayer.autoskip.coming_next`, `videoPlayer.autoskip.outro`, `videoPlayer.autoskip.ads` | Streaming chapter skipping |
| `videoPlayer.trackProfiles` | Streaming ordered audio/subtitle combinations; distinct from user profiles |
| `videoPlayer.subtitleLanguagePriority`, `videoPlayer.subtitleTranslationByLanguage`, `videoPlayer.subtitleLanguages` | Streaming subtitle preferences; retain legacy alias in the typed DTO, require both ordered language lists to agree when both supplied, return the canonical priority in both; no browser fallback |
| `videoPlayer.allowAudioDescription`, `securityMode` | Streaming player accessibility/security preferences |
| `arachnea.home.showSectionEditingButtons` | Streaming display preference |
| `arachnea.videoPlayer.preferences.quality` | Streaming player preference, string or null (Auto); resolve availability against each media |
| `arachnea.videoPlayer.preferences.audioTrack`, `arachnea.videoPlayer.preferences.textTrack` | Streaming last manual track preferences; preserve existing typed fields/origin, never treat a provider track ID as universal; unavailable choices fall back to configured combinations |
| `arachnea.videoPlayer.preferences.textTrackSettings` | Streaming subtitle appearance; existing allowed keys only |
| `arachnea.videoPlayer.preferences.volume`, `arachnea.videoPlayer.preferences.muted` | Device, retain local keys; finite volume 0–1 and boolean muted |
| `arachnea.navigation.visible`, `arachnea.navigation.sourcesExpanded` | Device, retain local keys |
| `admin.theme` | Administrator UI/device, retain; independent of public theme |
| `arachnea.theme.key` | Declared `THEME_KEY` with no active consumer; do not read as fallback |
| `arachnea.test` | Transient availability probe in `storage/persistence.ts`, no preference |

The public theme currently uses `arachnea.param.theme` via `useTheme`; i18n and theme initialization must start from defaults and then the common server snapshot. Window sizes, temporary navigation state, fetched pages, pagination and notifications stay on the device/in memory. No `sessionStorage` usage was found. `localStorage.clear()` exists in the generic helper; it must not be used by this feature.

IndexedDB database `arachnea-db` has these personal object stores:

| Store / current key | Target document / identity |
| --- | --- |
| `custom_homes` / `id`, index `navigationOrder` | `profile_homes`; preserve `CustomHomeDefinition` schemaVersion 1, name, order, banner sources, shortcuts, blocks with distinct IDs/display options, merge mode, bookmark visibility and timestamps |
| `home_sections` / `name`, index `order` | `profile_home_sections`; preserve order, collection mode, thumbnail orientation/fit; `name` maps to `section_key`, including reserved `local:bookmarks` |
| `entry_bookmarks` / `key` | `profile_bookmarks`; preserve source/entry identity and all `EntryBookmark` fields: groupId, selectedItemId, playbackTime, title, alternativeTitleLabel, imagePosterUrl, imageLandscapeUrl, imageUrl, description, mediaTypeLabel, seasonLabel, episodeNumber, isFullyWatched; modifiedAt becomes server time |

The base64url bookmark key remains a route/display token, not authorization. Preserve source and entry trimming; do not otherwise rewrite entry URLs or invent source identities. Old stores stay physically present; home/section/bookmark persistence is inactive from step 5, with remaining personal preference paths removed in step 8.

All access paths to replace, including indirect consumers:

- `services/indexDB.ts` is the low-level opener/CRUD helper; `customHomes.ts` also opens direct read/write transactions through `withDatabaseRetry`.
- `services/storage/index.ts` initializes local parameters, persists with watchers, loads sections/bookmarks and reloads them on visibility/focus; `storage/sectionRecords.ts` and `storage/bookmarks.ts` perform writes.
- Homes bypass the storage store in `router/index.ts`, `services/defaultHome.ts`, `composables/home/useCustomHomeManager.ts` and `useCustomHomeDefinition.ts`.
- `composables/home/useHomeCatalog.ts` consumes bookmark identities and the reactive cache; entry/player bookmark writes flow through the storage bridge. Replacing only Pinia does not replace these paths.
- `main.ts`, `App.vue`, `i18n/index.ts` and `composables/useTheme.ts` consume preferences during startup; `Parameters.vue` and `parameters/ParametersDialog.vue` currently edit shared state immediately.
- `services/rustify/apiClient.ts` caches REST ETags by command/parameters only. Disable personal response caching; clear authenticated in-memory caches on account/profile/server changes. Public catalogue cache reuse must still follow an authorization check before 304 responses.

New non-secret resume keys: `arachnea.resume.<encoded-server-identity>.lastUserId` and `arachnea.resume.<encoded-server-identity>.<user_id>.lastProfileId`. Encode the canonical server origin plus configured application root with base64url; native local mode uses a stable installation identity. Revalidate both IDs through the server. Never persist passwords/session tokens in JavaScript storage.

## 3. Wire DTOs and validation

New account/profile endpoints use snake_case envelope/metadata fields. The `values` object retains the exact existing frontend preference keys and document fields listed above (including dotted/camelCase keys); adapters isolate this from Rust naming. UTC timestamps are integer epoch milliseconds. Revisions are positive JSON-safe integers incremented by the server; `expected_revision: 0` means create-only. Reject unknown mutation fields, invalid enum values, non-finite numbers and unsupported schema versions, with field errors; do not silently sanitize remote writes.

| DTO | Fields / invariants |
| --- | --- |
| `AccountView` | `user_id`, `login`, `must_change_password`; no hash, session secret or administrator permissions |
| `SessionView` | `authenticated`, `account: AccountView|null`, `expires_at: integer|null`, `persistent: boolean`; anonymous state has null account/expiry |
| `ProfileView` | `profile_id`, `user_id`, `name`, `avatar_id`, `order`, `state: creating|active|deleting`, `revision`, `created_at`, `updated_at` |
| `VersionedSettings` | `schema_version: 1`, `revision`, `values`; common values contain only language/theme; streaming values contain only the streaming rows in section 2 |
| `OnboardingView` | `scope: common|arachnea-stream`, `state: pending|completed`, `revision`; absent streaming state means not initialized |
| `ProfileSnapshot` | `profile: ProfileView`, `common: VersionedSettings`, `streaming: VersionedSettings|null`, `onboarding: OnboardingView[]`, `pending_operation: OperationView|null`; does not inline all homes/bookmarks |
| `ProfileList` | `profiles: ProfileView[]`, `max_profiles_per_user`, `used_slots`; deleting/creating identities count until cleanup, only active identities are selectable |
| `OperationView` | `operation_id`, `kind`, `profile_id: UUID|null`, `state`, `step`, `retryable`, `result: object|null`; never expose durable payload/password hash |
| `AvatarCatalogue` | `schema_version: 1`, `fallback_avatar_id`, `avatars: [{avatar_id, url, mime_type}]`; URLs resolved through active controller prefix |
| `HomeRecord` | `profile_id`, `home_id`, `revision`, `definition: CustomHomeDefinition`; definition.id equals home_id, timestamps server-owned |
| `SectionRecord` | `profile_id`, `section_key`, `revision`, `order`, `collectionMode`, `thumbnailOrientation`, `thumbnailImageFit` |
| `BookmarkRecord` | `profile_id`, `source_id`, `entry`, `revision`, `modified_at`, `bookmark: EntryBookmark`; complete replacement, no implicit max-position merge |

Login: trim, ASCII lowercase, 3–64 characters `[a-z0-9._-]`, unique across all accounts. Password: 12–128 Unicode scalar values, no trimming/normalization/truncation; Argon2id with fresh random salt. Creation and administrator reset require changing the temporary password; replacement must differ from it. Failed login uses the same error for missing account, wrong password or inactive account.

Profile name: trim, NFC normalization, 1–64 Unicode scalar values, no control characters. Uniqueness key is Unicode default case folding of the normalized name followed by NFC; enforce `(user_id, normalized_name)` in the common transaction, not just the UI. Renaming preserves UUID. `avatar_id` must be present in the embedded catalogue. Manifest format is `{schema_version:1, fallback_avatar_id, avatars:[{id,file}]}`; IDs unique, files relative without traversal/absolute paths, allowed verified PNG/JPEG/WebP MIME. Missing files/invalid manifest fail the build. A removed avatar renders using fallback without rewriting stored identity; a new avatar selection must be valid.

Quota: positive JSON-safe integer, default 3; lowering below current usage does not delete/block existing profiles, but prevents further creation until below the limit. Store only `max_profiles_per_user` in Core's additional `config.json` options. Serialize with all configuration mutations, persist before publishing the effective value; failure preserves the previous effective value. No scraper reconstruction for quota-only updates.

UI language must be null or a supported locale, theme an existing theme key. Playback language lists follow existing normalization and bounds (`MAX_TRACK_PROFILES`, subtitle limits 10); translation keys must belong to the canonical priority. Preserve current enums and defaults from section 2. Display enums are `portrait|landscape`, `contain|cover` and `grid|single-row|list`; thumbnailSizeMultiplier is finite in 0.5–1.5. Boolean preferences must be JSON booleans; securityMode is `safe|confirmation|unsafe`. Track appearance accepts only existing `TEXT_TRACK_SETTINGS_KEYS`. Positions are null or finite non-negative seconds; episode number null or positive integer; orders non-negative JSON-safe integers. Resolve home/default/section references only within the authorized profile. Removing a default home clears defaultHomeId in the same streaming transaction. Reorder requests provide the complete relevant ID sequence, with no duplicates/missing/foreign IDs.

## 4. Operation surface and errors

`<api>` means the configured controller API prefix under the application root, normally `/api`. Names below are target commands, also used by Tauri. GET reads use query parameters; POST mutations use JSON bodies and are rejected over GET (405). New APIs return `{data: T, error: null}` on success, `{data: null, error: {code, message, field_errors, operation_id}}` on failure. `message` is English, UI translates stable codes; field_errors maps field paths to English messages. No scraper aggregation envelope for these APIs.

Every profile request carries `profile_id`; session/admin context is injected by the transport, never accepted as an authoritative `user_id` field. Every durable mutation carries a client UUID `operation_id`; key is scoped to actor and kind. Store a normalized request digest and result; same ID/body resumes or returns original result, different body returns conflict. Keep receipts for the account lifetime, or profile lifetime for profile-only operations, and prevent in-flight retries from recreating a deleted profile. Authentication/ownership is checked even when replaying a receipt.

| Method / command | Request → data |
| --- | --- |
| POST `accounts/login` | `{login,password}` → SessionView; session delivered only via protected transport |
| GET `accounts/session` | `{}` → SessionView (minimal anonymous status allowed) |
| POST `accounts/change_password` | `{operation_id,current_password,new_password}` → SessionView; revoke other sessions, rotate current secret |
| POST `accounts/logout` | `{operation_id}` → `{revoked:true}`; clear transport secret, repeat safely |
| GET `profiles/list` | `{}` → ProfileList |
| POST `profiles/create` | `{operation_id,name,avatar_id,common_values}` → OperationView; common profile only |
| GET `profiles/get` | `{profile_id}` → ProfileSnapshot |
| POST `profiles/update` | `{operation_id,profile_id,expected_revision,name,avatar_id}` → ProfileView |
| POST `profiles/delete` | `{operation_id,profile_id,expected_revision}` → OperationView; UI confirmation required |
| POST `profiles/initialize_streaming` | `{operation_id,profile_id,values}` → OperationView; create defaults/documents once |
| POST `profiles/save_preferences` | `{operation_id,profile_id,common?:{expected_revision,values},streaming?:{expected_revision,values}}` → OperationView; at least one domain, no initialization by accident |
| POST `profiles/complete_onboarding` | `{operation_id,profile_id,scope,expected_revision}` → OnboardingView; required identity/settings already confirmed |
| GET `profiles/operation` | `{operation_id}` → OperationView, owner/admin only as appropriate |
| GET `profiles/avatars`, `profiles/avatar/<avatar_id>` | `{}` → AvatarCatalogue / image bytes |
| GET `profiles/homes`, `profiles/sections`, `profiles/bookmarks` | `{profile_id}` → `{items: Record[], revision}`; collection revision orders/reconciles changes |
| POST `profiles/save_home`, `profiles/save_section`, `profiles/save_bookmark` | `{operation_id,profile_id,expected_revision,value}` → corresponding Record; value contains home/section/bookmark identity and document |
| POST `profiles/delete_home`, `profiles/delete_section`, `profiles/delete_bookmark` | `{operation_id,profile_id,expected_revision,identity}` → `{deleted:true}` |
| POST `profiles/reorder_homes`, `profiles/reorder_sections` | `{operation_id,profile_id,expected_revision,ids}` → `{items,revision}`; expected collection revision |
| GET `admin/users`, `admin/profile_settings` | administrator → `{users:[AccountView + state,created_at]}` / `{max_profiles_per_user}` |
| POST `admin/create_user` | `{operation_id,login,temporary_password}` → AccountView |
| POST `admin/delete_user` | `{operation_id,user_id}` → OperationView; UI confirmation required |
| POST `admin/reset_user_password` | `{operation_id,user_id,temporary_password}` → `{user_id,must_change_password:true}` |
| POST `admin/save_profile_settings` | `{operation_id,max_profiles_per_user}` → effective `{max_profiles_per_user}` |

`identity` is `{home_id}`, `{section_key}` or `{source_id,entry}`. `value` is `{home_id,definition}`, the SectionRecord content, or `{source_id,entry,bookmark}` without server metadata. New home creation omits home_id and receives a server UUID; an optional temporary definition.id is replaced by that UUID. Updates require the existing home_id and matching definition.id; retries reuse the generated UUID. Record revisions and collection revisions are separate; every collection mutation increments its collection revision transactionally. Empty collections start at revision 1.

| Stable error | HTTP | Client behavior |
| --- | --- | --- |
| `invalid_credentials`, `session_required`, `session_expired`, `session_revoked` | 401 | Show login, invalidate personal caches for invalid session |
| `password_change_required` | 403 | Password replacement gate; no content/profile access |
| `admin_required`, `origin_rejected` | 403 | Refuse operation |
| `profile_inaccessible`, `resource_not_found` | 404 | Do not reveal foreign resource existence; revalidate selection |
| `invalid_data`, `unsupported_schema_version` | 422 | Preserve draft, display field errors |
| `login_taken`, `profile_name_taken`, `profile_quota_reached` | 409 | Refresh affected list/quota, preserve input |
| `revision_conflict`, `operation_id_conflict`, `operation_in_progress` | 409 | Reload/compare explicitly; never silently overwrite |
| `storage_unavailable` | 503 | Preserve draft; retry with same operation ID |
| `method_not_allowed` | 405 | Correct transport method |

Pending coordinated work returns 202 with OperationView; complete work returns 200. A transient failure after a durable commit includes operation_id so the client can poll/resume. Snapshot exposes pending work and individual domain revisions; the UI must not claim that the whole draft was saved. Passwords must never enter operation receipts/logs: store only hashes/internal intent needed for atomic operations; authenticated retries use the bound receipt without retaining plaintext or a password-derived request digest.

## 5. Consistency and lifecycle

Accounts have `active|deleting` states. Profiles have `creating|active|deleting`; onboarding is separate. Non-active identities cannot authorize streaming access. Operation kinds: `create_profile`, `initialize_streaming`, `save_preferences`, `delete_profile`, `delete_user`; durable states `pending|running|retry_pending|completed`, with a stored step, expected revisions, intended versioned payload, actor and last contextual error. Local single-store mutations/receipts are committed atomically. Permanent validation/revision failures before the first commit create no pending business operation.

| Workflow | Durable ordered checkpoints |
| --- | --- |
| Common creation | Reserve quota/name and creating UUID plus operation → insert common defaults/onboarding → mark active and completed; no streaming data |
| Streaming initialization | Lock/revalidate active owner → persist intent → initialize streaming settings at same UUID → mark service onboarding pending and complete operation |
| Mixed preferences | Lock/revalidate → validate both revisions before any write and persist intent → commit common values → commit streaming values → complete; retry skips already committed steps by operation marker, never by guessed value equality |
| Profile deletion | Lock → mark deleting plus operation in common store → delete all streaming rows → delete common settings/onboarding/identity and finish receipt |
| Account deletion | Mark deleting and revoke sessions atomically → drain/coordinate all child profile operations and deletions → remove common account data and complete administrator receipt |

The same per-profile serialization guard covers validation, streaming writes and deletion, across all windows/requests. Account lifecycle guard precedes profile guards; acquire multiple profiles in sorted UUID order. Revalidate account/session/profile inside the guard before each write. A deletion waits for in-flight profile commits, blocks new ones and supersedes/drains unfinished saves before cleanup. Recovery never recreates streaming rows for a deleting/missing identity. Quota/name uniqueness also requires a common-store transaction across concurrent creations; a UI lock is insufficient. Deployment uses one application writer per data directory unless database-level equivalent coordination is supplied.

Mixed saves deliberately use forward recovery, not a claimed global rollback. They block other mutations of the same profile while incomplete, but expose confirmed per-domain snapshots and pending state. Restart reconciles operation markers before serving profile writes. Repeated deletion is safe; the operation receipt survives identity cleanup for its authorized caller. Suppression of the last profile returns the user to onboarding. Reorder/default-home changes remain atomic within streaming. Revision conflicts require user reload/comparison/reapplication, not last-writer-wins or maximum playback position.

Playback writes: at most once per 10 seconds during playback, plus pause/end/media switch. Serialize per bookmark; collapse pending unsent progress, send next only after prior revision is confirmed. A clean connection can lose up to the last 10 seconds on abrupt closure; network failures can lose more, show an error and keep pending state in memory. No guaranteed unload write and no offline persistence. Profile switching flushes where possible, discards stale callbacks and never sends old progress under the new UUID.

## 6. Sessions and protected-route inventory

Browser session secret: cryptographically random, HttpOnly cookie, SameSite=Lax, scoped to the API prefix under the application root, Secure on HTTPS; explicit non-TLS local deployments cannot set Secure. Store only its cryptographic digest in `user_sessions`, with user_id, last activity, expiry/revocation. Every authenticated use validates the account, mandatory-password flag and persisted session; extend idle expiry by 30 days. Logout/reset/delete invalidate access on the next request, including cached catalogue requests and media segments. Already delivered bytes cannot be recalled.

Restrict write origins to the application origin (including login/logout); validate Origin or same-origin Referer, reject unverifiable browser writes. Do not grant broad credentialed CORS. Personal/session responses use `Cache-Control: no-store`. All cookie handling honors the configured root. Content sessions cannot authorize administrator operations and administrator cookies cannot authorize public content.

| Existing/future surface | Required access |
| --- | --- |
| Login shell, bundled JS/CSS/fonts and necessary default assets | Anonymous only for rendering/login; no personal/catalogue prefetch |
| `accounts/login`, minimal `accounts/session` | Anonymous permitted; no profiles/content in anonymous response |
| `accounts/change_password`, `accounts/logout` | Valid session; permitted during mandatory replacement |
| `search`, `load_home`, `get_service`, `list_lives`, `get_category`, `get_section`, `get_recommendations`, `get_banners`, `get_players`, `get_entry`, `get_season`, `get_live`, `get_stream` | Valid unrestricted public account; check before scraper work or ETag/304 |
| `proxy` and **all descendant paths** | Same account authorization for images/media/manifest rewrites, HLS/DASH initialization/segments, audio, subtitles and thumbnails; range requests included |
| `get_drm_license` (binary POST) | Same authorization before reading/forwarding license body |
| `translation/get_services`, `translation/translate_url` | Same authorization, including streaming translated subtitle responses |
| All `profiles/*`, avatars/images included | Unrestricted session; verify active ownership on profile-specific operations |
| `admin/*`, administration UI operations | Existing independent administrator authentication; new user/quota operations use it too |
| Tauri IPC commands and custom-protocol binary routes | Same public-session/ownership checks; window presence is no authentication |

Inventory sources: `reloadable_stream_scraper.rs::register_routes`, `stream_scraper/registration.rs`, translation `api.rs`, Core REST `service.rs`, Tauri `ipc.rs`, `protocol.rs`, `stream_response.rs`. Provider-bound outgoing requests are separate; these checks concern application entry points. No anonymous alternate media route is allowed. Third-party media fetched directly by an already authorized player remains subject to the provider's controls; Arachnea cannot revoke a URL already disclosed.

Tauri now captures native webview label/document URL for IPC and custom-protocol requests. External documents cannot supply this metadata; administration commands require the trusted admin window. The application session broker binds trusted public tabs to the same persisted session, stores its secret in the OS vault by installation/account and gives JavaScript session state only. New tabs restore through this broker. Keychain, Credential Manager and Linux Secret Service (`secret-tool`) back the vault; unavailable/locked storage produces a visible memory-only session, with login required after restart and no plaintext fallback. Native invalidation events complement browser BroadcastChannel, focus/visibility and 30-second session polling. Interactive native lifecycle and vault checks remain manual validation work.

## 7. Existing checks and acceptance boundaries

No test code or test infrastructure is added in step 1. Source inventory found no frontend test/spec suites or test scripts in either app. Existing backend suites to reassess during implementation:

| Existing suite | Affected contract |
| --- | --- |
| `arachnea-core/tests/persistence/sqlite_store.rs`, `tests/persistence/typed_store/{memory,file}.rs`, `tests/support/{persistence,typed_store}.rs` | Typed schemas, shared database/table views, local atomicity and constructor parity |
| `arachnea-core/tests/controler/rest_settings.rs` | JSON configuration precedence, persistence failure and hot application |
| `arachnea-scrapyfy/tests/scrapyfy/admin_settings.rs` | Additional configuration options, preservation of existing settings |
| `arachnea-stream/tests/stream_scraper.rs`, `tests/support/stream_scraper.rs`, `tests/support/application.rs` | Memory-backed constructors, application composition, catalogue handlers |
| `arachnea-stream/tests/stream_resolver.rs`, `tests/support/stream_resolver.rs` | Proxy/media/license URLs and registration context |
| `arachnea-scrapyfy/tests/scrapyfy/global_etag.rs` | Cached catalogue responses remain behind authentication |

Current scraper/resolver tests do not prove route authorization. There are no existing account/profile/session suites; manually verify restart/idle expiry, reset restriction, account isolation, concurrent quota/name checks, per-store failure recovery, late writes during deletion, anonymous GET/POST/range/media rejection, prefix handling, Tauri tab/protocol context, avatar packaging, conflict/draft retention and inactive old browser data. Do not create tests without an explicit request. Run existing affected suites and Rust/frontend checks when runtime changes begin. For step 1, validate document links, inventory coverage, bilingual parity and `git diff --check`; compilation is unnecessary for documentation-only changes.

## 8. Step 2 implementation and operational limits

`ProfileStores::open(application_path)` creates/reopens only the two new databases under `data/`, with six common tables and four streaming tables. Existing typed schema metadata reconciles physical declarations; profile documents and operation payloads start at `PROFILE_DOCUMENT_VERSION = 1`. UUIDs are typed in Rust and persisted as canonical text, including the composite reference keys. Common profile creation checks quota and name uniqueness in the same transaction that reserves the identity and durable intent. The effective quota and validated names/avatar/preferences are injected by later services; no configuration/admin endpoint is implemented here.

`ProfileStores::memory()` uses two independent in-memory SQLite databases for identical local transaction/recovery semantics without files. Existing scraper memory constructors are preserved. `main.rs` composes the profile stores separately from scraper stores, resumes pending operations before starting controllers, then validates account/profile/settings/onboarding/session and streaming references. Recovery/schema/reference failure aborts startup with context; it does not delete orphaned data. Recoverable operations remain durable for the next startup/retry.

The common repository treats operation payloads as opaque versioned JSON and has no streaming table dependency. The application coordinator supports common creation, first streaming initialization, mixed preference saves and profile/account deletion. It stores receipts and applied-operation markers, supersedes pending intents on deletion and retains deletion receipts after cleanup. Cloned stores/coordinators share lifecycle guards. This implementation serializes mutations per account (including different profiles), with a profile guard where applicable, and retains guards inside blocking tasks through cancellation. One application writer per data directory is required; no cross-process coordinator is provided.

Common settings are typed; nested streaming home/section/bookmark/settings payloads use versioned JSON envelopes pending the detailed service validators/APIs in later steps. Repository/coordinator methods are internal application building blocks, not authentication boundaries; callers must supply validated data and authorized identity. At step 2 the authentication/routes remained pending; step 3 below implements accounts, sessions and content guards. Avatar checks and profile routes were pending at that stage; step 4 below implements them.

The existing SQLite round-trip scenario was adapted to exercise atomic multi-table commits/rollback, composite uniqueness, cross-database rejection, reopening indexes and in-memory operation; no new test suite was added. At step 2, crash/restart and concurrent-deletion workflows were not covered. Step 3 manually validates partial-commit recovery and populated account cleanup; deletion during real concurrent playback remains a manual check. Existing scraper tests do not cover the new account workflows.

Step 2 checks passed from `server/`: `cargo fmt --all --check`, `cargo check -p arachnea-stream --offline`, the 9 existing Core SQLite tests, and 14 Stream library tests (5 live-provider scenarios ignored). Stream needed a rerun with local socket permissions because the sandbox denied its HTTP test servers. `git diff --check` also passed.

## 9. Step 3 implementation and validation

Account login/state/password-change/logout and administrator users/create/reset/delete operations are implemented with the section 4 envelopes. Temporary passwords always require replacement. Hashes use the shared Argon2id primitive; only SHA-256 digests of random session secrets enter SQLite. Reset and password replacement atomically revoke previous sessions; replacement rotates the current secret. A successful login atomically revokes the previous session presented by this transport. Independent account sessions on other devices remain available unless reset/password-change/deletion revokes them.

POST JSON and verified same-origin writes apply to account mutations; `login_rate_limited` (429) bounds failed public sign-ins. Validation can include field errors; storage failures expose a safe diagnostic and operation ID without credentials. Password-bearing local operations keep completed non-secret receipts atomically; repeated requests replay the original result without storing plaintext or password-derived request digests. If a password-change response is lost before receiving the new cookie, sign in with the new password.

`ContentController` wraps every scraper JSON/binary registration before callbacks and ETag validation, including proxy descendants/ranges, manifests/segments/subtitles/images, DRM and translation. The login shell and its assets are public; the admin UI retains its independent policy. That existing policy permits loopback and native-admin access without a public account, and requires administrator authentication for remote server callers. Public cookies grant no admin permission; native public webviews cannot invoke admin commands.

Server idle expiry advances on each authenticated request. Browser cookies are renewed by the fast session-state heartbeat (every 30 seconds while visible); slow content/media responses do not reissue an old account cookie after an account switch. Cookie paths follow the incoming API prefix. HTTPS Origin/Referer preserves Secure; session reads without scheme metadata do not rewrite the cookie. Reverse proxies must preserve consistent Host and Origin/Referer and use the configured application root. Native events/browser BroadcastChannel clear in-memory context, and delayed replies are rejected by generation.

Public Vue uses a separate AccountSessionGate/credential form and initializes content only after unrestricted authentication. An independent administration entry is available before login for first-account setup. Admin Vue adds user management, temporary-password forms and deletion confirmation. Existing local personal storage remains active after login until steps 5–8; no import, server profile guide or profile API is claimed at this stage.

Manual HTTP validation used an isolated copied server with generated accounts/databases under `/private/tmp` and a `/stage3` prefix. Verified anonymous JSON/proxy/range/license/translation rejection, methods/origins, normalized unique logins and creation replay, mandatory password gate, password replacement, allowed catalogue access, reset revocation across two sessions, logout, forced idle expiry, restart session restoration, recovery of a partially committed mixed save without double revision, full deletion cascade across both stores, safe deletion replay and retained independent local-admin access. The temporary server was stopped afterwards. Native graphical tab/vault behavior and Linux/Windows execution remain platform checks; no new test suite was introduced.


## 10. Step 4 implementation and validation

REST/Tauri profile operations from `profiles/list` through `profiles/operation` in section 4 are implemented. Strict DTOs reject unexpected fields. Session/mandatory-password validation runs again inside the account/profile lifecycle guards before store access, so reset/deletion cannot race a validated mutation. Identity names use trim/NFC and Unicode default case folding followed by NFC. Ownership, name uniqueness, quota and revision checks run in common transactions. Single-store identity/onboarding changes have atomic receipts; mixed saves reuse forward recovery. Snapshots omit internal uniqueness keys, password data and durable operation payloads. Operation polling returns 202 for incomplete work, 200 for completed receipts.

Common preferences accept the existing `en`/`fr` locales (or null) and seven frontend theme keys. Streaming initialization is explicit and fills omitted fields from crate-owned `resources/profile-stream-defaults.json`; full saves require every supported key. It validates enums, booleans, bounds, track shapes, subtitle appearance and profile-local home references. ISO aliases/casing/separators follow the frontend language normalizer; the legacy subtitle list is returned equal to canonical priority. The alias table, limits, themes/locales and defaults must stay aligned with frontend catalogues. Initializing a common profile creates no streaming data; initializing streaming creates no homes/sections/bookmarks.

`resources/avatars/avatars.json` ships three PNG images. The build rejects unsupported manifest versions, duplicate/invalid IDs, unsafe paths, missing files, escaping symlinks and unsupported image signatures; generated `include_bytes!` references embed all images in the executable. `profiles/avatars` returns controller-derived URLs and MIME types; image routes are authenticated and no-store, including native binary requests. The catalogue fallback handles a subsequently removed avatar without rewriting its stored identity. New selections must exist in the current catalogue. No runtime repository/avatar directory is required.

Admin Settings contains an independent profile-limit form, also in desktop. `admin/profile_settings` and `admin/save_profile_settings` read/persist `max_profiles_per_user`, default 3, as a Core additional option in `data/config.json`. The shared admin mutation lock serializes quota changes with port/network/password/proxy updates; persistence succeeds before publication and quota-only writes do not rebuild the scraper. Positive JSON-safe integers are accepted; lowering preserves existing profiles and prevents further creation while at/above quota. A `profile_quota_operations` additional-option receipt map is committed in the same JSON for durable idempotency; replay returns the original result. Invalid persisted quotas prevent startup.

Manual isolated HTTP checks with `/stage4` verified embedded authenticated PNG bytes without an external avatar directory, anonymous/restricted rejection, account isolation, Unicode uniqueness (`Straße`/`STRASSE`), creation/update/delete replay, strict fields and revisions, six concurrent creations for two remaining slots, lazy initialization, mixed preferences/onboarding, quota decrease/increase and invalid values, failed JSON save retaining the effective value, full cascading cleanup and zero remaining profiles. Restart of the final binary preserved quota, session and profile identity; alias normalization and invalid language/bound/locale rejection were also checked. A zero-profile list supports the guide; profile selection and the graphical guide are still step 6. Rust build/check/format, 14 Stream tests (5 live-provider tests ignored), 75 Scrapyfy tests and admin type-check/build passed. Socket tests required local-network permission. No new test suite was added. Graphical Tauri/native-platform validation remains outstanding.


## 11. Step 5 implementation and validation

`streaming/operations` and `streaming/validation` implement the collection/CRUD/reorder endpoints in section 4. `coordination/streaming_api` revalidates the session, active ownership and pending coordinated work inside the shared lifecycle guards before using the streaming transaction. Streaming must already be initialized; personal writes never initialize it accidentally. Catalogue scraping is unchanged.

Two typed tables are added to the existing streaming database: `stream_profile_collections` holds collection revisions (initially 1) and deleted record versions; `stream_profile_operations` holds normalized requests and completed results. Each local mutation commits its documents, collection increment and receipt together. Replays validate ownership before returning the original result; changed bodies conflict. Tombstones prevent an old section/bookmark revision from becoming valid after deletion and recreation. Profile deletion removes both metadata tables along with its data; startup reference checks include them. `profiles/operation` can inspect completed streaming receipts without exposing their request body. No new physical store is introduced.

Home schema 1 preserves references, blocks/display options, navigation order, merging and bookmark visibility. IDs/timestamps are server-owned; creation omits home_id, updates carry matching home_id/definition.id. URL uniqueness follows frontend NFKD/mark removal/lowercase/underscores. Client timestamps map to the exact server record revision already read, never an automatically fetched replacement revision. Reordering requires a complete distinct profile-local ID list and the collection revision, preserving all other fields. Removing the default home clears defaultHomeId and increments settings in the same streaming transaction. Default destinations are null/aggregation or a home owned by this profile; old local service:* selections are not imported.

Frontend home/section/bookmark services use the new no-store REST/Tauri transport and no longer access IndexedDB. Focus/visibility refreshes read the server; pending bookmark snapshots stay in memory and are not rebased onto newer server revisions. Writes retain stable retry IDs after uncertain responses. Missing profile context yields empty lists and refused writes; `setActiveProfileContext()` is the explicit integration point for the step 6 selector after validating ownership and initializing streaming. No first-profile selection or creation is inferred here. Other local preferences remain until steps 7–8.

The player forwards current progress into memory; the bookmark queue writes at most once per 10 seconds during ordinary playback, coalescing unsent updates and waiting for the prior acknowledgement. Pause/end/media changes, unmount and hiding attempt an immediate flush. Lost responses replay the immutable request before the newest pending snapshot. A visible status/error component exposes unconfirmed writes and explicit retry; conflicts require reload/reapplication, never silent last-writer-wins. Profile switching attempts a 5-second bounded flush under the old UUID and drops stale replies/callbacks. Pending data has no offline persistence and abrupt closure has no delivery guarantee.

Validation passed: Rust format/check/build, 14 existing Stream tests (5 live-provider tests ignored), public type-check/build and the new component's CSS lint. Isolated HTTP checks at `/stage5` cover composed homes, invalid nested schemas/fields/credential descriptors, server IDs/times, accent-equivalent slugs, complete order/revision conflicts, section display preservation, episode bookmarks/zero progress, profile/account isolation, immutable replay, concurrent writes (one of six succeeds), tombstones, atomic default clearing, injected receipt-write failure rolling back data/counter, full profile/account cascade across six streaming tables, concurrent deletion and restart/replay. Manual runs of actual frontend modules verify 10-second coalescing, pause/lost-response/profile flushing and real REST adapters without any local persistence. No tests/infrastructure were added to the repository. Graphical selection/onboarding, real-provider playback and native Tauri windows/vaults remain later validation work.
