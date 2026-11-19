# front

The public application uses Vue 3, TypeScript, Vite and Video.js.

Video players consume the structured `streams[]` backend contract without
legacy payload adapters. Shared metadata is read from the same parent as
`streams`; the former `stream` wrapper is unsupported. A source has no ID and
carries its URL, canonical format, optional `quality` and language, and its
own DRM context. The renderer owns
variant selection, preferring HLS, DASH, MP4, then other supported files. Language
and fixed-quality menus live in the player alongside adaptive manifest controls.
Fatal errors try remaining variants before emitting one final error; active loads
have a 20-second watchdog. Shared storyboard, chapters, subtitles and compatible
playback position survive source switches. This applies to episodes, live,
trailers and resolved banner/background videos. Independent providers stay separate.

For entry and episode server selectors, native candidates precede iframe embeds,
with backend order preserved within each group. Embed links provide an initial hint;
resolver results refine the ordering. An `embed-link` can resolve to native video and
never confirms an iframe by itself. Known native streams come first, unresolved candidates
next and confirmed iframes last. A newly discovered iframe waits until remaining native
and unresolved candidates have been tried, including other `embed-link` candidates,
except when explicitly selected or retained by name
after a language change. Each entry/episode starts at the first sorted server in the
selected language. Changing language first tries the current server's name when present.
Resolution failures or exhaustion of a server's video variants advance through remaining
servers in sorted order after the preferred initial attempt. Each failed server
is skipped until the episode, language or explicit server selection changes. Only server
exhaustion displays the final error; server fallback never changes the selected language.
Explicitly selecting a server disables server fallback for that selection: resolution or
final video errors are displayed immediately. Changing the episode, entry or language
restores automatic server fallback, including when the same server name is retained.
Episode defaults and the reset of an explicit server selection are applied before stream
resolution, so each new episode starts with its first sorted candidate.
Deferred episode lists and stale resolver responses cannot restore another episode's
players. Iframe fallback can only react to browser-reported errors; failures inside a
cross-origin embedded player may be invisible to the application. Iframe security and
confirmation policies still apply.

Deploy this frontend with the matching server/YAML contract. Run `npm run type-check`
and `npm run build-only` to validate changes. Browser playback still requires manual
qualification of the providers, media engines and DRM environments in use.

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Vue (Official)](https://marketplace.visualstudio.com/items?itemName=Vue.volar) (and disable Vetur).

## Recommended Browser Setup

- Chromium-based browsers (Chrome, Edge, Brave, etc.):
  - [Vue.js devtools](https://chromewebstore.google.com/detail/vuejs-devtools/nhdogjmejiglipccpnnnanhbledajbpd)
  - [Turn on Custom Object Formatter in Chrome DevTools](http://bit.ly/object-formatters)
- Firefox:
  - [Vue.js devtools](https://addons.mozilla.org/en-US/firefox/addon/vue-js-devtools/)
  - [Turn on Custom Object Formatter in Firefox DevTools](https://fxdx.dev/firefox-devtools-custom-object-formatters/)

## Type Support for `.vue` Imports in TS

TypeScript cannot handle type information for `.vue` imports by default, so we replace the `tsc` CLI with `vue-tsc` for type checking. In editors, we need [Volar](https://marketplace.visualstudio.com/items?itemName=Vue.volar) to make the TypeScript language service aware of `.vue` types.

## Customize configuration

See [Vite Configuration Reference](https://vite.dev/config/).

## Project Setup

```sh
npm install
```

### Compile and Hot-Reload for Development

```sh
npm run dev
```

### Type-Check, Compile and Minify for Production

```sh
npm run build
```

### Lint CSS

```sh
npm run lint:css
```

### Lint CSS and auto-fix

```sh
npm run lint:css:fix
```


## Embedded media security

YouTube embed normalization accepts HTTP(S) URLs on `youtube.com`,
`youtube-nocookie.com`, their dot-separated subdomains, and the exact `youtu.be`
short-link host. Lookalike suffix domains are not classified as YouTube and do
not inherit its trust in `public/video-sources-whitelist.json`. Safe iframe mode
continues to require an allowlisted source.

## Source homes

Hero banner navigation displays at most ten indicators in an automatically
scrolling window that follows the selected banner, including carousel wraparound.
The active indicator is a 23px pill (two 8px dots plus their 7px gap). Indicators
outside the window are hidden from keyboard navigation and assistive technology;
scrolling respects reduced-motion preferences. Indicators remain hidden on mobile.

`/` keeps the aggregated catalog. `/source/:sourceId` loads only the exact
backend source selected by the route. The navigation source group uses service
metadata and registry-derived `supported_queries`, without loading every
provider home. Direct links, reloads and browser history preserve the selection.
Unknown/unavailable sources never fall back to the aggregated catalog.

Source homes display their own bookmarks only. Section order and preference
writes use profile-local, source-scoped keys through the authenticated server API.
Pinned records preserve their collection mode, orientation and image fit; the
former IndexedDB section/bookmark stores are no longer active or imported. Deferred
banner and section pagination updates are guarded against stale catalog loads.
Navigation includes metadata refresh on errors and the catalog includes retry actions.
Custom homes and source destinations are available through the permanent sidebar.

## Catalog section layouts

Home rails keep their saved collection mode (single row by default). Category
catalogs display a single section as a multi-row grid and multiple sections as
one row each. In both screens a chevron beside the section title toggles between
the single-row and multi-row layouts without changing the section thumbnail
orientation or image fit; the arrow only appears for those two modes. The
chevron choice is local to the current screen and is not persisted.
The floating scroll button scrolls up to the nearest expanded (multi-row)
section title, positioned just below the sticky toolbar. When no expanded
section starts above the viewport, it returns to the page top.

## Custom home rendering (lot 2B)

Public accueil URLs use `/home/<title_slug>` (lowercase, accents removed, words
separated by underscores). Internal UUIDs remain unchanged for preferences and
default-home selection. Save/rename checks URL uniqueness inside the same streaming
SQLite transaction, including concurrent views/devices. Renaming updates the active route; legacy
`/homes/<UUID>` links redirect to the current title URL. Existing collisions are
reported for manual renaming, never automatically renamed or deleted.

The home management dialog includes one persisted default landing page selector
(removed from the general settings): All, an existing
custom home belonging to the selected profile. The default reference is saved in
server streaming preferences with a revision check; deleting that home clears
the reference atomically. Older local `service:*` defaults are not imported. Every navigation to `/`
and brand clicks resolve that destination. On startup, profile-dependent router guards wait for an active profile. The mounted application resolves `/`, `/home/<title_slug>` and legacy `/homes/<UUID>` again after the profile is restored or selected, because initial navigation can run before authentication/profile activation. Direct custom-home links are therefore resolved against the selected profile rather than an empty pre-profile list. This redirect replaces the root history entry and ignores obsolete profile responses or a route changed during resolution; query/hash values are preserved and
explicit service/custom/search routes remain untouched. The home manager also
shows saved homes in a table with a first-column default checkbox and tooltip-labeled
icon actions for editing, reordering and confirmed deletion. The table has no visible
column headings, names are plain text, and creation appears at the left of its final row.
Editing or creating from this list opens a separate dialog; Save and Cancel return
to management without changing the route. Direct editing retains its existing flow.
Direct menu editing never exposes the management list while loading, saving or closing;
the editor snapshot is retained during the closing transition.
Checking a home replaces
the unique default; unchecking it selects All. Close uses the Save button styling.
Existing custom-home selections remain
compatible; service selections use the `service:` prefix in `defaultHomeId`.
Deleted, unavailable or unreadable destinations fall back to All without selecting
another service. Creation
is available through the Home heading edit control and content-add dialogs, not a
separate sidebar entry. Management uses a compact 680px dialog, expanded to 1280px
for editing tables, shared parameter switches/theme colors, service titles and no technical
reference keys. Stored reference identities remain unchanged.
The dialog title and Save/Cancel footer remain visible while only the body scrolls.
The discovery table has four columns (type, label, two action columns), including
centered action buttons without visible action headings; single actions occupy
the rightmost column. The section draft display menu is enabled independently
of the hidden pin/movement controls.
source-banner collection additions and individual subsection rows. Section rows
reuse the display-options menu; block defaults are held in the detached draft and
saved atomically with the home. Existing rendered-group style overrides retain
priority; merged groups use the first contribution's block defaults as fallback.

Open `/homes/:homeId` to render an existing persisted definition. Creation/editing
is available through the sidebar manager.
`HomeCatalog` accepts `mode="custom"` and a validated `customDefinition` for rendering.
The definition is validated before discovery; empty homes contact no providers.
Only referenced sources and category blocks are discovered/loaded. Selected
sections hydrate near the viewport; banner hydration starts from the hero.
Section hydration is concurrency-limited, and in-flight discovery/category/section
responses are shared without sharing mutable block cursors.

Title merge is optional, compares NFC/case/whitespace-normalized effective titles,
preserves accents/punctuation, interleaves contributions and uses existing media
deduplication. Group preferences are scoped by home and member ids. Unselected
subsections remain inside separate parent rails rather than merging temporal variants.
Group display options apply the saved layout (single row, grid or list) to the
rendered collection; the shared setting is labelled “Layout” (“Disposition” in French).
Edition visibility also controls navigation edit/create actions. Custom hero pins
select one or more banner services for confirmed, revision-checked removal while
preserving other content. Source-home toolbar logos retain their aspect ratio;
desktop navigation reserves its measured, content-dependent width.
Active service and custom-home names appear beside the brand in the toolbar with
the same typography, including the separator; custom homes use a home icon.
Source screens omit the duplicate heading above the hero.
Custom-home screens also omit the duplicate title above the hero. Hero pins follow
the service logo; toolbar context icons are vertically centered independently of
the shared brand-text offset. Current navigation entries and Lives are bold.
Navigation groups are Home/Accueil and Video Services/Services Vidéo, with indented single-line children
and their home/projector icons. Clicking Home/Accueil navigates to the configured
default home and its children remain visible; only Video Services is collapsible.
Missing references are reported without same-title substitution; remove and re-add
the content to repair it. Partial failures leave successful blocks usable, with targeted hydration
retry. Category-level pagination uses `get_category`; linked rails use `get_section`.
The declared block order takes precedence over global pinning. Bookmarks are shown
only when enabled. Definitions refresh on focus/visibility; stale responses cannot
overwrite a newer definition. Browser/Tauri interactive validation remains necessary.

### Persistence and source identities

The management overlay uses the shared strong backdrop blur. While editing a
draft, Save/Cancel are the only closing actions; Close remains on the management
list. Draft banners, shortcuts and blocks use compact up/down and trash icons next
to their service/title. Removal remains local until Save and Cancel discards it.
The editor no longer offers a separate Replace reference action or draft preview.
Banners, category shortcuts and blocks use tables with horizontal scrolling on narrow
screens. Block title overrides share their row with service, content and arrow/trash
actions. Save/Cancel use the accent styling of the hero details action.
Editor groups are Home/Accueil, Banners, Categories, Sections and Add content from a service.
With title merging enabled, the Sections table groups blocks by their effective
custom title or saved original label using the renderer's NFC/case/whitespace rules.
Header arrows move whole groups; row arrows reorder contributions within a group.
While editing a title, group membership stays visually stable until the field loses
focus, preserving typing and cursor position; the draft still updates immediately.
Display changes propagate only the changed options to every member of that group.
Category blocks use their saved block title without fetching category columns.
Untitled blocks remain separate; changes stay local until Save. With merging off,
movement and display changes remain individual.
They reuse `ParametersSection` for the preferences typography, colored marker and
themed surface. Service discovery has a group heading and a separate service field label.
Action columns are compact
and right-aligned in all three tables; the service selector explicitly invites
adding content from a service.

The lot 2C manager creates detached drafts, renames, confirms deletion and atomically
reorders homes. The editor orders banners/shortcuts/blocks, overrides titles, selects
merge/bookmarks, edits without a preview step, and saves only
explicitly. Source discovery starts only after service selection. Missing references
stay visible; missing content can be removed and re-added. Save errors/conflicts keep the draft;
cancel and reopen to reload a conflicting revision. Storage success requires commit.
Source-home copying is explicit and refuses ambiguous identities. No arbitrary URLs
are accepted. Edition-only pins add service banners and source sections. A single
pin beside “Explore by category” opens destination selection and category checkboxes.
Only checked categories are resolved, preserving their source contributions. Add
requests use a single dialog: choose an existing home or name a new one, choose
Shortcut/Section for categories, then Cancel/Add. Add resolves all clicked source
contributions and commits one revision-checked definition without opening the editor
or navigating away. Failure keeps the dialog open; cancellation does not write.
Source-page preference namespaces are stripped only for discovery matching; they are
never persisted as content identities. When a displayed rail groups several sections
with the same preference key, all unambiguous source-owned references are added.
References are copied, not linked to the originating custom home. Advanced options
remain in the home editor. Only custom homes support configurable content order;
source and aggregated catalogs ignore legacy pin order without deleting preferences.
Compact translucent pin actions share one style. Category cards remain navigation
links without individual pins. The sidebar heading has an edit icon
instead of a separate management row. The Home icon is shown only at the group heading,
not beside individual custom homes. Custom-home catalogs have no inline edition
controls for banners, categories or sections. Ordering, removal and display options
are configured exclusively in the home manager, with revision-checked saves.

### Responsive shell navigation (lot 2D)

The sidebar orders custom homes, Lives, then capability-filtered sources in a
collapsible group. Home ids and source ids remain the routing identities. Source
navigation uses metadata only, never provider-home preloading. The source group
expansion is stored in `arachnea.navigation.sourcesExpanded` in localStorage;
storage failures are reported without disabling navigation. The stored value is
read once at page load; runtime toggles stay local to the page and are never
synchronized across tabs or windows. Home lists refresh on focus,
visibility and committed local mutations, without replacing editor drafts.

Above 1000px the sidebar is toggled by the leftmost toolbar menu button; its visibility is persisted locally. When visible, it and reserves content width (260px, or
220px up to 1280px). On narrower screens the toolbar opens a native modal dialog
with keyboard focus containment, Escape/backdrop/close dismissal and trigger focus
restoration; navigation closes it. Sidebar editing closes the drawer before opening
the editor. Desktop navigation starts below the measured toolbar and does not
overlap settings or notifications. Fullscreen changes hide navigation and clear
temporary drawer state, including prefixed fullscreen APIs. The brand links to `/`;
Lives is no longer duplicated in the toolbar. Search and history-back
remain there. Route changes clear stale catalog background candidates; source and
custom routes retain their existing titles and Vue Router scroll restoration.
Interactive Safari/Tauri fullscreen, modal focus and independent native-view storage
checks are still required; no browser test dependency has been introduced.

The persistence and discovery APIs are connected to the lot 2B composition renderer
and `/homes/:homeId` route and lot 2C editor. `types/customHome.ts` defines the version 1
declarative schema: stable home/block ids, ordered source banners, category
shortcuts/content references, section/subsection references, optional title
overrides, `sectionMergeMode` (`none` by default or `title`) and bookmarks.
Fetched entries, signed playback URLs, cache validators and pagination are not
part of this schema.

`services/customHomes.ts` creates detached drafts and provides profile-backed
load/list/save/delete operations through REST/Tauri. A new home receives its UUID
and timestamps from the server. Save still accepts the snapshot's last-read
`updatedAt` (or `null` for creation); the adapter maps that timestamp to the exact
confirmed server revision. Delete/reorder require confirmed snapshots. Record
changes and receipts commit atomically; conflicts preserve the editor draft.
Listing and focus/visibility refreshes read only the selected profile's server
collection. Old `custom_homes`, `home_sections` and `entry_bookmarks` IndexedDB
stores are untouched but no longer read or written, and no browser data is imported.

`loadHomeSourceDiscovery(sources)` in `services/rustify.ts` selects only requested
providers and returns independent, unmerged catalogs with availability flags. An
empty selection makes no request; discovery does not hydrate deferred banners or
sections. Existing aggregated normalization stays unchanged. References must be
created from this discovery via `services/customHomeReferences.ts`, not aggregated
catalogs. Identity uses exact source-scoped provider keys, canonical descriptors
excluding pagination, then exact labels as a fragile fallback; it never uses array
positions. Duplicate identities are marked ambiguous and rejected for selection.
Inline-only sections and subsection identity under the parent are retained. A
changed label fallback must be repaired, never matched to a similar title.

Runtime validation rejects unknown fields/schema versions, invalid types,
duplicate block ids, descriptor/source mismatches and recognizable credential
fields. This is defensive validation, not a guarantee that arbitrary provider
parameters are secret-free: only YAML-discovered category descriptors may be
selected, and the future resolver must rediscover them before execution. Repeated
content references remain permitted under distinct block ids. YAML identity
corrections are a separate approval-dependent task; no provider YAML is modified
by these foundations.

## Media collection pagination

Media thumbnails use the same yellow crown for subscriptions and purchase/rental
prices, without displaying the monetary amount on the image. Card detail popups
retain the localized access label and amount, preceded by the crown. The thumbnail
crown exposes that label through its tooltip and accessible name. Free, missing,
and unrecognized price values do not display a crown.

`MediaCardCollection` automatically invokes `onLoadMore` in `single-row` mode when the inline load-more button becomes visible in the horizontally scrollable rail and the page viewport. Visibility is checked again after items are appended or the layout changes. Automatic loading requires `haveMore`, pauses while `isLoadingMore` is true, and stops on `loadMoreErrorMessage` so failures can be retried manually using the button.

Grid and list layouts keep automatic pagination opt-in through `autoLoadMore` (default: `false`), with a 200px vertical preload margin. The button remains available for manual loading in every layout.

## Desktop tabs

In Tauri desktop mode, the native host creates a dedicated webview for `DesktopTabShell.vue` and an independent content webview for each tab. The injected `window.__DESKTOP_TAB_SHELL__` flag selects the shell at bootstrap; normal browser and content webviews continue to mount `App.vue` with Vue Router. The shell uses `useDesktopTabs` and the typed `desktopTabs` bridge to receive native state and request tab actions.

The tab strip supports creation, activation, closing and arrow/Home/End keyboard navigation. Closing the final tab opens a fresh home page. Hidden tabs keep their page state and can continue media playback; closing a tab destroys that page. Administration remains in its dedicated window. The strip is hidden with only one tab, and while a managed page owns browser fullscreen, giving the page the full content height. From two tabs onward outside fullscreen, native host bounds reserve 44 logical pixels for the strip, matching its CSS height. The native controller can alternatively select independent window mode through `TauriControlerConfiguration::browsing_mode`; that mode mounts the regular application root.

Settings are opened from the bold Settings entry below Video Services, in a scrollable
modal dialog rather than a right toolbar popover. The toolbar settings button is removed.
The Video Services expansion state remains persisted independently of sidebar visibility.

Content navigation uses the single registry in `src/services/mediaRouting.ts`:
`collection` opens `get_category`, exact `video/live` opens `get_live`, and `video`
/ `video/*` opens `get_entry`. `audio/*` and `images/*` (and their base families)
temporarily open `get_entry`; dedicated views remain future work. The same route
builder serves native links, selections, banners, search and recommendations.
Unknown/conflicting explicit types and incomplete targets are non-openable;
absent/empty types preserve legacy entry navigation. Types are normalized and
retained through pagination, merged rails and custom homes. Collection targets
preserve the complete source request (`request.query_url` required), display an
“Open category” action and have no entry URL. No saved-home migration is needed.
Future media views extend this registry instead of individual card components.

Episode lists inside an entry use `selectionMode="local"` on the shared card
collection. Their actions are buttons that select an episode in the current player,
including episodes with inline players and no episode URL (such as Anime-Sama and
French Animes). The episode mapper supplies a source only when inline players or a
deferred player link are available. Other collections default to route selection
and retain their validated native links.

Category artwork uses two decorative thumbnail layers behind the foreground image
in grid, single-row and list layouts. The rear layers reuse the foreground artwork
after its lazy image loads, with a darker treatment. They use `--bg-surface-strong`
when artwork is unavailable. The layers do not add interactive elements or alter
entry/live cards.

Live catalog cards carry `request.channel`, an opaque source-owned channel or
event identifier, and navigate independently of the channel list. They retain a
separate live target instead of an entry-details URL. Optional catalog title,
artwork, description and public URL survive reloads in the existing v1 live route.
The player requests `get_live` immediately even if `list_lives` is pending, empty
or fails; stale selections and stream resolutions cannot overwrite a newer route.
France TV classifies `is_live` videos per card, keeps collection/category/event
navigation in its rails, includes live and collection rails, and preserves card
order. Its static `get_live` emits the existing live resolver without a catalogue
request. Search/recommendation media anchors and saved custom-home formats remain
compatible. Actual authorized playback still depends on the provider.


Chapter metadata is filtered when loading stream data: only finite time ranges
with an end strictly after their start are retained. Placeholder intro/outro
entries with both timecodes at zero are discarded, so they cannot create an
empty chapter selection menu. Valid chapters starting at zero are retained.

Subtitle translation choices use the same English-name/native-name label before
and after generation, for example `French - Français (Automatic translation)`.
The automatic translation marker follows the interface language. The native
subtitle menu grows to fit its labels up to 70% of the viewport width, then wraps
long entries to keep them readable.
Translation targets remain visible while generation is in progress; activating
the pending target again does not restart the request.

At the top of the subtitle menu, the delay field accepts positive or negative seconds in
0.1-second increments. Positive values display subtitles later; negative values
display them earlier. The delay applies to subtitle/caption cues, including newly
loaded and translated tracks, for the lifetime of the player instance. It starts
at zero for a new player and is never persisted in preferences or playback state.


Automatic subtitle translation requires the retained track profile to use
`auto` subtitles and the target language to be authorized in the ordered subtitle
favorites. Enabling a language alone does not override a `disabled` profile or a
manual subtitle choice. Each `get_entry` load applies the configured audio/subtitle
profiles instead of durable manual track choices. Manual choices made in the player
remain preferred across episodes and seasons within that entry; opening or reloading
an entry starts from the configured profiles again. Saving edited track settings clears
manual choices and applies the confirmed server settings. There is no separate
button to reapply preferences.
An existing higher-priority subtitle also takes precedence over translating a
lower-priority favorite. A supported external subtitle source and an enabled,
configured translation service are required. The player and subtitle settings share
one effective translation catalog. Settings retain saved authorizations but disable
their controls while loading, after a failed load, and for targets without a
compatible WebVTT translation direction. Failed catalog loads can be retried by
reopening the settings, returning focus to the window, or using the player again.
After a service configuration reload, reload the frontend to refresh a successful
cached catalog.

Refreshing a media source object with the same video URL and subtitle tracks
preserves an automatic or manual translation request while its catalog is
loading. Changes to the video URL or subtitle tracks still invalidate that work.

The renderer keeps its automatic subtitle evaluation callback across Video.js
instance cleanup. Track preferences trigger evaluation on the first player
creation and after a recreation, while pending requests and instance listeners
are still released during cleanup.

The REST server and desktop application embed the frontend in their executable.
After `npm run build-only` in `front/public-app/`, rebuild the executable with
`cargo build -p arachnea-stream --bin arachnea` in `server/`, restart the running
application, and reload the browser. Rebuilding the frontend alone does not
replace the assets embedded in an already running server.


## Public account session gate

`AccountSessionGate` restores the server session before mounting content and starting metadata, allowlist and personal-cache loading. Public login uses a dedicated POST JSON/GET state transport, with HttpOnly cookies in browsers and the native session broker in Tauri. Administrator-created/reset accounts must replace their temporary password before content can render. The login gate exposes the independent administration entry for first-account setup; the navigation menu signs out by revoking the session.

Account changes clear in-memory catalogue validation, metadata, section/bookmark and notification caches; late replies are discarded by generation. BroadcastChannel/native events and focus/visibility/session polling revalidate other tabs. Credential drafts stay in component memory, and neither password nor token is written to localStorage/IndexedDB. An unavailable native OS vault is reported as memory-only persistence. Home/section/bookmark services now persist only through the selected profile API. Legacy personal preference paths are removed in step 8; no local import is performed, and the onboarding/selection UI is delivered by the step 6 gate below.


## Profile-backed personal collections (step 5)

`services/profileApi.ts` uses no-store REST requests or the same Tauri commands,
without the catalogue ETag cache. `services/profileContext.ts` supplies an immutable
profile/account generation to each request. The step 6 selection flow calls
`setActiveProfileContext(profileId)` after validating the selected profile and
initializing streaming. The step 5 adapter deliberately did not select or create a
profile: unselected lists are empty and personal writes display an error. Account/profile
changes clear caches, invalidate delayed replies and never route an old queued
write under a new UUID. General settings use server drafts (step 7 below); step 8 removes remaining local personal preference paths.

Section changes update reactive snapshots after server confirmation; reorders send
the full set of section/home IDs with a collection revision. Bookmark snapshots
update in memory immediately. Their queue coalesces ordinary progress into one
write per 10 seconds, serializes by entry and waits for the previous revision before
sending the next. Pause/end/media changes and hidden/unmounted views attempt an
immediate flush. An uncertain reply retains the immutable request/operation UUID,
then retries it before sending newer progress. Failures and unconfirmed writes are
visible; explicit retry never silently overwrites a newer server revision. Reload
and reapply after a revision conflict. Pending state is memory-only, and abrupt
closure can lose approximately the last 10 seconds on a healthy connection, more
during network failures. Profile switching attempts a bounded 5-second flush.

Validated with public type-check/build, CSS lint for the status component, actual
module runs for queue timing/replay/context and real REST adapter checks against
isolated databases. Graphical profile selection, provider playback and native
window/vault checks remain later work; no new test infrastructure is introduced.

## Profile gate, resume and onboarding (step 6)

`ProfileSessionGate` now mounts inside `AccountSessionGate` with the account/session key. It owns one `useProfiles()` controller per authenticated session and provides it to the onboarding, switcher and `App`. Content mounts only after a server-validated profile is active, so profile-bound caches and personal writes never start under an anonymous selection.

Initialization lists profiles and the embedded avatar catalogue in parallel. The last profile is restored from a local key scoped by server identity and account id when the server still reports it active; otherwise the reusable onboarding or the selector is shown. Loading a profile waits for any durable pending operation, completes the `common` guide, initializes streaming with `{}` server defaults at first service use, completes the `arachnea-stream` guide and then publishes the context. Interrupted creation retries the same operation id, and interrupted initialization/guide operations are resumed before activation. Concurrent completion by another tab is tolerated.

The avatar selector offers flat man, woman, boy and girl portraits in all seven theme colors (28 variants, four colors per image), four streaming icons (cinema clapperboard, popcorn, television and headphones), six animals (cat, dog, fox, panda, owl and rabbit), and seven neutral silhouettes in the theme colors, each using exactly two colors. The neutral `blue`, `green` and `purple` IDs are preserved for existing profiles. Streaming and animal icons each use a fixed palette of at most four colors, without theme variants. The server embeds the catalogue from `server/crates/arachnea-stream/resources/avatars/`; recompile it to publish updated images. Avatar color is chosen independently of the interface theme and remains part of the stored avatar ID.

`ProfileOnboarding`/`ProfileForm` collect name, avatar, language and theme, then show the guide's service step. `ProfileSwitcher` lists active profiles, reports quota/preparation/errors, creates an additional profile and deletes with confirmation. In management mode, each profile has an Edit action opening a detached name/avatar draft with Save/Cancel. Saving calls the existing revision-checked `profiles/update` endpoint, updates the list and active identity without changing the profile context, and invalidates other windows. Unchanged retries after uncertain responses reuse the operation UUID. Conflicts preserve the draft and offer an explicit reload of the latest server identity before saving again. The navigation exposes the same switcher in a dialog. Deletion of the active profile unmounts content; deleting the last profile returns to the guide.

Browser tabs use a per-server `BroadcastChannel`, desktop webviews use the native `arachnea-profiles-changed` event, and focus/visibility revalidate list/revision state without forcing another window's selection. A profile deleted or revoked elsewhere clears the local context. Device keys contain no secret; old local personal preferences are not imported, and new profiles start with automatic language and the `arachnea-blue` theme.

Validated with `npm run type-check`, `npm run build-only` and `npm run lint:css`. No tests or test infrastructure were added. Graphical first-login/interruption/resume, real Tauri windows/vaults and playback with coalesced writes remain manual validation work.


Step 6 review fixes: the native mobile drawer closes before opening the profile dialog. Selecting the active profile revalidates it without renewing its generation, preserving mounted bookmark/progress callbacks. Uncertain deletions retain their original operation UUID and expected revision in the session controller's memory; the selector offers explicit retries even when a profile has left the active list. A deletion error invalidates only its target. After a transport error, refreshing the list also excludes an active profile whose deletion has already started. Closing the page/session loses the local request, while accepted server operations remain durable and recover on server restart. No new endpoint or browser storage is introduced. Validation: public type-check/build/CSS lint and targeted scenarios with simulated transport; mobile handler ordering is verified, while the graphical flow remains to be checked.

## Settings drafts (step 7)

The dialog loads current server revisions and edits detached refs. Language and theme reuse the onboarding form. Save sends only changed domains to `profiles/save_preferences` with their expected revisions, preserving fields outside the form. Global preferences change only after the operation completes and an authoritative refresh succeeds. Profile selection and refresh also apply confirmed streaming form preferences, without resetting track preferences when values are unchanged. Partial operations are not applied to runtime state.

Cancel confirms discarding edits when needed and performs no write. Saving or an uncertain outcome disables editing and ordinary dismissal; lost replies and partial operations retain the exact request and UUID for Retry. The UI distinguishes already committed common preferences from a fully confirmed save. Drafts remain in memory; forced closure loses unsent edits. Accepted operations remain durable on the server and resume on restart. The dialog can refresh an operation owned by another window but cannot reconstruct its intent: the originating window must finish it, or the server must recover it on restart.

Conflicts preserve the draft. The user explicitly chooses to load server values or reapply only their changed fields to the latest snapshot, then reviews and saves with the new revisions. No automatic overwrite occurs. Legacy local access removal and ancillary player preferences are delivered in step 8.

Validation: public type-check, build, CSS lint and targeted execution of the actual composable with Vue and simulated transport (cancel, double click, mixed revisions, lost replies, partial saves, conflict/reapplication and obsolete replies). No tests added to the repository. Graphical/Tauri flows, restart and a real between-store failure from this dialog still require manual validation. Existing server coordination is unchanged.

## Personal storage retirement (step 8)

Delivered: no active IndexedDB or legacy personal `localStorage` access. Parameters start from detached defaults and then the server snapshot; context changes reset them, including language/theme. Volume, mute, navigation and account/server-scoped resume remain local; cookie/native vault handling is unchanged. Old data is neither imported nor deleted.

Quality, manual tracks and subtitle appearance are restored from confirmed streaming settings and saved through revision-checked `save_preferences`. Player changes are serialized; installing a snapshot performs no write. Lost replies retain the exact body and UUID for Retry in the personal-data banner. Conflicts block player writes until server settings are applied again (reload/selection/refresh), without automatic overwrite. Saving changed track rules also clears persisted manual track choices. Profile-switch flushing is bounded to five seconds; stale callbacks are rejected. No offline storage is introduced; forced closure may lose unconfirmed requests.

Validation: public type-check, build, CSS lint and targeted checks of real modules with simulated transport. Graphical flows in fresh/previously used browsers, a real provider and Tauri windows/vaults remain to be qualified (see the step 9 record).

## Delivery checks (step 9)

Public and admin type-check/build, public CSS lint, backend workspace check and
existing Core/Stream local suites pass on macOS ARM64. The step 8 targeted
storage/player checks were rerun successfully. These checks do not qualify native
window/vault lifecycle, graphical Save/Cancel and fault-recovery flows, or real
provider playback across devices. See the [validation record and remaining scenarios](../../docs/dev-tracking/server-user-profiles-analysis.md#étape-9--vérifier-et-documenter-la-livraison).
For coherent backups/restores, use the [backend operations procedure](../../server/crates/arachnea-stream/README.md#profile-backup-and-restore).
