# Spécification du format YAML arachnea-stream

## 1. Aperçu

Une collection arachnea-stream déclare une source de streaming (plateforme
légale ou catalogue alternatif). Chaque source définit jusqu'à 9 requêtes qui
produisent des données catalogues structurées — pages d'accueil, catégories,
recherche, détails d'entrée, épisodes, directs.

Ces requêtes sont consommées par `StreamScraper` (`stream_scraper.rs`) qui
centralise les appels au moteur `ScraperAgregator` et expose les résultats au
contrôleur.

Ce document ne décrit que les champs et conventions propres au format stream.
La structure générale d'un fichier YAML (`id`, `parameters`, `http`, `queries`,
`entries`, `actions`, `shared`, etc.) est documentée dans
`docs/arachnea-scrapyfy-yaml.md`.

---

## 2. Structure de la collection

Les fichiers YAML sont organisés en sous-dossiers thématiques :

```
services/arachnea-stream/
├── services.json                 # Activation et paramètres par source
├── legal-stream/                 # Plateformes légales (TF1+, M6 Play, etc.)
│   ├── m6play-fr.yaml
│   ├── tf1-fr.yaml
│   ├── francetv.yaml
│   ├── rtlplay-be.yaml
│   └── rtbf-auvio-be.yaml
└── dark-stream/                  # Catalogues alternatifs
    ├── anime-sama.yaml
    ├── coflix.yaml
    ├── frenchanimes.yaml
    └── animeultime.yaml
```

### 2.1 Paramètres et configuration HTTP

Chaque source définit ses propres paramètres (`base_url`, clés d'API,
identifiants de chaîne, limites de pagination). La configuration HTTP peut
être `direct` (APIs sans protection) ou `auto` (sites avec Cloudflare).

```yaml
http:
  mode: direct
  user_agent_profile: firefox
```

---

## 3. Requêtes standard

Chaque source peut implémenter tout ou partie des 9 requêtes suivantes :

| Nom de requête | Type | Description |
|---|---|---|
| `service_stream_metadata` | `static` | Métadonnées du service (nom, logo, thèmes) |
| `load_home` | `json` / `html` | Page d'accueil (catégories, sections, bannières) |
| `get_category` | `json` / `html` | Contenu filtré par chaîne/catégorie |
| `get_section` | `json` / `html` | Section paginée (liste d'entrées) |
| `search` | `json` / `html` | Recherche avec filtres |
| `get_entry` | `json` / `html` | Détail d'une entrée (programme) |
| `get_season` | `json` / `html` | Épisodes d'une saison |
| `list_lives` | `json` / `html` | Liste des chaînes en direct |
| `get_live` | `json` / `html` | Détail d'un direct (joueur) |

---

## 4. Types de médias

Les sources déclarent leurs types de médias via une section `shared` avec un
alias YAML :

```yaml
shared:
  media_types_ma_source: &media_types_ma_source
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
    - name: search_themes
      type: string[]
      value:
        - "Action"
        - "Comedy"
        - "Drama"
```

| Champ | Type | Obligatoire | Description |
|---|---|---|---|
| `id` | `string` | oui | Identifiant unique de la source (« m6play-fr », « tf1-fr ») |
| `title` | `string` | oui | Nom affiché (ex: « M6 Play », « TF1+ ») |
| `logo` | `string` | oui | URL du logo |
| `description` | `object` | oui | Description multilingue (ex: `{ fr: "…", en: "…" }`) |
| `search_themes` | `string[]` | non | Liste des thèmes disponibles pour filtrer la recherche |
| `themes` | `string[]` | non | Variante de `search_themes` |

---

### 5.2 Page d'accueil — `load_home`

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
        - name: entries       # Objets MediaItem
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
| `key` | `string` | Identifiant de la catégorie (ex: `m6`, `tf1`, `all`) |
| `label` | `string` | Nom affiché (ex: « M6 », « TF1 », « Tout le catalogue ») |
| `link` | `string` | URL ou chemin pour les appels suivants |
| `description` | `string` | Description optionnelle |
| `request > query_url` | `string` | URL de l'API pour `get_category` |
| `request > channel` | `string` | Identifiant de chaîne pour les paramètres |
| `request > channel_label` | `string` | Nom de chaîne |
| `request > page_size` | `number` | Taille de page |
| `request > source` | `string` | Source pour les sous-requêtes |

#### Section

| Champ | Type | Description |
|---|---|---|
| `label` | `string` | Titre du rail (ex: « Derniers ajouts », « Tendances ») |
| `entries` | `object[]` | Objets `MediaItem` (voir section suivante) |
| `link` | `string` | URL pour `get_section` (pagination) |

#### Bannière

| Champ | Type | Description |
|---|---|---|
| `key` | `string` | Identifiant interne |
| `title` | `string` | Titre affiché |
| `description` | `string` | Accroche / description |
| `image` | `string` | URL de l'image de fond |
| `logo` | `string` | URL du logo superposé |
| `video` | `string` | URL de la vidéo d'arrière-plan |
| `link` | `string` | URL interne vers `get_entry` |
| `web-link` | `string` | URL publique |
| `subtitle` | `string` | Sous-titre |
| `entryUrl` | `string` | Variante de lien |

---

### 5.3 Objet MediaItem (entrée catalogue)

Structure commune utilisée dans `sections[].entries`, `search.entries`,
`get_category.sections[].entries` et `list_lives.entries`.

| Champ YAML | Type YAML | Obligatoire | Description |
|---|---|---|---|
| `source` | `string` | voir note | Identifiant de la source (injecté automatiquement par le moteur) |
| `title` | `string` | oui | Titre principal |
| `title/alt` | `string` | non | Titre alternatif |
| `link` | `string` | oui | URL interne pour `get_entry` |
| `web-link` | `string` | non | URL publique du contenu |
| `description` | `string` | non | Synopsis / description |
| `media-type` | `string` ou `string[]` | non | Type de média (ex: `video/show/serie`) |
| `duration` | `string` | non | Durée formatée |
| `release-date` | `string` | non | Date de publication (`YYYY-MM-DD`) |
| `expire` | `string` | non | Date d'expiration |
| `year` | `number` | non | Année de production ou copyright |
| `theme` | `string[]` | non | Genres / thèmes |
| `lang` | `string[]` | non | Langues disponibles |
| `audio` | `string[]` | non | Variante de `lang` |
| `language` | `string[]` | non | Variante de `lang` |
| `label` | `string` | non | Étiquette de substitution (fallback titre) |
| `rating` | `number` | non | Note / score |
| `episode > label` | `string` | non | Étiquette d'épisode sur les cartes |
| `season` | `string` | non | Saison |
| `tags` | `string[]` | non | Tags |
| `key` | `string` | non | Identifiant pour les lives |
| `img/poster > link` | `string` ou `string[]` | non | URL(s) du poster |
| `img/portrait > link` | `string` | non | URL portrait |
| `img/landscape > link` | `string` | non | URL paysage |
| `img/preview > link` | `string` | non | URL d'aperçu |
| `img/logo > link` | `string` | non | URL du logo |

> **Note** : le champ `source` est injecté automatiquement par
> `execute_query_async` quand `source_field_name` vaut `Some("source")`.
> Il ne doit pas être défini manuellement.

---

### 5.4 Page catégorie — `get_category`

Reprend la structure de `load_home` (catégories + sections). Les sections
contiennent des `entries` (MediaItem) et des métadonnées de pagination :

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
        - name: entries   # MediaItem[]
    post_process:
      - type: compute_items_field
      - type: derive_pagination
```

---

### 5.5 Section paginée — `get_section`

Retourne une liste d'entrées avec pagination.

| Champ YAML | Type | Description |
|---|---|---|
| `current_page` | `number` | Page courante |
| `have_more` | `boolean` | Indique si plus de pages existent |
| `total_pages` | `number` | Nombre total de pages (consommé par `derive_pagination`) |
| `page_size` | `number` | Taille de page |
| `infer_have_more_from_full_page` | `boolean` | Inférer `have_more` depuis une page pleine |
| `entries` | `object[]` | Objets MediaItem |

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
| `video/trailer` | `string` | URL de la bande-annonce |
| `year` | `number` | Année |
| `release-date` | `string` | Date de sortie |
| `expire` | `string` | Date d'expiration |
| `duration` | `string` | Durée |
| `count-season` | `number` | Nombre de saisons |
| `content-advisor` | `string` | Avertissement (ex: « Déconseillé aux -10 ans ») |
| `language` | `string[]` | Langues |
| `subtitles` | `string[]` | Sous-titres |
| `theme` | `string[]` | Genres |
| `genre` | `string[]` | Genres (variante) |
| `director` | `string[]` | Réalisateurs |
| `casting` | `string[]` | Acteurs |
| `rating` | `number` | Note |
| `program-id` | `string` | Identifiant interne du programme |
| `seasons` | `object[]` | Saisons (voir 5.7.1) |
| `players` | `object[]` | Lecteurs (voir 5.9) |
| `img/poster > link` | `string` | Poster |
| `img/portrait > link` | `string` | Portrait |
| `img/landscape > link` | `string` | Paysage |
| `img/logo > link` | `string` | Logo |
| `img/preview > link` | `string` | Aperçu |

#### 5.7.1 Saison

| Champ YAML | Type | Description |
|---|---|---|
| `label` | `string` | Nom de la saison |
| `link` | `string` | URL pour `get_season` |
| `episodes` | `object[]` | Épisodes pré-chargés (optionnel) |

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
| `duration` | `string` | Durée |
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

---

### 5.9 Lecteur (`players[]`)

Structure utilisée dans `get_entry`, `get_season` et `get_live` pour
décrire les options de lecture.

| Champ YAML | Type | Description |
|---|---|---|
| `name` | `string` | Nom du lecteur |
| `lang` | `string` | Langue de la piste |
| `direct-link` | `string` | Lien direct (média immédiatement lisible) |
| `resolver` | `string` | Identifiant global du résolveur (format plat, ex: `stream-resolver`, `m6play-video`) |
| `target` | `string` | Cible pour le résolveur (URL ou identifiant) |
| `resolver > kind` | `string` | Type de résolveur (format objet legacy, ex: `tf1-video`, `m6play-video`) |
| `resolver > target_id` | `string` | Identifiant cible pour le résolveur (format objet legacy) |
| `resolver > stream > kind` | `string` | Type de flux (legacy, ex: `tf1-license-proxy`, `widevine-license-proxy`) |

Un joueur est soit un `direct-link` (média direct), soit un descripteur
`{ resolver, target }` (résolution par le backend). Les deux champs `resolver`
et `target` peuvent être déclarés en format plat (champs racine) ou en format
objet legacy (`resolver > kind`, `resolver > target_id`).

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

Exemple avec résolveur légal (format objet legacy) :

```yaml
- name: players
  type: object[]
  entries:
    - name: name
      type: string
      actions:
        - type: format_text
          argument: "TF1+"
    - name: resolver
      type: object
      entries:
        - name: kind
          type: string
          actions:
            - type: format_text
              argument: "tf1-video"
        - name: target_id
          type: string
          pointer: /id
```

### 5.9.1 Résolution de flux — `get_stream`

La commande backend `get_stream` reçoit `{ resolver, target }` et retourne
une réponse de type union exclusive :

| Champ | Type | Description |
|---|---|---|
| `stream_url` | `string[]` | URLs de flux média (première = préférée) |
| `manifest_type` | `string` | Type de manifeste (`mpd`, `m3u8`, etc.) |
| `title` | `string` | Titre optionnel extrait par le résolveur |
| `image/title > link` | `string` | Image de titre optionnelle, sérialisée en JSON sous `{ "image/title": { "link": "…" } }` |
| `license_url` | `string` | URL du proxy de licence DRM (optionnel) |
| `license_headers` | `object` | En-têtes HTTP pour la requête de licence |
| `storyboard_vtt_url` | `string` | URL du WebVTT de miniatures du storyboard (optionnel, préféré à `storyboard`) |
| `storyboard` | `object` | Métadonnées du sprite storyboard (optionnel) |
| `embed-link` | `string` | URL de repli iframe (exclusif de `stream_url`) |

Les champs `stream_url` et `embed-link` sont mutuellement exclusifs :
- `stream_url` produit un lecteur vidéo natif
- `embed-link` produit une iframe

Les `stream_headers` du résolveur ne font pas partie de la réponse publique : ils sont intégrés
aux URLs de flux proxy générées.

Lorsqu'un résolveur déclare `storyboard_vtt_url` avec l'action `resolve_url`, `proxy: true` et une
règle `proxy_replace_all`, `get_stream` retourne une URL proxy. Le proxy réécrit alors chaque cue
WebVTT dont la ligne commence par `http://` ou `https://` vers une URL proxy, puis le frontend
convertit les coordonnées `#xywh` en configuration de sprite Video.js. Les VTT qui utilisent une
image sprite unique et des cellules homogènes sont pris en charge. En cas de chargement ou de format
incompatible, le frontend utilise le champ `storyboard` lorsqu'il est disponible.

Le champ `storyboard` contient :

| Champ | Type | Description |
|---|---|---|
| `url` | `string` | URL de l'image sprite ; peut contenir le placeholder littéral `{index}` |
| `width` | `number` | Largeur d'une vignette |
| `height` | `number` | Hauteur d'une vignette |
| `columns` | `number` | Nombre de colonnes dans le sprite |
| `rows` | `number` | Nombre de lignes de vignettes par image sprite |
| `first_page_index` | `number` | Index optionnel de la première image d'un modèle URL `{index}` ; vaut `0` par défaut |
| `interval` | `number` | Intervalle optionnel entre vignettes (secondes) ; le frontend utilise la durée vidéo divisée par `rows × columns` lorsqu'il est absent |

Par exemple, un storyboard à douze colonnes, six lignes et `first_page_index: 1` remplace `{index}`
par `1` pour charger sa première image. Un sprite unique déclare tout de même `rows: 1`.

### 5.9.2 Proxy de licence DRM — `get_drm_license`

La commande binaire `get_drm_license` remplace l'ancienne commande
`get_stream` pour le proxy de licence DRM. Son chemin public est
`/api/get_drm_license/{source}/{token}`.

### 5.9.3 Groupe de résolveurs YAML — `arachnea-stream-resolver`

Le groupe `arachnea-stream-resolver` contient des résolveurs configurables
par YAML pour les hébergeurs externes. Chaque fichier YAML déclare au minimum
`resolve_stream`. Les requêtes de reconnaissance sont optionnelles et servent
à éviter des requêtes réseau inutiles ou à reconnaître une page déjà chargée.

| Requête | Type | Description |
|---|---|---|
| `can_resolve_url` | `static` | Préfiltre optionnel avant tentative directe ; un résultat positif autorise `resolve_stream` à charger l'URL |
| `can_resolve_html` | `static` / `html` | Reconnaissance optionnelle d'un HTML déjà téléchargé avec les paramètres `{url, html}` |
| `resolve_stream` | `html` / `json` / `text` | Extrait le flux média et les métadonnées optionnelles `title`, `image/title > link`, `stream_headers`, `storyboard_vtt_url` et `storyboard` |

La façade `StreamResolver` parcourt les YAML actifs dans l'ordre de `services.json`.
Pour le chemin direct, elle appelle `can_resolve_url` quand la requête existe ; seuls les
services qui répondent positivement tentent alors `resolve_stream` avec chargement réseau.
Si aucun flux direct n'est obtenu, le résolveur télécharge la page une seule fois, vérifie
un `Content-Type` HTML et une taille maximale de 1 Mio, puis appelle `can_resolve_html`
avec `{url, html}`. Le premier service reconnu relance `resolve_stream` avec ce même
HTML en mémoire via `input_html`, sans deuxième GET sur la page d'embed. En l'absence de
service compatible, `get_stream` retourne `{ "embed-link": "<url>" }`.

`stream_headers` est une métadonnée interne du résolveur : elle sert à construire les URLs proxy
et n'est donc volontairement pas sérialisée dans la réponse JSON de `get_stream`.

---

### 5.10 Directs — `list_lives` et `get_live`

#### `list_lives`

Retourne une liste d'objets MediaItem avec `key` et `media-type: video/live` :

| Champ YAML | Description |
|---|---|
| `key` | Identifiant de la chaîne (ex: `M6`, `tf1`) |
| `title` | Nom de la chaîne |
| `link` | Identifiant ou URL pour `get_live` |
| `web-link` | URL publique du direct |
| `release-date` | Début du programme en cours |
| `expire` | Fin du programme en cours |
| `channel` | Nom de la chaîne |

#### `get_live`

Retourne les informations de lecture d'un direct :

| Champ YAML | Description |
|---|---|
| `players` | Tableau de lecteurs (voir 5.9) avec `resolver > kind: "tf1-live"` ou `"m6play-live"` |

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

parameters:
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

## 8. Correspondance côté Rust

Les résultats sont retournés sous forme d'arbres `ScraperDataNode` et
sérialisés en JSON. Les conventions de nommage YAML sont conservées telles
quelles (kebab-case, chemins hiérarchiques avec `>`).

Le point d'entrée côté backend est `StreamScraper` dans
`server/crates/arachnea-stream/src/stream_scraper.rs`. Chaque méthode :

1. Construit une `HashMap<String, String>` de paramètres
2. Appelle `agregator.execute_query_async("arachnea-stream", "<nom_requete>", …)`
3. Retourne les lignes brutes ou fusionnées

Côté frontend, les normalisateurs dans `front/src/services/rustify.ts`
transforment les champs YAML (kebab-case, chemins `img/poster > link`) en
objets TypeScript typés.
