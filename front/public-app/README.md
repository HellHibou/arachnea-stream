# front

This template should help get you started developing with Vue 3 in Vite.

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
writes use source-scoped keys in the existing `home_sections` IndexedDB store;
legacy display styles are read-only fallbacks until a scoped value exists.
Existing profiles and global bookmarks/preferences are preserved. Deferred
banner and section pagination updates are guarded against stale catalog loads.
Navigation includes metadata refresh on errors and the catalog includes retry actions.
Custom homes and source destinations are available through the permanent sidebar.

## Custom home rendering (lot 2B)

Public accueil URLs use `/home/<title_slug>` (lowercase, accents removed, words
separated by underscores). Internal UUIDs remain unchanged for preferences and
default-home selection. Save/rename checks URL uniqueness inside the same IndexedDB
transaction, including concurrent views. Renaming updates the active route; legacy
`/homes/<UUID>` links redirect to the current title URL. Existing collisions are
reported for manual renaming, never automatically renamed or deleted.

The home management dialog includes one persisted default landing page selector
(removed from the general settings): All, an existing
custom home, or a known service supporting `load_home`. Every navigation to `/`
and brand clicks resolve that destination; query/hash values are preserved and
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
storage failures are reported without disabling navigation. Cross-tab storage
events and focus refresh synchronize this preference; home lists refresh on focus,
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

`services/customHomes.ts` creates detached drafts and provides load/list/save/delete
operations for the additive `custom_homes` store in `arachnea-db`, keyed by `id`
and indexed by `navigationOrder`. Each operation initializes the store; existing
bookmarks and section preferences are preserved. Save requires the last read
`updatedAt` (or `null` for creation), and delete requires the last read timestamp.
The revision comparison and mutation share a single transaction, and success is
reported only on commit. A concurrent modification fails rather than overwriting
another view's changes. Listing returns valid homes and separate issues for
invalid/unsupported records, which remain untouched in storage. No automatic
migration to an unknown schema is performed. Storage is local to the origin/profile,
not synchronized across devices. The custom route reloads definitions on focus and
when the document becomes visible, with guards against stale reads.

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
