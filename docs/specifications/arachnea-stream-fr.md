# Spécification du format YAML arachnea-stream

## 1. Aperçu

Une collection arachnea-stream déclare une source de streaming (plateforme
légale ou catalogue alternatif). Chaque source définit des requêtes nommées qui
produisent des données catalogues structurées — pages d'accueil, catégories,
recherche, détails d'entrée, épisodes, directs.

Ces requêtes produisent les données de catalogue et de lecteurs exposées par le
backend. Le YAML doit mettre en forme la réponse selon les structures décrites
ci-dessous.

Ce document ne décrit que les champs et conventions propres au format stream.
La structure générale d'un fichier YAML (`id`, `constants`, `parameters`, `http`, `queries`,
`entries`, `actions`, `shared`, etc.) est documentée dans
[la spécification YAML commune](arachnea-scrapyfy-fr.md).

---

## 2. Structure de la collection

Les fichiers YAML sont organisés en sous-dossiers thématiques :

```
services/arachnea-stream/
├── services.json                 # Inscription et activation des sources
├── legal-stream/                 # Plateformes légales
│   └── example-source.yaml
└── dark-stream/                  # Catalogues alternatifs
    └── example-catalogue.yaml
```

### 2.1 Constantes, paramètres et configuration HTTP

Chaque source définit ses propres paramètres (`base_url`, clés d'API,
identifiants de chaîne, limites de pagination). Utiliser le mode HTTP `direct`
pour les requêtes directes ou `auto` pour la gestion automatique des challenges.

```yaml
http:
  mode: direct
  user_agent_profile: firefox
```

---

## 3. Requêtes standard


Chaque source peut implémenter tout ou partie des requêtes suivantes :

| Nom de requête | Type | Description |
|---|---|---|
| `service_stream_metadata` | `static` | Métadonnées du service (nom, logo, thèmes) |
| `load_home` | `json` / `html` | Page d'accueil (catégories, sections, bannières) |
| `get_category` | `json` / `html` | Contenu filtré par chaîne/catégorie |
| `get_section` | `json` / `html` | Section paginée (liste d'entrées) |
| `get_banners` | `json` / `html` | Données de bannières promotionnelles chargées depuis un lien de l'accueil |
| `search` | `json` / `html` | Recherche avec filtres |
| `get_entry` | `json` / `html` | Détail d'une entrée (programme) |
| `get_recommendations` | `json` / `html` | Recommandations différées d'une entrée (voir 5.7.2) |
| `get_season` | `json` / `html` | Épisodes d'une saison |
| `get_players` | `json` / `html` | Liste différée de lecteurs |
| `resolve_stream` | `json` / `html` / `text` | Extraction facultative propre à la source, choisie via `scraper-query` |
| `list_lives` | `json` / `html` | Liste des chaînes en direct |
| `get_live` | `json` / `html` | Détail d'un direct (joueur) |

Les commandes qui produisent un ETag global participent à la validation
conditionnelle sur les deux transports. REST les renvoie avec
`Cache-Control: private, no-cache` et `Vary: Cookie` : le cache privé du
navigateur conserve la réponse, renvoie `If-None-Match` au chargement suivant
et résout un `304 Not Modified` avec le corps mis en cache sans impliquer
l'application. Les réponses sans validateur restent en `no-store`. L'IPC Tauri
n'a pas de cache HTTP : le client natif envoie le validateur dans le champ
réservé `arachneaEtag` du payload d'invoke et reçoit une réponse conditionnelle
sous `__arachnea_conditional_reply` avec `etag`, `not_modified` et `value`. Les
commandes sans validateur conservent leur payload brut.

---

## 4. Types de médias

Les sources déclarent leurs types de médias via une section `shared` avec un
alias YAML :

```yaml
shared:
  media_types_ma_source: &media_types_ma_source
    - collection
    - video/movie
    - video/show/serie
    - video/show/anime
    - video/show/documentary
    - video/show/sport
    - video/show/other
    - video/news
    - video/live
    - images/manga
    - images/webtoon
```

Hiérarchie des types :

| Type | Description |
|---|---|
| `collection` | Collection / catégorie |
| `video/movie` | Film |
| `video/show/serie` | Série TV |
| `video/show/anime` | Animé / dessin animé |
| `video/show/documentary` | Documentaire |
| `video/show/sport` | Sport |
| `video/show/kids` | Jeunesse / enfant |
| `video/show/other` | Autre type d'émission |
| `video/news` | Information / magazine |
| `video/live` | Direct TV |
| `video/other` | Vidéo non classée |
| `images/manga` | Manga / scan |
| `images/webtoon` | Webtoon |

`media_types` sur une requête déclare ses capacités de sélection/filtrage backend ;
il est distinct du `media-type` de sortie qui route chaque carte. Déclarer
`collection` pour les requêtes qui retournent des collections.

---

## 5. Structure des données de sortie

### 5.1 Métadonnées du service — `service_stream_metadata`

Requête statique. Chaque source retourne exactement une ligne.

```yaml
- name: service_stream_metadata
  scraper_type: static
  entries:
    - name: id
      type: string
      value: "{service_id}"
    - name: title
      type: string
      value: "{service_title}"
    - name: logo
      type: string
      value: "{service_logo}"
    - name: description
      type: object
      value: "{service_description}"
    - name: metadata
      type: object
      value: "{service_metadata}"
    - name: search_themes
      type: string[]
      value:
        - "Action"
        - "Comedy"
        - "Drama"
```

| Champ | Type | Obligatoire | Description |
|---|---|---|---|
| `id` | `string` | oui | Identifiant unique de la source (ex. `example-source`) |
| `title` | `string` | oui | Nom affiché (ex. Source exemple) |
| `logo` | `string` | oui | URL du logo |
| `description` | `object` | oui | Description multilingue (ex: `{ fr: "…", en: "…" }`) |
| `metadata` | `object` | non | Tags déclarés à la racine du YAML, ou `{}` |
| `category_url_patterns` | `string[]` | non | Expressions régulières JavaScript qui doivent correspondre à l’URL publique entière pour retrouver directement une catégorie épinglée ou son contenu |
| `search_themes` | `string[]` | non | Liste des thèmes disponibles pour filtrer la recherche |
| `themes` | `string[]` | non | Variante de `search_themes` |

La commande publique `get_service` ajoute `supported_queries: string[]` à chaque
ligne, trié depuis le registre chargé et joint par l'identifiant exact
`id`/nom de collection. Ce champ n'est pas maintenu manuellement en YAML. Un
identifiant de métadonnées sans correspondance reçoit une liste vide ; le
sélecteur des homes sources liste uniquement les services annonçant `load_home`.
Ni la découverte ni le filtre ne réactivent une source désactivée/non chargée.

Quand l’URL correspond et que la source annonce `get_category`, l’accueil
personnalisé utilise l’identifiant de la source et `query_url` pour retrouver la
catégorie. Le contenu et les bannières passent par `get_category`. Une section
épinglée reste sélectionnée par son identifiant stable. Déclarer uniquement des
URL publiques stables que `get_category` accepte sans paramètre supplémentaire.
En l’absence de correspondance, la recherche suit le chemin depuis `load_home`.

---

### 5.2 Page d'accueil — `load_home`

La commande publique REST/Tauri accepte `sources: string[]` optionnel : des
identifiants exacts de collections backend, pas des titres ou des descripteurs
de catégories.

- `{}` conserve le comportement toutes sources chargées.
- `{"sources":[]}` retourne `{"data":[],"errors":[]}` sans fournisseur ni ETag.
- `{"sources":["example-source","example-catalogue"]}` exécute uniquement ces sources ; la
  sélection est triée et les doublons sont supprimés.
- `null`, les types JSON incorrects et les identifiants vides/blancs sont rejetés
  à la désérialisation. Aucun identifiant n'est corrigé ou tronqué implicitement.
- Les sources inconnues, désactivées/non chargées ou sans `load_home` produisent
  des erreurs contextuelles dans `errors`, sans repli toutes sources. Une
  sélection mixte conserve les résultats valides. Aucun ETag n'est publié sur
  une réponse explicitement filtrée contenant des erreurs ; les requêtes sans
  filtre conservent leur comportement de cache existant.

REST : `GET /api/load_home?sources=%5B%22example-source%22%5D` (tableau encodé en JSON).
Tauri : `invoke('load_home', { sources: ['example-source'] })`. L'omission et le tableau
vide sont distincts. L'enveloppe et la provenance restent inchangées ; les
`sources` de `get_category` conservent leur signification de descripteurs.

Le frontend public utilise `/sources` pour une vue d'ensemble des sources
annonçant `load_home` et expose `/source/:sourceId` pour les accueils de source
isolés. Le lien de navigation `Accueil` ouvre toujours `/sources`, tandis que
`/` résout l'accueil par défaut du profil. Il découvre les métadonnées sans
précharger chaque accueil de fournisseur.
Les signets sont filtrés par source ; les préférences de sections sont écrites
sous `source:<identifiant-source-encodé>:<clé-héritée>` dans le magasin IndexedDB
existant, avec les styles hérités comme repli de lecture uniquement. Aucun
signet ni préférence globale n'est migré ou réinitialisé.

`load_home` est la requête légère de découverte de la page. Quand l'API amont
propose des endpoints séparés pour les bannières, le contenu d'une catégorie ou
les éléments d'une section, elle doit retourner leurs liens au lieu de charger
et d'intégrer tout leur contenu immédiatement. Le frontend appelle ensuite
`get_banners`, `get_category` ou `get_section` à la demande.

**Ne pas utiliser des `sub_queries` YAML dans `load_home` pour enrichir chaque
bannière, catégorie ou section lorsqu'une de ces requêtes stream standard peut
charger le même payload.** Ce modèle transforme une requête d'accueil en N+1
requêtes, augmente la charge du cache et du fournisseur, et retarde le premier
affichage. Les `sub_queries` restent réservées aux cas où aucune requête stream
dédiée ne peut représenter le contrat de suivi.

Structure produite par chaque source :

```yaml
- name: load_home
  scraper_type: json
  entries:
    - name: categories        # Filtres par chaîne
      type: object[]
      entries:
        - name: key
          type: string
        - name: label
          type: string
        - name: link
          type: string
        - name: request > query_url
          type: string

    - name: sections          # Rails de contenu
      type: object[]
      entries:
        - name: label
          type: string
        - name: link
          type: string
        - name: entries       # Cartes de fiches/catégories/directs
          type: object[]
          entries: [ … ]
        - name: subsections   # Rails sélectionnables optionnels
          type: object[]
          entries:
            - name: key
              type: string
            - name: label
              type: string
            - name: entries   # Cartes de fiches/catégories/directs propres à la sous-section
              type: object[]
              entries: [ … ]

    - name: banners           # Bannières promotionnelles
      type: object[]
      entries:
        - name: key
          type: string
        - name: title
          type: string
        - name: description
          type: string
        - name: image
          type: string
        - name: logo
          type: string
        - name: link
          type: string
        - name: web-link
          type: string
```

#### Catégorie

| Champ | Type | Description |
|---|---|---|
| `key` | `string` | Identifiant de la catégorie (ex: `news`, `sports`, `all`) |
| `label` | `string` | Nom affiché (ex: « Actualités », « Sports », « Tout le catalogue ») |
| `link` | `string` | URL ou chemin pour les appels suivants |
| `description` | `string` | Description optionnelle |
| `request > query_url` | `string` | URL de l'API pour `get_category` |
| `request > channel` | `string` | Identifiant de chaîne pour les paramètres |
| `request > channel_label` | `string` | Nom de chaîne |
| `request > page_size` | `number` | Taille de page |
| `request > source` | `string` | Identifiant de source pour `get_category` |

Pour une catégorie chargée paresseusement, `request > query_url` est l'endpoint
de catégorie consommé par `get_category`. `request > name` (ou
`request > source`) doit identifier la source propriétaire de cet endpoint.

Chaque élément de `get_category.data` peut fournir un objet `channel` au même
niveau que `banners`, `categories` et `sections`. Il reprend les champs `name`
et `logo` de `get_entry`. `name` identifie la chaîne et `logo` est une URL
d’image facultative. Le client lit la chaîne après avoir chargé la réponse de
la route courante. La barre de titre affiche la chaîne si toutes les lignes
s’accordent sur son identité. En l’absence de chaîne, elle affiche l’identité
du service lorsqu’il est unique. Le nom de la chaîne reste visible si son logo
est absent ou échoue au chargement. Les URL de catégorie contiennent le libellé,
les requêtes des sources et les chemins d’origine facultatifs.

#### Section

| Champ | Type | Description |
|---|---|---|
| `label` | `string` | Titre du rail (ex: « Derniers ajouts », « Tendances ») |
| `key` | `string` | Identité persistante du rail, à portée source et indépendante du titre |
| `entries` | `object[]` | Cartes de fiches, catégories ou directs (voir section suivante) |
| `link` | `string` | URL pour `get_section` (pagination) |
| `subsections` | `object[]` | Rails optionnels sélectionnables sous la section parente |

Pour une section chargée paresseusement, émettre `link` même si `entries` est
initialement absent ou vide. `get_section` reçoit ce lien et le numéro de page
un basé demandé. Ne pas précharger les entrées de chaque section dans
`load_home` au moyen d'une `sub_query`.

Les nouveaux mappings d'identité utilisent `key: rail:<provider-id>` en JSON
comme en HTML. Utiliser l'identifiant du rail/conteneur fournisseur, jamais
celui d'un média, le titre, l'index ou un identifiant de slider aléatoire.
Un unique rail fixe de catégorie peut utiliser `rail:category-content`, dans
la portée de son bloc de catégorie. Les clés déjà publiées restent inchangées
pour préserver les références. La portée source vient de la ligne parente.
Conserver le secours par descripteur lorsqu'il existe pour les rails sans
identifiant de conteneur persistant. Les références par titre sont fragiles.
Un identifiant fournisseur peut changer si le rail est remplacé : une référence
manquante doit être réparée, jamais rapprochée d'un titre similaire.

Une sous-section contient un `key` stable, un `label`, des `entries` et peut
porter les mêmes champs de pagination que la section (`link`, `current_page`,
`have_more`). Les entrées de sous-sections distinctes ne sont jamais aplaties ni
fusionnées entre elles : lors de l'agrégation multi-source, elles sont fusionnées
par `key` sous leur section parente. Le frontend affiche un seul rail actif à la
fois, dont le titre combine le libellé de la section et celui de la sous-section.

#### Collection de bannières

`load_home` et `get_category` peuvent retourner la même collection de bannières.

| Chemin de sortie YAML | Type | Description |
|---|---|---|
| `banners > entries` | `object[]` | Entrées fournies directement. Un tableau `banners` fournit aussi des entrées directes. |
| `banners > link` | `string` | Lien différé transmis à `get_banners`. Il peut accompagner des entrées directes. Utiliser `get_banners` pour charger les données différées après la requête initiale. |
| `banners > source` | `string` | ID de la source de la collection. Par défaut, il provient de la ligne parente. Il sert à la requête différée et les entrées en héritent. |

Les bannières de catégorie intégrées ou différées utilisent `channel.logo` de
la ligne dans l’emplacement du logo du fournisseur. Le logo du service sert
de repli si celui de la chaîne est absent ou échoue au chargement.

#### Entrée de bannière

| Chemin de sortie YAML | Type | Description |
|---|---|---|
| `key`, `id` | `string` | Identifiant stable facultatif. `id` est prioritaire sur `key`. |
| `source` | `string` | ID de source facultatif prioritaire sur celui de la collection. Les anciennes entrées peuvent fournir `source > name`. |
| `title`, `subtitle`, `description` | `string` | Titre principal, texte secondaire et description. |
| `image` | `string` | Visuel affiché derrière le contenu de la bannière, y compris pendant la lecture. Alias `image_url` et `imageUrl`. |
| `logo` | `string` | Logo facultatif du contenu mis en avant, affiché séparément du logo de la collection. |
| `media-type`, `request` | `string` ou `string[]`, `object` | Respecter le contrat de navigation des MediaItem de la section 5.3. Les catégories utilisent `request > query_url` et les directs utilisent `request > channel`. |
| `link` | `string` | URL interne de la fiche. Le client accepte aussi `entry_url` et `entryUrl`. |
| `web-link` | `string` | URL publique. Le client accepte aussi `web_url` et `webUrl`. |
| `video-kind` | `object` | Descripteur de lecture facultatif. Dans une définition de requête, utiliser des entrées imbriquées ou un chemin comme `name: video-kind > kind`. |
| `video-kind > kind` | `string` | `video` limite la lecture à la bannière. `trailer` permet aussi la lecture en arrière-plan de la page si l’option correspondante est activée. |
| `video-kind > url` | `string` | URL de lecture directe facultative. Le client accepte aussi les alias `video_url`, `videoUrl` et `video` à la racine de l’entrée. |
| `video-kind > image` | `string` | Affiche facultative de la zone du lecteur. Elle peut s’afficher pendant la résolution, avant la lecture native, au chargement d’un lecteur intégré et pendant sa mise en pause. |
| `players` | `object[]` | Lecture facultative via un résolveur. Le premier lecteur fournissant `resolver > kind` et `resolver > target_id` non vides fournit le descripteur. |
| `players > resolver > kind`, `players > resolver > target_id` | `string` | ID du résolveur enregistré et cible à résoudre. |
| `players > resolver > source` | `string` | ID de collection facultatif pour un résolveur propre à une source. |
| `players > resolver > proxy > countries` | `string[]` | Pays facultatifs pour la résolution (voir section 5.10). |
| `players > resolver > proxy > rewrite_manifest_urls` | `string` ou `boolean` | Option de réécriture des URL HLS absolues (voir section 5.10). |

Une URL directe utilisable est prioritaire sur `players`. Avec un résolveur, le
frontend appelle `get_stream` à la demande. Les anciennes réponses peuvent
fournir `video-kind` sous forme de chaîne. La valeur de repli générale est
`trailer` si le type est absent ou inconnu. Renseigner explicitement le type
dans les nouvelles bannières. Le visuel d’une vidéo intégrale reste utilisable
comme image d’arrière-plan.

Une bannière s’affiche si elle fournit un titre, un visuel, un logo, une URL
vidéo, une affiche ou un résolveur utilisable. Le bouton de navigation apparaît
lorsque les champs définissent une route valide. Une bannière de chaîne peut
utiliser le nom de la chaîne comme titre et ajouter le nom de la vidéo mise en
avant comme sous-titre, son aperçu comme affiche et l’URL de sa fiche comme cible.
Une bannière de direct peut employer les mêmes champs et une cible de direct.
Le YAML sélectionne la vidéo à lire et peut prévoir un enregistrement comme repli
lorsque le direct est indisponible.

Exemple de résultat émis par `get_category`.

```yaml
channel:
  name: "Example Studio"
  logo: "https://images.example.org/studio.png"
banners:
  entries:
    - key: studio-feature
      title: "Example Studio"
      subtitle: "Behind the scenes"
      image: "https://images.example.org/studio-cover.jpg"
      media-type: video
      link: "https://example.org/videos/42"
      video-kind:
        kind: video
        image: "https://images.example.org/videos/42-poster.jpg"
        url: "https://media.example.org/videos/42.mp4"
```

---

### 5.3 Objet MediaItem (entrée catalogue)

Présentation utilisée dans les rails d'accueil/catégorie, sous-sections,
recherches, recommandations, bannières et listes de directs. Le `media-type` de
chaque carte choisit sa page via le registre frontend unique
`front/public-app/src/services/mediaRouting.ts` :

| Type | Route | Opération backend |
|---|---|---|
| `collection` | `category` | `get_category` |
| `video/live` | `live-details` | `get_live` |
| `video` ou `video/*` | `entry-details` | `get_entry` |
| `audio` / `audio/*`, `images` / `images/*` | `entry-details` | `get_entry` (compatibilité temporaire) |

Les contenus audio et images ne disposent pas encore de vues dédiées ; ce mapping
ne fournit pas leur prise en charge complète. Les identifiants sont nettoyés,
convertis en minuscules et dédupliqués. `video/live` prime sur les autres types
vidéo. Les familles respectent la frontière `/`. Plusieurs types résolus vers le
même gestionnaire sont acceptés ; les combinaisons incompatibles comme
`collection` et `video/movie`, les types explicites inconnus et les valeurs non
textuelles rendent la carte non ouvrable, avec un diagnostic de normalisation
pour les types inconnus ou contradictoires. Un type absent/null/vide préserve la
fiche historique, notamment pour les anciens signets. Une cible de collection ou
de direct incomplète ne se replie jamais vers `get_entry`.

Une collection fournit `title`, une source émise ou héritée et `request.query_url`.
Les autres paramètres textuels de `request` sont conservés ; `request.name` ou
`request.source` peuvent remplacer la source héritée. `link` ne remplace pas ce
descripteur. Les collections normalisées conservent leurs types bruts et requêtes,
vident les métadonnées exclusivement média et n'ont aucune URL de fiche. Leur
présentation garde l'effet de vignettes empilées. La déduplication utilise
l'opération et les descripteurs complets de source. La bande des catégories
utilise le même registre.

Un direct fournit une source et `request.channel`, l'identifiant opaque accepté
par `get_live`. Les listes live historiques peuvent le fournir dans `link`. Les
cartes de catalogue conservent une cible dédiée sans URL de fiche ; la présentation
historique des listes live conserve son lien opaque en interne. La déduplication
utilise la source et l'identifiant live. Les libellés de durée et de prix sont vidés.

Les directs appellent immédiatement `get_live`, indépendamment de la présence,
de la disponibilité ou du chargement de `list_lives`. Les champs facultatifs de
titre, image, description et URL publique restent dans la route live v1 ; les
anciens liens source/channel restent valides. Une réponse tardive de liste,
sélection ou résolution de flux ne remplace pas une cible plus récente.

Les vrais liens et les sélections utilisent le même constructeur de route, y
compris les recherches, recommandations et bannières. Les rails différés/paginés,
sous-sections fusionnées et accueils personnalisés conservent types et cibles.
Les descripteurs de rails choisissent toujours `get_section` ; aucun champ de type
de carte ou section supplémentaire n'est utilisé. Les versions de routes et
références sauvegardées ne nécessitent aucune migration. Les futures familles
prises en charge enregistreront leur destination et présentation dans ce registre.

```yaml
entries:
  - media-type: collection
    source: example-source
    title: "Exemple de catégorie"
    request:
      query_url: "https://example.org/category/1"
  - media-type: video/movie
    source: example-source
    title: "Exemple de film"
    link: "https://example.org/movie/1"
  - media-type: video/live
    source: example-source
    title: "Exemple de direct"
    request:
      channel: "event-1"
```

| Champ YAML | Type YAML | Obligatoire | Description |
|---|---|---|---|
| `source` | `string` | oui pour une carte ouvrable | Identifiant de la source. Il doit être émis par le YAML sur chaque `MediaItem`, ou sur son enveloppe de résultat afin que le frontend puisse l'hériter. Il n'est pas injecté automatiquement pour toutes les formes de réponse. |
| `title` | `string` | oui | Titre principal |
| `title/alt` | `string` | non | Titre alternatif |
| `link` | `string` | pour les fiches | URL interne pour `get_entry` |
| `request` | `object` | pour les catégories/directs | Paramètres textuels de cible : `query_url` pour une catégorie, `channel` pour un direct |
| `web-link` | `string` | non | URL publique du contenu |
| `description` | `string` | non | Synopsis / description |
| `media-type` | `string` ou `string[]` | pour la navigation typée | Type de routage (ex : `collection`, `video/live`) et sous-type média (ex: `video/show/serie`). Les formes personnalisées `video/other/<libellé>` et `video/show/other/<libellé>` affichent directement leur libellé dans le frontend. |
| `duration` | `number` ou `string` numérique | non | Durée formatée |
| `release-date` | `string` | non | Date de publication (`YYYY-MM-DD`) |
| `expire` | `string` | non | Date d'expiration |
| `year` | `number` | non | Année de production ou copyright |
| `theme` | `string[]` | non | Genres / thèmes |
| `lang` | `string[]` | non | Langues disponibles |
| `audio` | `string[]` | non | Variante de `lang` |
| `language` | `string[]` | non | Variante de `lang` |
| `label` | `string` | non | Étiquette de substitution (fallback titre) |
| `rating` | `number` | non | Note / score |
| `price` | `string` | non | `free`, `premium` ou `<montant> <DEVISE>`, par exemple `2.99 EUR` ; les formats inconnus sont ignorés |
| `episode > label` | `string` | non | Étiquette d'épisode sur les cartes |
| `season` | `string` | non | Saison |
| `tags` | `string[]` | non | Tags |
| `key` | `string` | non | Identifiant pour les lives |
| `img/poster > link` | `string` ou `string[]` | non | URL(s) du poster |
| `img/portrait > link` | `string` | non | URL portrait |
| `img/landscape > link` | `string` | non | URL paysage |
| `img/preview > link` | `string` | non | URL d'aperçu |
| `img > link` | `string` | non | Image générique de repli |
| `img/logo > link` | `string` | non | URL du logo |

> **Note** : `execute_query_async` peut injecter `source` lorsque le champ
> `ScraperAggregationOptions::source_field_name` vaut `Some("source")`. Les requêtes qui ne demandent pas
> cette injection, notamment `search`, doivent extraire `source` dans le YAML
> sur la ligne de résultat ou sur chaque `MediaItem` afin de conserver une carte
> ouvrable.

---

### 5.4 Page catégorie — `get_category`

Reprend la structure de `load_home` (catégories + sections). Les sections
contiennent des `entries` (cartes de fiches/catégories/directs) et des métadonnées de pagination :

```yaml
- name: get_category
  entries:
    - name: categories   # Mêmes catégories que load_home
    - name: sections     # Rails paginés
      entries:
        - name: label
        - name: link
        - name: current_page
        - name: total_pages
        - name: page_size
        - name: total
        - name: entries   # Cartes de fiches/catégories/directs
    post_process:
      - type: compute_items_field
      - type: derive_pagination
```

`get_category` est le pendant chargé à la demande d'une catégorie émise par
`load_home`. Elle reçoit le descripteur de catégorie via les champs `request`,
dont `request > query_url`, et doit charger elle-même le payload de catégorie.
Ne pas précharger chaque catégorie dans `load_home` par des `sub_queries`.

M6Play lit les menus publics des chaînes et des catégories de M6+ dans
`load_home`. Les cartes des chaînes reprennent les logos publiés dans le menu.
Les chaînes de replay ouvrent leurs sections de programmes, les chaînes 24/24
affichent le programme en direct et les rubriques ouvrent une liste paginée.

---

### 5.5 Section paginée — `get_section`

Retourne des cartes de fiches, catégories ou directs avec pagination. C'est le pendant chargé à la
demande d'une section émise par `load_home` : le backend reçoit le `link` de la
section et la valeur `page` un basée demandée.

| Champ YAML | Type | Description |
|---|---|---|
| `current_page` | `number` | Page courante |
| `have_more` | `boolean` | Indique si plus de pages existent |
| `total_pages` | `number` | Nombre total de pages (consommé par `derive_pagination`) |
| `page_size` | `number` | Taille de page |
| `infer_have_more_from_full_page` | `boolean` | Inférer `have_more` depuis une page pleine |
| `entries` | `object[]` | Cartes typées de fiches ou catégories |

La pagination est généralement calculée via le post-processeur
`derive_pagination` :

```yaml
post_process:
  - type: derive_pagination
    total_pages_field: total_pages
    page_size_field: page_size
    remove_fields:
      - total_pages
      - page_size
```

### 5.5.1 Bannières promotionnelles — `get_banners`

Retourne un tableau `banners` d’entrées de bannière (voir section 5.2) pour le
`banners > link` émis par `load_home` ou `get_category`. Le backend expose ce
lien au YAML sous les paramètres `{query_url}` et `{link}`.

```yaml
- name: get_banners
  scraper_type: json
  base_url: "{base_url}"
  query_url: "{query_url}"
  row_pointer: /
  entries:
    - name: banners
      type: object[]
      pointer: /items/*
      select: all
      entries:
        - name: key
          type: string
          pointer: /id
        - name: title
          type: string
          pointer: /title
        - name: image
          type: string
          pointer: /image
```

Utiliser `get_banners` plutôt qu'une `sub_query` de `load_home` dès que les
données de bannières ont leur propre endpoint ou sont coûteuses à assembler.
Cela accélère le chargement initial de l'accueil et permet aux bannières de
profiter de leur propre durée de cache client.

---

### 5.6 Recherche — `search`

Structure similaire à `get_section` :

| Champ YAML | Type | Description |
|---|---|---|
| `source` | `string` | Source (injectée) |
| `current_page` | `number` | Page courante |
| `total_pages` | `number` | Nombre total de pages |
| `have_more` | `boolean` | Indicateur de suite |
| `entries` | `object[]` | Objets MediaItem |

Paramètres d'exécution disponibles :

| Paramètre | Description |
|---|---|
| `{search_query}` | Requête brute (encodée) |
| `{search_terms}` | Termes de recherche (alias) |
| `{search_query_json}` | Requête formatée pour JSON |
| `{media_types}` | Types de médias filtrés |
| `{media_type_filter}` | Filtre formaté pour URL |
| `{themes}` | Thèmes sélectionnés |
| `{themes_filter}` | Thèmes formatés pour URL |
| `{page}` | Numéro de page |
| `{page_index}` | `page - 1` |

Les mappings de paramètres (`query_param_mappings`) permettent de traduire
les types de médias et thèmes internes en paramètres d'URL :

```yaml
query_param_mappings:
  - target_param: media_type_filter
    source_param: media_types
    item_suffix: "&"
    values:
      video/show/anime: "type%5B%5D=Anime"
      images/manga: "type%5B%5D=Scans"
```

---

### 5.7 Détail d'une entrée — `get_entry`

Retourne un objet unique avec les métadonnées complètes d'un programme.

| Champ YAML | Type | Description |
|---|---|---|
| `title` | `string` | Titre |
| `title/alt` | `string` | Titre alternatif |
| `description` | `string` | Synopsis |
| `media-type` | `string[]` | Type de média |
| `link` | `string` | Lien API |
| `web-link` | `string` | Lien public |
| `channel` | `object` | Attribution de chaîne facultative (voir ci-dessous) |
| `channel > name` | `string` | Nom affiché de la chaîne |
| `channel > logo` | `string` | URL du logo autonome, facultative |
| `channel > request` | `object` | Paramètres de catégorie à transmettre à `get_category`, dont `query_url` |
| `video/trailer` | `string` | URL de la bande-annonce |
| `year` | `number` | Année |
| `release-date` | `string` | Date de sortie |
| `expire` | `string` | Date d'expiration |
| `duration` | `number` ou `string` numérique | Durée |
| `count-season` | `number` | Nombre de saisons |
| `content-advisor` | `string` | Avertissement (ex: « Déconseillé aux -10 ans ») |
| `price` | `string` | Indication facultative : `free`, `premium` ou montant décimal et devise de trois lettres majuscules, par exemple `2.99 EUR` |
| `lang/audio` | `string[]` | Libellés des langues audio |
| `lang/subtitles` | `string[]` | Libellés de langues de sous-titres ; ce ne sont pas des URL de pistes |
| `language` | `string[]` | Langues |
| `subtitles` | `string[]` | Sous-titres |
| `theme` | `string[]` | Genres |
| `genre` | `string[]` | Genres (variante) |
| `director` | `string[]` | Réalisateurs |
| `casting` | `string[]` | Acteurs |
| `rating` | `number` | Note |
| `program-id` | `string` | Identifiant interne du programme |
| `seasons` | `object[]` | Saisons (voir 5.7.1) |
| `recommendations` | `object` | Recommandations éditoriales (voir 5.7.2, optionnel) |
| `players` | `object[]` | Lecteurs (voir 5.9) |
| `img/poster > link` | `string` | Poster |
| `img/portrait > link` | `string` | Portrait |
| `img/landscape > link` | `string` | Paysage |
| `img/logo > link` | `string` | Logo |
| `img/preview > link` | `string` | Aperçu |

`channel` est additif : les réponses qui ne l’exposent pas restent valides. Le
scraper omet l’attribution lorsqu’il ne peut pas identifier la chaîne ou construire
sa catégorie. Le client masque également le lien si le nom ou la destination est
inutilisable ; un logo absent ou en échec de chargement n’empêche pas l’affichage
du nom. Les paramètres de `channel > request` sont opaques et appartiennent au
fournisseur : le client ajoute l’identifiant de source de la fiche pour construire
le descripteur envoyé à `get_category`.

`duration` représente des secondes, y compris sous forme de chaîne numérique.
Utiliser `normalize_duration` pour les durées textuelles avant sérialisation.
`price` est une chaîne : point décimal, espace et devise en majuscules ; `2,99 €`
n'est pas le format canonique. `free` et l'absence de prix n'affichent aucun
avertissement d'accès. Les champs fournisseurs arbitraires (`morality`, `quality`,
`hoster-infos`, `server-index`) ne deviennent pas automatiquement des fonctionnalités
publiques du lecteur. Garder les champs temporaires locaux et les retirer après
post-traitement.

#### 5.7.1 Saison

| Champ YAML | Type | Description |
|---|---|---|
| `label` | `string` | Nom de la saison |
| `link` | `string` | URL pour `get_season` |
| `episodes` | `object[]` | Épisodes pré-chargés (optionnel) |

#### 5.7.2 Recommandations

`recommendations` est un objet section optionnel : une source qui ne fournit
aucune relation l'omet sans faire échouer `get_entry`. Son `label` est
facultatif et n'est jamais requis pour afficher les recommandations.

| Champ YAML | Type | Description |
|---|---|---|
| `entries` | `object[]` | Objets `MediaItem` immédiats |
| `link` | `string` | URL pour `get_recommendations` (données différées ou paginées) |
| `current_page` | `number` | Page déjà renvoyée (vaut normalement `1` pour une réponse immédiate) |
| `have_more` | `boolean` | Indique la disponibilité d'une page suivante via `get_recommendations` |
| `label` | `string` | Libellé de section facultatif |

Une source fournit soit les cartes déjà présentes dans la réponse de détail
(`recommendations > entries`), soit un `link` de recommandations. Dans ce
second cas, le frontend appelle `get_recommendations` avec le `source` courant,
ce `link` et la page un basée demandée (défaut `1`). La réponse reprend le
format de `get_section` (`entries`, `current_page`, `have_more`), sans
enveloppe de section additionnelle. `get_section` ne doit pas être réutilisé :
son contrat représente les rails génériques du catalogue. Ne pas charger des
recommandations externes au moyen d'une `sub_query` de `get_entry` et ne pas
dériver les recommandations depuis une recherche ou une catégorie : la relation
doit être explicitement fournie par la source.

Paramètres d'exécution disponibles pour `get_recommendations` :

| Paramètre | Description |
|---|---|
| `{query_url}` / `{link}` | URL de recommandations émise par `get_entry` |
| `{page}` | Numéro de page un basée demandée (défaut `1`) |
| `{page_index}` | `page - 1` |

---

### 5.8 Saison et épisodes — `get_season`

| Champ YAML | Type | Description |
|---|---|---|
| `current_page` | `number` | Page courante |
| `have_more` | `boolean` | Suite |
| `episodes` | `object[]` | Liste d'épisodes |

#### Épisode

| Champ YAML | Type | Description |
|---|---|---|
| `title` | `string` | Titre de l'épisode |
| `title/alt` | `string` | Titre alternatif |
| `description` | `string` | Synopsis |
| `duration` | `number` ou `string` numérique | Durée |
| `release-date` | `string` | Date de diffusion |
| `expire` | `string` | Date d'expiration |
| `link` | `string` | Lien de détail |
| `web-link` | `string` | Lien public |
| `season-name` | `string` | Nom de la saison associée |
| `episode-number` | `string` | Numéro de l'épisode |
| `season` | `string` | Saison |
| `players` | `object[]` | Lecteurs (voir 5.9) |
| `img/preview > link` | `string` | Aperçu |
| `img/poster > link` | `string` | Fallback aperçu |

### 5.8.1 Lecteurs différés — `get_players`

Commande dédiée au chargement différé des lecteurs d'un élément isolé
(typiquement un épisode) dont le payload d'origine ne déclare aucun `players`.
Le backend la lance avec l'URL média de l'élément.

| Champ YAML | Type | Description |
|---|---|---|
| `query_url` | `string` | URL média de l'élément, fournie par le backend |
| `link` | `string` | Alias de `query_url` |
| `players` | `object[]` | Lecteurs (voir 5.9) |

La requête produit le même `players` que `get_entry`. Lorsqu'une source
n'expose pas de lecteurs embarqués sur ses épisodes, elle doit déclarer
`get_players` : le frontend publie le `link` média de l'épisode comme collection
`players` et charge celle-ci via cette commande. Sans cette requête, l'API ne
renvoie aucun lecteur et la lecture reste indisponible pour ces éléments. Le
lien peut contenir un fragment utilisé uniquement par le scraper pour sélectionner
une table d'épisode côté client ; ce fragment n'est jamais envoyé à la source et
est exposé au YAML sous la forme `{url_fragment}`.

```yaml
- name: get_players
  scraper_type: json
  base_url: "{base_url}"
  query_url: "{query_url}"
  row_pointer: /
  entries:
    - name: players
      type: object[]
      pointer: /uuid
      select: all
      entries: *entries_player
```

---

### 5.9 Lecteur (`players[]`)

Structure utilisée dans `get_entry`, `get_season` et `get_live` pour
décrire les options de lecture.

| Champ YAML | Type | Description |
|---|---|---|
| `name` | `string` | Nom du lecteur |
| `lang` | `string` | Langue de la piste |
| `streams` | `object[]` | Flux directs ; métadonnées communes au niveau du lecteur ; voir ci-dessous |
| `resolver` | `string` | Identifiant global du résolveur (format plat, ex: `stream-resolver`) |
| `target` | `string` | Cible pour le résolveur (URL ou identifiant) |
| `resolver > kind` | `string` | Type de résolveur (format objet legacy, ex: `example-video`) |
| `resolver > target_id` | `string` | Identifiant cible pour le résolveur (format objet legacy) |
| `resolver > stream > kind` | `string` | Type de flux (legacy, ex: `example-license-proxy`) |
| `subtitles` | `object[]` | Pistes externes WebVTT communes aux flux |

Un player fournit un tableau `streams` directement lisible ou un descripteur
`{ resolver, target }` à résoudre côté backend. Les hébergeurs distincts restent des
players séparés ; formats, qualités et langues interchangeables appartiennent à
`streams`. Les champs communs `storyboard_vtt_url`, `storyboard`, `chapters` et `subtitles`
sont voisins de `streams` sur le lecteur, ou à la racine des réponses `get_stream`
et `resolve_stream`. Aucune rétrocompatibilité du schéma vidéo : l’ancien objet
`stream`, `direct-link`, `stream_url` et `manifest_type` ne sont plus lus ni produits. Déployer serveur et
frontend ensemble.

Exemple avec résolveur générique (format plat) :

```yaml
- name: players
  type: object[]
  entries:
    - name: name
      type: string
      actions:
        - type: format_text
          argument: "Hébergeur externe"
    - name: resolver
      type: string
      actions:
        - type: format_text
          argument: "stream-resolver"
    - name: target
      type: string
      actions:
        - type: build_url
          base: "https://player.example/embed/{id}"
          fields:
            id: /video_id
```

Exemple avec résolveur enregistré (format objet legacy) :

```yaml
- name: players
  type: object[]
  entries:
    - name: name
      type: string
      actions:
        - type: format_text
          argument: "Example player"
    - name: resolver
      type: object
      entries:
        - name: kind
          type: string
          actions:
            - type: format_text
              argument: "example-video"
        - name: target_id
          type: string
          pointer: /id
```

### 5.9.1 Résolution de flux — `get_stream`

La commande backend `get_stream` reçoit `{ resolver, target }` et retourne
une réponse de type union exclusive :

| Champ | Type | Description |
|---|---|---|
| `streams` | `object[]` | Alternatives non vides, sans `id` |
| `streams > url` | `string` | URL du média ou du manifeste |
| `streams > type` | `string` | `hls`, `dash`, `mp4`, `webm`, `ogg` |
| `streams > quality` | `string` | Label de qualité facultatif (HD, SD, 1080p) |
| `streams > lang` | `string` | Langue facultative ; absente = inconnue |
| `streams > license_url` | `string` | Licence DRM de cette variante uniquement |
| `streams > license_headers` | `object` | En-têtes de licence propres à cette variante |
| `storyboard_vtt_url` | `string` | URL WebVTT des miniatures communes |
| `storyboard` | `object` | Métadonnées du sprite commun |
| `chapters` | `object[]` | Chapitres de la chronologie commune |
| `subtitles` | `object[]` | Pistes WebVTT externes communes |
| `title`, `image/title > link` | `string` | Titre/image facultatifs du player |
| `embed-link` | `string` | Repli iframe, exclusif de `streams` |

La qualité des variantes utilise `quality` (SD, HD ou 1080p, par exemple).
L’ancien champ de variante `label` n’est plus accepté ; les libellés des
sous-titres et des lecteurs sont des champs distincts.

`streams` et `embed-link` sont exclusifs. Chaque source porte son format et son
contexte DRM ; les en-têtes de transport restent intégrés aux URL proxy. Aucun
identifiant dans les sources : leur index est interne à la génération de lecture.

Le frontend préfère HLS, puis DASH, puis MP4 (autres fichiers pris en charge ensuite),
après application des choix manuels de langue/qualité. Les qualités fixes et langues
se sélectionnent dans le lecteur ; les contrôles du manifeste adaptatif actif restent
accessibles.
Une erreur vidéo fatale essaie les variantes restantes de même langue/qualité,
puis les autres qualités, la langue inconnue et les autres langues. Une erreur de source
n'est émise au parent qu'après épuisement des variantes. Pour une fiche ou un épisode,
le parent essaie ensuite les serveurs restants dans la langue sélectionnée, avec les lecteurs
natifs avant les iframes et l'ordre backend conservé dans chaque groupe. Le chargement d'une
fiche ou d'un épisode démarre au premier serveur ; un changement de langue privilégie le
serveur portant le même nom que le serveur courant, puis parcourt les candidats restants
dans l'ordre trié. Les serveurs en échec sont ignorés jusqu'au
changement d'épisode, de langue ou de sélection explicite du serveur. Seul l'épuisement
des serveurs affiche l'erreur finale. Les erreurs de résolution suivent ce même repli.
La sélection explicite d'un serveur désactive ce repli : son erreur de résolution ou son
erreur vidéo finale s'affiche directement. Changer d'épisode, de fiche ou de langue réactive
le repli automatique ; conserver un serveur par son nom n'est pas une sélection explicite.
Les liens embed sont des indices initiaux : ils peuvent se résoudre en flux vidéo natifs.
Les flux natifs connus précèdent les candidats à résoudre, qui précèdent les iframes
confirmées ; chaque groupe conserve l'ordre backend. Une iframe découverte à la résolution
attend les candidats natifs et à résoudre restants, y compris les autres liens embed, sauf si ce serveur
a été choisi explicitement ou conservé par nom lors d'un changement de langue. Les erreurs
internes aux iframes externes peuvent être indétectables ; les règles de sécurité restent actives.
Un chargement actif dispose d'un délai de 20 secondes ; pause, refus d'autoplay et
échec des ressources auxiliaires ne sont pas des erreurs vidéo fatales. Une bascule
conserve l'état de lecture compatible, les métadonnées communes et la fenêtre live.

Les champs de métadonnées ci-dessous sont au même niveau que `streams`.

Lorsqu'un résolveur déclare `storyboard_vtt_url` avec l'action `resolve_url`, `proxy: true` et une
règle `proxy_replace_all`, `get_stream` retourne une URL proxy. Le proxy réécrit alors chaque cue
WebVTT dont la ligne commence par `http://` ou `https://` vers une URL proxy, puis le frontend
convertit les coordonnées `#xywh` en configuration de sprite Video.js. Les VTT qui utilisent une
image sprite unique et des cellules homogènes sont pris en charge. En cas de chargement ou de format
incompatible, le frontend utilise le champ `storyboard` lorsqu'il est disponible.

`chapters` contient les chapitres ordonnés partagés par les variantes de flux.

| Champ | Type | Description |
|---|---|---|
| `start` | `number` | Début obligatoire en secondes, sous forme de nombre fini. |
| `end` | `number` | Fin obligatoire en secondes, sous forme de nombre fini supérieur à `start`. |
| `type` | `string` | Type obligatoire. Utiliser `chapter` pour les chapitres du contenu. |
| `title` | `string` | Titre facultatif affiché dans le lecteur. |

Le lecteur public ignore les chapitres dont les bornes sont absentes ou invalides.
Pour construire les chapitres à partir de repères temporels, utiliser le repère
suivant comme fin du chapitre courant. Utiliser la durée de la vidéo pour fermer
le dernier chapitre lorsqu’elle est disponible.

`subtitles` contient des pistes WebVTT externes lisibles par le navigateur, dans l'ordre du
résolveur :

| Champ | Type | Description |
|---|---|---|
| `lang` | `string` | Code de langue optionnel annoncé par la source ; il est conservé sans conversion |
| `label` | `string` | Libellé humain optionnel affiché par le lecteur |
| `link` | `string` | URL WebVTT HTTP(S), ou URL de proxy applicatif, obligatoire |
| `auto_translate` | `boolean` | Indicateur optionnel de piste générée ou traduite automatiquement |

Les pistes dont `link` est vide ou invalide sont ignorées. Les valeurs `lang` et `label` vides sont
omises ; une piste reste toutefois conservée lorsqu'elles sont toutes les deux absentes.
`auto_translate` est omis lorsqu'il est absent ou invalide. Une liste vide n'est pas sérialisée dans
la réponse JSON. Les résolveurs doivent proxifier les URLs de sous-titres qui nécessitent des en-têtes
de requête ou qui ne seraient pas lisibles directement par le navigateur. Le lecteur public ajoute ces
pistes comme pistes distantes Video.js après avoir défini
la source média et retire ses propres pistes distantes lors d'un
changement de source ou de la destruction du lecteur. Les pistes texte natives HLS/DASH restent
gérées par leurs moteurs de lecture respectifs. Les pistes peuvent être activées automatiquement
par les profils de sélection de piste configurés (langue audio, sous-titres selon la priorité de
langues, audiodescription en dernier recours) ; sinon, aucune piste n'est activée par défaut.

Chaque chargement `get_entry` initialise le lecteur public avec les profils audio/sous-titres
configurés, sans reprendre les choix manuels durables des fiches précédentes. Les choix manuels
faits ensuite restent prioritaires entre les épisodes, saisons et sources de cette même fiche.
Ouvrir une autre fiche, rouvrir la même fiche ou recharger ses détails réapplique les profils
configurés. Les autres réglages persistants, dont le volume et la qualité, restent disponibles.

Dans une fiche, la correspondance d'un choix manuel repose sur un même identifiant non généré
par DASH ou sur une même langue normalisée, un même type et un même libellé. Un choix indisponible
laisse le profil s'appliquer sans remplacer la préférence manuelle conservée. La désactivation
manuelle des sous-titres s'applique aux épisodes de cette fiche. Les préférences conservent leur
origine `manual` ou `profile` en stockage ; les lecteurs contrôlés autonomes peuvent toujours
reprendre les choix durables, y compris les anciennes préférences sans origine. Les lecteurs
d'arrière-plan n'appliquent pas les profils.

Modifier les profils de pistes, la priorité des langues de sous-titres, les autorisations de
traduction ou l'autorisation de l'audiodescription efface les choix audio/sous-titres manuels
enregistrés et réapplique immédiatement les nouveaux paramètres aux lecteurs contrôlés montés.
L'état conservé lorsque la surface média est temporairement démontée est également actualisé.
Il n'y a plus d'action distincte pour réappliquer les préférences.

Les codes ISO 639-1 à deux lettres et ISO 639-2 terminologiques/bibliographiques à trois lettres
sont comparés via le catalogue partagé (`fr`/`fra`/`fre`, `en`/`eng`, `de`/`deu`/`ger`), y compris
avec des sous-étiquettes régionales. Lorsque le moteur n'expose aucune liste de pistes audio, un
profil `original` applique tout de même son mode de sous-titres ; l'audio reste géré nativement par
le moteur. Un événement de métadonnées relit les paramètres courants au lieu de reprendre ceux
capturés lors de l'affectation de la source.

Les paramètres du lecteur public proposent des profils audio/sous-titres ordonnés, à raison d'un
profil par langue audio du catalogue partagé (piste originale incluse), et une liste de priorité
de dix langues de sous-titres au maximum, reliée à
`videoPlayer.subtitleLanguagePriority`. Cette liste suit la langue d'interface jusqu'à sa
personnalisation et se réordonne avec les boutons monter/descendre. L'ancienne valeur
`videoPlayer.subtitleLanguages` sert uniquement de repli de migration en l'absence de priorité
enregistrée. À langue correspondante identique, les sous-titres externes humains sont préférés
aux pistes marquées `auto_translate: true`. Les ajouts tardifs de pistes HLS réappliquent la
sélection pendant toute la source courante, y compris après le début de lecture ; les écouteurs
et le délai de réapplication sont libérés au changement de source ou à la destruction du lecteur.

Le champ `storyboard` contient :

| Champ | Type | Description |
|---|---|---|
| `url` | `string` | URL de l'image sprite ; peut contenir le placeholder littéral `{index}` |
| `width` | `number` | Largeur optionnelle d'une vignette ; déduite de la première image sprite lorsqu'elle est absente |
| `height` | `number` | Hauteur optionnelle d'une vignette ; déduite de la première image sprite lorsqu'elle est absente |
| `columns` | `number` | Nombre de colonnes dans le sprite |
| `rows` | `number` | Nombre de lignes de vignettes par image sprite |
| `first_page_index` | `number` | Index optionnel de la première image d'un modèle URL `{index}` ; vaut `0` par défaut |
| `index_padding` | `number` | Largeur de remplissage optionnelle du `{index}` substitué avec des zéros ; sans remplissage si absent ou `0` |
| `interval` | `number` | Intervalle optionnel entre vignettes (secondes) ; le frontend utilise la durée vidéo divisée par `rows × columns` lorsqu'il est absent |

Par exemple, un storyboard à douze colonnes, six lignes et `first_page_index: 1` remplace `{index}`
par `1` pour charger sa première image. Un sprite unique déclare tout de même `rows: 1`.
Avec `index_padding: 3` et `first_page_index: 1`, les trois premières URL se terminent par
`001`, `002`, `003`.
Lorsque les dimensions sont absentes, le frontend précharge cette première image et calcule la taille
d'une cellule à partir de sa largeur naturelle divisée par `columns` et de sa hauteur naturelle
divisée par `rows`. Pour un sprite à image unique, les lignes de séparation homogènes entre les
cellules sont retirées avant le rendu des vignettes.

### 5.9.2 Proxy de licence DRM — `get_drm_license`

La commande binaire `get_drm_license` remplace l'ancienne commande
`get_stream` pour le proxy de licence DRM. Son chemin public est
`/api/get_drm_license/{source}/{token}`.

### 5.9.3 Résolveurs d'hébergeurs externes

Avec `resolver: stream-resolver`, la cible est une URL de lecteur externe. La
reconnaissance, `resolve_stream`, la réutilisation HTML, les en-têtes proxy, les
erreurs terminales et les exemples complets sont décrits dans
[arachnea-stream-hoster](arachnea-stream-hoster-fr.md). Conserver ces YAML dans
le groupe `arachnea-stream-hoster`.

Avec `resolver: scraper-query`, conserver `resolve_stream` dans le YAML de cette
source de catalogue et définir `resolver > source` avec son ID de collection.
Ce chemin exécute seulement la requête fixe `resolve_stream` de la source ; il
ne permet pas d'appeler un nom de requête arbitraire. La cible doit être une URL
HTTP(S) absolue, exposée comme `{url}` et `{query_url}`. `{proxy_countries}` est un
tableau JSON sérialisé. Un `error_message` non vide est terminal ; un flux absent
ou vide est une erreur, sans repli iframe. Utiliser les champs de sortie de flux
de la spécification hoster. Les URL retournées doivent être HTTP(S). Ce chemin
ne nécessite pas de requêtes de reconnaissance d'hébergeur.

---

### 5.10 Directs — `list_lives` et `get_live`

#### `list_lives`

Retourne une liste d'objets MediaItem avec `key` et `media-type: video/live` :

| Champ YAML | Description |
|---|---|
| `key` | Identifiant de la chaîne (ex: `news-live`) |
| `title` | Nom de la chaîne |
| `link` | Identifiant ou URL pour `get_live` |
| `web-link` | URL publique du direct |
| `release-date` | Début du programme en cours |
| `expire` | Fin du programme en cours |

Un lecteur peut demander un routage géographique ordonné pour une résolution
`scraper-query` en déclarant `resolver > proxy > countries` dans le YAML comme
liste de codes ISO alpha-2, par exemple `countries: ["FR", "BE"]`. Les valeurs
vides, invalides et dupliquées sont écartées ; les valeurs valides sont mises
en majuscules en préservant leur ordre. Le frontend transmet le résultat comme
`proxy_countries` à `get_stream`.

La query `resolve_stream` de la source peut consommer la liste JSON structurée
avec `http.proxy_countries: ["{proxy_countries}"]`. Les URLs proxy générées
embarquent la liste JSON ordonnée dans leurs `opts` sous
`Arachnea-Proxy-Countries`, afin que la récupération du manifeste tente les
pools de proxy dynamiques de chaque pays dans l'ordre. Une liste absente ou vide
ne demande aucun proxy géolocalisé et ne bascule pas vers un pays singulier.

#### Migration du routage géographique pour les intégrations externes

La liste plurielle est l'unique contrat de routage géographique accepté. Une
requête pour un seul pays utilise elle aussi une liste JSON/YAML à un élément.

| Ancien contrat (non accepté) | Contrat actuel |
|---|---|
| En-tête HTTP `Arachnea-Proxy-Country: FR` | `Arachnea-Proxy-Countries: ["FR"]` |
| Paramètre proxy `country=FR` | `countries=["FR"]` |
| YAML lecteur `resolver.proxy.country: FR` | `resolver.proxy.countries: ["FR"]` |
| JSON `get_stream` `{ "proxy_country": "FR" }` | `{ "proxy_countries": ["FR"] }` |
| Scrapyfy `http.proxy_country: "{proxy_country}"` | `http.proxy_countries: ["{proxy_countries}"]` |

`Arachnea-Proxy-Country`, `country`, `resolver.proxy.country`,
`proxy_country` et `proxyCountry` sont rejetés et ne doivent plus être émis par
les intégrations.

Un lecteur peut en outre demander la réécriture des URLs absolues rencontrées
dans les manifestes HLS proxifiés en déclarant
`resolver > proxy > rewrite_manifest_urls: "true"`. Le frontend la transmet
comme `proxy_rewrite_manifest_urls` à `get_stream`, et le backend ajoute alors
une action proxy `ReplaceAll` (`{proxy_inherited}/$1`) restreinte aux
content-types de manifeste HLS. Les playlists enfants et les segments
réutilisent ainsi les options de proxy de la requête courante (pays, en-têtes).

#### `get_live`

Retourne les informations de lecture d'un direct :

| Champ YAML | Description |
|---|---|
| `players` | Tableau de lecteurs pour le direct demandé (voir 5.9) |

---

## 6. Section `shared` et alias YAML

Les sources utilisent la section `shared` avec des ancres YAML (`&`) et des
alias (`*`) pour factoriser les configurations répétitives :

```yaml
shared:
  media_types_ma_source: &media_types_ma_source
    - video/movie
    - video/show/serie

  actions_image_to_url: &actions_image_to_url
    - type: format_text
      argument: "https://images.example.com/{}/raw"

  entries_catalog_cards: &entries_catalog_cards
    - name: title
      type: string
      selector: "h2.card-title"
      actions:
        - type: get_text
    - name: link
      type: string
      selector: "a"
      actions:
        - type: get_attribut
          argument: href
        - type: resolve_url
```

---

## 7. Exemple minimal

```yaml
id: ma-source-stream
title: "Ma Source Streaming"
logo: "https://example.com/logo.png"
description:
  fr: "Description de ma source de streaming."
http:
  mode: direct

constants:
  - name: base_url
    value: https://api.example.com

queries:
  - name: service_stream_metadata
    scraper_type: static
    entries:
      - name: id
        type: string
        value: "{service_id}"
      - name: title
        type: string
        value: "{service_title}"
      - name: logo
        type: string
        value: "{service_logo}"
      - name: description
        type: object
        value: "{service_description}"

  - name: search
    scraper_type: json
    base_url: "{base_url}"
    query_url: "{base_url}/search?q={search_terms}&page={page}"
    media_types:
      - video/movie
      - video/show/serie
    row_pointer: /results
    entries:
      - name: current_page
        type: number
        actions:
          - type: format_text
            argument: "{page}"
      - name: total_pages
        type: number
        pointer: /total_pages
      - name: entries
        type: object[]
        pointer: /items/*
        select: all
        entries:
          - name: title
            type: string
            pointer: /title
          - name: description
            type: string
            pointer: /synopsis
          - name: media-type
            type: string[]
            pointer: /type
            actions:
              - type: map
                default: "video/show/other"
                argument:
                  movie: video/movie
                  serie: video/show/serie
          - name: link
            type: string
            pointer: /id
          - name: img/poster > link
            type: string
            pointer: /poster
    post_process:
      - type: derive_pagination
        total_pages_field: total_pages
        remove_fields:
          - total_pages
```

---

## 8. Entrées des opérations et procédure de rédaction

### Entrées d'exécution

Toutes les requêtes peuvent utiliser les valeurs par défaut de la collection,
`{service_id}`, `{service_title}`, `{service_logo}`, `{service_description}` et
`{source}`. Les entrées suivantes appartiennent à des opérations précises ; ne
pas supposer qu'elles existent pour les autres opérations :

| Requête | Entrées d'exécution |
|---|---|
| `service_stream_metadata`, `load_home`, `list_lives` | Métadonnées et valeurs par défaut ; aucune URL de contenu nécessaire |
| `search` | `search_query` (texte brut), `search_query_json` (chaîne JSON littérale avec guillemets), `search_terms` (texte encodé URL), `media_types` et `themes` (séparés par des virgules), `page` (au moins 1), paramètres de continuation propres à la source |
| `get_category` | Descripteurs contenant `name` ou `source` ; les autres clés deviennent des paramètres propres à la source, dont `query_url` s'il est déclaré ; `page` et paramètres de continuation. Les descripteurs répétés d'une même source sont exécutés indépendamment, chacun avec ses propres paramètres |
| `get_entry` | `query_url` |
| `get_season` | `query_url`, `page` (au moins 1) |
| `get_section`, `get_recommendations` | Lien non vide dans `query_url` et `link`, `page` (au moins 1) |
| `get_banners`, `get_players` | Lien non vide dans `query_url` et `link` |
| `get_live` | `channel` — utiliser `{channel}`, sans supposer l'existence de `{query_url}` |
| `resolve_stream` via `scraper-query` | `url`, `query_url`, `proxy_countries` ; voir 5.9.3 |

Les paramètres de pagination dérivés sont décrits dans la spécification commune.
Un `page_size` d'exécution ou par défaut est nécessaire pour dériver `offset`.
Retourner les paramètres de continuation de chaque source sous `source_params` ;
ne pas construire la requête suivante depuis le curseur d'une autre source.
Les paramètres de continuation existants sont prioritaires sur les paramètres
du descripteur de catégorie.

### Construire et vérifier une source

1. Inscrire le YAML sous `server/services/arachnea-stream/`, directement ou via un
   manifeste importé. Choisir un `id` unique et l'utiliser dans les descripteurs.
2. Rédiger `service_stream_metadata` comme une ligne statique. Déclarer les types
   de médias des requêtes de catalogue ; réutiliser une liste ancrée.
3. Ajouter les opérations prises en charge. Une catégorie `load_home` doit porter
   le descripteur attendu par `get_category` ; un lien de section doit fonctionner
   avec `get_section` ; un lien d'entrée avec `get_entry.query_url`.
4. Utiliser les identités stables du fournisseur pour les sections si disponibles.
   Ne pas créer de clé stable à partir d'une position ou d'un titre traduit.
5. Pour une série, choisir des épisodes intégrés ou des liens de saisons. Chaque
   lien doit fonctionner avec `get_season`. Pour des lecteurs différés, émettre
   `players_link` et implémenter `get_players`. Ne pas précharger toutes les
   saisons ou pages de lecteurs depuis l'accueil.
6. Choisir un média direct, un résolveur natif enregistré, `stream-resolver` pour
   les hébergeurs externes ou `scraper-query` pour l'extraction YAML de cette
   source. Inclure la cible et la source nécessaires au chemin choisi. Une page
   publique n'est pas une URL de média directement lisible.
7. Utiliser `search_terms` dans les URL et `search_query_json` sans guillemets
   supplémentaires pour insérer une chaîne JSON dans un corps POST.
8. Vérifier première page, continuation, résultats vides et médias indisponibles.
   Les cartes doivent conserver leur source et appeler la bonne opération.
9. Suivre [la spécification de contrôle des sources](arachnea-stream-source-checks-fr.md)
   pour les vérifications existantes de métadonnées et en ligne. Le chargement
   ne qualifie pas la source distante ; distinguer réseau et contrat.

### Requêtes supplémentaires et enveloppes

Les requêtes auxiliaires restent internes à la collection. La navigation des
épisodes utilise les opérations documentées d'entrée, de saison et de lecteurs.

`>` crée des objets JSON imbriqués ; les noms contenant `/`, comme `img/poster`,
restent des clés littérales. Le backend enveloppe la sortie dans un résultat
agrégé contenant `data`, `errors` et les métadonnées de validation applicables.
Écrire dans le YAML les champs du contenu décrits en section 5 ; ne pas ajouter
une enveloppe YAML `data` sauf exigence explicite du contrat de contenu.
Des erreurs partielles peuvent accompagner un résultat ; un contenu vide ne
prouve pas à lui seul la réussite d'une opération.

## 9. API d’administration

Le backend expose une API d’administration indépendante du front de consultation.
En HTTP elle est montée sous `/api/admin/<opération>` (après le root public le cas
échéant) ; en desktop, elle est invocable via le handler Tauri `invoke` sous les
mêmes noms de commande.

### 9.1 Contrôle d’accès

- Les clients desktop sont toujours autorisés.
- En mode serveur, les clients loopback sont autorisés sans authentification.
- Tout autre client doit s’authentifier avec le mot de passe administrateur pour
  obtenir un cookie de session (`Set-Cookie:
  arachnea_admin_session=...; Path=/; HttpOnly; SameSite=Strict`).
- Les écritures doivent utiliser la méthode `POST` ; une requête portant un
  en-tête `Origin` ou `Referer` est refusée quand cette origine ne correspond
  pas à l’en-tête `Host`.
- Les échecs de connexion sont limités par adresse cliente.
- Toute réponse porte `Cache-Control: no-store`.

### 9.2 Opérations

| Opération | Méthode | Objet |
|---|---|---|
| `status` | GET | Mode d’exécution, capacités, authentification requise pour l’appelant et autorisation en cours. |
| `login` | POST | Valide le mot de passe administrateur et émet un cookie de session. |
| `logout` | POST | Invalide la session courante. |
| `services` | GET | Catalogue complet (services désactivés et indisponibles inclus), descriptions localisées, logos résolus depuis les paramètres connus et état d’activation effectif. |
| `set-service-enabled` | POST | Persiste un override d’activation pour un service. |
| `reset-service-enabled` | POST | Supprime l’override et revient au défaut déclaré par le manifest. |
| `credentials` | GET | État des credentials par service (présence et login masqué uniquement). |
| `set-credentials` | POST | Enregistre les credentials d’un service dans le store chiffré. |
| `clear-credentials` | POST | Supprime les credentials stockés d’un service. |
| `settings` | GET | Port, réseau et root effectifs avec leur provenance. |
| `update-settings` | POST | Persiste port/root/réseau, puis les applique à chaud au serveur en cours quand c’est possible (mode serveur) : réponse `applied: true` et `admin_url` si le port ou la racine change ; sinon `restart_required: true`. Détail de la réponse : `{restart_required, applied, apply_error, admin_url}`. |
| `set-admin-password` | POST | Définit ou change le mot de passe administrateur permanent (Argon2id). Lorsqu’un mot de passe permanent est déjà configuré, le mot de passe actuel est obligatoire ; une valeur absente ou incorrecte renvoie une erreur d’authentification. |
| `reload` | POST | Valide puis recharge tous les groupes administrables ; chaque runtime applicable est échangé atomiquement. Le corps doit être l’objet JSON vide `{}`. La réponse conserve `{applied, build_error}` pour le premier groupe et ajoute `groups: [{service_store_id, applied, build_error}]` pour tous les groupes. |

Les paramètres HTTP fournis en ligne de commande (`--server-port`, `--network`,
`--entrypoint-root`) sont masqués individuellement dans le formulaire admin et
absents des requêtes de sauvegarde. `update-settings` ignore ces champs imposés,
même explicitement envoyés, et conserve leurs valeurs persistées existantes.
Les autres paramètres HTTP restent modifiables et sont persistés normalement.

Le rechargement applique les changements d’activation des sources proxy
(`arachnea-proxies`) et IP-country (`arachnea-ip-countries`), même si toutes les
sources proxy étaient désactivées au démarrage. La route stable `/proxy` utilise
le runtime courant ; les requêtes en cours conservent leur runtime initial.
Les listes de proxys restent chargées à la demande pour le routage par pays,
et non immédiatement lors du rechargement. L’inventaire reconstruit réinitialise
son délai de nouvelle consultation fournisseur en mémoire, sans supprimer les
enregistrements persistants.

### 9.3 Format d’erreur

Les erreurs emploient une forme stable : `{"error":{"code":"<code>","message":"<message>"}}`
avec des statuts HTTP tels que `400 bad_request`, `401 unauthorized`,
`403 forbidden`, `404 not_found`, `405 method_not_allowed` et
`429 rate_limited`.


## Métadonnées, filtres et options administrables

Déclarer les langues dans `metadata.languages`. Le tag facultatif
`geo_countries` contient les pays où le service est accessible, sous forme de codes ISO 3166-1
alpha-2 minuscules. Un tag absent ou vide est non renseigné et
ne signifie pas « disponible partout ». Ne pas déduire les restrictions du nom du service.
Ces tags ne changent ni le pays du catalogue ni le routage proxy et ne garantissent pas
la disponibilité de chaque vidéo. Ils sont exposés dans `service_stream_metadata` sous
`metadata` via `{service_metadata}`, sans seconde déclaration dans la requête.

Déclarer les valeurs fixes dans `constants` et les valeurs éditables dans
`parameters`. Définir leurs valeurs par défaut dans le YAML. Les surcharges des
manifestes JSON non vides sont refusées. Le schéma `label`/`editor` est décrit
dans la spécification Scrapyfy.

Le catalogue admin expose `metadata`, `has_parameters` et `configuration_error`. Les lignes de services
affichent les tags entre le titre et la description, avec les libellés localisés des filtres configurés.
L’interrupteur indique l’activation ; les badges Activé/Désactivé sont supprimés.
Les boutons d’identifiants et de paramètres sont affichés côte à côte.
Les logos
sont calculés à partir des constantes et paramètres enregistrés, avec leurs dépendances et actions.
Le bouton Options apparaît seulement si des paramètres éditables existent, même pour une source
désactivée. « Rétablir les valeurs par défaut » restaure le brouillon ; « Enregistrer » confirme
et supprime les surcharges égales aux défauts YAML actuels. « Annuler » ne persiste rien.
La sauvegarde et la réinitialisation demandent ensuite le rechargement du runtime existant.

| Opération admin | Méthode HTTP | Contrat |
|---|---|---|
| `service-parameters` | GET | `service_store_id`, `service_id` obligatoires ; renvoie `parameters` avec nom, libellé, description, `default_value`, `value`, `has_override`, `editor` |
| `save-service-parameters` | POST | Même identité et objet `values` ; remplace atomiquement les surcharges après validation des valeurs et des requêtes ; noms inconnus et constantes refusés |
| `reset-service-parameters` | POST | Même identité ; supprime toutes les surcharges du service après validation des défauts |

Les écritures renvoient `{ "reload_required": true }`. Les mêmes opérations sont enregistrées
sur les transports REST et Tauri et utilisent les contrôles d’accès admin existants.
Les paramètres sont enregistrés dans la nouvelle table `arachnea-services-parameters`, dans
`data/arachnea-services.sqlite3`, sans modifier la table d’activation et d’identifiants.

Dans `front/admin-app/public/config.json`, la section optionnelle `service-filters` associe
chaque type de service à une liste de tags de recherche :

```json
{
  "service-filters": {
    "arachnea-stream": [
      {"key": "languages", "label": "services.filters.languages", "value_format": "language"},
      {"key": "geo_countries", "label": "services.filters.geo_countries"},
      {"key": "content_types", "label": "services.filters.content_types",
       "value_labels": {
         "local_tv": "services.contentTypes.local_tv",
         "movies": "services.contentTypes.movies",
         "series": "services.contentTypes.series",
         "documentaries": "services.contentTypes.documentaries"
       }}
    ]
  }
}
```

`label` est une clé de traduction non vide, résolue par la fonction `t()` existante dans
`front/admin-app/public/locales/*.json`. Les traductions intégrées à la configuration et les
libellés littéraux ne sont pas pris en charge. Les options viennent du catalogue complet du groupe ;
les filtres sont locaux, avec OU entre valeurs d’un tag et ET entre tags et recherche textuelle
(identifiant, titre, description). Ils sont réinitialisés lors d’un changement de groupe.

### Activation groupée des services filtrés

À gauche du champ de recherche, une case permet d’activer ou de désactiver tous les services
affichés après application de la recherche et des filtres de métadonnées. Elle est cochée
si tous les services modifiables affichés sont activés et partiellement cochée si seulement
certains le sont. Un clic sur l’état partiel active les services restants. Les services
indisponibles sont exclus et la case est désactivée en l’absence de résultats modifiables
ou pendant une opération. Les groupes et services masqués ne sont pas modifiés.

L’action capture les résultats visibles et ne modifie que ceux dont l’activation doit changer.
Si des identifiants requis manquent, une confirmation liste les services concernés et propose
d’annuler ou d’activer quand même ; aucun service n’est modifié avant cette confirmation.
Les écritures utilisent les opérations unitaires existantes, dans l’ordre, puis la liste est
actualisée. Le premier échec arrête le traitement : les changements déjà confirmés restent
appliqués, sans transaction globale. Le rechargement manuel reste nécessaire comme pour
les interrupteurs individuels.

Les messages de résultat de rechargement et les confirmations de sauvegarde des paramètres,
du mot de passe administrateur, du quota de profils et des identifiants de services proposent
un bouton Fermer. La fermeture masque le message courant ; une opération ultérieure peut
afficher une nouvelle confirmation.

### Disposition des paramètres et des utilisateurs

Les cartes des paramètres occupent au maximum trois colonnes, avec une largeur minimale
cible de 540 px (+50 % par rapport aux 360 px initiaux), et une seule colonne réduite à la
largeur disponible sur petit écran. Elles s’enchaînent de haut en bas dans chaque colonne,
puis dans la suivante, avec 24 px d’espacement, sans alignement sur des lignes communes :
une carte courte ne laisse donc pas de vide imposé par la hauteur d’une carte voisine.
Chaque carte reste entière. La fenêtre dédiée aux paramètres desktop conserve une colonne. Les formulaires et leurs actions de sauvegarde restent identiques.
La page Utilisateurs affiche le bouton Actualiser en haut à droite, à côté du titre,
comme le tableau de bord ; le bouton Ajouter reste dans la carte des comptes utilisateurs.

### Tableau de bord administrateur

L’accueil admin (`/` redirigé vers `/dashboard`) et la connexion réussie ouvrent le tableau
de bord. Une entrée du menu permet d’y revenir. Il affiche une carte cliquable par groupe
de `service-store`, dans l’ordre configuré, avec une icône de serveur à gauche du nom localisé, le nombre de services
activés / le total et un état. Le compteur utilise le champ `enabled` du catalogue admin ;
il ne vérifie pas la disponibilité réseau et ne garantit pas que le runtime a été rechargé.
Le total inclut les sources déclarées dont le YAML ne peut pas être chargé, sans compter
une seconde fois celles déjà présentes dans le catalogue.

La section optionnelle `service-empty-states` de `front/admin-app/public/config.json`
configure le niveau et le message lorsqu’aucun service du groupe n’est activé :

```json
{
  "service-empty-states": {
    "arachnea-stream": {"severity": "error", "label": "dashboard.empty.stream"},
    "arachnea-stream-hoster": {"severity": "warning", "label": "dashboard.empty.hoster"},
    "arachnea-translation": {"severity": "info", "label": "dashboard.empty.translation"},
    "arachnea-proxies": {"severity": "info", "label": "dashboard.empty.proxies"},
    "arachnea-ip-countries": {"severity": "info", "label": "dashboard.empty.ipCountries"}
  }
}
```

Chaque entrée doit désigner un groupe de `service-store`. `severity` est obligatoire
(`error`, `warning` ou `info`) ; `label` est une clé de traduction optionnelle, non vide
si présente. Sans entrée, un groupe vide affiche Info sans message. Dès qu’un service
est activé, l’état est OK et le message est masqué. Les messages fournis signalent
l’absence de streaming, d’hébergeurs ou de traduction ; pour les proxys, l’impossibilité
de contourner la géolocalisation ; pour les pays IP, la nécessité de préciser le code pays
dans les paramètres. Les traductions françaises et anglaises sont fournies.

Deux avertissements distincts apparaissent si aucun mot de passe administrateur permanent
n’est configuré ou si la liste des comptes utilisateurs est vide. Ils renvoient respectivement
aux paramètres et aux comptes utilisateurs. Le bouton Actualiser recharge les trois sources
indépendamment (catalogue, statut admin et comptes). Une lecture échouée affiche une erreur ;
elle ne produit ni faux compteur à zéro ni avertissement d’absence de comptes ou de mot de passe.
Un groupe absent de la réponse est indiqué comme indisponible.

### Types de contenu et libellés des filtres

Le tag multivalué `content_types` est exposé par les API de métadonnées et proposé
comme filtre admin. Les codes sont les mêmes dans le YAML, les réponses API et les filtres.

| Valeur | Libellé français |
|---|---|
| `local_tv` | TV locale |
| `movies` | Films |
| `series` | Séries |
| `documentaries` | Documentaires |

Une source peut combiner plusieurs types de contenu.

Le champ facultatif `value_labels` d’un filtre associe les valeurs à des clés de
traduction `locales/*`, utilisées dans les sélections et
les tags affichés sur les lignes. Sans correspondance, la valeur brute est affichée. Le champ
facultatif `value_format` n’accepte que `language` : il formate les valeurs brutes avec le
libellé partagé `<drapeau> <nom anglais> - <nom natif>`, dans les sélections comme dans les
tags des lignes. Il est déclaré filtre par filtre afin de laisser intacts les codes pays
comme `fr` ou `de`. Les valeurs de langue sont séparées par `, ` dans les tags, au lieu de ` - `.

Les valeurs d’une même métadonnée sont regroupées dans un seul tag, séparées par ` - `
(ou par `, ` pour un filtre de langues), par exemple « Contenu : Films - Séries - Documentaires ».
À partir du breakpoint `md`, chaque combobox occupe deux colonnes de grille, contre quatre
pour la recherche. Sur les petits écrans, les champs restent empilés et occupent toute la
largeur.

### Schéma de persistance des paramètres

`arachnea-services-parameters` contient une ligne par paramètre : clé primaire
`(service_store_id, source_id, parameter_name)` et colonne chaîne `value`.
Sauvegarde et réinitialisation remplacent ce périmètre atomiquement. La table est
ouverte directement, sans migration des anciennes données JSON. Une clé primaire
existante incompatible est refusée, sans conversion ni suppression de lignes.
Les contrats admin et le mécanisme de rechargement sont conservés.
