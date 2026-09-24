# Analyse — sous-titres externes dans `resolve_stream`

## Objectif

Étendre le contrat générique de résolution de flux afin qu'une réponse `resolve_stream` puisse
exposer une liste facultative de sous-titres externes :

```json
{
  "stream_url": ["https://example.test/video.m3u8"],
  "manifest_type": "hls",
  "subtitles": [
    {
      "lang": "en",
      "label": "English",
      "link": "/api/http-proxy/https://cdn.example.test/subtitles/en.vtt"
    },
    {
      "lang": "fr",
      "label": "Français",
      "link": "/api/http-proxy/https://cdn.example.test/subtitles/fr.vtt"
    }
  ]
}
```

Le premier fournisseur concerné est Vidzy, dont les pages embed déclarent actuellement leurs
pistes WebVTT avec `player.loadTracks`. La solution doit toutefois rester indépendante de Vidzy et
réutilisable par les autres résolveurs YAML ou Rust.

## État actuel

### Backend

`ResolvedPlayerStream`, dans
`server/crates/arachnea-stream/src/services/player_resolver.rs`, expose désormais une liste
facultative de `ResolvedPlayerSubtitle`. La conversion générique dans
`server/crates/arachnea-stream/src/stream_resolver.rs` conserve l'ordre de la source, normalise les
valeurs facultatives et ignore les liens non lisibles par le navigateur.

Le résolveur Vidzy, dans `server/services/arachnea-stream-hoster/vidzy.yaml`, extrait les objets
`player.loadTracks`, retient les seules pistes `kind: 'subtitles'` et proxifie leurs WebVTT avec les
en-têtes nécessaires.

### Frontend

La réponse backend est normalisée dans
`front/public-app/src/services/rustify.ts`, représentée par
`EntryResolvedPlayerStream` dans `front/public-app/src/types/entry.ts`, puis transmise à
`ResolvedVideoMediaSource` par `resolveBackendStreamMediaSource`.

Le renderer `front/public-app/src/composables/video/useVideoJsMediaRenderer.ts` ajoute les pistes
distantes avec `addRemoteTextTrack` après `player.src(...)`, retire explicitement les pistes qu'il
possède lors des changements de source et de la destruction, puis réapplique les préférences de
pistes texte déjà enregistrées.

### Distinction avec `storyboard_vtt_url`

`storyboard_vtt_url` est un WebVTT spécialisé pour les miniatures de prévisualisation. Son contenu
est téléchargé et interprété par le code de storyboard. Il ne doit pas être réutilisé pour les
sous-titres : les deux ressources ont des sémantiques, des consommateurs et des cycles de vie
différents.

## Contrat proposé

### Structure publique

Ajouter le champ facultatif suivant à la réponse sérialisée de `ResolvedPlayerStream` :

```text
subtitles?: Array<{
  lang?: string
  label?: string
  link: string
}>
```

Signification des champs :

| Champ | Obligatoire | Description |
|---|---:|---|
| `lang` | non | Code de langue annoncé par la source, idéalement BCP 47 (`en`, `fr`, `fr-CA`, etc.). |
| `label` | non | Libellé humain affiché dans le menu de sous-titres Video.js. |
| `link` | oui | URL HTTP(S), ou URL de proxy applicatif, vers une piste WebVTT lisible par le navigateur. |

`lang` et `label` sont individuellement facultatifs. Une piste contenant uniquement `link` reste
valide : le frontend dérive alors son libellé depuis le nom du fichier présent dans l'URL. Si aucun
nom de fichier exploitable ne peut être extrait, le renderer ignore finalement la piste.

Le contrat demandé reste volontairement minimal. Les attributs Video.js `kind` et `default` ne
sont pas ajoutés dans cette première version. Le frontend utilisera donc `kind: 'subtitles'` et
n'activera aucune piste par défaut. Ce choix évite qu'une préférence propre au fournisseur ne
prenne le dessus sur la préférence de piste déjà conservée par le lecteur.

### Modèle Rust recommandé

Ajouter un modèle dédié dans `player_resolver.rs` :

```rust
/// External subtitle track exposed by a resolved player stream.
#[derive(Serialize)]
pub(crate) struct ResolvedPlayerSubtitle {
    /// Language code declared by the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Human-readable track label displayed by the player.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Browser-readable WebVTT URL.
    pub link: String,
}
```

Puis compléter `ResolvedPlayerStream` avec :

```rust
/// Optional external subtitle tracks associated with the resolved stream.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub subtitles: Vec<ResolvedPlayerSubtitle>,
```

Une `Vec` vide avec `serde(default)` est préférable à `Option<Vec<_>>` :

- le champ reste absent du JSON lorsqu'aucune piste n'existe ;
- `Default` continue de fonctionner naturellement pour tous les résolveurs Rust existants ;
- les constructeurs utilisant `..Default::default()` ne demandent aucune adaptation ;
- le code consommateur manipule directement une collection sans double état `None`/liste vide.

Les initialiseurs exhaustifs de `ResolvedPlayerStream` devront néanmoins recevoir
`subtitles: Vec::new()` ou être convertis localement vers `..Default::default()` lorsque cela ne
modifie pas leur comportement.

### Validation et normalisation backend

`convert_resolver_entry_to_stream` doit lire `subtitles.items` et construire une piste dès que
`link` est présent après suppression des espaces de début et de fin. `lang` et `label` restent
facultatifs ; une valeur optionnelle vide doit être normalisée vers `None` et omise du JSON.

Règles recommandées :

1. conserver l'ordre du resolver, qui devient l'ordre du menu Video.js ;
2. ignorer un item sans `link` plutôt que rejeter tout le flux vidéo ;
3. refuser ou ignorer les liens qui ne sont ni HTTP(S) ni des chemins applicatifs de proxy
   explicitement produits par le backend ;
4. conserver `lang` exactement comme fourni, sans normaliser pour l'instant les codes tels que
   `eng`/`fre` vers `en`/`fr` et sans en modifier arbitrairement la casse ;
5. dédupliquer les doublons exacts au minimum sur `(lang, label, link)`, y compris lorsque l'un des
   deux champs optionnels est absent ;
6. ne jamais exposer au navigateur des en-têtes, cookies ou jetons séparément du lien proxifié.

L'extraction peut être isolée dans une fonction `extract_subtitles` analogue à
`extract_storyboard`, mais parcourant tous les `items` du nœud `subtitles`.

## Adaptation du resolver Vidzy

### Source des données

Les embeds Vidzy observés appellent `player.loadTracks` avec plusieurs objets contenant les
informations de langue, de libellé et d'URL WebVTT. Le YAML doit extraire chaque objet comme un
item distinct de `subtitles`, sans coder en dur les langues anglaise et française.

La structure de sortie attendue du YAML est :

```yaml
subtitles:
  - lang: eng
    label: English
    link: /api/http-proxy/https://.../english.vtt
  - lang: fre
    label: Français
    link: /api/http-proxy/https://.../french.vtt
```

Les noms exacts employés dans le JavaScript Vidzy peuvent varier (`lang`, `srclang`, `language`,
`file`, `src`, etc.). L'expression d'extraction doit être basée sur la forme réellement observée
de `player.loadTracks` et rester limitée à ce bloc pour éviter de capturer d'autres URLs VTT de la
page, notamment un éventuel storyboard.

### Construction YAML recommandée

Scrapyfy prend déjà en charge les tableaux d'objets et le post-processus
`extract_regex_items`. Une implémentation déclarative peut donc :

1. placer le corps HTML dans un champ interne temporaire, par exemple `_subtitle_tracks_source` ;
2. appliquer `extract_regex_items` sur le bloc `player.loadTracks(...)` ou sur ses objets ;
3. produire la cible `subtitles` avec `link` et les entrées disponibles parmi `lang` et `label` ;
4. appliquer à `link` les actions de nettoyage, `resolve_url` et proxy nécessaires ;
5. laisser `convert_resolver_entry_to_stream` ignorer le champ temporaire, qui ne sera jamais
   sérialisé dans la réponse publique `ResolvedPlayerStream`.

Cette voie est préférable à un traitement Vidzy codé dans `stream_resolver.rs` : le Rust reste
générique et le format propre au fournisseur demeure dans `vidzy.yaml`.

### Proxy et en-têtes

Une piste distante est téléchargée par le navigateur après l'appel à `addRemoteTextTrack`.
Contrairement aux requêtes backend, cette API ne permet pas de fournir librement `Referer`,
`Origin` et `User-Agent`. De plus, le CDN peut ne pas autoriser l'origine du frontend avec CORS.

Le `link` Vidzy doit donc être transformé en URL du proxy HTTP Arachnea dans le YAML, avec les
mêmes métadonnées amont nécessaires au manifeste :

```yaml
- type: resolve_url
  proxy: true
  proxy_headers:
    Referer: "{url}"
    Origin: "{request_origin}"
    User-Agent: "Mozilla/5.0 ..."
```

Le proxy doit renvoyer un type compatible WebVTT, idéalement `text/vtt`, et permettre une lecture
same-origin depuis le frontend. Aucune règle de réécriture HLS n'est requise dans un VTT de
sous-titres ordinaire.

Il faut éviter une double proxification. L'invariant recommandé est le suivant : le champ public
`subtitles[].link` contient déjà une URL directement consommable par le navigateur. Pour Vidzy,
c'est le YAML qui remplit cet invariant avec `resolve_url.proxy: true`. Le traitement générique
Rust ne doit donc pas envelopper une seconde fois ces liens.

## Propagation frontend

### Type unique et normalisation

Définir un seul type frontend dans `front/public-app/src/types/entry.ts` et le réutiliser dans toute
la chaîne, y compris dans `ResolvedVideoMediaSource`. Une seconde structure identique n'apporterait
aucune isolation utile puisque la résolution frontend ne transforme pas le contrat des pistes :

```ts
export interface ResolvedPlayerSubtitle {
  lang?: string
  label?: string
  link: string
}
```

`EntryResolvedPlayerStream` et `ResolvedVideoMediaSource` recevront tous deux
`subtitles: ResolvedPlayerSubtitle[]`. Le service `players.ts` importera donc ce type au lieu de
déclarer un type vidéo intermédiaire. La normalisation dans `rustify.ts` doit :

- accepter uniquement un tableau ;
- ignorer les valeurs qui ne sont pas des objets ;
- exiger et normaliser `link` ;
- normaliser `lang` et `label` lorsqu'ils sont présents, en convertissant les chaînes vides vers
  l'absence de valeur ;
- conserver une piste lorsque `lang` et `label` sont tous deux absents, afin que le renderer puisse
  appliquer le fallback fondé sur le nom du fichier ;
- retourner une liste vide si le champ backend est absent, pour préserver la compatibilité avec
  les anciennes réponses.

`resolveBackendStreamMediaSource` propagera la liste sans changer sa structure pour les transports
HLS, DASH et fichier. Les URLs peuvent être passées par le normaliseur d'URL déjà utilisé dans
`players.ts`, sans perdre les chemins relatifs du proxy applicatif.

### Tous les points d'appel

La nouvelle donnée ne doit pas être propagée uniquement par la page de détail. Tous les appels à
`resolveBackendStreamMediaSource` doivent conserver le nouveau paramètre ou, de préférence,
passer à un argument objet afin d'éviter les erreurs liées à une signature positionnelle devenue
longue.

Les chemins actuellement concernés comprennent au minimum :

- `front/public-app/src/composables/entry-details/entryVideoPlayer.ts` ;
- `front/public-app/src/components/LiveEntryDetails.vue` ;
- `front/public-app/src/components/home/HomeHeroBanner.vue` ;
- `front/public-app/src/composables/home/useHomeCatalog.ts`.

Les changements d'URL de secours doivent conserver les mêmes sous-titres, car les différentes
valeurs de `stream_url` représentent des alternatives pour le même média résolu.

## Intégration Video.js

### Ajout des pistes

Après l'application de `player.src(...)`, construire les options en omettant `srclang` lorsqu'il
n'existe pas. Le libellé Video.js utilise `label` en priorité, puis `lang`, puis le nom du fichier
extrait de `link` :

```ts
const getSubtitleFileLabel = (link: string): string | undefined => {
  try {
    const url = new URL(link, window.location.origin)
    const pathSegments = url.pathname.split('/').filter(Boolean)
    const filename = pathSegments[pathSegments.length - 1]

    if (!filename) {
      return undefined
    }

    const decodedFilename = decodeURIComponent(filename)
    const label = decodedFilename.replace(/\.vtt$/i, '').trim()
    return label || undefined
  } catch {
    return undefined
  }
}

const label = subtitle.label ?? subtitle.lang ?? getSubtitleFileLabel(subtitle.link)

if (!label) {
  return
}

player.addRemoteTextTrack(
  {
    kind: 'subtitles',
    src: subtitle.link,
    label,
    ...(subtitle.lang ? { srclang: subtitle.lang } : {}),
  },
  true,
)
```

Les règles de fallback sont donc explicites :

- avec `lang` et `label`, transmettre les deux valeurs ;
- avec seulement `lang`, utiliser ce code comme `srclang` et comme `label` visible ;
- avec seulement `label`, transmettre le libellé sans inventer de `srclang` ;
- sans `lang` ni `label`, utiliser le nom du fichier de `link`, décodé et privé de son extension
  `.vtt`, comme libellé visible ;
- si aucun nom de fichier exploitable n'est disponible, ignorer la piste plutôt que d'inventer un
  libellé générique.

Le fallback ne modifie jamais `lang`. En particulier, une valeur source telle que `eng` ou `fre`
est transmise telle quelle à `srclang` pour cette première implémentation.

Le paramètre `manualCleanup: true` permet au renderer de posséder explicitement le cycle de vie
des pistes. Les éléments ou pistes retournés doivent être conservés dans une collection locale
afin de pouvoir les retirer proprement avec `removeRemoteTextTrack` avant d'appliquer une nouvelle
source et lors de la destruction du player.

La déclaration TypeScript locale `VideoJsPlayer` doit exposer les méthodes Video.js utilisées par
le composable. Les types installés de Video.js déclarent actuellement
`addRemoteTextTrack(options, manualCleanup)` et `removeRemoteTextTrack(...)`; le type local doit
les refléter sans recourir à des conversions non typées dispersées.

### Cycle de vie et changement de source

Le nettoyage et l'ajout doivent être intégrés à `applySourceToPlayer`, car cette fonction est
utilisée aussi bien à la création du player qu'au remplacement d'une source existante.

Ordre recommandé :

1. supprimer toutes les pistes distantes ajoutées pour la source précédente ;
2. appeler `player.src(...)` avec la nouvelle source vidéo ;
3. ajouter les nouvelles pistes externes ;
4. laisser le mécanisme existant restaurer les préférences de pistes après le chargement des
   métadonnées ;
5. nettoyer une dernière fois les pistes lors de `player.dispose()`.

Cette séquence évite :

- les doublons dans le menu CC après un changement de lecteur ou d'URL de secours ;
- la conservation de sous-titres appartenant à l'ancien média ;
- les conflits entre pistes intégrées au manifeste et pistes externes ;
- les fuites de références DOM lors de la destruction du composable.

Le watcher actuel ne réapplique une source que lorsque `source.src` change. Si une même URL peut
être réutilisée avec une liste de sous-titres différente, il faudra soit élargir la condition de
comparaison à `source.subtitles`, soit garantir que l'identité de source change dans ce cas. La
comparaison explicite du contenu des pistes est la solution la plus robuste.

### Préférence utilisateur

Le renderer capture et restaure déjà l'état des pistes texte. Les pistes externes doivent être
ajoutées avant la restauration de cet état pour que Video.js puisse retrouver la préférence de
langue ou de libellé. En l'absence de préférence correspondante, toutes les pistes restent
désactivées par défaut.

## Compatibilité et sécurité

Le changement est rétrocompatible :

- un resolver sans `subtitles` produit le même JSON qu'avant ;
- un ancien frontend ignore le nouveau champ ;
- le nouveau frontend traite l'absence du champ comme une liste vide ;
- les flux avec sous-titres intégrés au manifeste continuent de fonctionner sans modification.

Les précautions suivantes restent nécessaires :

- ne jamais transférer les en-têtes Vidzy dans le JSON public ;
- ne jamais autoriser le frontend à choisir arbitrairement des en-têtes de proxy ;
- conserver les contrôles SSRF et la liste de confiance déjà appliqués par le proxy ;
- limiter `link` aux ressources HTTP(S) résolues par le backend ;
- ne pas interpréter le contenu VTT comme du HTML.

## Fichiers à modifier lors de l'implémentation

### Backend

- `server/crates/arachnea-stream/src/services/player_resolver.rs`
  - ajouter `ResolvedPlayerSubtitle` ;
  - ajouter `subtitles` à `ResolvedPlayerStream`.
- `server/crates/arachnea-stream/src/stream_resolver.rs`
  - importer le nouveau modèle ;
  - ajouter `extract_subtitles` ;
  - renseigner le champ lors de la conversion de la sortie YAML.
- `server/services/arachnea-stream-hoster/vidzy.yaml`
  - extraire génériquement les objets de `player.loadTracks` ;
  - résoudre et proxifier chaque lien WebVTT avec les en-têtes Vidzy.
- Les résolveurs Rust qui initialisent exhaustivement `ResolvedPlayerStream`
  - initialiser la nouvelle liste à vide si nécessaire.

### Frontend

- `front/public-app/src/types/entry.ts`
  - ajouter le type frontend unique `ResolvedPlayerSubtitle` et le champ `subtitles`.
- `front/public-app/src/services/rustify.ts`
  - normaliser la liste et filtrer les items invalides.
- `front/public-app/src/services/players.ts`
  - importer et réutiliser `ResolvedPlayerSubtitle` dans `ResolvedVideoMediaSource` ;
  - propager les pistes sans créer de second type.
- `front/public-app/src/composables/entry-details/entryVideoPlayer.ts`
  - transmettre les pistes pour la source principale, la bande-annonce et les URLs de secours.
- `front/public-app/src/components/LiveEntryDetails.vue`
  - transmettre les pistes pour le direct et ses URLs de secours.
- `front/public-app/src/components/home/HomeHeroBanner.vue`
  - transmettre les pistes pour les flux résolus utilisés par les bannières.
- `front/public-app/src/composables/home/useHomeCatalog.ts`
  - maintenir la signature cohérente lors de la résolution des vidéos de bannière.
- `front/public-app/src/composables/video/video-js-media-renderer/types.ts`
  - typer les méthodes et poignées de pistes distantes Video.js.
- `front/public-app/src/composables/video/useVideoJsMediaRenderer.ts`
  - ajouter, suivre et supprimer les pistes distantes à chaque changement de source.

### Documentation

- `docs/specifications/arachnea-stream-en.md`
- `docs/specifications/arachnea-stream-fr.md`
  - documenter le nouveau champ public et la forme YAML attendue.
- `CHANGELOG.md`
  - ajouter les entrées dans les modules `arachnea-stream` et `front-public`, sous les rubriques
    conformes à la structure du changelog.

## Plan d'implémentation

### Étape 1 — Étendre le contrat backend ✅

- [x] Créer `ResolvedPlayerSubtitle` avec `link` obligatoire et `lang`/`label` optionnels.
- [x] Ajouter `subtitles: Vec<ResolvedPlayerSubtitle>` à `ResolvedPlayerStream` avec valeur vide par
  défaut et omission Serde lorsque la liste est vide.
- [x] Adapter les initialiseurs Rust exhaustifs qui ne compilent plus (aucun initialiseur exhaustif : tous utilisent `..Default::default()`).
- [x] Ajouter un extracteur générique des items `subtitles` dans `stream_resolver.rs`.
- [x] Vérifier que la sérialisation publique omet bien le champ pour les resolvers existants (`cargo check -p arachnea-stream`, `cargo test -p arachnea-stream --lib` : 13 passed).

### Étape 2 — Extraire les pistes Vidzy ✅

- [x] Capturer les objets de `player.loadTracks` depuis la réponse HTML par
  `extract_regex_items`.
- [x] Produire les items `{ lang, label, link }` à partir de `srclang`, `label` et `src`, sans
  coder en dur les langues.
- [x] Écarter les pistes non textuelles grâce au filtre `kind = subtitles`, notamment les
  storyboards/sprites WebVTT.
- [x] Résoudre et proxifier chaque VTT avec `Referer`, `Origin` et `User-Agent` Vidzy.
- [x] Vérifier la regex sur la capture Vidzy (`eng`/English et `fre`/French uniquement), les
  réponses VTT capturées (`Content-Type: text/vtt`) et la conversion YAML→stream via
  `resolve_stream_keeps_proxied_subtitle_items_and_filters_storyboards`.
- [x] Exécuter `cargo test -p arachnea-stream --lib` : 14 passed, 1 ignored (test réseau).

### Étape 3 — Propager les données dans le frontend ✅

- [x] Ajouter l'interface TypeScript unique `ResolvedPlayerSubtitle` dans `types/entry.ts` et la
  réutiliser dans `ResolvedVideoMediaSource`.
- [x] Normaliser `subtitles` dans `normalizeResolvedPlayerStream`, avec `link` obligatoire et les
  valeurs `lang`/`label` vides omises.
- [x] Ajouter les pistes à `ResolvedVideoMediaSource` en résolvant les URLs relatives du proxy.
- [x] Faire évoluer `resolveBackendStreamMediaSource` vers un argument objet localement typé.
- [x] Mettre à jour tous les points d'appel, y compris les changements d'URL de secours.

### Étape 4 — Intégrer Video.js ✅

- [x] Étendre le type local du player pour `addRemoteTextTrack` et `removeRemoteTextTrack`.
- [x] Introduire une collection locale des pistes externes possédées par le renderer.
- [x] Ajouter une fonction de nettoyage idempotente.
- [x] Ajouter les pistes après chaque `player.src(...)`, avec le fallback `label` → `lang` → nom du
  fichier WebVTT.
- [x] Nettoyer avant chaque changement de source et lors de `dispose`.
- [x] Étendre la détection de changement lorsque seule la liste de sous-titres change.
- [x] Réappliquer les préférences de pistes après l'ajout des sous-titres distants et conserver la
  restauration existante après les métadonnées et les renouvellements DASH.

### Étape 5 — Documentation et validation

- [x] Mettre à jour les spécifications anglaise et française d'`arachnea-stream`.
- [x] Ajouter les entrées requises dans `CHANGELOG.md`.
- [x] Exécuter les vérifications Rust existantes du crate `arachnea-stream` le 24 septembre 2026
  (`cargo test -p arachnea-stream` : 14 réussites, 1 test de fournisseurs tiers ignoré).
- [x] Exécuter le typecheck et le build du frontend public le 24 septembre 2026.
- [ ] Effectuer une validation manuelle Vidzy de bout en bout : aucun embed Vidzy de test valide ni
  session navigateur de l'application n'était disponible dans le dépôt le 24 septembre 2026.

## Scénarios de validation

### Backend

- Une page Vidzy avec deux pistes retourne deux objets dans l'ordre source.
- `eng`/English et `fre`/Français conservent leur code et leur libellé.
- Les codes de langue `eng` et `fre` ne sont pas convertis en `en` et `fr`.
- Une piste avec seulement `lang` ou seulement `label` est conservée.
- Une piste sans `lang` ni `label` est conservée lorsque son `link` est valide.
- Chaque `link` est une URL proxy et non l'URL CDN brute nécessitant des en-têtes.
- Une piste sans `link` est ignorée sans rendre le flux principal inutilisable.
- Un resolver sans piste ne sérialise pas `subtitles`.
- Les autres résolveurs continuent de produire leurs réponses actuelles.

### Frontend

- Le menu CC de Video.js apparaît lorsque la liste contient au moins une piste valide.
- Les pistes anglaise et française chargent effectivement leur WebVTT.
- Une piste avec seulement `lang` affiche ce code comme libellé et le transmet comme `srclang`.
- Une piste avec seulement `label` affiche ce libellé sans attribut `srclang` inventé.
- Une piste sans `lang` ni `label` utilise le nom décodé de son fichier, sans l'extension `.vtt`,
  comme libellé.
- Une piste dont le lien ne contient aucun nom de fichier exploitable est ignorée par le renderer.
- Aucune piste n'est activée arbitrairement au premier chargement.
- La préférence de piste est restaurée lorsqu'une piste correspondante existe.
- Le changement d'URL de secours conserve la liste de pistes sans créer de doublons.
- Le changement de média supprime les pistes de l'ancien média.
- Une erreur de chargement d'un VTT n'empêche pas la lecture vidéo.
- Les flux sans sous-titres externes et les pistes intégrées HLS/DASH continuent de fonctionner.

## Risques et points ouverts

1. **Variations du JavaScript Vidzy** : la forme exacte des objets `loadTracks` peut évoluer. La
   regex doit être testée sur plusieurs embeds et limitée au bloc concerné.
2. **Échappement JavaScript** : les URLs ou labels peuvent contenir des séquences échappées. Si
   les actions regex ne suffisent pas, une primitive générique de décodage de chaîne JavaScript
   devra être évaluée plutôt qu'un traitement Vidzy codé en Rust.
3. **CORS et hotlinking** : une URL CDN brute peut sembler valide lors d'un test backend mais
   échouer dans Video.js. Le test doit impérativement passer par le navigateur et le proxy final.
4. **Content-Type WebVTT** : certains CDN renvoient un type générique. Il faut vérifier si le proxy
   conserve ou corrige un type compatible avec le navigateur utilisé.
5. **Pistes DASH natives** : l'ajout de pistes externes ne doit pas modifier la configuration
   actuelle `nativeCaptions: false` ni perturber les pistes gérées par dash.js.
6. **Signature de `resolveBackendStreamMediaSource`** : ajouter un paramètre positionnel est le
   changement minimal, mais un argument objet réduit le risque d'inversion entre storyboard,
   chapitres et sous-titres. Cette petite refactorisation doit rester limitée à la fonction et à
   ses points d'appel.

## Décision recommandée

Implémenter un champ générique public `subtitles`, sérialisé comme une liste d'objets
`{ lang?, label?, link }`, où seul `link` est obligatoire, et faire de `subtitles[].link` une URL
déjà consommable par le navigateur. Le frontend doit réutiliser un type unique
`ResolvedPlayerSubtitle` de la réponse normalisée jusqu'au renderer et dériver le libellé depuis le
nom du fichier lorsque `label` et `lang` sont absents.
Vidzy doit produire cette structure dans son YAML et proxifier chaque VTT avec les en-têtes requis.
Le frontend doit propager la liste jusqu'à `ResolvedVideoMediaSource`, puis Video.js doit gérer les
pistes avec `addRemoteTextTrack` et un nettoyage explicite à chaque changement de source.

Cette solution conserve les responsabilités existantes : extraction spécifique dans le YAML,
contrat et validation génériques dans le backend, normalisation dans les services frontend et
cycle de vie des pistes dans le renderer Video.js.