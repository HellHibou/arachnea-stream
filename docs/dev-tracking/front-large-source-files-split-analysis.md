# Analyse : découpage des fichiers sources front ≥ 1000 lignes

> Créée le 2026-11-04.
> Périmètre : `front/public-app/src/**` et `front/admin-app/src/**` (hors `node_modules/` et `dist/`).
> Inventaire mesuré : 8 fichiers ≥ 1000 lignes, 12 368 lignes cumulées.
> Nature : analyse et plan d'implémentation, sans modification de code dans cette étape.

## Objectif

Ramener chaque fichier source front sous un seuil lisible, en découpant **par responsabilité** plutôt que par
taille, sans changer le comportement visible. Le découpage doit conserver les API publiques existantes
(`@/services/rustify`, `@/services/storage`, composables exportés) afin que les ~70 points d'import actuels
continuent de fonctionner sans réécriture.

Cibles proposées :

- plafond dur : 1000 lignes par fichier (aucun fichier au-dessus après refactor) ;
- cible de travail : 400 à 600 lignes pour les modules de logique, 250 à 400 pour les composants `.vue` ;
- les blocs `<style>` des SFC ne doivent pas dépasser ~250 lignes ; au-delà, extraction en feuilles CSS.

## Méthode de mesure

Le comptage retenu est `(Get-Content <file>).Count`, c'est-à-dire **toutes les lignes, lignes vides incluses**.
Attention : `Measure-Object -Line` exclut les lignes vides et sous-estime fortement les fichiers aérés
(par exemple `rustify.ts` : 3118 avec `-Line`, 3508 en comptage réel). Les chiffres de ce document sont les
comptages réels.

## Inventaire

| # | Fichier | Lignes | Découpage interne | Rôle principal |
|---|---------|-------:|-------------------|----------------|
| 1 | `front/public-app/src/services/rustify.ts` | 3508 | 100 % TS | Client REST/Tauri + normalisation de toutes les réponses backend |
| 2 | `front/public-app/src/composables/video/useVideoJsMediaRenderer.ts` | 1921 | 100 % TS | Cycle de vie Video.js, source switching, storyboard, sous-titres, autoplay |
| 3 | `front/public-app/src/composables/video/useVideoPlayer.ts` | 1233 | 100 % TS | View-model de la surface média (iframes, attributs vidéo, handlers) |
| 4 | `front/public-app/src/components/media/VideoJsMediaRenderer.vue` | 1189 | script 1-180, template 182-241, **style 243-1189** | Hôte du lecteur + skin CSS Video.js (946 lignes de CSS) |
| 5 | `front/public-app/src/services/storage.ts` | 1170 | 100 % TS | Store Pinia unique : paramètres, préférences home, signets, préférences lecteur |
| 6 | `front/public-app/src/components/ProgramEntryDetails.vue` | 1140 | script 1-1010, template 1012-1140 | Orchestration fiche programme : épisodes, signets, lecture, recommandations |
| 7 | `front/public-app/src/composables/video/video-js-media-renderer/dash.ts` | 1108 | 100 % TS | Manifest DASH (adaptation sets), bridge qualité dash.js, chapitres de période |
| 8 | `front/public-app/src/components/home/HomeHeroBanner.vue` | 1099 | script 1-546, template 548-700, style 702-1099 | Carrousel hero : rotation, vidéo de bannière, action, style |

Répartition : 3 composants `.vue` (3 428 lignes, dont 1 344 de CSS), 5 modules TS (8 940 lignes).
`front/admin-app` ne contient aucun fichier ≥ 1000 lignes (maximum : `src/views/ServicesView.vue`, 659 lignes).

### Fichiers de surveillance (900-999 lignes)

Ils ne déclenchent pas ce lot mais deviendront éligibles au prochain passage et doivent être découpés avec les
mêmes conventions plutôt que laissés grossir :

| Fichier | Lignes | Note |
|---------|-------:|------|
| `composables/entry-details/entryVideoPlayer.ts` | 992 | Même domaine que `ProgramEntryDetails.vue` ; candidat immédiat après le lot |
| `composables/video/video-js-media-renderer/controls.ts` | 869 | Déjà un module feuille ; à scinder si le menu autoplay y est déplacé |
| `components/media/VideoPlayer.vue` | 845 | Template + style du view-model `useVideoPlayer` |
| `components/HomeCatalog.vue` | 829 | Composition de rails home |
| `composables/home/useHomeCatalog.ts` | 778 | Chargement/pagination des rails home |

## Diagnostic transverse

Trois causes structurelles expliquent la majorité du volume :

1. **Modules « god » de service** : `rustify.ts` mélange transport HTTP, cache de validation conditionnelle,
   gestion d'erreurs, et normalisation de six domaines métier (search, home, entry, live, players, metadata).
   `storage.ts` mélange quatre domaines persistés plus des utilitaires `localStorage` et des sanitizers.
2. **Composables « view-model totaux »** : `useVideoJsMediaRenderer` et `useVideoPlayer` exposent un objet plat
   de 60 à 90 propriétés, chacune dépendant du même contexte de refs ; la logique interne s'est accumulée par
   fonctionnalité sans frontière de module.
3. **CSS dans les SFC** : les deux composants concernés portent 1 344 lignes de skin/theme, ce qui gonfle le
   fichier sans ajouter de logique et masque la taille réelle du script.

Point positif : le dépôt possède déjà les patterns nécessaires (`composables/video/video-js-media-renderer/*`,
`composables/entry-details/*`, `services/customHome*.ts`). Le découpage consiste à étendre des conventions
existantes, pas à en inventer.

## Stratégie générale

### 1. Convertir les modules feuilles en dossiers avec barrel

Pour `rustify` et `storage`, remplacer le fichier par un dossier dont `index.ts` réexporte exactement l'API
publique actuelle :

```
services/rustify.ts  ->  services/rustify/index.ts + modules internes
```

`@/services/rustify` et `@/services/storage` continuent de résoudre via `resolve.alias` + résolution
`bundler` (Vite et TypeScript résolvent tous deux `<dir>/index.ts`). Aucun des 21 importateurs de
`rustify` ni des 25 importateurs de `storage` n'a besoin d'être modifié. Même technique pour `dash.ts`
(`video-js-media-renderer/dash/index.ts`).

Règles pour que le barrel reste sain :

- `index.ts` ne contient **que** des réexports (`export * from './...'` ciblés ou liste explicite) ;
- aucune logique dans le barrel, aucune constante partagée ;
- les modules internes n'importent jamais `index.ts` (pas de cycle) ;
- ordre de dépendance imposé : `types` → `primitives` → `formatting`/`media` → `apiClient`/`persistence` →
  domaines → `index`.

### 2. Extraire les feuilles CSS des SFC

Deux options, à trancher avant le lot composants :

- **Option A (recommandée)** : fichiers CSS dédiés sous `components/media/video-js/` et
  `components/home/hero-banner/`, référencés par plusieurs blocs `<style scoped src="...">`. Vue conserve
  l'attribut `data-v-*` par bloc et donc l'isolation scoped. Avantage : SFC court, CSS linté par
  `npm run lint:css`, aucune règle de style déplacée dans un TS.
- Option B : plusieurs blocs `<style scoped>` inline. Elle réduit la lecture logique mais laisse 946 lignes
  dans le fichier ; elle ne satisfait pas le plafond de 1000 lignes pour `VideoJsMediaRenderer.vue`.

Contraintes pour les deux options : conserver **l'ordre exact** des règles (la cascade et les `:deep()` en
dépendent), ne pas fusionner de sélecteurs, et vérifier `prefers-reduced-motion`, `:fullscreen` et les media
queries après extraction.

### 3. Extraire des composables par sous-domaine, avec dépendances explicites

Pour les gros composables, la cible n'est pas « un fichier par fonction » mais des unités
fonctionnelles cohérentes recevant un contexte explicite :

```
useXxx(options)            // point d'entrée conservé, orchestration + API publique
video-js-media-renderer/   // helpers sans état (parsing, manifest, options)
useXxxSlice(context)       // sous-composable stateful, reçoit refs et callbacks
```

Une sous-composable ne lit pas de variable de module ; tout ce qui est partagé est passé en paramètre typé
(`{ activePlayer, preferredQuality, emitPlayerState, ... }`) ou exposé par un petit objet de contexte créé par
le point d'entrée. C'est la partie la plus risquée : elle doit être découpée en deux vagues (helpers purs
d'abord, état ensuite).

### 4. Garde-fous de non-régression

- Déplacement de code sans réécriture : les fonctions gardent signature, JSDoc et nom ; on ne renomme qu'en cas
  de collision (par exemple `home/banners.ts` vs `home/catalog.ts`).
- Aucun nouveau test (politique du dépôt) ; validation par `npm run type-check`, `npm run build`,
  `npm run lint:css`, plus la checklist manuelle du lecteur.
- Un commit par fichier source traité, pour conserver un historique lisible (`git mv` quand un fichier devient
  un dossier).
- Vérifier `vite.config.ts` : le découpage de chunks regex `src[\\/](components[\\/]media|composables[\\/]video)[\\/]`
  couvre les nouveaux sous-dossiers ; aucune modification attendue, mais à confirmer après extraction.

## Analyse par fichier

### 1. `services/rustify.ts` — 3508 lignes

**État actuel.** Un seul module, sans frontière interne : constantes et types (1-118), pont Tauri
(119-134), transport REST avec cache ETag et enveloppe d'agrégation (136-365), recherche paginée
(367-606), chargement home/catégories/métadonnées (608-817), API entry/live (818-873), normalisation home
(874-1746), normalisation entry/players/seasons/stream (1882-2739), et enfin ~790 lignes de primitives
génériques (2722-3508).

**Responsabilités identifiées.**

| Bloc | Lignes (≈) | Contenu |
|------|-----------:|---------|
| Types et constantes publiques | 80 | `mediaTypeValues`, `SearchFilterOption`, `SearchMediaItemsFilters`, `SearchMediaItemsPage`, `JsonRecord`, `TauriWindow` |
| Pont desktop | 30 | `isDesktopApp`, `openAdministrationWindow` |
| Transport REST/Tauri | 230 | `call_api`, cache ETag, `parseResponseBody`, `extractErrorMessage`, `unwrapAggregationPayload`, `handleFailedRestResponse`, `ReportedApiError` |
| Diagnostic d'erreurs | 100 | `readAggregationPayload`, `isScraperExecutionError`, `reportExecutionErrors`, `throwFrontendApiError`, `nextFrontendErrorCode` |
| Recherche | 240 | `searchMediaItems`, `searchMediaItemsPage`, regroupement et pagination par source |
| Home — chargement | 210 | `loadHomeCatalog`, `loadHomeSourceDiscovery`, `loadServiceMetadata`, `getCategoryCatalog`, `loadHomeSectionPage`, `loadInitialHomeSectionPages` |
| Home — normalisation | 550 | catégories, sections, sous-sections, sources, bannières, fusion/interleave |
| Entry — normalisation | 760 | détails, recommandations, players, résolveurs, storyboard, chapitres, saisons, épisodes, flux |
| Live | 90 | `listLiveMediaItems`, `getLivePlayers`, `liveTargetMediaItem` |
| Média partagé | 230 | `normalizeMediaItem`, `dedupeMediaItems`, clé de déduplication, `buildMediaId` |
| Primitives et formats | 590 | `readPath`, listes/maps/records, nombres/booléens, dates/durées, libellés de langue, résolution d'URL |

**Problèmes.** 26 exports publics pour un seul module ; des helpers de domaine sont isolés au milieu d'autres
blocs (`liveTargetMediaItem` ligne 1147 dans la normalisation home, `buildHomeSectionSourceKey` ligne 2291
dans la normalisation entry) ; les primitives sont déclarées après leurs appelants (légal par hoisting, mais
empêche de lire le module de haut en bas) ; toute modification d'un domaine recompile et relit un fichier de
3500 lignes.

**Découpage proposé.**

```
services/rustify/
  index.ts                      ~110   barrel : réexporte l'API publique actuelle à l'identique
  types.ts                      ~110   mediaTypeValues, filtres/pages de recherche, JsonRecord, AggregationPayload
  desktop.ts                     ~40   pont Tauri
  apiClient.ts                  ~330   call_api, cache ETag, enveloppe d'agrégation, erreurs, réponse HTTP
  primitives/
    readers.ts                  ~330   readPath, readRecordList, readStringMap, readBoolean*, firstNonEmptyString, firstNumber
    formatting.ts               ~200   durées, dates, libellés de langue, buildMediaId, slugify, normalizeString
  media.ts                      ~230   normalizeMediaItem, dedupeMediaItems, resolveAssetUrl/resolveEntryUrl, toDisplayMediaType
  search.ts                     ~240   recherche et pagination par source
  home/
    catalog.ts                  ~250   loadHomeCatalog, normalizeHomeCatalog, catégories, fusion, interleave
    sections.ts                 ~430   sections, sous-sections, sources, pages initiales, préférences de section
    banners.ts                  ~230   loadHomeCatalog->bannières, getBanners, fragments indexés, expand/merge
    serviceMetadata.ts           ~90   loadServiceMetadata et normalisation des métadonnées/thèmes
  entry/
    details.ts                  ~200   getEntryDetails, normalizeEntryDetails, buildMetaLine
    players.ts                  ~330   getPlayers, normalizeEntryPlayer(+resolver/storyboard/chapitres), normalizePlayersResponse
    episodes.ts                 ~260   getSeasonEpisodes, saisons, épisodes, pagination de saison
    stream.ts                   ~120   getStream, recommandations
  live.ts                        ~90   listLiveMediaItems, getLivePlayers, liveTargetMediaItem
```

**Risques spécifiques.** Cycles entre `home`, `entry` et `media` : `normalizeMediaItem` est utilisé par les
trois, il doit rester dans `media.ts` et les domaines ne doivent dépendre que de `primitives`, `formatting`,
`media`, `apiClient`. Vérifier aussi que le cache ETag reste un singleton (un seul `apiClient.ts`) : le
dupliquer dans deux modules casserait la validation conditionnelle sans erreur visible.

**Ordre interne conseillé.** (1) `types` + `primitives` + `formatting` (déplacement mécanique),
(2) `media`, (3) `apiClient` + `desktop`, (4) `search` et `live` (petits domaines, valident le barrel),
(5) `home/serviceMetadata`, `home/banners`, `home/catalog`, `home/sections`, (6) `entry/*`.

---

### 2. `composables/video/useVideoJsMediaRenderer.ts` — 1921 lignes

**État actuel.** Une seule fonction `useVideoJsMediaRenderer` (161-1921) qui contient ~45 fonctions internes,
7 `watch`, 2 hooks de cycle de vie et un état mutable de module (player actif, bridge DASH, plugin storyboard,
pistes distantes, qualité préférée, état retenu, compteur `storyboardLoadId`).

**Responsabilités identifiées.**

| Bloc | Lignes (≈) | Contenu |
|------|-----------:|---------|
| Options, constantes, types locaux | 110 | seuils storyboard, types `StoryboardVttCue`, helpers `getSubtitleFileLabel`/`areSubtitlesEqual` |
| État du renderer | 120 | refs `activePlayer`, overlay, loading, navigation preview, `storyboardPreviewSize` |
| Émissions vers le parent | 110 | playback progress/started/ended, player state, metadata, load complete |
| Sous-titres distants | 70 | `clearRemoteSubtitleTracks`, `addRemoteSubtitleTracks`, comparaison |
| Attributs et poster | 90 | `syncVideoElementAttributes`, `installPosterImageErrorHandler`, `syncBigPlaySuppressionAttribute` |
| Storyboard / sprites / VTT | 490 | parsing VTT, dimensions de grille, `resolveSpriteThumbnailOptions`, override des cues, intervalle dérivé |
| Construction de source et configuration | 240 | `buildPlayerSource`, `syncExistingPlayerConfiguration`, `attemptAutoplay`, sélecteur de qualité |
| Menu autoplay d'épisode | 130 | entrées de menu, sync du contrôle, préférence |
| Bridge qualité DASH | 120 | installation différée et nettoyage |
| Création/destruction du player | 280 | `createPlayer`, `destroyActivePlayer`, installation des plugins et contrôles |
| Application d'une source | 460 | `applySourceToPlayer`, restauration position/UI/qualité, listeners de métadonnées |
| Watchers | 130 | source, attributs, autoplay, contrôles, démontage |

**Problèmes.** Le storyboard (490 lignes) est un sous-système autonome (parsing VTT + plugin tiers) noyé dans
le cycle de vie ; le menu autoplay est une fonctionnalité UI complète ; l'état mutable partagé rend toute
extraction non triviale ; `applySourceToPlayer` porte à lui seul 460 lignes et six dépendances d'état.

**Découpage proposé (deux vagues).**

Vague 1 — helpers sans état, dans `video-js-media-renderer/` (déjà le bon dossier) :

```
video-js-media-renderer/
  storyboard.ts     ~520   parsing VTT, grille, options sprite, override des cues, intervalle dérivé
  subtitles.ts      ~120   pistes distantes + libellé de fichier + égalité de listes
  playerAttributes.ts ~120 syncVideoElementAttributes, poster error handler, big play suppression
  autoplayMenu.ts   ~160   entrées de menu, sync du contrôle, préférence d'autoplay
```

Vague 2 — état et cycle de vie :

```
useVideoJsMediaRenderer.ts   ~500   orchestration, état, watchers, API retournée
video-js-media-renderer/
  playerLifecycle.ts         ~300   createPlayer/destroyActivePlayer + installation des plugins
  sourceApplication.ts       ~430   applySourceToPlayer + restauration
  playerStateEmitters.ts     ~120   émissions/typings vers le parent
```

Interface de contexte à définir pour la vague 2 (exemple, pas une signature finale) :

```ts
interface RendererContext {
  props: VideoJsMediaRendererProps
  emit: VideoJsMediaRendererEmits
  activePlayer: ShallowRef<VideoJsPlayer | null>
  videoElement: Readonly<ShallowRef<HTMLVideoElement | null>>
  preferredQuality: { get(): string | null; set(value: string | null): void }
  retainedPlayerState: { get(): VideoJsPlayerState | null; set(value: VideoJsPlayerState | null): void }
  emitPlayerState: (state: VideoJsPlayerState | null) => void
}
```

**Risques spécifiques.** Le plugin `videojs-sprite-thumbnails` est configuré avant `player.src()` : l'ordre
d'initialisation doit être préservé à l'identique. Les ids monotoniques (`storyboardLoadId`) protègent contre
les chargements asynchrones obsolètes ; un découpage qui recrée un compteur par module réintroduirait des
courses. Ne pas déplacer `activePlayer` dans un module à variable de module (instance unique par composant).

---

### 3. `composables/video/useVideoPlayer.ts` — 1233 lignes

**État actuel.** Composables props/emits (72-315), helpers de préférences (36-67), puis un view-model plat :
~60 `computed` (350-1000) et 12 handlers (1015-1155), retournés en un objet de 75 entrées (1156-1232).

**Responsabilités identifiées.**

| Bloc | Lignes (≈) | Contenu |
|------|-----------:|---------|
| Props/emits/types | 245 | contrat public de `VideoPlayer.vue` |
| Préférences de lecteur | 40 | état initial et extraction des préférences durables |
| Décision de surface | 200 | mode `media`/`trailer`/`standalone`, source active, render states, classes |
| Iframe | 160 | source, titre, allow, referrer policy, confirmation/blocage sécurité |
| Attributs vidéo | 260 | poster, logo, autoplay, muted, loop, controls, preload, a11y, navigation |
| Handlers et retours | 220 | événements du template, pickers langue/lecteur, retours |

**Découpage proposé.**

```
video/useVideoPlayer.ts            ~220   orchestration + objet retour (spread des slices)
video/playerTypes.ts               ~250   VideoPlayerProps, VideoPlayerEmits, types de préférences
video/usePlayerSurface.ts          ~220   mode de surface, source active, render/loading/error state, classes
video/usePlayerIframe.ts           ~170   attributs et flux de confirmation iframe (reçoit activeSurfaceSource)
video/usePlayerVideoAttributes.ts  ~270   attributs vidéo et navigation (reçoit activeSurfaceSource + surface mode)
video/usePlayerHandlers.ts         ~180   handlers, sélection langue/lecteur, persistance des préférences
```

Le point d'entrée conserve le nom et la signature `useVideoPlayer(props, emit)` et retourne
`{ ...surface, ...iframe, ...videoAttributes, ...handlers }`. Aucun changement d'API pour `VideoPlayer.vue`.

**Risques spécifiques.** L'ordre des clés de l'objet retourné n'a pas d'impact fonctionnel, mais un spread
partiel casserait silencieusement le template (propriétés `undefined` non détectées à la compilation car le
template est vérifié par `vue-tsc` — à valider explicitement avec `type-check`). Les slices doivent recevoir
les computed déjà résolus (`isEntryDetailsMode`, `activeSurfaceSource`) pour éviter de dupliquer la logique
de décision de surface.

---

### 4. `components/media/VideoJsMediaRenderer.vue` — 1189 lignes

**État actuel.** Script 1-180 (props, refs, 6 computed de présentation), template 182-241 (60 lignes :
hôte, overlay logo, aperçu de navigation), **style 243-1189 : 946 lignes**.

**Découpage proposé.** Le script est déjà mince : ne pas le découper. Extraire le skin en feuilles CSS
ordonnées, référencées par blocs `<style scoped src>` successifs (ordre conservé) :

```
components/media/video-js/
  host.css              ~110   hôte, overlay, poster, dimensions, fullscreen
  control-bar.css       ~350   barre de contrôle, progression, volume, temps, boutons prev/next/autoplay
  menus.css             ~170   menus qualité/sous-titres, time tooltips, items sélectionnés
  chapters.css          ~140   overlay/séparateurs de chapitres, boutons « passer »
  navigation-preview.css ~80   aperçu enrichi de l'épisode adjacent
```

**Risques spécifiques.** Les sélecteurs `:deep(.vjs-*)` comptent sur l'attribut scoped du composant hôte ; les
blocs `src` scoped le conservent, mais il faut vérifier le CSS produit (`vite build` puis inspection des
selecteurs générés) et non seulement le rendu dev. Les règles de layout dépendant des classes d'état Video.js
(`vjs-*`) doivent rester dans le même ordre relatif.

---

### 5. `services/storage.ts` — 1170 lignes

**État actuel.** Types (32-129, 174-311), constantes de clés (130-172), **un store Pinia de 650 lignes**
(313-964) qui construit les paramètres, les préférences home, les signets et les préférences lecteur, puis
~205 lignes de sanitizers (966-1170). Le store expose 14 fonctions et applique des `watch` de persistance
`localStorage`/IndexedDB. Indentation mixte (4 espaces dans le store, 2 ailleurs) et usages `as any` sur les
paramètres dynamiques.

**Responsabilités identifiées.**

| Bloc | Lignes (≈) | Contenu |
|------|-----------:|---------|
| Types de domaine | 230 | `ParameterEntry`, `EntryBookmark`, `VideoPlayerPreferences`, `Parameters`, `HomePreferences` |
| Clés et définitions | 180 | préfixes, clés vidéo/theme, `PARAMETERS_DEF`, `SectionRecordDefaults` |
| Paramètres + watchers | 160 | refs dynamiques, persistance `localStorage` |
| Préférences home | 120 | ordre des sections, modes de collection/vignettes, épinglage, déplacement |
| Signets | 150 | cache + IndexedDB, restauration, suppression, lookup |
| Préférences lecteur | 110 | volume, mute, qualité, pistes audio/texte + watchers |
| Utilitaires `localStorage` | 90 | `getStoreItem`, `setStoreItem`, `removeStoreItem`, `clearStore`, `isStoreAvailable` |
| Sanitizers | 205 | normalisation de toutes les valeurs relues |

**Options de découpage.**

- **Option A (recommandée, risque faible)** : conserver **un** store `useStorage` comme racine de composition,
  extraire uniquement les couches non réactives.

```
services/storage/
  index.ts             ~420   useStorage : refs, watchers, API publique (le store reste unique)
  types.ts             ~230   Parameters, HomePreferences, EntryBookmark, VideoPlayerPreferences, ParameterEntry
  keys.ts              ~180   préfixes, clés, PARAMETERS_DEF
  persistence.ts        ~90   wrappers localStorage + test de disponibilité
  sanitizers.ts        ~230   sanitizeEntryBookmark, boolean, numbers, tracks, settings
  sectionRecords.ts    ~150   helpers de cache/records de sections (fonctions pures réutilisées par le store)
  bookmarks.ts         ~120   pont cache ↔ IndexedDB (add/remove/refresh)
  videoPreferences.ts  ~110   pont refs ↔ préférences durables
```

- Option B (séparation complète) : quatre stores Pinia (`useParametersStore`, `useHomePreferencesStore`,
  `useBookmarksStore`, `useVideoPreferencesStore`) plus un `useStorage` façade. Meilleure isolation, mais
  impose de vérifier 25 importateurs, la réactivité croisée (paramètres ↔ thème ↔ lecteur) et l'ordre
  d'initialisation (`initStorage` unique). À traiter comme un changement de contrat séparé.

**Recommandation.** Faire A maintenant (aucun changement d'API), garder B comme évolution documentée dans
`docs/TODO.md` si le besoin de stores indépendants apparaît (par exemple pour l'admin app).

**Risques spécifiques.** Le store est instancié tôt dans `main.ts` et ses watchers persistent les valeurs :
extraire les sanitizers est sans risque, extraire les watchers ne l'est pas (ordre de création et `deep`).
Conserver impérativement un seul `defineStore('storage', ...)`.

---

### 6. `components/ProgramEntryDetails.vue` — 1140 lignes

**État actuel.** Script 1-1010 : wiring de 4 composables `entry-details`, observateur de recommandations,
état de signet, orchestration de navigation d'épisodes (le plus gros bloc), handlers de lecture, et ~20
computed de présentation. Template 1012-1140 (délègue à `EntryDetails`, `EntryDetailsCatalogSection`,
`MediaCardCollection`). Aucun style.

**Responsabilités identifiées.**

| Bloc | Lignes (≈) | Contenu |
|------|-----------:|---------|
| Wiring et état local | 130 | props, refs, compteur de navigation, bookmarks lus |
| Recommandations différées | 90 | `IntersectionObserver`, watch, sélection d'une recommandation |
| Composition des données | 90 | appel de `entryDetailsData`, `entryRecommendations`, `entryEpisodeSelection`, `entryVideoPlayer`, `entryDetailsPresentation` |
| Signets | 190 | restauration d'épisode, `saveCurrentBookmark`, toggle, progression |
| Navigation d'épisodes | 340 | transition, prochain épisode jouable, chargement de saison, steps, annulation |
| Surface média | 90 | source de fond, poster/logo, temps initial, `hasNextEpisode` |
| Handlers de lecture | 120 | progression, démarrage, fin, autoplay d'épisode |
| Template | 130 | composition des sous-composants |

**Découpage proposé.** Le composant doit rester le point d'assemblage ; sortir quatre composables dans le
dossier existant `composables/entry-details/` :

```
composables/entry-details/
  useEntryRecommendationsRail.ts  ~110   observer + chargement différé + sélection
  useEntryBookmark.ts             ~210   état, restauration, sauvegarde, toggle, progression
  useEntryEpisodeNavigation.ts    ~380   transitions, next playable, saisons, steps, annulation
  useEntryMediaSurface.ts          ~90   source de fond, poster/logo, temps initial
components/ProgramEntryDetails.vue ~380   wiring + template
```

`useEntryEpisodeNavigation` reçoit en paramètres les sorties de `entryDetailsData` et
`entryEpisodeSelection` (`selectSeasonById`, `selectEpisodeById`, `handleEpisodeStep`,
`handleLoadMoreSeasonEpisodes`, `hasMoreSeasonEpisodes`) plus les dépendances de lecture
(`mediaSource`, `showMediaPlayer`, `handleMediaSourceError`). Il ne doit pas importer le composant.

**Risques spécifiques.** Les transitions d'épisode reposent sur `latestEpisodeNavigationId` et sur
`shouldKeepMediaSurfaceMounted` partagé avec la surface média : le découpage doit conserver ces deux états
dans **un seul** module (navigation), avec des setters exposés à la surface. La restauration depuis signet
(`watch` immédiat sur `displayedEpisodes`) doit rester dans le composable signet et non dans le composant,
sinon l'ordre des `watch` change.

---

### 7. `composables/video/video-js-media-renderer/dash.ts` — 1108 lignes

**État actuel.** Module feuille déjà nommé par fonctionnalité, mais qui porte trois sujets distincts :
fusion des adaptation sets DASH (32-353), bridge de qualité dash.js (354-825), chapitres de période
publicitaire (826-1107).

**Découpage proposé.**

```
video-js-media-renderer/dash/
  index.ts            ~90   barrel (isDashSource, buildDashKeySystemOptions, installDashQualityBridge, ...)
  source.ts           ~120  isDashSource, buildDashKeySystemOptions, getDashMediaPlayer, ids d'audio générés
  manifest.ts         ~330  vues de manifest, mergeSwitchableVideoManifestAdaptationSets, événements
  qualityBridge.ts    ~450  installDashQualityBridge, DashQualityBridge, niveaux/qualité, auto
  periodChapters.ts   ~330  chapitres de période pub, fusion de chapitres, options
```

Les imports existants (`from '.../video-js-media-renderer/dash'`) restent valides grâce au barrel.

**Risques spécifiques.** `mergeSwitchableVideoManifestAdaptationSets` mute le manifest reçu : aucune copie ne
doit apparaître lors du déplacement. Les constantes de préfixe (`dash-video-`, `dash-audio-`) doivent rester
dans un seul module pour éviter deux sources de vérité.

---

### 8. `components/home/HomeHeroBanner.vue` — 1099 lignes

**État actuel.** Script 1-546 (rotation, résolution vidéo, sécurité, action), template 548-700 (150 lignes,
lisible), style 702-1099 (398 lignes, dont le sélecteur `.home-hero-banner__dot` déclaré deux fois et des
media queries en fin de bloc).

**Responsabilités identifiées.**

| Bloc | Lignes (≈) | Contenu |
|------|-----------:|---------|
| Props/constantes/état | 120 | timers, index actif, fenêtre d'indicateurs, resize observer, taille/aspect |
| Résolution de la vidéo de bannière | 180 | `createBannerPlayer`, source résolue, sécurité/allowlist, style, son |
| Action de bannière | 110 | cible média, routeur, href, libellé |
| Rotation et navigation | 120 | timer, previous/next/show, select |
| Cycle de vie | 60 | observer, `onMounted`/`onBeforeUnmount` |
| Template | 150 | média + panneau texte + contrôles |
| Style | 398 | skin du hero |

**Découpage proposé.**

```
composables/home/
  useBannerRotation.ts   ~180  index actif, fenêtre d'indicateurs, timer, navigation, observer de taille
  useBannerVideo.ts      ~200  résolution de source, allowlist/sécurité, son, aspect, fin de lecture
  useBannerAction.ts     ~100  cible, href, libellé, sélection
components/home/HomeHeroBanner.vue  ~330  props/émisions + composition + template (150)
components/home/hero-banner/
  layout.css             ~90   racine, chargement, backdrop, panneau, content
  media.css             ~150   image, vidéo, logo-background, logo, sound-toggle
  typography.css         ~90   eyebrow, titre, description, action
  controls.css          ~110   flèches, dots, viewport/track, reduced motion, responsive
```

**Risques spécifiques.** Le timer de rotation et le `ResizeObserver` sont couplés à la lecture vidéo
(`shouldRotateActiveBannerOnVideoEnd`) : ils doivent rester dans le même composable que la vidéo ou recevoir
un callback explicite `onVideoEnded`. Le `:key` du `VideoPlayer` dépend de `activeBannerMediaKey` calculé
depuis la source : ne pas le déplacer sans conserver l'identité exacte de la clé.

## Séquencement recommandé

| Lot | Contenu | Pourquoi cet ordre | Volume déplacé |
|-----|---------|--------------------|----------------|
| 0 | Validation de la convention barrel + `npm run type-check`/`build` de référence | Établit la preuve de non-régression | 0 |
| 1 | `dash.ts` → `dash/` | Module feuille, aucun état partagé, valide le barrel | 1108 |
| 2 | CSS de `VideoJsMediaRenderer.vue` puis de `HomeHeroBanner.vue` (option A) | Aucun changement de logique, gros gain immédiat | 1344 |
| 3 | `storage.ts` → `storage/` (option A) | Domaine isolé de l'UI, contrat conservé | 1170 |
| 4 | `rustify.ts` → `rustify/` | Le plus gros volume, mais déplacement mécanique et barrel large | 3508 |
| 5 | `useVideoJsMediaRenderer` vague 1 (storyboard, sous-titres, attributs, autoplay) | Helpers sans état, testables par type-check | ~920 |
| 6 | `useVideoPlayer` → slices | Contrat conservé, risque limité au view-model | 1233 |
| 7 | `ProgramEntryDetails.vue` → composables `entry-details` | Dépend des conventions posées aux lots 5-6 | 760 |
| 8 | `useVideoJsMediaRenderer` vague 2 (cycle de vie, application de source) | Le plus risqué : à faire seulement après stabilisation | ~850 |
| 9 | `entryVideoPlayer.ts` (992) et `VideoPlayer.vue` (845) | Déjà sous le seuil, à traiter pour éviter le retour au-dessus | ~1840 |

Chaque lot est indépendant et laisse le front compilable : aucun lot ne doit laisser un import cassé.

## Critères d'acceptation

- Aucun fichier de `front/public-app/src` ni `front/admin-app/src` au-dessus de 1000 lignes, et aucun nouveau
  fichier au-dessus de 600 lignes sans justification dans ce document.
- `@/services/rustify`, `@/services/storage` et `@/composables/video/video-js-media-renderer/dash` conservent
  exactement leur surface d'export (noms et signatures).
- `npm run type-check`, `npm run build` et `npm run lint:css` passent dans `front/public-app`.
- Le CSS déplacé produit les mêmes sélecteurs scoped (contrôle sur le bundle).
- Checklist manuelle lecteur : lecture simple, changement de qualité, pistes audio/sous-titres, storyboard,
  navigation épisode précédent/suivant, autoplay d'épisode, DASH + chapitres pub, direct, iframe bloquée en
  mode `safe`, bannière hero avec et sans vidéo, rotation automatique, son de bannière, reduced motion.

## Décisions à valider avant implémentation

1. Seuil cible : 600 lignes souhaité / 1000 lignes maximum (proposition ci-dessus) ou autre valeur.
2. CSS : extraction en fichiers `.css` référencés par `<style scoped src>` (option A) ou blocs `<style scoped>`
   multiples dans le SFC (option B, insuffisante pour `VideoJsMediaRenderer.vue`).
3. `storage.ts` : option A (store unique + couches extraites) ou option B (quatre stores + façade).
4. Vague 2 de `useVideoJsMediaRenderer` : incluse dans ce chantier ou reportée après validation des autres lots.
5. `entryVideoPlayer.ts` et `VideoPlayer.vue` : traités dans la foulée (lot 9) ou laissés en surveillance.

## Découpe retenue pour le lot 5 après revue (lot 5.4)

La revue de `ccfaeab0` a montré que les extractions `sourceApplication.ts` et
`playerLifecycle.ts` laissaient la propriété de l’état répartie entre les fichiers.
La proposition initiale de contexte global et de découpe de la vague 2 est remplacée
par quatre propriétaires instanciés une seule fois par appel du composable :

| Module | Responsabilité et état possédé |
|--------|-------------------------------|
| `useVideoJsMediaRenderer.ts` | Assemblage, création/destruction du lecteur, intégrations et watchers des props ; API publique inchangée |
| `qualityController.ts` | Préférence de qualité, état de pistes retenu pour DASH, bridge, retries, surcharge VHS et écoute du menu |
| `storyboard.ts` | Parsing VTT, inférence des dimensions d’image et préparation des options sprite |
| `storyboardController.ts` | Handle du plugin, cues courantes, grille, dimensions réactives, tooltip et intervalle dérivé |
| `sourceController.ts` | Compteur d’opération, listeners de transition, sous-titres distants et restauration de lecture/état |
| `playerUiState.ts` | Chargement, poster, aperçu de navigation et garde-fous d’émission |

Le composable principal passe de 932 à 543 lignes. Les nouveaux modules comptent
83 lignes pour l’UI, 225 pour les transitions, 252 pour la qualité et 256 pour
l’intégration storyboard ; la préparation storyboard reste à 381 lignes.

Les contrôleurs reçoivent des options typées et des lecteurs explicites du lecteur
ou de la source courante. Le contrôleur de source ne voit que les opérations utiles
aux transitions via des interfaces `Pick`, sans accès aux variables internes des autres
contrôleurs. Aucun état mutable n’est partagé au niveau des modules.

Une transition invalide d’abord l’opération précédente, libère ses listeners, ses
pistes et ses ressources de qualité, puis prépare le storyboard. Après l’attente, le
compteur et le lecteur courant sont vérifiés avant toute publication. Le cleanup des
listeners est enregistré avant `player.src()`. La fin de chargement reste émise une
fois par instance de lecteur ; les métadonnées restent émises une fois par source.

La destruction reste dans l’orchestrateur : sauvegarde de l’état, invalidation et
nettoyage de la source, destruction Video.js, puis remise à zéro du storyboard et
de l’UI. Les fichiers `sourceApplication.ts` et `playerLifecycle.ts` sont supprimés.
`subtitles.ts`, `playerAttributes.ts` et `autoplayMenu.ts` sont conservés.

Validation : type-check, build et lint CSS ; aucune nouvelle infrastructure ni
nouveau test. La checklist manuelle de la revue reste à exécuter sur des médias réels.
Les nouveaux fichiers restent couverts par la règle de chunk `video-player-core`
existante de Vite.

## Implémentation du lot 6 — `useVideoPlayer`

La signature `useVideoPlayer(props, emit)` et les 70 propriétés retournées sont
conservées. `VideoPlayer.vue`, son template et ses valeurs par défaut restent
inchangés. La découpe retenue reprend les cinq modules proposés :

| Fichier sous `composables/video/` | Lignes | Responsabilité |
|----------------------------------|-------:|----------------|
| `useVideoPlayer.ts` | 30 | Assemblage synchrone des slices et retour de leurs refs/handlers |
| `playerTypes.ts` | 260 | Props, événements, modes de surface et politique de référent iframe |
| `usePlayerSurface.ts` | 318 | Dernière source résolue, intention de transition, modes et états de rendu |
| `usePlayerIframe.ts` | 214 | Confirmation iframe, restrictions de sécurité et attributs iframe |
| `usePlayerVideoAttributes.ts` | 256 | Attributs vidéo, restauration de lecture et contrôles de navigation |
| `usePlayerHandlers.ts` | 259 | Préférences persistées, modèles des sélecteurs et émissions vers le parent |

Les slices reçoivent les mêmes props réactives et des interfaces `Pick` limitées
aux computed dont elles ont besoin. Elles partagent les refs, sans copier leurs
valeurs. La décision d’allowlist reste calculée une seule fois par la surface,
puis transmise à l’iframe. L’état d’une sélection de langue/lecteur en attente
appartient à la surface ; les handlers le modifient via `markSelectionPending()`.
Les préférences et leur opération de sauvegarde restent réunies dans les handlers.

Les deux watchers d’origine sont conservés avec les mêmes sources, callbacks et
options : suivi immédiat de la source média dans la surface, réinitialisation de
la confirmation lors du changement de source iframe dans la slice iframe. Toutes
les slices sont créées pendant le même setup, sans watcher ou état global ajouté.

Le cast `(computed as any)` de `renderedMediaSource` est remplacé par un
`ComputedRef<ResolvedPlayerMediaSource | null>`. Les branches lisant une source
optionnelle dans `activeSurfaceSource` normalisent son absence à `null`, conformément
au type annoncé et aux valeurs par défaut déjà appliquées par `VideoPlayer.vue`.
Les 69 autres types de propriétés retournées sont identiques.

Validation : comparaison des 70 noms et types retournés (aucun manque ni doublon),
comparaison structurelle des déclarations (71 déclarations de variables identiques ;
les quatre adaptations concernent le typage des sources et les deux appels à
`markSelectionPending()`), vérification des deux watchers, build avec type-check,
lint CSS et `git diff --check`. Aucun test ajouté. Les parcours manuels média,
bande-annonce, standalone et iframe dans les trois modes de sécurité restent à
valider avec des médias réels.

## Implémentation du lot 7 — `ProgramEntryDetails.vue`

Le composant passe de 1 140 à 341 lignes. Son template et ses props, valeurs par
défaut comprises, sont conservés. Il reste le point d’assemblage des composables
existants et des quatre extractions prévues :

| Fichier | Lignes | Responsabilité |
|---------|-------:|----------------|
| `useEntryRecommendationsRail.ts` | 71 | Chargement différé, observation du rail, déconnexion et navigation des recommandations |
| `useEntryBookmark.ts` | 244 | Préférences restaurées, progression, snapshot des signets, sauvegarde et toggle |
| `useEntryEpisodeNavigation.ts` | 470 | Identifiant monotone, transitions, autoplay, pagination intersaisons et handlers de navigation/lecture |
| `useEntryMediaSurface.ts` | 93 | Source de fond, images, aperçu des épisodes adjacents et temps initial restauré |

Le module navigation conserve ensemble `latestEpisodeNavigationId`, les flags
d’autoplay et `shouldKeepMediaSurfaceMounted`. Il reçoit des interfaces `Pick`
des contrôleurs existants, sans état mutable au niveau des modules ni copie des
valeurs des refs. La lecture et la remise à zéro de la progression passent par
le propriétaire des signets ; les changements de saison y délèguent le snapshot.
Les 470 lignes de navigation regroupent les handlers de lecture avec les transitions
qu’ils pilotent, plutôt que de partager les flags entre deux fichiers.

L’initialisation des signets est explicitement faite en deux étapes :

1. `useEntryBookmark()` crée les préférences et la progression avant
   `entryDetailsData()`, afin de fournir la saison préférée au chargement.
2. Après les données, les recommandations, la sélection, le lecteur et la
   présentation, la navigation enregistre les resets d’entrée puis de surface.
   `bookmark.bindSelection()` installe ensuite, une seule fois pendant setup,
   le watcher immédiat de restauration d’épisode et ses handlers de sauvegarde.

Cet ordre conserve celui du composant d’origine. Le callback de sauvegarde passé
à la navigation est invoqué uniquement lors des événements de sélection/lecture,
après le raccordement des signets. Les deux hooks du rail et son watcher
`flush: 'post'` sont préservés, ainsi que ses marges d’observation.

Validation : template et props comparés structurellement, 20 fonctions sur 22 et
53 déclarations de variables sur 54 identiques. Les deux fonctions adaptées
délèguent la progression/le snapshot au signet ; le computed de fond lit la même
prop par ref. Trois watchers sur quatre sont identiques ; le reset d’entrée
remplace ses trois affectations par la méthode de reset du signet. Ses effets et
l’ordre des quatre watchers sont conservés. Build avec type-check, lint CSS et
`git diff --check` validés ; aucun test ajouté. La restauration de signet,
la navigation paginée entre saisons, les annulations et les recommandations
restent à vérifier manuellement avec des médias réels.

## Suivi

- [x] Lot 0 — référence de validation (`type-check`, `build`, `lint:css`).
- [x] Lot 1 — `dash.ts` → `dash/`.
- [x] Lot 2 — extraction CSS `VideoJsMediaRenderer.vue` et `HomeHeroBanner.vue`.
- [x] Lot 3 — `storage.ts` → `storage/`.
- [x] Lot 4 — `rustify.ts` → `rustify/`.
- [x] Lot 5.1 — `useVideoJsMediaRenderer` vague 1 (storyboard, sous-titres, attributs, autoplay).
- [x] Lot 5.2 — `useVideoJsMediaRenderer` vague 2 (cycle de vie, application de source, état joueur).
- [x] Lot 5.3 — correction du storyboard VTT : lecture des dimensions/cues courantes via getters, sélection sur la barre de progression réelle et une seule mise à l’échelle après le plugin, avec positionnement au-dessus de la barre. Build (avec type-check) et lint CSS validés ; vérification visuelle sur une source réelle encore à effectuer. Revue du commit `ccfaeab0` : les sept constats sont corrigés, avec détail et validation dans le [rapport](review-ccfaeab0.md).
- [x] Lot 5.4 — consolidation par propriétaires d’état (`qualityController`, `storyboardController`, `sourceController`, `playerUiState`) ; suppression des extractions partielles de cycle de vie/source.
- [x] Lot 6 — `useVideoPlayer` → types, surface, iframe, attributs vidéo et handlers ; 70 propriétés conservées.
- [x] Lot 7 — `ProgramEntryDetails.vue` → recommandations, signets, navigation et surface média ; template et props conservés.
- [x] Lot 8 — `useVideoJsMediaRenderer` cycle de vie et application de source, couvert par les lots 5.2 et 5.4.
- [ ] Lot 9 — `entryVideoPlayer.ts` et `VideoPlayer.vue` (surveillance).
