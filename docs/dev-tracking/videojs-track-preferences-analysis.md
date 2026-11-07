# Analyse — Sélection automatique des pistes audio et sous-titres Video.js

## 1. Objectif

Rendre la sélection des pistes audio et des sous-titres **déterministe et pilotée par des
paramètres** lors d'un changement de source vidéo. Le lecteur ne doit plus conserver une piste
choisie par le moteur de lecture par simple effet de bord, et les pistes d'audiodescription ne
doivent pas être sélectionnées par accident.

Le mécanisme visé doit permettre de définir trois combinaisons ordonnées, par exemple :

| Ordre | Audio | Sous-titres |
|---|---|---|
| Primaire | Français | Désactivés |
| Secondaire | Anglais | Activés selon la priorité de langues |
| Tertiaire | Version originale (VO) | Activés selon la priorité de langues |

Avec une liste de priorité de langues de sous-titres, par exemple `fr` puis `en`.

## 2. État actuel

### 2.1 Capture de l'état

[`capturePlayerState()`](front/public-app/src/composables/video/video-js-media-renderer/state.ts:377)
capture notamment :

- `audioTrack` : la première piste audio dont `enabled === true` ;
- `textTrack` : la première piste de type `captions` ou `subtitles` dont `mode === 'showing'`,
  sinon un objet `mode: 'disabled'` ;
- `textTrackSettings` : les réglages visuels des sous-titres.

Les préférences sont stockées globalement sous `videoPlayer.preferences.audioTrack` et
`videoPlayer.preferences.textTrack` (`front/public-app/src/services/storage/keys.ts`). Elles ne
sont donc pas associées à une source ou à un lecteur particulier.

### 2.2 Restauration

[`restoreAudioTrack()`](front/public-app/src/composables/video/video-js-media-renderer/state.ts:277)
et [`restoreTextTrack()`](front/public-app/src/composables/video/video-js-media-renderer/state.ts:309)
recherchent la meilleure piste via
[`getTrackMatchScore()`](front/public-app/src/composables/video/video-js-media-renderer/state.ts:207) :

| Critère | Score |
|---|---|
| `id` identique, sauf identifiant généré par DASH (`dash-audio-*`) | 100 |
| `language` identique | 30 |
| `kind` identique | 20 |
| `label` identique | 10 |

Si aucune piste n'obtient un score supérieur à 0 :

- la piste audio courante du moteur reste inchangée ;
- la piste de sous-titres courante reste inchangée, sauf si la préférence stockée est
  `disabled`, auquel cas toutes les pistes affichables sont désactivées.

### 2.3 Cycle de changement de source

Dans [`useVideoJsMediaRenderer.ts`](front/public-app/src/composables/video/useVideoJsMediaRenderer.ts:414),
le changement de `props.source` :

1. capture l'état courant via `quality.captureState(player)` ;
2. émet `update:player-state`, ce qui persiste les préférences ;
3. applique la nouvelle source via `sources.applySource()`.

[`sourceController.applySource()`](front/public-app/src/composables/video/video-js-media-renderer/sourceController.ts:77)
appelle ensuite :

- `player.src(...)` ;
- `addRemoteSubtitleTracks(...)` ;
- `restorePersistedTrackPreferences(...)` immédiatement ;
- `applyPersistedPlayerState(...)` sur `loadedmetadata` et `canplay`.

Pour DASH, [`qualityBridge.ts`](front/public-app/src/composables/video/video-js-media-renderer/dash/qualityBridge.ts:382)
réapplique les préférences après `playbackMetadataLoaded`, `periodSwitchCompleted` et
`allTextTracksAdded`, car le source handler reconstruit les listes de pistes.

### 2.4 Sous-titres externes

[`addRemoteSubtitleTracks()`](front/public-app/src/composables/video/video-js-media-renderer/subtitles.ts:75)
ajoute les pistes WebVTT du résolveur avec `kind: 'subtitles'` et `srclang` lorsque `lang` est
fourni. Aucun `mode` n'est forcé : les pistes restent désactivées tant qu'aucune préférence ne les
active.

### 2.5 Documentation existante

Les spécifications indiquent que le lecteur public ajoute les pistes externes, « n'en active
aucune par défaut » et retire ses propres pistes au changement de source
(`docs/specifications/arachnea-stream-en.md:956` et
`docs/specifications/arachnea-stream-fr.md`). Cette phrase devra être nuancée après
l'implémentation, puisque les profils pourront activer une piste automatiquement.

## 3. Problème

Le comportement actuel est correct tant que la nouvelle source contient une piste dont l'identité
ou la langue correspond à la préférence stockée. Il devient imprévisible dans les cas suivants :

1. **La nouvelle source n'a pas la langue précédemment choisie.**
   `restoreAudioTrack()` ne trouve aucune correspondance et ne fait rien : c'est le moteur
   (HLS/VHS, dash.js ou le navigateur) qui décide, souvent via le flag `default` du manifeste ou
   `selectionPriority` pour DASH.

2. **La nouvelle source a plusieurs pistes de la même langue.**
   Le score ne distingue pas une piste principale d'une piste d'audiodescription si le `kind`
   ou le `label` diffèrent mais que la langue est identique. Une piste AD peut être retenue.

3. **La préférence stockée est globale.**
   Un choix manuel fait sur une source s'applique à toutes les sources suivantes, même lorsque
   celles-ci proposent une autre langue. Inversement, un choix devenu impossible n'est jamais
   remplacé par une règle explicite.

4. **Les changements programmatiques sont persistés comme des choix manuels.**
   Les écouteurs `audioTracks`/`textTracks` `change` émettent l'état après chaque application,
   y compris lorsque le lecteur applique lui-même une piste par défaut. Une future application de
   profils serait donc immédiatement enregistrée comme un choix utilisateur, puis rejouée sur les
   sources suivantes.

5. **Les sous-titres n'ont pas de règle de priorité de langue.**
   Le lecteur restaure une piste par identité ou la désactive. Il ne sait pas dire « active le
   français, sinon l'anglais, sinon rien ».

## 4. Modèle proposé

### 4.1 Profils ordonnés

Un profil décrit une combinaison audio + sous-titres. Les profils sont évalués dans l'ordre :
le premier profil dont la piste audio est disponible sur la source courante est appliqué.

```ts
/** Langue audio d'un profil : code BCP-47, `original`, ou `null` pour ignorer l'audio. */
export type VideoPlayerTrackProfileAudio = string | 'original' | null

/** Comportement des sous-titres une fois le profil retenu. */
export type VideoPlayerTrackProfileSubtitles = 'disabled' | 'auto'

/** Une combinaison audio + sous-titres parmi les profils ordonnés. */
export interface VideoPlayerTrackProfile {
  /** Langue audio recherchée, ou `original` pour la piste principale non-AD. */
  audio: VideoPlayerTrackProfileAudio
  /** Mode des sous-titres appliqué lorsque ce profil est retenu. */
  subtitles: VideoPlayerTrackProfileSubtitles
}
```

Le modèle recommandé est un **tableau ordonné** plutôt qu'un objet
`{ primary, secondary, tertiary }` :

- l'ordre est explicite et facile à parcourir ;
- l'ajout d'un quatrième profil ne change pas le contrat ;
- l'interface peut afficher « Profil 1/2/3 » sans figer la structure ;
- un tableau se sérialise directement en JSON dans `localStorage`.

Le tableau est toutefois borné à trois entrées dans un premier temps pour rester aligné sur le
besoin exprimé.

### 4.2 Paramètres proposés

| Paramètre | Type | Rôle |
|---|---|---|
| `videoPlayer.trackProfiles` | `VideoPlayerTrackProfile[]` | Profils ordonnés audio + sous-titres |
| `videoPlayer.subtitleLanguagePriority` | `string[]` | Langues de sous-titres par ordre de préférence |
| `videoPlayer.allowAudioDescription` | `boolean` | Autorise les pistes AD comme solution de dernier recours |

Un quatrième paramètre optionnel peut être ajouté si le comportement de repli des sous-titres
n'est pas fixé :

| Paramètre | Type | Rôle |
|---|---|---|
| `videoPlayer.subtitleFallback` | `'disabled' \| 'first'` | Comportement lorsqu'aucune langue prioritaire n'existe |

Valeur recommandée : `'disabled'`, cohérente avec le comportement actuel et avec la documentation
publique. Le paramètre peut être omis dans une première version et la valeur `disabled` codée en
dur dans l'algorithme.

### 4.3 Valeurs par défaut proposées

La valeur par défaut retenue est un profil neutre qui préserve le comportement actuel :

```ts
videoPlayer.trackProfiles = [
  { audio: 'original', subtitles: 'disabled' },
]
videoPlayer.subtitleLanguagePriority = [/* code de la langue d'interface */]
videoPlayer.allowAudioDescription = false
```

La priorité de sous-titres est dérivée du code de langue d'interface sélectionné dans les
paramètres (`language`). Lorsque l'interface est en mode automatique, le code du navigateur est
utilisé, avec `en` en dernier recours. Tant que la priorité n'a pas été personnalisée, elle suit
les changements de langue d'interface ; une valeur modifiée par l'utilisateur est conservée. Elle
n'a d'effet que si l'utilisateur ajoute ou modifie un profil en mode `auto`. L'exemple FR/EN/VO de
la section 1 reste le cas d'usage cible, configurable depuis l'interface.

### 4.4 Résolution d'un profil

Pour chaque source, l'algorithme suivant s'exécute après que les listes de pistes sont
disponibles :

```text
1. Construire la liste des pistes audio disponibles.
2. Si une sélection manuelle existe et correspond à la source courante :
     appliquer cette sélection et s'arrêter.
3. Pour chaque profil dans l'ordre :
     3.1. Chercher une piste audio correspondant à `profile.audio`.
     3.2. Si aucune piste n'est trouvée, passer au profil suivant.
     3.3. Appliquer la piste audio trouvée.
     3.4. Appliquer `profile.subtitles` :
            - `disabled` : désactiver toutes les pistes affichables ;
            - `auto` : appliquer la priorité de langues de sous-titres.
     3.5. S'arrêter.
4. Si aucun profil ne correspond :
     appliquer le repli implicite `original` + sous-titres désactivés.
```

Un profil est considéré comme « applicable » sur la base de l'audio uniquement. Le mode de
sous-titres est une conséquence du profil, pas une condition de sélection. Cela évite qu'un
profil soit ignoré parce que la source ne propose aucun sous-titre.

### 4.5 Algorithme de sélection audio

Pour une langue cible `target` :

1. **Normaliser les codes de langue** : minuscules, remplacement de `_` par `-`, alias
   (`fre`/`fra` → `fr`, `eng` → `en`, `ger`/`deu` → `de`), puis comparaison exacte avant
   comparaison sur la langue de base (`fr-CA` → `fr`).
2. **Exclure les pistes AD** lorsque `allowAudioDescription === false`.
3. **Chercher une correspondance exacte** sur la langue normalisée.
4. **Chercher une correspondance sur la langue de base**.
5. **Départager les candidats** avec l'ordre de préférence suivant :
   - piste `main` ou rôle `main` ;
   - piste `alternative` ou rôle `alternate` ;
   - piste sans rôle particulier ;
   - piste `description`/`main-desc` uniquement si les AD sont autorisées.
6. **Ne jamais utiliser l'index de la liste comme identité stable** pour DASH : les identifiants
   `dash-audio-*` sont reconstruits à chaque période.

Pour `target === 'original'` :

1. Chercher la piste dont le `kind` vaut `main` ou dont les rôles dash.js contiennent `main`.
2. Sinon, prendre la première piste non-AD dans l'ordre du manifeste.
3. Si seules des pistes AD existent :
   - si `allowAudioDescription === true`, prendre la première piste AD ;
   - si la source n'expose qu'une seule piste audio, la prendre en dernier recours pour éviter
     le silence, même lorsque `allowAudioDescription === false` ;
   - sinon, ne pas modifier la piste audio et journaliser un avertissement en développement.

Cette définition de « original » est volontairement indépendante de la piste actuellement
sélectionnée par le moteur : le moteur peut avoir choisi une piste AD ou une version doublée.

### 4.6 Algorithme de sélection des sous-titres

Pour `subtitles === 'auto'` :

1. Construire la liste des pistes affichables : `kind` dans `{ 'captions', 'subtitles' }`.
   Les pistes `chapters`, `metadata` et `descriptions` sont ignorées.
2. Parcourir `subtitleLanguagePriority` dans l'ordre.
3. Pour chaque langue, chercher une piste dont la langue normalisée correspond :
   - correspondance exacte d'abord (`fr` avec `fr`) ;
   - puis correspondance sur la langue de base (`fr` avec `fr-CA`).
4. À langue égale, préférer une piste non traduite automatiquement
   (`autoTranslate !== true`) à une piste traduite automatiquement.
5. Si aucune langue prioritaire ne correspond :
   - si `subtitleFallback === 'first'`, activer la première piste affichable ;
   - sinon, désactiver toutes les pistes affichables.
   La désactivation est la politique retenue ; `subtitleFallback` reste une extension possible.
6. Toujours désactiver les autres pistes affichables pour éviter que le moteur en affiche deux.

Pour `subtitles === 'disabled'`, désactiver toutes les pistes affichables.

Les pistes WebVTT externes ajoutées par `addRemoteSubtitleTracks()` sont traitées comme les
autres : elles possèdent `kind: 'subtitles'` et, lorsque le résolveur le fournit, `language`
(issu de `srclang`). Une piste sans langue ne peut être retenue que par le repli `first`.

### 4.7 Détection des pistes d'audiodescription

La détection doit combiner plusieurs sources, car aucun champ unique n'est fiable sur tous les
moteurs.

#### HLS / VHS

VHS expose déjà l'information dans le `kind` de la piste audio :

- `main-desc` lorsque les caractéristiques HLS contiennent
  `public.accessibility.describes-video` ;
- `main` pour la piste par défaut ;
- `alternative` pour les autres.

La détection principale est donc :

```ts
track.kind === 'main-desc'
```

#### DASH / dash.js

`videojs-contrib-dash` crée les pistes Video.js à partir de `mediaPlayer.getTracksFor('audio')`.
Chaque piste dash.js (`MediaInfo`) expose notamment :

- `index`, `lang`, `kind`, `labels` ;
- `roles` et `rolesWithSchemeIdUri` ;
- `accessibility` et `accessibilitiesWithSchemeIdUri` ;
- `selectionPriority`.

Les rôles ou accessibilités pertinents sont :

- `description` ;
- `alternate` avec `description` ;
- `main` pour la piste principale ;
- `dub`, `commentary` pour les autres variantes.

L'identifiant Video.js `dash-audio-${index}` permet de retrouver le `MediaInfo` correspondant :

```ts
const dashAudioTracks = mediaPlayer.getTracksFor('audio')
const dashTrack = dashAudioTracks.find((track) => `dash-audio-${track.index}` === videoJsTrack.id)
```

Il faut ajouter `getTracksFor('audio')` et un type `VideoJsDashMediaInfo` à
`VideoJsDashMediaPlayerHandle` dans
[`types.ts`](front/public-app/src/composables/video/video-js-media-renderer/types.ts:641).

#### Repli générique

Lorsque ni le `kind` ni les rôles DASH ne sont disponibles, utiliser une heuristique sur le
`label` et éventuellement la langue :

```text
/audio\s*description|audiodescription|descriptive|description|malvoyant|AD\b/i
```

Cette heuristique doit rester un **repli**, jamais le critère principal, car un label peut
contenir « description » sans être une piste AD.

#### Règle de sélection

- `allowAudioDescription === false` : les pistes détectées comme AD sont exclues des profils
  ordinaires et de la sémantique `original`, sauf lorsque la source n'expose qu'une seule piste
  audio, sélectionnée en dernier recours.
- `allowAudioDescription === true` : elles restent exclues tant qu'une piste non-AD correspond ;
  elles ne sont utilisées qu'en dernier recours.
- Un choix manuel explicite d'une piste AD est toujours respecté, même si le paramètre est
  `false` : le paramètre contrôle les sélections automatiques, pas la volonté de l'utilisateur.

### 4.8 Priorité entre sélection manuelle et profils

Le principe recommandé est le suivant :

1. **Choix manuel de l'utilisateur** : il gagne lorsqu'il peut être associé à la source
   courante (même `id` non généré, ou même langue + `kind` + `label`).
2. **Profils** : ils s'appliquent lorsqu'aucun choix manuel ne correspond.
3. **Repli implicite** : `original` + sous-titres désactivés si aucun profil ne correspond.

Pour que cela fonctionne, il faut distinguer un choix **manuel** d'une sélection **issue d'un
profil**. Deux approches sont possibles.

#### Option A — Ajouter une origine aux préférences (retenue)

Ajouter un champ optionnel `origin?: 'manual' | 'profile'` à `VideoJsTrackPreference` et
`VideoJsTextTrackPreference`. Le renderer mémorise la dernière sélection qu'il a appliquée pour
un profil. Lors d'un événement `change` :

- si l'état capturé est identique à la dernière sélection de profil, émettre `origin: 'profile'` ;
- sinon, émettre `origin: 'manual'`.

Lors de la restauration, seules les préférences `origin: 'manual'` (ou sans `origin`, pour la
rétrocompatibilité) sont candidates au choix manuel. Les préférences `origin: 'profile'` sont
ignorées, ce qui permet aux profils de rester maîtres du comportement sur une nouvelle source.

Avantages :

- pas besoin d'intercepter les clics dans les menus Video.js ;
- les changements programmatiques ne polluent plus les préférences manuelles ;
- la rétrocompatibilité est simple : une préférence sans `origin` est traitée comme manuelle.

Inconvénient : il faut comparer proprement les états (id, langue, label, kind, mode) et maintenir
cette comparaison dans un seul module.

#### Option B — Ne plus persister les pistes

Supprimer la persistance de `audioTrack` et `textTrack` et appliquer systématiquement les profils
à chaque source. C'est le comportement le plus déterministe, mais il supprime la mémoire du choix
manuel entre deux sessions et entre deux épisodes.

#### Option C — Choix manuel par source

Stocker le choix manuel avec une clé de source (URL ou empreinte stable). Le choix ne se rejoue
que sur la même source. C'est le plus précis, mais cela ajoute une structure de stockage et une
gestion de cycle de vie des clés.

L'option A est retenue : elle conserve la mémoire du choix utilisateur tout en garantissant que
les profils reprennent la main dès qu'une source ne correspond plus.

#### Cas particulier des sous-titres désactivés

Une préférence `mode: 'disabled'` n'a pas d'identité de piste. Deux politiques sont possibles :

- **Désactivation globale** : un utilisateur qui désactive explicitement les sous-titres ne les
  voit plus réactivés par un profil, même sur une nouvelle source. C'est le comportement le plus
  respectueux de l'intention, mais il neutralise les profils `auto` jusqu'à réactivation manuelle.
- **Désactivation limitée à la source** : le profil `auto` réactive les sous-titres sur la source
  suivante. C'est plus proche de l'objectif « valeurs par défaut par source », mais un utilisateur
  qui coupe les sous-titres devra le refaire à chaque épisode.

La désactivation globale est retenue. Elle pourra être rendue configurable plus tard.

## 5. Points d'intégration

### 5.1 Nouveau module

Créer un module dédié, par exemple
`front/public-app/src/composables/video/video-js-media-renderer/trackPreferences.ts`, contenant :

- les types de profils et de configuration ;
- la normalisation des codes de langue ;
- la détection AD (HLS, DASH, repli label) ;
- la résolution du profil applicable ;
- la sélection de la piste audio et de la piste de sous-titres ;
- l'application des modes (`enabled`, `mode`);
- la comparaison d'identité utilisée pour distinguer manuel et profil.

Garder `state.ts` responsable de la capture/restauration de l'état global, et déléguer la
sélection des pistes à ce module évite d'alourdir davantage `state.ts`.

### 5.2 `state.ts`

- `restoreAudioTrack()` et `restoreTextTrack()` deviennent des appels au nouveau module, avec
  une configuration de profils passée en paramètre.
- `applyPersistedPlayerState()` et `restorePersistedTrackPreferences()` acceptent une
  `VideoPlayerTrackSelectionConfig` supplémentaire.
- La capture ajoute l'origine (`manual`/`profile`) si l'option A est retenue.

### 5.3 Contrôleurs et composable

- `sourceController.applySource()` transmet la configuration de profils à
  `applyPersistedPlayerState()` et `restorePersistedTrackPreferences()`.
- `qualityController` conserve la configuration dans sa fermeture et la transmet au callback
  `restoreTrackPreferences` du bridge DASH.
- `useVideoJsMediaRenderer` lit les paramètres via `useStorage().getParameters()` au moment de
  l'application, ou expose un `getTrackSelectionConfig()` pour que les valeurs restent à jour
  sans recréer le lecteur.
- Les profils ne s'appliquent que lorsque `initialPlayerState` est non nul, c'est-à-dire pour les
  lecteurs contrôlés par l'utilisateur. Les vidéos d'arrière-plan et les lecteurs sans contrôles
  conservent leur comportement actuel.

### 5.4 DASH

- Ajouter `getTracksFor('audio')` au type `VideoJsDashMediaPlayerHandle`.
- Ajouter un type `VideoJsDashMediaInfo` avec les champs `index`, `lang`, `kind`, `labels`,
  `roles`, `rolesWithSchemeIdUri`, `accessibility`, `accessibilitiesWithSchemeIdUri` et
  `selectionPriority`.
- Conserver le mécanisme existant de réapplication après renouvellement des listes de pistes.
- Ne pas se fier à l'ordre du tableau `getTracksFor('audio')` pour l'identité : utiliser
  `index` et l'identifiant `dash-audio-${index}`.

### 5.5 Stockage

- Ajouter les trois clés à `PARAMETERS_DEF` dans
  [`keys.ts`](front/public-app/src/services/storage/keys.ts:25).
- Ajouter les champs correspondants à l'interface `Parameters` dans
  [`types.ts`](front/public-app/src/services/storage/types.ts:115).
- Ajouter des sanitizers :
  - `sanitizeTrackProfiles()` : tableau borné, valeurs audio valides, mode `disabled`/`auto`,
    déduplication éventuelle ;
  - `sanitizeSubtitleLanguagePriority()` : tableau de codes non vides, normalisés, longueur
    bornée ;
  - `sanitizeBoolean()` pour `allowAudioDescription`.
- Les tableaux sont déjà sérialisés en JSON par
  [`persistence.ts`](front/public-app/src/services/storage/persistence.ts:8), aucune évolution
  du stockage n'est nécessaire.
- Prévoir une migration douce : si `videoPlayer.trackProfiles` est absent ou invalide, utiliser
  la valeur par défaut sans écraser les autres préférences.

### 5.6 Interface

Ajouter une section « Pistes du lecteur » dans
[`Parameters.vue`](front/public-app/src/components/Parameters.vue:329), sous la section
« Lecteur ». Contenu proposé :

1. **Trois lignes de profil**, chacune avec :
   - un select audio : `original`, `fr`, `en`, et une option « autre langue » ;
   - un select sous-titres : `Désactivés` / `Automatiques`.
2. **Liste de priorité des sous-titres** : un champ texte acceptant des codes séparés par des
   virgules (`fr, en`) ou un composant de liste ordonnée avec ajout/suppression/réordonnancement.
3. **Switch « Autoriser l'audiodescription »**.

La priorité des sous-titres utilise un composant dédié `ParameterLanguagePriority.vue` :

- liste ordonnée de codes de langue ;
- ajout, suppression et réordonnancement via des boutons monter/descendre ;
- validation et normalisation des codes à la saisie ;
- commandes accessibles au clavier.

Le reste de la section réutilise les composants existants :

- `ParameterSegmented` pour le mode des sous-titres ;
- un `<select>` pour la langue audio, avec une liste curatée (`original`, `fr`, `en`) et une
  option de saisie libre ;
- `ParameterSwitch` pour l'audiodescription.

### 5.7 i18n

Ajouter les libellés dans `front/public-app/public/locales/en.json` et `fr.json`, sous
`settings.playerTracks` :

```json
{
  "settings": {
    "playerTracks": {
      "title": "Track selection",
      "profile": "Profile {index}",
      "audio": "Audio language",
      "subtitles": "Subtitles",
      "subtitlesDisabled": "Disabled",
      "subtitlesAuto": "Automatic",
      "original": "Original version",
      "subtitlePriority": "Subtitle language priority",
      "subtitlePriorityHint": "Highest priority first.",
      "addLanguage": "Add language",
      "removeLanguage": "Remove language {code}",
      "moveLanguageUp": "Move {code} up",
      "moveLanguageDown": "Move {code} down",
      "allowAudioDescription": "Allow audio description",
      "invalidLanguage": "Invalid language code: {code}"
    }
  }
}
```

Les libellés doivent rester en anglais dans le code et être traduits dans les fichiers de locale,
conformément aux règles du projet.

### 5.8 Documentation

Après implémentation :

- mettre à jour la phrase « n'en active aucune par défaut » dans
  `docs/specifications/arachnea-stream-fr.md` et `arachnea-stream-en.md` pour décrire les profils
  et la priorité de langues ;
- ajouter une entrée dans `CHANGELOG.md`, section `Unreleased` > `front-public` >
  `Added`/`Changed` ;
- mettre à jour `docs/TODO.md` si des suites sont identifiées (par exemple, liste ordonnée de
  langues dans l'interface).

## 6. Migration et compatibilité

- Les préférences existantes `videoPlayer.preferences.audioTrack` et `textTrack` restent
  lisibles. Une préférence sans `origin` est traitée comme manuelle.
- Si l'utilisateur n'a jamais choisi de piste, les profils s'appliquent dès la première source.
- Si l'utilisateur avait choisi une piste qui n'existe plus, le profil prend le relais au lieu de
  laisser le moteur décider.
- Les paramètres absents ou invalides ne doivent jamais empêcher la lecture : les sanitizers
  retombent sur les valeurs par défaut.
- Le comportement des vidéos d'arrière-plan et des lecteurs sans contrôles reste inchangé tant que
  `initialPlayerState` est nul.

## 7. Cas limites

| Cas | Comportement attendu |
|---|---|
| Une seule piste audio | Elle est sélectionnée, même si elle est AD et que `allowAudioDescription` est `false` (dernier recours). |
| Aucune piste audio exposée | Ne rien changer ; le moteur garde son comportement natif. |
| Piste AD étiquetée uniquement par `label` | Détection par heuristique, exclue des profils si `allowAudioDescription` est `false`. |
| Piste `main` + piste AD de même langue | La piste `main` est préférée ; l'AD n'est jamais choisie par un profil. |
| Plusieurs pistes de la même langue (doublage, commentaire) | Préférer `main`, puis `alternate`, puis la première non-AD. |
| Sous-titres `fr-CA` avec priorité `fr` | Correspondance sur la langue de base. |
| Sous-titres `fr` humains + `fr` auto-traduits | Préférer la piste humaine. |
| Aucune langue de sous-titre prioritaire | Désactivation (ou première piste si `subtitleFallback === 'first'`). |
| Source DASH multi-périodes | Réapplication après `periodSwitchCompleted` ; les identifiants générés sont ignorés. |
| Source HLS dont les pistes audio arrivent après `loadedmetadata` | Réappliquer sur `addtrack`/`change` avec anti-rebond, sans boucle. |
| Changement de source pendant une sélection manuelle | Le choix manuel est capturé avant le changement ; s'il ne correspond pas, les profils s'appliquent. |
| Utilisateur désactive les sous-titres | Selon la politique retenue : désactivation globale ou limitée à la source. |
| Lecteur d'arrière-plan | Profils non appliqués (`initialPlayerState` nul). |
| Safari avec pistes audio natives | Les pistes peuvent être recréées par le moteur ; utiliser langue + label, pas l'index. |
| Flux live | Mêmes règles ; éviter de réappliquer en boucle à chaque mise à jour de playlist. |

## 8. Matrice de vérification manuelle

Aucun test automatisé n'est prévu par défaut (règle du projet). La vérification suivante peut être
faite manuellement sur les sources de test existantes :

1. Source avec audio FR/EN/VO et sous-titres FR/EN.
   - Profil primaire FR + désactivés → audio FR, aucun sous-titre.
   - Profil secondaire EN + auto → audio EN, sous-titres FR.
2. Source avec audio EN/VO uniquement.
   - Profil primaire FR indisponible → profil secondaire EN appliqué.
3. Source avec audio VO uniquement.
   - Profil tertiaire `original` appliqué.
4. Source avec piste principale + piste AD.
   - `allowAudioDescription: false` → piste principale.
   - `allowAudioDescription: true` → piste principale tant qu'elle existe.
5. Source avec uniquement une piste AD.
   - `allowAudioDescription: false` → dernier recours AD, pas de silence.
6. Source sans sous-titres.
   - Profil `auto` → audio appliqué, sous-titres désactivés.
7. Sous-titres `fr-CA` avec priorité `fr`.
   - Piste `fr-CA` activée.
8. Choix manuel d'une piste sur la source A, puis retour à la source A.
   - Le choix manuel est restauré.
9. Choix manuel d'une piste sur la source A, puis passage à la source B sans cette piste.
   - Les profils s'appliquent ; le choix manuel reste stocké pour la source A.
10. Désactivation manuelle des sous-titres, puis changement de source.
    - Comportement conforme à la politique retenue.
11. DASH multi-périodes.
    - La piste reste cohérente après `periodSwitchCompleted`.
12. HLS avec pistes audio ajoutées tardivement.
    - Le profil est appliqué après l'ajout des pistes.
13. Lecteur d'arrière-plan.
    - Aucun changement de comportement.

## 9. Décisions retenues

| Sujet | Décision |
|---|---|
| Structure des profils | Tableau ordonné `VideoPlayerTrackProfile[]`. |
| Sémantique de `original` | Piste `main` non-AD, puis première piste non-AD. |
| Choix manuel | Global, appliqué s'il correspond à la nouvelle source ; sinon profils. |
| Sous-titres désactivés manuellement | Désactivation globale, respectée par les profils `auto`. |
| Repli des sous-titres | Désactivation lorsqu'aucune langue prioritaire n'est disponible. |
| Valeurs par défaut | Profil neutre `original` + sous-titres désactivés ; priorité dérivée de la langue d'interface (navigateur en mode automatique, `en` en dernier recours). |
| Interface de priorité | Composant dédié de liste ordonnée. |
| Audiodescription | `allowAudioDescription: false` par défaut ; dernier recours uniquement. |
| Portée d'application | Lecteurs contrôlés uniquement (`initialPlayerState` non nul). |
| Liste des langues audio | Liste curatée (`original`, `fr`, `en`) + saisie libre d'un code. |

## 10. Découpage d'implémentation proposé

1. **Types et stockage** : profils, priorité, sanitizers, paramètres, valeurs par défaut. *(implémenté)*
2. **Module de sélection** : normalisation des langues, détection AD, résolution des profils,
   application des pistes. *(implémenté dans `trackPreferences.ts`)*
3. **Intégration au renderer** : `state.ts`, `sourceController`, `qualityController`, bridge
   DASH, gestion de l'origine manuel/profil.
4. **Interface et i18n** : section « Pistes du lecteur », composant de liste ordonnée,
   traductions.
5. **Documentation et changelog** : spécifications, `CHANGELOG.md`, `docs/TODO.md` si nécessaire.
6. **Vérification manuelle** : matrice de la section 8.
