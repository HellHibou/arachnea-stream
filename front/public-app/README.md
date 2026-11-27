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

Subtitle cues stay hidden before playback starts and when the large paused-play
overlay is enabled. This uses the same Video.js playback state as the existing
chapter controls; the selected subtitle track remains enabled and appears again
when playback starts or resumes.

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

Episodes restored from bookmarks select the episode video rather than the trailer.
When the stored episode is not on the first page of its bookmarked season, the entry
loads further pages until that episode is found or no more episodes remain; other
seasons keep their current list. Deferred episode player lists are loaded on
restoration as well as on manual selection. While the episode resolves, the player
keeps its episode loading/error surface; it does not fall back to the trailer. The
trailer remains available through its explicit toggle.

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

Video banners keep the logo, title and description in a left-hand column. The
text wraps within that column, with a 25px gap before the player. The player sits
flush with the right edge. The carousel navigation is centered along the bottom
of the full banner, above the player in the stacking order. The play and pause
preference applies to banner videos and is stored in the current browser.
The carousel keeps the current video banner visible while playback is paused.
For native banner videos, hovering or focusing the mute button reveals a vertical
volume slider below it. The mute button restores the last audible banner volume.
Embedded players retain the existing mute toggle.
The banner artwork stays visible behind the banner content during playback.
The player shows its separate poster while resolving the source, before native
playback starts and while an embed loads. Pausing an embed unloads it and shows
its poster when available. Resuming loads the embed again.
Only trailers can play in the page background when the trailer background
setting is enabled. Full videos, including Twitch broadcasts and recordings,
play only in the banner. Their artwork remains eligible for image backgrounds.
Category pages use section thumbnails as background images only when the
category has no usable banner image.

`/sources` lists every source advertising `load_home` as landscape
logo-and-title cards instead of loading an aggregated catalog. Hovering or
focusing a card opens the same anchored preview style as media cards, showing
the service title and its metadata facts. `/` resolves the profile’s configured
default home, and the sidebar `Accueil`/`Home` link always opens `/sources`.
`/source/:sourceId` loads only the exact backend source selected by the route.
The navigation source group uses service metadata and registry-derived
`supported_queries`, without loading every provider home. Direct links, reloads
and browser history preserve the selection. Unknown/unavailable sources never
fall back to the source list or another source home.

Source homes display their own bookmarks only. Section order and preference
writes use profile-local, source-scoped keys through the authenticated server API.
Pinned records preserve their collection mode, orientation and image fit; the
former IndexedDB section/bookmark stores are no longer active or imported. Deferred
banner and section pagination updates are guarded against stale catalog loads.
Navigation includes a retry action for failed loads and reports errors through the
shared notification stack. Catalog and custom-home load/resolution failures also use
that stack instead of an inline alert; pagination, empty-state and partial-failure
retries stay next to the affected content.
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
and their home/projector icons. Clicking Home/Accueil opens the source overview and its
children remain visible; the brand link and `/` still use the configured default
home. Only Video Services is collapsible.
Missing references are reported through the shared notification stack without
same-title substitution; remove and re-add the content to repair it. Partial failures
leave successful blocks usable, with targeted hydration retry. Category-level
pagination uses `get_category`; linked rails use `get_section`.
The declared block order takes precedence over global pinning. Bookmarks are shown
only when enabled. The view and toolbar reuse the home definition resolved by the
router. Concurrent home reads share one request for the selected profile, and
closed management dialogs stay idle. Definitions refresh when the window returns
to the foreground or a local home change is confirmed. Stale responses cannot
overwrite a newer definition.

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
Shortcut/Category content for categories, then Cancel/Add. Add resolves all clicked source
contributions and commits one revision-checked definition without opening the editor
or navigating away. Failure keeps the dialog open; cancellation does not write.
The category page header opens the same dialog with Category content selected
when the category can be resolved and the service
advertises `get_category`. The hero pin adds the featured collection of the
current category for the active service. The category strip can add discovered
children as shortcuts or content. Tags show the category image when available
alongside the label. A section pin adds only the chosen rail or
active subsection, with its category context. With editing controls enabled, pins
appear as soon as the respective content is displayed. The add dialog checks
the content's origin before saving and displays any resolution failure.
Category content renders media sections in provider order, including linked sections and subsections. A custom title labels a
single section or heads a multi-section block. Title merging keeps multi-section
blocks together. A directory or an exhausted category without media shows a
status and offers to add a shortcut while editing controls are enabled.
The category block's **Load more from this category** button requests
`get_category`, even when the current page has no media sections. Linked rails
continue independently through `get_section`. A failed request can be retried on
the block or rail that owns its cursor. Category lookups normally begin with
`load_home` and replay up to eight parent categories through `get_category`.
Sources can advertise `category_url_patterns` for stable public URLs. Category
pins, category shortcuts, banners and sections use direct category resolution
on matching pages. Quick add refreshes service metadata before resolving them.
Twitch declares its channel and tab URLs so pins work even when the channel is
absent from the Twitch home. Existing v1 documents are read
directly. New category contributions use the optional category context in the
same document version and require the updated server.
Source-page preference namespaces are stripped only for discovery matching; they are
never persisted as content identities. When a displayed rail groups several sections
with the same preference key, all unambiguous source-owned references are added.
References are copied, not linked to the originating custom home. Advanced options
remain in the home editor. Only custom homes support configurable content order;
source catalogs ignore legacy pin order without deleting preferences.
Compact translucent pin actions share one style. Category cards remain navigation
links without individual pins. The sidebar heading has an edit icon
instead of a separate management row. The Home icon is shown only at the group heading,
not beside individual custom homes. Custom-home catalogs offer a shortcut action
for empty category blocks. Ordering, removal and display options
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
declarative schema: stable home/block ids, ordered source or category banners, category
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
sections. Category-owned references use identities from unmerged `get_category`
responses after the parent path is resolved. Route hints are checked against
provider results before adding the category to a home. Collections published
as inline media cards can also be rediscovered from their home or parent
category. Repeated declarations of the same source identity and request count
as one category. Identity uses exact source-scoped provider keys, canonical descriptors
excluding pagination, then exact labels as a fragile fallback; it never uses array
positions. Duplicate identities are marked ambiguous and rejected for selection.
Inline-only sections and subsection identity under the parent are retained. A
changed label fallback must be repaired, never matched to a similar title.

Runtime validation rejects unknown fields/schema versions, invalid types,
duplicate block ids, descriptor/source mismatches and recognizable credential
fields. This is defensive validation, not a guarantee that arbitrary provider
parameters are secret-free: only YAML-discovered category descriptors may be
selected, and the resolver rediscovers them before execution. Repeated
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

Entry details can expose an optional `channel` attribution returned by `get_entry`: a display name, an optional logo and a provider-owned category request. The poster panel renders it as an internal collection link directly below the source action, using the same route builder as category cards and preserving every request parameter. The link is hidden when the name or destination is unusable, and a missing or failed logo leaves the name visible. M6Play channel categories also expose `banners.link`, so their programme carousel is loaded through the existing deferred banner carousel and remains independent from the category rails.

For a discoverable category, the quick-add pin sits next to the title. The
category toolbar reads `channel.name` and `channel.logo` from the loaded
`get_category` result. Inline and deferred banners use the same channel logo in
the provider logo slot and preserve its aspect ratio. The provider logo serves
as the fallback when the channel logo is unavailable.

Episode lists inside an entry use `selectionMode="local"` on the shared card
collection. Each entire episode row acts as a keyboard-accessible button that
selects the episode in the current player, without a separate watch button,
including episodes with inline players and no episode URL (such as Anime-Sama and
French Animes). The episode mapper supplies a source only when inline players or a
deferred player link are available. Other collections default to route selection
and retain their validated native links.

The entry episode panel scrolls vertically when its content exceeds the smaller
of 60% of the viewport height and 36rem. The episode label and season selector stay outside the
scrollable panel, and the panel supports keyboard scrolling. Pagination remains
inside the panel so more episodes can load as its end becomes visible.

Entry players offer an Expanded player / Compact player toggle in the player toolbar that toggles a page-local
viewing layout. The icon appears only in the native Video.js control bar,
between Download and Picture-in-Picture, including in compact players. Embedded
players do not expose a separate layout button.
The toggle uses horizontal expand/collapse arrows without a selected-state color.
When Expanded player mode has no active episode or trailer surface, it selects
and loads the first playable episode from the available list after entry/season
loading and bookmark restoration. Existing selections and active trailers remain
preferred; this automatic selection does not scroll the page or force autoplay.
In Expanded player mode, the player starts a new row across the available page width,
with a right-hand column containing the season dropdown and the scrollable
episode list. The episode heading is hidden in this mode; a larger season label
sits to the left of the enlarged dropdown, while episode titles use a smaller font than in
Compact player mode. On wider screens, the episode column takes 36% of
the available width and fits the player's height. When the episode list is hidden,
the player fills the available width instead. The episode column keeps compact
spacing, plus an additional 10px gap above the row. Pagination stays
inside the list. The row height fits the
viewport below the application toolbar; the player frame can change proportions,
while the video itself uses contain sizing to avoid distortion. Player selectors
fit inside that height. The poster, title, description and metadata move below
the row. On wider screens the poster keeps its normal 400px width, with the title,
badges and facts on its right. Descriptions follow the metadata in that same
right-hand column, including when the text extends below the poster. On
screens up to 1120px wide, the season and episode column stacks below the player.
Switching layouts keeps the same mounted renderer and its current playback.
Selecting an episode in Cinema mode keeps the page's scroll position; the Compact player
layout still scrolls to the title when an episode is selected.
Opening a different entry applies the device's Expanded player default (disabled initially).
The Player settings section edits this default through Save/Cancel; it is stored
only in `localStorage` under `arachnea.videoPlayer.preferences.cinemaModeByDefault`,
shared by profiles on this browser or desktop webview and never added to server
preference documents. Manual toolbar toggles affect only the current entry.
In the normal layout, the description also appears below the metadata badges and facts.

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

Subtitle translation progress and download progress share one status area above
the controls. Translation takes priority while it is preparing or downloading,
and a concurrent download message reappears only after translation finishes.
The status drops to the lower position, like the skip-chapter buttons, when the
control bar is hidden. Translation failures are not rendered in the player:
they are queued in the global error notification stack with their stage and
HTTP status.

The renderer keeps its automatic subtitle evaluation callback across Video.js
instance cleanup. Track preferences trigger evaluation on the first player
creation and after a recreation, while pending requests and instance listeners
are still released during cleanup.

### Video player downloads

The Video.js control bar offers MP4 downloads when the player provides a file
variant, including during HLS or DASH playback. The same menu lists remote and
injected subtitle files, including the current generated translation. It reads
DASH subtitle tracks from the manifest loaded by dash.js and HLS subtitle
renditions from VHS or the master playlist. It assembles finite HLS WebVTT
renditions and DASH WebVTT (`wvtt`) and text TTML (`stpp`) fragments into a
complete WebVTT file on demand, independently of the current playback position
and subtitle selection. DASH text TTML exports preserve text and timing.
Standalone files retain their original format. Starting a download shows a
localized status in the player: `Downloading the video file…` for MP4 variants
and the relevant downloading message for assembled or archived exports. The
status stays visible briefly even when the browser starts the download
immediately.

Segmented exports require a finite, readable, unencrypted subtitle rendition.
In-band captions without a separate rendition and bitmap TTML tracks cannot
be exported by this menu. The last item saves the current video frame as a PNG
at its native resolution. The menu omits this item when the active stream
declares DRM or the browser signals encrypted playback. The download button
is hidden when the menu has no actions.

When the source exposes a storyboard, the menu also offers a ZIP archive of its
sprite images. The player reads the storyboard WebVTT when available, and
otherwise enumerates the declared sprite template using the video duration and
grid. Each sprite image is archived unchanged as `sprite-0001.jpg`,
`sprite-0002.jpg`, and so on, with its original format extension. A generated
`storyboard.vtt` keeps every cue's `#xywh=` cell geometry but points to those
local files, so sprites holding several thumbnails work without re-encoding.
The archive is saved as `<video-name> - storyboard.zip`. The option stays hidden
when neither a storyboard VTT nor a usable sprite template with a finite
duration is available. The same menu group also exports the source chapters as
`<video-name> - chapters.vtt` when chapters are available; untitled chapters use
their localized type label (`Intro`, `Ad`, ...). Sprite images must be readable
by the browser, like any cross-origin download. Failures are only queued in the
application's global error notification stack, where the technical details
include the reason the export failed; the player only shows the shared transient
status.

Download filenames start with the player-supplied filename when present.
Otherwise they use the entry title, or `<entry title> - <episode title>` when a
distinct episode is selected. Additional parts are joined with ` - ` (spaces
included), and diacritics are removed while filesystem-reserved characters are
replaced, so names stay portable. MP4 filenames include the displayed player
label when multiple players are available, and the variant label when multiple
MP4 versions are offered. Subtitles use `<video-name> - <language> - <label>`
with their file extension and `und` for an unknown language. Cross-origin files
require browser-readable access when the player fetches them. Odysee MP4 downloads use the application
media proxy to keep signed redirects on the same origin. Downloads without an
added label retain Odysee's attachment filename.
Frame capture depends on browser access to the decoded frame.

The REST server and desktop application embed the frontend in their executable.
After `npm run build-only` in `front/public-app/`, rebuild the executable with
`cargo build -p arachnea-stream --bin arachnea` in `server/`, restart the running
application, and reload the browser. Rebuilding the frontend alone does not
replace the assets embedded in an already running server.


## Public account session gate

`AccountSessionGate` restores the server session before mounting content and starting metadata, allowlist and personal-cache loading. Public login uses a dedicated POST JSON/GET state transport, with HttpOnly cookies in browsers and the native session broker in Tauri. Administrator-created/reset accounts must replace their temporary password before content can render. The login gate exposes the independent administration entry for first-account setup; the navigation menu signs out by revoking the session.

Account changes clear in-memory catalogue validation, metadata, section/bookmark and notification caches; late replies are discarded by generation. The gate reads `accounts/session` once per document load. Account operations update the initiating tab from their responses. Browser BroadcastChannel and native events clear stale account state in other tabs, where users can reload to restore the shared session. Protected API session errors close the current account context. Credential drafts stay in component memory, and neither password nor token is written to localStorage/IndexedDB. An unavailable native OS vault is reported as memory-only persistence. Home/section/bookmark services now persist only through the selected profile API. Legacy personal preference paths are removed in step 8; no local import is performed, and the onboarding/selection UI is delivered by the step 6 gate below.


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
immediate flush. After an uncertain reply, the client reads the confirmed bookmark before
sending newer progress. When it differs from the attempted change, the client
uses the current server value and reports the conflict. Pending state is memory-only, and abrupt
closure can lose approximately the last 10 seconds on a healthy connection, more
during network failures. Profile switching attempts a bounded 5-second flush.

Validated with public type-check/build, CSS lint for the status component, actual
module runs for queue timing/replay/context and real REST adapter checks against
isolated databases. Graphical profile selection, provider playback and native
window/vault checks remain later work; no new test infrastructure is introduced.

## Profile gate, resume and onboarding (step 6)

`ProfileSessionGate` now mounts inside `AccountSessionGate` with the account/session key. It owns one `useProfiles()` controller per authenticated session and provides it to the onboarding, switcher and `App`. Content mounts only after a server-validated profile is active, so profile-bound caches and personal writes never start under an anonymous selection.

Initialization lists profiles and the embedded avatar catalogue in parallel. The last profile is restored from a local key scoped by server identity and account id when the server still reports it active; otherwise the reusable onboarding or the selector is shown. Loading a profile completes the `common` guide, initializes streaming with `{}` server defaults at first service use, completes the `arachnea-stream` guide and then publishes the context. After an uncertain creation response, the client refreshes the profile list. Concurrent completion by another tab is tolerated.

The avatar selector offers flat man, woman, boy and girl portraits in all seven theme colors (28 variants, four colors per image), four streaming icons (cinema clapperboard, popcorn, television and headphones), six animals (cat, dog, fox, panda, owl and rabbit), and seven neutral silhouettes in the theme colors, each using exactly two colors. The neutral `blue`, `green` and `purple` IDs are preserved for existing profiles. Streaming and animal icons each use a fixed palette of at most four colors, without theme variants. The server embeds the catalogue from `server/crates/arachnea-stream/resources/avatars/`; recompile it to publish updated images. Avatar color is chosen independently of the interface theme and remains part of the stored avatar ID.

`ProfileOnboarding`/`ProfileForm` collect name, avatar, language and theme, then show the guide's service step. `ProfileSwitcher` lists active profiles, reports quota and errors, creates an additional profile and deletes with confirmation. In management mode, each profile has an Edit action opening a detached name/avatar draft with Save/Cancel. Saving calls the existing revision-checked `profiles/update` endpoint, updates the list and active identity without changing the profile context, and invalidates other windows. An uncertain update refreshes the profile list. Conflicts preserve the draft and offer an explicit reload of the latest server identity before saving again. The navigation exposes the same switcher in a dialog. Deletion of the active profile unmounts content; deleting the last profile returns to the guide.

Browser tabs use a per-server `BroadcastChannel`, desktop webviews use the native `arachnea-profiles-changed` event, and focus/visibility revalidate list/revision state without forcing another window's selection. A profile deleted or revoked elsewhere clears the local context. Device keys contain no secret; old local personal preferences are not imported, and new profiles start with automatic language and the `arachnea-blue` theme.

Focus and visibility events from the same return to the foreground are grouped
before refreshing the profile, sections, bookmarks and custom homes. Concurrent
section and bookmark refreshes for one profile share their pending reads.

Validated with `npm run type-check`, `npm run build-only` and `npm run lint:css`. No tests or test infrastructure were added. Graphical first-login/interruption/resume, real Tauri windows/vaults and playback with coalesced writes remain manual validation work.


The native mobile drawer closes before opening the profile dialog. Selecting the
active profile revalidates it without renewing its generation, preserving mounted
bookmark/progress callbacks. If a deletion reply is lost, the switcher reloads
the list and checks whether the target UUID is still present.

## Settings drafts (step 7)

The dialog edits detached refs. Save submits changed domains with their expected
revisions in a single transaction. Global preferences use the confirmed server
snapshot. If a response is lost, the client checks whether the requested values
are present. The draft stays available for review if server values differ. The
user can load server values or reapply the changed fields to the latest snapshot
before saving. Cancel discards the local draft after confirmation.

The dialog fills the available height first and fits its width to the occupied
columns, within the screen bounds. Settings groups are capped at 780px wide and
shrink to fit narrower screens.
Complete groups stack in one column when they fit; additional columns are opened
only when needed and when at least 520px per column is available. If the groups
still cannot fit, the dialog scrolls vertically, without splitting groups or
adding horizontal scrolling. Layout is recalculated when the window or group
contents change. The Expanded player default is a
separate device-only draft: saving only that switch makes no server write.
When other preferences also change, the local default is committed only after
the server save succeeds; a failed save or Cancel leaves the local default intact.

## Personal storage retirement (step 8)

Delivered: no active IndexedDB or legacy personal `localStorage` access. Parameters start from detached defaults and then the server snapshot; context changes reset them, including language/theme. Volume, mute, the Expanded player default, navigation and account/server-scoped resume remain local; cookie/native vault handling is unchanged. Old data is neither imported nor deleted.

Quality, manual tracks and subtitle appearance are restored from confirmed streaming settings and saved through revision-checked `save_preferences`. Player changes are serialized; installing a snapshot performs no write. After a lost reply, the client checks confirmed streaming preferences before continuing. Conflicts block player writes until server settings are applied again (reload/selection/refresh), without automatic overwrite. Saving changed track rules also clears persisted manual track choices. Profile-switch flushing is bounded to five seconds; stale callbacks are rejected. No offline storage is introduced; forced closure may lose unconfirmed requests.

Validation: public type-check, build, CSS lint and targeted checks of real modules with simulated transport. Graphical flows in fresh/previously used browsers, a real provider and Tauri windows/vaults remain to be qualified (see the step 9 record).

## Delivery checks (step 9)

Public and admin type-check/build, public CSS lint, backend workspace check and
existing Core/Stream local suites pass on macOS ARM64. The step 8 targeted
storage/player checks were rerun successfully. These checks do not qualify native
window/vault lifecycle, graphical Save/Cancel and fault-recovery flows, or real
provider playback across devices. See the [profile transaction analysis](../../docs/dev-tracking/analyse-tables-operations-profils.md#validation-à-prévoir) for the current validation scenarios.
For coherent backups/restores, use the [backend operations procedure](../../server/crates/arachnea-stream/README.md#profile-backup-and-restore).
