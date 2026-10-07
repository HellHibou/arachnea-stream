# Analyse : vidéos recommandées dans `get_entry`

> Créée le 2026-10-05.  
> Statut : étapes 1-5 (partielle) + 11 sources migrées (dont tf1-fr le 2026-10-07) — contrat public documenté (FR/EN), façade backend générique, modèle et affichage frontend ; 11/14 sources candidates migrées ; quatre sources sont confirmées sans recommandations.
> Périmètre : sources de catalogue de `server/services/arachnea-stream/` qui déclarent `get_entry`.

## Objectif

Ajouter à la réponse `get_entry` une liste de vidéos recommandées par l'entrée consultée, puis l'afficher dans la page de détail publique. La donnée doit être extraite de manière déclarative dans les YAML de chaque source lorsqu'elle est exposée par son site ou son API.

Une recommandation reste éditoriale et locale à la source courante : ce travail n'introduit ni recherche globale Arachnea ni algorithme de recommandation Rust.

## État actuel

- `StreamScraper::get_entry` exécute la requête YAML de la source sélectionnée et renvoie directement ses champs. Aucun DTO Rust spécifique ne limite l'ajout d'un champ YAML.
- Le frontend appelle `get_entry` dans `front/public-app/src/services/rustify.ts::getEntryDetails`, puis normalise la réponse vers `EntryDetails`.
- `EntryDetails` et `ProgramEntryDetails.vue` ne portent ni n'affichent actuellement de recommandations ; la page affiche les saisons et épisodes.
- `arachnea-stream-hoster` ne participe qu'à la résolution de flux `get_stream`. Ses YAML ne sont pas concernés.
- `docs/TODO.md` contient déjà « Ajouter un système de vidéos associées ». Ce document est le suivi opérationnel de cet item.

## Contrat cible proposé

Le champ public proposé est `recommendations`. Il est optionnel : une source qui ne fournit aucune relation peut l'omettre sans faire échouer `get_entry`.

`recommendations` est un **objet section**, avec le même format JSON de résultat que `get_section`, et non un tableau direct de cartes. Son `label` est facultatif et ne doit pas être imposé : l'interface peut afficher son libellé localisé de recommandations.

Une source fournit soit les données déjà présentes dans la réponse de détail, soit un `link` de recommandations. Dans ce second cas, le frontend appelle le nouveau endpoint `get_recommendations` lorsque le composant de recommandations est affiché. Le backend exécute alors la requête YAML `get_recommendations` de la source avec ce `link` ; `get_section` ne doit pas être réutilisé, car son contrat représente les rails génériques du catalogue.

### Données disponibles directement

```json
{
  "title": "Programme courant",
  "recommendations": {
    "entries": [
      {
        "title": "Programme recommandé",
        "link": "https://source.example/programme-recommande",
        "web-link": "https://source.example/programme-recommande",
        "image": "https://cdn.example/programme-recommande.jpg",
        "description": "Résumé facultatif",
        "media-type": "video/show/serie"
      }
    ],
    "current_page": 1,
    "have_more": false
  }
}
```

### Données différées par `get_recommendations`

```json
{
  "title": "Programme courant",
  "recommendations": {
    "link": "https://source.example/programme-courant/recommendations"
  }
}
```

La réponse de `get_recommendations` reprend exactement le format JSON de données de `get_section` : `entries` et, si nécessaire, `current_page` et `have_more`. Les objets de `recommendations.entries` sont des cartes de catalogue, compatibles avec les champs déjà produits par `search`, `load_home`, `get_category` et `get_section`.

| Champ de `recommendations` | Requis | Règle |
|---|---:|---|
| `entries` | oui, si les données sont immédiates | Tableau d'objets `MediaItem`. |
| `link` | oui, si les données sont différées ou paginées | URL transmise à `get_recommendations` avec le `source` courant et la page demandée. |
| `current_page` | non | Page déjà renvoyée ; vaut normalement `1` pour une réponse immédiate paginée. |
| `have_more` | non | Indique la disponibilité d'une page suivante via `get_recommendations`. |
| `label` | non | Libellé de section facultatif, jamais requis pour afficher les recommandations. |

| Champ d'un élément de `entries` | Requis | Règle |
|---|---:|---|
| `title` | oui | Libellé visible de la vidéo recommandée. |
| `link` | oui | Identifiant ou URL exploitable par le prochain `get_entry`. |
| `web-link` | non | URL publique, à émettre lorsqu'elle diffère de `link`. |
| `image` ou `image > portrait` / `image > landscape` | non | Visuel de carte ; conserver les conventions de la source. |
| `description`, dates, durée, genres, `media-type` | non | Métadonnées disponibles dans la réponse source. |

Le frontend doit normaliser cet objet dans un modèle de section paginable, en réemployant la normalisation des `MediaItem`. `EntryDetails` recevra donc une recommandation de type section — cartes immédiates et/ou descripteur `link`, `currentPage`, `haveMore` — et `ProgramEntryDetails.vue` chargera le `link` avec `get_recommendations` lorsque le composant devient visible. La liste doit être rendue par `MediaCardCollection` en mode `single-row` : un seul rail horizontal défilable, jamais une grille ni une liste verticale.

## Règles de migration YAML

1. Analyser une réponse réelle représentative avant toute modification : HTML serveur, JSON embarqué ou endpoint API.
2. Préférer le payload de `get_entry` déjà chargé : déclarer alors `recommendations > entries`. Si la source exige une URL supplémentaire, déclarer seulement `recommendations > link` et créer la requête YAML `get_recommendations` correspondante ; le frontend l'appellera lors de l'affichage du composant.
3. Ne pas charger des recommandations externes au moyen d'une `sub_query` de `get_entry` et ne pas détourner `get_section` : le chargement supplémentaire passe par `get_recommendations`.
4. Déclarer `recommendations` en objet de section, jamais en `object[]` direct ; y ajouter `current_page` et `have_more` lorsque la source fournit une pagination. Réemployer les ancres/actions de cartes existantes pour `recommendations > entries` lorsque leur structure est compatible.
5. Écarter l'entrée courante, navigation, publicité et contenus sans `link` exploitable.
6. Ne pas générer de recommandations depuis la recherche ou une catégorie : la relation doit être explicitement fournie par la source.
7. Après migration, vérifier une réponse réelle, l'appel différé éventuel, l'ouverture d'une carte recommandée et l'absence de régression des saisons, épisodes et joueurs.

## Inventaire

`server/services/arachnea-stream/services.json` importe deux collections. Les 18 fichiers suivants déclarent `get_entry` : 5 sources DarkStream et 13 sources légales. Parmi elles, `anime-sama`, `animeultime`, `frenchanimes` et `antennereunion-fr` sont confirmées sans recommandations ; elles restent inventoriées car elles exposent `get_entry`, mais ne doivent pas recevoir le champ `recommendations`. Sur les 14 sources candidates, 10 sont migrées (dont `tv5mondeplus-fr`) et 4 restent à analyser. Les statuts d'activation restent à consulter dans les `services.json` de collection avant une validation réseau.

| Groupe | Sources `get_entry` | Total | Sans recommandations confirmées | À analyser pour migration |
|---|---|---:|---:|---:|
| `dark-stream` | anime-sama, animeultime, coflix, frenchanimes, papadustream-v2 | 5 | 3 | 0 |
| `legal-stream` | antennereunion-fr, arte-fr, bouke-be, canalzoom-be, francetv, ln24-be, rtbf-auvio-be, rtlplay-be, telemb-be, tf1-fr, tv5mondeplus-fr, tvmonaco-fr, m6play-fr | 13 | 1 | 4 |
| **Total** | Sources de catalogue avec `get_entry` | **18** | **4** | **4** |

## Tableaux de migration par service

`À analyser` signifie qu'aucune conclusion n'a encore été tirée d'une réponse live. La piste est un point de départ d'audit, non une garantie que la donnée existe. `Non disponible` est un constat confirmé : ne pas ajouter `recommendations` dans le YAML correspondant.

### DarkStream

| Service | YAML / format `get_entry` | Piste à analyser | Migration et validation | Statut |
|---|---|---|---|---|
| `anime-sama` | `dark-stream/anime-sama.yaml` — HTML | Aucune recommandation fournie par le service. | Ne pas ajouter `recommendations` ni `get_recommendations` ; conserver l'absence du champ dans `get_entry`. | Non disponible |
| `animeultime` | `dark-stream/animeultime.yaml` — HTML | Aucune recommandation fournie par le service. | Ne pas ajouter `recommendations` ni `get_recommendations` ; conserver l'absence du champ dans `get_entry`. | Non disponible |
| `coflix` | `dark-stream/coflix.yaml` — HTML | Bloc éditorial `section#related` (« Films liés ») dans la réponse de détail. | `recommendations > entries` immédiates via `entries_related_card` (`div.ani.items > div.item` : titre `a.name[data-jp]`, lien canonique `a.name` — le poster pointe vers une variante `/ep-<id>` de la même fiche —, poster `a.ani.poster > img`, `lang` VF/VOSTFR/French/TrueFrench) ; pas de `link`/`get_recommendations` (données déjà dans le détail, pas de pagination). Contrôle live du 2026-10-06 : `/film/spider-man-brand-new-day-vf` (7 cartes aux liens canoniques ouvrables, dont la variante VOSTFR du même film). Service désactivé dans `services.json` ; affichage frontend non vérifié. | Migré |
| `frenchanimes` | `dark-stream/frenchanimes.yaml` — HTML | Aucune recommandation fournie par le service. | Ne pas ajouter `recommendations` ni `get_recommendations` ; conserver l'absence du champ dans `get_entry`. | Non disponible |
| `papadustream-v2` | `dark-stream/papadustream-v2.yaml` — HTML avec pré-traitements. | Bloc éditorial « Voir Aussi: » (`div.full_content-inner--title` suivi d'`article.grid_short`) dans la réponse de détail. | `recommendations > entries` immédiates via `entries_related_card` (même forme que `catalog_card` sauf le titre en `div.short_title_related` : lien `a.short_img`, poster `data-src`, année, `lang` VF/VOSTFR) ; pas de `link`/`get_recommendations` (données déjà dans le détail, pas de pagination). Pré-traitements anti-Cloudflare préservés ; sélecteur limité au bloc « Voir Aussi: », sans mélange avec les onglets de saisons ni les épisodes. Contrôle live du 2026-10-06 : fiche Antigang (`/categorie-series/series-vf/22382-antigang.html`, 5 cartes aux liens ouvrables). Service désactivé dans `services.json` ; affichage frontend non vérifié. | Migré |

### LegalStream

| Service | YAML / format `get_entry` | Piste à analyser | Migration et validation | Statut |
|---|---|---|---|---|
| `antennereunion-fr` | `legal-stream/antennereunion-fr.yaml` — JSON POST, `/result/items/0`. | Aucune recommandation fournie par le service (analyse confirmée). | Ne pas ajouter `recommendations` ni `get_recommendations` ; ne pas assimiler catégories et saisons à des suggestions. | Non disponible |
| `arte-fr` | `legal-stream/arte-fr.yaml` — JSON EMAC v3, racine. | Zone éditoriale `program_recommendations` (« Vous pourriez aimer aussi »), distincte de `program_playNext` et des zones `collection_videos`/`collection_subcollection`. | `recommendations > entries` depuis `/zones/*[code/name=program_recommendations]/data/*` avec `card_entries` (liens `programId` vers EMAC v3, URL publique et images paysage) ; aucune requête différée. Contrôle live du 2026-10-06 : fiche `125633-004-A` (9 cartes, ouverture de la première vérifiée). La collection `RC-022041` (« Red Light ») n'a pas de zone `program_recommendations` : sa zone `collection_associated` (« Sur le même thème ») est vide et ses dix vidéos sont des épisodes sous `collection_subcollection`. L'absence de rail sur cette fiche est donc attendue ; ne pas transformer ses épisodes en recommandations. | Migré |
| `bouke-be` | `legal-stream/bouke-be.yaml` — HTML. | Cartes Drupal liées, émission parente, suggestions de page vidéo. | `recommendations > entries` : pages info via `entries_recommendation_card` (`div.editor-suggestion--main-container div.views-row div.view-mode-editor_suggestion:has(div.icon-camera-fill)` pour exclure les articles sans vidéo), pages `/emission/<slug>` via `entries_program_card` (`div.composant-emission div.view-mode-portrait`, bloc « Nos émissions / A ne pas manquer ») ; pas de `link`/`get_recommendations` (données déjà dans le détail, pas de pagination). Validé le 2026-10-06 : page info + page `/emission/laccent` + carte recommandée OK. | Migré |
| `canalzoom-be` | `legal-stream/canalzoom-be.yaml` — HTML. | Suggestions Drupal sur les fiches vidéo et bloc « Nos émissions / A ne pas manquer » sur les fiches de programme ; rail d'accueil distinct. | `recommendations > entries` immédiates : `div.editor-suggestion--main-container div.views-row div.view-mode-editor_suggestion:has(div.icon-camera-fill)` via `entries_suggestion_card` pour exclure les articles sans vidéo, `div.composant-emission div.view-mode-portrait` via `entries_program_card` pour les programmes ; `div.view-episode` reste réservé aux saisons. Contrôle live du 2026-10-06 : `/emissions/c-decouvrir/21812` et `/emission/le-jt` (titres, liens et images mappés). Service désactivé dans `services.json` ; affichage frontend non vérifié. | Migré |
| `francetv` | `legal-stream/francetv.yaml` — JSON Yatta, racine. | Sur les fiches de programme, la collection `playlist_to_discover` (« À découvrir aussi ») est distincte des saisons `playlist_video` et peut mêler programmes et collections. | `recommendations > entries` immédiates depuis `/collections/*[type=playlist_to_discover]` : cartes `program` via `program_card_entries`, cartes `collection` via `recommendation_collection_entries` (URL catalogue et URL publique `/collection/<slug>/`, images portrait/paysage). Contrôle live du 2026-10-06 : fiche `france-2_soeurs` (20 cartes, dont « Thrillers » et la fiche « Sœurs » également présentes sur le site) et `france-2_code-rouge` ; les épisodes restent dans `seasons`. Une fiche vidéo `/standard/publish/contents/8848887` ne comporte pas cette collection et n'émet pas de recommandations. | Migré |
| `ln24-be` | `legal-stream/ln24-be.yaml` — HTML avec proxy BE. | Bloc « Dans la même catégorie » (`section.emission-related`), distinct des épisodes `section.emission-videos` ; les cartes contiennent `data-video-id`, `data-show-id` et `data-player-url`. | `recommendations > entries` immédiates depuis les cartes liées portant `data-show-id` et `data-video-id` ; titre, description, image paysage et lien vers la fiche d'émission `?e=<show-id>`, **sans `players` sur les cartes**. Les cartes `blocked=1` restent des recommandations navigables vers leurs émissions, sans garantie de lecture de la vidéo. Aucun endpoint différé ni pagination du bloc. Contrôle live du 2026-10-06 : `/categories/series/navarro/` a 4 cartes liées, toutes avec `blocked=1` ; deux fiches d'émission ont chacune 4 cartes liées dont 1 avec un flux. La fiche d'émission reste le lien de navigation ; la vidéo précise n'est accessible directement que par le lecteur embarqué de la fiche ou des épisodes lorsqu'il est disponible. Contrôle du rendu frontend à effectuer. | Migré |
| `m6play-fr` | `legal-stream/m6play-fr.yaml` — JSON `programs/{id}?with=links,subcats,rights`. | Relations du programme, sous-catégories ou endpoint éditorial. | Étendre `with=` ou ajouter une sous-requête uniquement après audit du schéma ; contrôler les droits. | À analyser |
| `rtbf-auvio-be` | `legal-stream/rtbf-auvio-be.yaml` — JSON, racine. | Widget contextuel « A découvrir aussi » (observé : `PROGRAM_LIST`, widget `21342`, `context[programId]`), distinct de `TAB_LIST` et de `next`/`previous`. | `recommendations > link` depuis le `contentPath` du widget présent sur la fiche ; `get_recommendations` extrait les cartes `PROGRAM` avec titre, type, lien API `pages`, lien public Auvio et image `/illustration/m` dans `img/poster` et `img/portrait`. Pagination depuis `/meta/page/current` et `/links/next`. Validation Rust : Presque (10 cartes complètes, page 1, aucune suite), carte En tongs au pied de l'Himalaya ouvrable ; Vital sans recommandations, épisodes conservés. Tests existants `test_query_get_entry` réussis sur ces trois fiches. Pas de récupération des recommandations générales de l'accueil ni de navigation précédent/suivant. Service laissé désactivé ; rendu frontend non vérifié. | Migré (validation partielle) |
| `rtlplay-be` | `legal-stream/rtlplay-be.yaml` — JSON avec en-têtes RTL Play. | `nextBestOffer.teasers` dans le JSON `detail3` (série et film) : jusqu'à six programmes liés avec `title`, `detailId`, `imageUrl`. | `recommendations > entries` immédiates via `teaser_entries_landscape` (`/nextBestOffer/teasers/*`) : l'image `imageUrl` (426 × 240 contrôlée le 2026-10-06) est déclarée en `img/landscape`, pas en `img/poster`, pour préserver l'orientation du rail ; `detailId` pointe vers `detail3` et le site public, sans charger `seasonPicker` ni requête différée. Contrôle live du 2026-10-06 : mapping des titres, liens et images sur une série, un film et une carte recommandée, sans confusion avec les saisons ; une réponse de film a omis le bloc lors d'un premier appel, puis l'a fourni au suivant. Le service reste désactivé dans `services.json` ; navigation visuelle en front non vérifiée. | Migré |
| `telemb-be` | `legal-stream/telemb-be.yaml` — HTML. | Suggestions Drupal sur les fiches vidéo et bloc « Nos émissions / A ne pas manquer » sur les fiches de programme. | `recommendations > entries` immédiates : `div.editor-suggestion--main-container div.views-row div.view-mode-editor_suggestion:has(div.icon-camera-fill)` via `entries_recommendation_card` (titre, lien, description, date et image paysage) pour exclure les articles sans vidéo, `div.composant-emission div.view-mode-portrait` via `entries_program_card` pour les programmes ; épisodes exclus. Contrôle live du 2026-10-06 : `/emission/la-pucelette-de-wasmes/la-pucelette-de-wasmes-2024/35137` et `/emission/la-pucelette-de-wasmes` (champs de cartes mappés). Service désactivé dans `services.json` ; affichage frontend non vérifié. | Migré |
| `tf1-fr` | `legal-stream/tf1-fr.yaml` — JSON, `/data/programById`. | Rail de programmes « Si vous aimez… » déclaré dans `editorialSections/*/sliders` ; cartes présentes dans la fiche HTML, absentes du détail Smart TV. | `recommendations > link` différé lorsque le libellé commence par « Si vous aimez » ; `get_recommendations` HTML cible uniquement `SliderOfProgram` avec `data-tracking-poc-id` commençant par `si-vous-aimez-`, puis les cartes `Program`. Identifiant de programme et lien public produisent des liens Smart TV ouvrables, avec titres et images portrait. Correction des imagettes : image fournie dans `img/poster` et `img/portrait`, car le composant public ne lit pas directement `img/portrait`. Périmètre confirmé par l'utilisateur : exclure acteurs, découvertes éditoriales, popularité et épisodes. Pas de pagination (`current_page=1`, `have_more=false`). Validation Rust live du 2026-10-07 : HPI (20 cartes complètes), Koh-Lanta (aucune carte, rail similarity vide ; découverte éditoriale exclue), test existant `test_query_get_entry` HPI et carte recommandée Revers de fortune OK. La requête d'épisodes retourne `listById: null` pour un rail de programmes et n'est pas réutilisée. Service désactivé ; rendu frontend non vérifié. | Migré (validation partielle) |
| `tv5mondeplus-fr` | `legal-stream/tv5mondeplus-fr.yaml` — JSON, `/items/*`. | Rail HTML « Vous pourriez aimer », absent de l’API asset. | `recommendations > link` vers la fiche publique ; `get_recommendations` extrait les cartes landscape hors rails saisons, avec liens API asset issus des images, titres, descriptions et URLs publiques. Pas de pagination (`current_page=1`, `have_more=false`). Le lien mosaic ne représente pas les mêmes contenus côté API. Analyse corrigée le 2026-10-07 : HTML fourni de Rassemblance (10 cartes, 4 saisons exclues). Test existant `test_query_get_entry` live OK. Correction réseau du 2026-10-07 : `get_recommendations` remplace localement le User-Agent Chrome par `curl/8.7.1` ; appel backend live sur Mukbang : 10 cartes, sans erreur (contre `entries: null` avec Chrome). Extraction également reproduite par le backend sur une copie HTML locale. Rassemblance reste bloquée par une boucle de redirections entre les chemins `/fr/documentaires/societe/rassemblance` et `/fr/info-et-societe/enquetes-et-reportages/rassemblance`. Rendu frontend non vérifié. Service désactivé. | Migré (validation partielle) |
| `tvmonaco-fr` | `legal-stream/tvmonaco-fr.yaml` — JSON, racine. | Liste de contenus associés API ou section de fiche. | Mapper au format de carte commun ; confirmer URLs et images. |Non disponible |

## Plan d'implémentation

Les étapes 1 à 5 établissent le contrat générique avant toute migration de source. Les étapes 6 et 7 sont répétées pour chacune des 14 sources candidates. Les quatre sources confirmées sans recommandations (`anime-sama`, `animeultime`, `frenchanimes`, `antennereunion-fr`) sont explicitement exclues des étapes YAML de migration.

### 1. Définir et documenter le contrat public ✅ (fait le 2026-10-06 : `get_recommendations` en requête standard + section 5.7.2 FR/EN avec entrée `source`/`link`/`page`, sortie `entries`/`current_page`/`have_more`, et `get_entry.recommendations` optionnel)

1. Ajouter `get_recommendations` à la liste des requêtes standard de la spécification stream française et anglaise.
2. Documenter son entrée : `source`, `link`, `page` (page un basée, défaut `1`).
3. Documenter sa sortie : le même objet de résultat que `get_section` (`entries`, `current_page`, `have_more`), sans enveloppe de section additionnelle.
4. Documenter `get_entry.recommendations` comme objet optionnel avec `entries` immédiates ou `link` différé ; conserver `label` facultatif.

### 2. Ajouter la façade backend générique ✅ (fait le 2026-10-06 : `GetRecommendationsRequest` + `StreamScraper::get_recommendations` sur le modèle de `get_section` avec requête YAML `get_recommendations`, page min 1, filtre source, cache/ETag catalogue ; commande enregistrée dans `reloadable_stream_scraper.rs` ; `cargo check -p arachnea-stream` OK)

1. Déclarer `GetRecommendationsRequest` dans `server/crates/arachnea-stream/src/stream_scraper.rs`, avec `source`, `link` et `page`.
2. Ajouter `StreamScraper::get_recommendations` sur le modèle de `get_section`, mais en exécutant exclusivement la requête de service `get_recommendations`.
3. Transmettre `link` comme `query_url` et `link`, normaliser `page` à une valeur minimale de `1`, filtrer l'exécution sur la source demandée et conserver le cache/ETag cohérent avec les requêtes de catalogue.
4. Enregistrer la nouvelle commande dans `server/crates/arachnea-stream/src/reloadable_stream_scraper.rs`.
5. Ne pas ajouter de branche Rust par fournisseur : l'extraction, le formatage et la pagination restent déclarés dans les YAML.

### 3. Étendre le modèle de données frontend ✅ (fait le 2026-10-06 : `EntryRecommendations` dans `types/entry.ts` avec `items`/`link`/`currentPage`/`haveMore` + `EntryDetails.recommendations` nullable ; `rustify.ts` normalise `get_entry.recommendations.entries` via `normalizeMediaItem`, expose `getRecommendations(source, link, page)`, tolère l'absence/invalide en `null` ; `vue-tsc --noEmit` OK)

1. Créer un type partagé de recommandations dans `front/public-app/src/types/entry.ts` : cartes `MediaItem[]`, `link`, `currentPage` et `haveMore`.
2. Ajouter ce type à `EntryDetails` sous le champ optionnel ou nullable `recommendations`.
3. Dans `front/public-app/src/services/rustify.ts`, normaliser `get_entry.recommendations.entries` avec le normaliseur existant de `MediaItem`.
4. Ajouter une fonction frontend `getRecommendations(source, link, page)` qui appelle `get_recommendations` et normalise la réponse au même modèle.
5. Tolérer une réponse absente, vide ou invalide en masquant la section sans faire échouer le chargement de l'entrée.

### 4. Implémenter le chargement et l'affichage frontend ✅ (fait le 2026-10-06 : composable `entryRecommendations` — items conservés de `get_entry`, `get_recommendations` appelé une fois à l'affichage du rail via IntersectionObserver, pages suivantes via `haveMore` sans doublon, réponses obsolètes ignorées ; rail dédié `MediaCardCollection mode="single-row"` landscape dans `ProgramEntryDetails.vue` distinct de `EntryDetailsCatalogSection` ; libellés localisés `entry.recommendations` + `entry.loadingRecommendationsMessage` FR/EN ; `vue-tsc --noEmit` OK)

1. Étendre `entryDetailsData` — ou créer un composable local dédié, sans modifier l'architecture globale — avec l'état des recommandations : éléments, erreur, chargement initial, chargement supplémentaire, page et disponibilité d'une suite.
2. Conserver les entrées déjà reçues dans `get_entry`. Lorsque seul `recommendations.link` est présent, appeler `get_recommendations` à l'affichage du composant de recommandations, une seule fois par fiche et en ignorant les réponses devenues obsolètes lors d'un changement d'entrée.
3. Lorsque `haveMore` est vrai, appeler `get_recommendations` avec la page suivante et ajouter les nouvelles cartes sans doublon fonctionnel.
4. Ajouter un rail dédié dans `ProgramEntryDetails.vue`, distinct de `EntryDetailsCatalogSection` utilisé pour les saisons et épisodes : ce dernier force actuellement un rendu d'épisodes en `list`.
5. Rendre ce rail avec `MediaCardCollection` et `mode="single-row"`, une orientation de vignette adaptée aux cartes recommandées et le bouton de chargement supplémentaire intégré au rail horizontal.
6. Ajouter ou réemployer un libellé localisé de recommandations sans exiger `recommendations.label` de la source.

### 5. Vérifier le contrat transversal avant les sources ⚠️ (partiel le 2026-10-06 : `cargo check -p arachnea-stream` OK, `vue-tsc --noEmit` OK, revue statique OK — `get_recommendations` isolée de `get_section`, normalisation `null` si absent/invalide, rail `v-if="showRecommendations"` masqué si rien, appel différé unique via observer ; `cargo test -p arachnea-stream` : 12 passed, 2 failed préexistants `test_query_get_entry`/`test_service` sur `anime-sama` « Missing link in load_home section », reproduits sans mes changements via stash — échecs live/réseau non liés ; NON RÉALISÉ : vérification manuelle live `get_entry` avec `recommendations.entries`/`link`, appel différé réel, pagination, navigation carte→fiche, états chargement/erreur — aucune source ne déclare encore `recommendations`, reporté à chaque étape 7)

1. Vérifier manuellement une réponse `get_entry` avec `recommendations.entries` et une autre avec `recommendations.link`.
2. Vérifier que l'appel différé n'est déclenché qu'à l'affichage du rail et vise `get_recommendations`, jamais `get_section`.
3. Vérifier le chargement d'une page suivante, la navigation d'une carte vers sa fiche, les états de chargement/erreur et l'absence de rail lorsque le champ est absent.
4. Exécuter les vérifications existantes backend et frontend pertinentes ; ne pas créer de nouvelle infrastructure ou de nouveaux tests sans demande explicite.

### 6. Migrer une source candidate

Pour chaque source marquée `À analyser` :

1. Capturer une réponse de détail représentative et identifier une relation explicite de contenus recommandés.
2. Si les cartes sont déjà dans la réponse de détail, ajouter `recommendations > entries` au `get_entry` YAML, avec les actions de carte existantes de la source.
3. Si une URL additionnelle est requise, ajouter `recommendations > link` au `get_entry` et une requête YAML `get_recommendations` qui produit `entries`, `current_page` et `have_more`.
4. Ne pas utiliser une `sub_query` de `get_entry`, ne pas appeler `get_section` et ne pas dériver les recommandations depuis une recherche ou une catégorie.
5. Mettre à jour la ligne de suivi de la source : chemin retenu (`entries` ou `link`), sélecteur/endpoint, pagination observée, date de validation et statut.

### 7. Valider chaque migration de source

1. Contrôler le JSON public : aucune donnée de session, aucun en-tête sensible et uniquement des cartes ouvrables.
2. Contrôler le rail `single-row` sur desktop et mobile, y compris son défilement horizontal et le chargement supplémentaire éventuel.
3. Ouvrir au moins une carte recommandée et vérifier le nouvel appel `get_entry`.
4. Vérifier l'absence de régression sur les métadonnées, joueurs, saisons et épisodes de la fiche initiale.
5. Mettre à jour les spécifications, le changelog et ce tableau de suivi dans la même modification lorsqu'une source est effectivement migrée.

## Impacts de code après audit

| Zone | Modification attendue |
|---|---|
| YAML | Ajouter l'objet section `recommendations` dans les `get_entry` dont la source expose des relations exploitables. Lorsqu'un appel supplémentaire est nécessaire, ajouter une requête YAML `get_recommendations` et publier seulement `recommendations > link`. |
| `server/crates/arachnea-stream/src/stream_scraper.rs` et contrôleur | Ajouter la requête et le contrat `get_recommendations` (`source`, `link`, `page`) ; elle exécute uniquement la requête YAML de même nom et renvoie le format JSON de `get_section`. |
| `front/public-app/src/types/entry.ts` | Ajouter un modèle de section de recommandations à `EntryDetails`, incluant les cartes, le lien et la pagination. |
| `front/public-app/src/services/rustify.ts` | Normaliser l'objet `recommendations` et exposer l'appel `get_recommendations` pour le chargement différé et les pages suivantes. |
| `front/public-app/src/components/ProgramEntryDetails.vue` | Charger le lien lors de l'affichage du composant et afficher les recommandations avec `MediaCardCollection` en mode `single-row`, avec navigation et chargement de page suivants lorsque disponibles. |
| Locales frontend | Ajouter le libellé de section si aucun libellé existant ne convient. |
| Spécification | Documenter `get_entry.recommendations` dans `docs/specifications/arachnea-stream-fr.md` et son équivalent anglais. |
| Changelog | Ajouter les changements fonctionnels au moment de l'implémentation effective. |

## Critères de fin

- Le backend transmet `recommendations` sans logique spécifique à une source en Rust ; `get_recommendations` reste une façade générique qui exécute la requête YAML homonyme.
- Le frontend tolère un champ absent ou mal formé sans casser le détail.
- Chaque source migrée produit une section issue d'une relation explicite : `entries` immédiates ou `link` résolu par `get_recommendations`.
- Les recommandations s'affichent dans un unique rail horizontal `single-row`, et une sélection charge une nouvelle fiche ; les pages suivantes sont chargées à travers `get_recommendations` lorsque `have_more` le permet.
- Les vérifications existantes pertinentes sont exécutées après chaque modification ; aucun nouveau test n'est créé sans demande explicite.
