# Analyse : Conversion vStream → Arachnea YAML (DarkStream)

> Généré le 2026-05-31 — Mis à jour au fur et à mesure de l'implémentation  
> Source : `docs/plugin.video.vstream-3.9.2/plugin.video.vstream/resources/`  
> Cible : `server/services/darkstream/*.yaml`

---

## 1. Vue d'ensemble

Le plugin vStream définit **70+ sites** dans `sites.json`, dont **~30 actifs**. Chaque site est
décrit par un fichier Python dans `resources/sites/` qui fait des requêtes HTTP impératives,
parse le HTML avec des regex, et alimente un GUI Kodi.

Arachnea utilise une approche **déclarative** : un fichier YAML définit des requêtes, des
sélecteurs CSS, et un pipeline d'actions pour transformer le HTML/JSON en données structurées.

---

## 2. Inventaire des sites actifs

| Identifiant          | Label                   | URL connue                              | Contenu principal              | Complexité | Cloudflare |
|----------------------|-------------------------|-----------------------------------------|-------------------------------|------------|------------|
| `adkami_com`         | Adkami                  | https://www.adkami.com/                 | Animés streaming VF/VOSTFR    | Moyenne    | ?          |
| `animesama`          | Anime Sama              | https://anime-sama.to/                  | Animés / Scans / Webtoons     | **Élevée** | Non        |
| `animeultime`        | Anime Ultime            | http://www.anime-ultime.net/            | Animés VOSTFR                 | Moyenne    | Non        |
| `animesultra`        | Animes Ultra            | https://ww.animesultra.org/             | Animés                        | Faible     | **Non**    |
| `buzzmonclick_com`   | BuzzMonClick            | https://buzzmonclick.io/category/replay-tv/ | Replay TV français        | Moyenne    | ?          |
| `coflix`             | Coflix                  | https://coflix.wales/                   | Films / Séries / Animés (API) | **Élevée** | Non        |
| `cpasmal`            | CpasMal                 | https://www.cpasmal.rip/                | Films / Séries                | Moyenne    | ?          |
| `cpasmieux`          | Cpasmieux               | https://www.cpasmieux.is/               | Films / Séries                | Moyenne    | ?          |
| `darkiworld`         | darkiworld              | https://darkiworld16.com/               | Films / Séries                | Faible     | ?          |
| `dulourd`            | DuLourd                 | https://www.dulourd.cash/               | DDL Films / Séries            | Faible     | ?          |
| `elitegol`           | Elitegol                | https://bolaloca.my/                    | Sports (Football)             | Moyenne    | ?          |
| `filmspourenfants`   | Films pour Enfants      | https://films-pour-enfants.com/         | Films enfants                 | Faible     | Non        |
| `french_stream`      | French-stream           | https://fs18.lol/                       | Films / Séries VF/VOSTFR      | **Élevée** | Non        |
| `frenchanimes`       | French Animes           | https://french-anime.com/               | Animés VOSTFR                 | Moyenne    | Non        |
| `gum_gum_streaming_com` | Gum-Gum-Streaming   | https://gum-gum-streaming.com/          | Films / Séries                | Moyenne    | ?          |
| `kepliz_com`         | Kepliz                  | https://kambad.com/                     | Films / Séries                | Faible     | ?          |
| `livetv`             | Live TV                 | https://livetv873.me/                   | Sports live                   | Moyenne    | Non        |
| `mysteriam`          | MySteriam               | https://www.inmysteriam.fr/             | Films / Séries mystère        | Faible     | ?          |
| `otaku_attitude`     | Otaku-Attitude          | https://www.otaku-attitude.net/         | Animés / Dramas DDL+Stream    | Moyenne    | Non        |
| `skyanimes`          | Sky-Animes              | http://www.sky-animes.com/              | Animés VOSTFR                 | Faible     | Non        |
| `sitedarkibox`       | darkiBox                | https://darkibox.com/                   | Hébergeur vidéo               | Faible     | ?          |
| `wiflix`             | WiFlix                  | https://flemmix.farm/                   | Films / Séries VF             | **Élevée** | **Non**    |
| `witv`               | wiTV                    | https://witv.team/                      | TV Live                       | Faible     | ?          |
| `youtitou_com`       | YouTitou                | https://youtitou.com/                   | Animés enfants                | Faible     | ?          |

### Sites actifs de type utilitaire / debrid (hors scope YAML DarkStream)

| Identifiant        | Label               | Raison d'exclusion                              |
|--------------------|---------------------|--------------------------------------------------|
| `alldebrid`        | AllDebrid           | Service de debrid, pas un site de contenu        |
| `debrid_link`      | Debrid Link         | Service de debrid                                |
| `cloudproxy`       | Proxy cloudflare    | Proxy technique interne                          |
| `dnspython`        | DNS Python          | Configuration DNS interne                        |
| `netu`             | Hoster NETU         | Hébergeur vidéo (hoster), pas source de contenu  |
| `pastebin`         | PasteBin            | Utilitaire                                       |
| `siteonefichier`   | Mon Compte 1Fichier | Compte personnel DDL                             |
| `themoviedb_org`   | The Movie Database  | Métadonnées (TMDB API)                           |
| `topimdb`          | Top IMDB            | Listes IMDB (pas de stream direct)              |

---

## 3. Catégorisation par type de contenu

### 3.1 Films & Séries (VOD généraliste)
- `french_stream`, `wiflix`, `coflix`, `cpasmal`, `cpasmieux`, `gum_gum_streaming_com`,
  `kepliz_com`, `mysteriam`, `darkiworld`, `dulourd`, `filmspourenfants`

### 3.2 Animés & Manga
- `animesama` *(déjà implémenté dans Arachnea — référence principale)*
- `adkami_com`, `animeultime`, `animesultra`, `frenchanimes`, `otaku_attitude`,
  `skyanimes`, `youtitou_com`

### 3.3 Sport & TV Live
- `elitegol`, `livetv`, `witv`, `buzzmonclick_com`

---

## 4. Patterns techniques Python → YAML

### 4.1 Architecture commune vStream

Tous les fichiers Python suivent ce patron :

```
load()                    → page d'accueil du site
showMenuMovies/Series()   → menus par catégorie
showMovies(sSearch='')    → liste de titres (catalogue + résultats de recherche)
showSaisons()             → sélection de saison (séries)
showEpisodes()            → liste d'épisodes
showEpisodeLinks()        → liens de lecture (hosters) par épisode
showMovieLinks()          → liens de lecture (hosters) pour un film
```

### 4.2 Méthodes de parsing et leur équivalent YAML

| Pattern Python (vStream)                            | Action Arachnea YAML                          | Notes                             |
|-----------------------------------------------------|----------------------------------------------|-----------------------------------|
| `oParser.parse(html, 'regex')` → position [0],[1]  | `regex_find_all` + `format: "{1}"`           | Toutes les captures sont mappées  |
| `el.attr('href')`                                   | `get_attribut: href`                         |                                   |
| Texte d'un élément HTML                             | `get_text`                                   |                                   |
| `URL_MAIN + sUrl`                                   | `resolve_url`                                |                                   |
| `json.loads(content)` sur API JSON                  | `scraper_type: json` + `ExtractField`        |                                   |
| `cRequestHandler.setRequestType(1)` (POST)          | Non supporté nativement (limitation actuelle)|                                   |
| Pagination : page suivante depuis HTML              | `regex_find_all` + `derive_pagination`       |                                   |
| Recherche via GET querystring                       | `query_url: "{base_url}/search?q={search_terms}"` | Standard                    |
| Recherche via POST                                  | ⚠️ Limitation — requête POST non supportée    | wiflix, french_stream en POST     |

### 4.3 Patterns HTML les plus courants

La majorité des sites utilisent une structure de cartes de type :
```html
<article class="...">
  <a href="/film/titre">
    <img src="https://..." alt="Titre du film" />
  </a>
  <h2>Titre du film</h2>
  <span class="year">2024</span>
</article>
```

Quelques sites (Coflix, French-Stream) exposent une **API JSON REST** interne :
- `wp-json/apiflix/v1/options/` pour Coflix → `scraper_type: json`
- `engine/ajax/film_api.php` pour French-Stream → `scraper_type: json`

### 4.4 Gestion Cloudflare

Les sites avec `"cloudflare": "True"` dans `sites.json` nécessitent une résolution Cloudflare.
Arachnea gère cela avec `http.mode: auto` dans le YAML.

---

## 5. Mapping des queries Arachnea à implémenter

### 5.1 Règles de décision

**Queries obligatoires (tout site) :**

| Query Arachnea            | Fonction vStream équivalente                          |
|---------------------------|-------------------------------------------------------|
| `service_stream_metadata` | Métadonnées statiques du service                      |
| `search`                  | `showMovies(sSearch=...)` + `showSeries(sSearch=...)`  |
| `get_entry`               | Page de détail film/série (affiche, desc, liens)      |

**`get_season` — conditionnel :**
- À implémenter **uniquement** si `get_entry` ne retourne pas déjà la liste des saisons.
- Si la page de détail contient toutes les saisons → les retourner directement dans `get_entry`
  (évite des appels supplémentaires inutiles).
- Si les saisons sont sur des pages séparées → implémenter `get_season` pour charger une saison
  à la fois.

**`load_home` + `get_category` — recommandés :**
- La majorité des sites ont au minimum des catégories Films / Séries → implémenter les deux.
- Si `get_category` est implémenté, `load_home` **doit** retourner au minimum les catégories.
- `load_home` peut aussi retourner directement les sections avec leur contenu (ex : derniers
  ajouts, populaires) sans passer par `get_section`.

**`get_section` — optionnel, principalement JSON :**
- Sert au chargement dynamique des sections de la page home.
- Rarement utile pour les sites HTML (le contenu est déjà dans la réponse de `load_home`).
- Privilégié pour les APIs JSON qui paginentles sections indépendamment.

### 5.2 Tableau de décision par query

| Query Arachnea            | Quand l'implémenter                                         |
|---------------------------|-------------------------------------------------------------|
| `service_stream_metadata` | Toujours                                                    |
| `search`                  | Toujours                                                    |
| `get_entry`               | Toujours                                                    |
| `get_season`              | Seulement si `get_entry` ne contient pas la liste des saisons |
| `load_home`               | Dès que le site a des catégories ou une page d'accueil utile |
| `get_category`            | Dès que le site a des catégories (films, séries, genres…)   |
| `get_section`             | Principalement pour les APIs JSON avec sections paginées    |

---

## 6. Analyse de complexité détaillée par site prioritaire

### 6.1 `wiflix` — Complexité : Élevée

- **URL** : https://flemmix.farm/
- **Contenu** : Films VF, Séries VF
- **Particularités** :
  - Recherche en **POST** (`do=search&subaction=search&story=...`) avec gestion de cookies → ⚠️ problème
  - Pattern HTML propriétaire : `mov clearfix` + regex multi-groupes
  - URL_MAIN changement fréquent (référencé dans `sites.json`)
  - `cloudflare: False` → OK pour `http.mode: auto`
- **Priorité** : Haute (site populaire français)
- **Stratégie** : Utiliser GET search si disponible, sinon documenter la limitation POST

### 6.2 `french_stream` — Complexité : Très Élevée

- **URL** : https://fs18.lol/ (change souvent)
- **Contenu** : Films VF/VOSTFR, Séries
- **Particularités** :
  - 3 méthodes de parsing selon la version du site (data-newsid, data-ep, episodesData JS)
  - API interne : `engine/ajax/film_api.php?id={id}`, `engine/ajax/get_seasons.php` (POST)
  - Structure d'épisodes en JSON via API
  - Nécessite `site_info: https://fstream.info/` pour trouver la bonne URL
- **Priorité** : Haute
- **Stratégie** : Implémenter search + get_entry + API JSON pour les liens de lecture

### 6.3 `coflix` — Complexité : Élevée (mais favorable)

- **URL** : https://coflix.wales/
- **Contenu** : Films, Séries, Animés
- **Particularités** :
  - Expose une **API WordPress JSON REST** (wp-json/apiflix/v1/) → très favorable pour Arachnea
  - Search via GET : `suggest.php?query={terms}`
  - Pagination propre
  - 3 types de contenu: films, series, animes
- **Priorité** : Haute
- **Stratégie** : `scraper_type: json` pour les listes, `scraper_type: html` pour les détails

### 6.4 `animesama` — Complexité : Élevée (déjà implémenté !)

- **Statut** : ✅ Déjà disponible dans `server/services/anime-sama.yaml`
- La version vStream est moins complète que l'implémentation Arachnea existante.

### 6.5 `otaku_attitude` — Complexité : Moyenne

- **URL** : https://www.otaku-attitude.net/
- **Contenu** : Animés VOSTFR + Dramas + OST
- **Particularités** :
  - Site DDL + streaming, plusieurs sources
  - Recherche via GET : `recherche.html?cat=1&q={terms}`
  - Pagination via scroll infini (paramètre scroll)
- **Priorité** : Moyenne

### 6.6 `frenchanimes` — Complexité : Faible

- **URL** : https://french-anime.com/
- **Contenu** : Animés VOSTFR
- **Priorité** : Moyenne
- **Statut Arachnea** : ✅ Implémenté dans `server/services/darkstream/frenchanimes.yaml`
- **Couverture actuelle** : recherche GET, catégories VF/VOSTFR/films animés, détail d'entrée et liens de lecture extraits depuis le bloc `div.eps`.

### 6.7 `filmspourenfants` — Complexité : Faible

- **URL** : https://films-pour-enfants.com/
- **Contenu** : Films pour enfants
- **Priorité** : Basse

---

## 7. Limitations et défis

### 7.1 Requêtes POST — évolution backend prévue

Plusieurs sites utilisent des **formulaires HTTP POST** pour la recherche, ce que les YAML
Arachnea ne supportent pas encore. Une évolution du backend est prévue.

**Sites bloqués en attente de cette évolution (exclus de la Phase 1 et 2) :**

| Site            | Usage POST                                                        |
|-----------------|-------------------------------------------------------------------|
| `wiflix`        | Recherche via `do=search&subaction=search&story=...` + cookies    |
| `french_stream` | Recherche via `index.php?story=...&do=search&subaction=search`    |

→ Ces sites doivent être documentés dans ce fichier et implémentés une fois le support POST
  disponible côté backend.

### 7.2 JavaScript dynamique
Certains sites chargent leur contenu en JavaScript (SPA). vStream contourne cela en parsant
les variables JS inline (`var episodesData = {...}`). Arachnea utilise `regex_find_all` sur le
body HTML brut — cela fonctionne pour les scripts inline, mais pas pour le contenu chargé via XHR.

### 7.3 URLs instables
Les sites de streaming changent fréquemment d'URL. Le paramètre `base_url` dans le YAML
(`parameters: [{name: base_url, value: ...}]`) permet de gérer cela facilement, de la même
façon que `siteManager().getUrlMain()` dans vStream.

### 7.4 Authentification / Cookies
Certains sites (wiflix) nécessitent la gestion de cookies de session. Non couvert actuellement
(lié à la limitation POST ci-dessus).

### 7.5 Protection Cloudflare
Cloudflare est géré automatiquement par Arachnea (`http.mode: auto`). Il nécessite cependant
une **intervention humaine** lors des tests (résolution du challenge). Les YAML peuvent donc
être écrits normalement ; les tests avec Cloudflare sont réalisés manuellement.

---

## 8. Template YAML recommandé pour DarkStream

### 8.1 Conventions

- `embed-link` : URL brute du lecteur hoster (streamtape, darkibox, etc.) — le front se charge
  de l'afficher en mode embedded.
- `base_url` : toujours en paramètre pour faciliter la mise à jour lors des changements de domaine.
- `http.mode: auto` : systématiquement présent pour la gestion Cloudflare.
- Queries minimum : `service_stream_metadata` + `search` + `get_entry`.
- `get_season` : à n'ajouter que si `get_entry` ne retourne pas la liste des saisons.
- `load_home` + `get_category` : à ajouter dès que le site a des catégories.

### 8.2 `darkstream/services.json`

Ce fichier est **standalone** : il n'est pas chargé automatiquement par le backend. Il sert
de référence pour alimenter manuellement `server/services/services.json`.

```json
[
  {
    "path": "darkstream/coflix.yaml",
    "enabled": true
  }
]
```

### 8.3 Template YAML de base

```yaml
id: {site_identifier}
title: {Site Name}
logo: {logo_url_or_placeholder}
description:
  fr: "{Description courte du site}"

http:
  mode: auto

parameters:
  - name: base_url
    value: {url_from_sites_json}
    description: "Base URL — update on domain change"

shared:
  actions_get_href_resolve: &actions_get_href_resolve
    - type: get_attribut
      argument: href
    - type: resolve_url

  actions_get_src: &actions_get_src
    - type: get_attribut
      argument: src

queries:
  - name: service_stream_metadata
    scraper_type: static
    entries:
      - name: id
        value: "{service_id}"
      - name: title
        value: "{service_title}"
      - name: logo
        value: "{service_logo}"
      - name: description
        value: "{service_description}"

  # -- search ----------------------------------------------------------
  - name: search
    scraper_type: html
    base_url: "{base_url}"
    query_url: "{base_url}/search?q={search_terms}&page={page}"
    media_types:
      - video/movie
      - video/show/serie
    row_selector: "article.card"   # à adapter
    entries:
      - name: title
        selector: "h2"
        actions:
          - type: get_text
      - name: link
        selector: "a"
        actions: *actions_get_href_resolve
      - name: web-link
        selector: "a"
        actions: *actions_get_href_resolve
      - name: img/poster > link
        selector: "img"
        actions: *actions_get_src
      - name: media-type
        actions:
          - type: format_text
            argument: "video/movie"

  # -- get_entry -------------------------------------------------------
  - name: get_entry
    scraper_type: html
    base_url: "{base_url}"
    query_url: "{query_url}"
    media_types:
      - video/movie
      - video/show/serie
    row_selector: "html"
    entries:
      - name: title
        selector: "h1"
        actions:
          - type: get_text
      - name: description
        selector: "div.synopsis"
        actions:
          - type: get_text
      - name: img/poster > link
        selector: "img.poster"
        actions: *actions_get_src
      # Saisons (si présentes sur cette page — sinon implémenter get_season)
      - name: season > label
        selector: "a.season"
        select: all
        actions:
          - type: get_text
      - name: season > link
        selector: "a.season"
        select: all
        actions: *actions_get_href_resolve
      # Liens de lecture directs (films ou épisodes sans saison)
      - name: players > embed-link
        selector: "iframe, a.player"
        select: all
        actions: *actions_get_href_resolve

  # -- load_home (recommandé si le site a des catégories) --------------
  - name: load_home
    scraper_type: html
    base_url: "{base_url}"
    query_url: "{base_url}"
    row_selector: "html"
    entries:
      - name: categories
        entries:
          - name: key
            actions:
              - type: format_text
                argument: "movies"
          - name: label
            actions:
              - type: format_text
                argument: "Films"
          - name: request > query_url
            actions:
              - type: format_text
                argument: "{base_url}/films/"
      - name: categories
        entries:
          - name: key
            actions:
              - type: format_text
                argument: "series"
          - name: label
            actions:
              - type: format_text
                argument: "Séries"
          - name: request > query_url
            actions:
              - type: format_text
                argument: "{base_url}/series/"

  # -- get_category (recommandé si load_home est implémenté) -----------
  - name: get_category
    scraper_type: html
    base_url: "{base_url}"
    query_url: "{query_url}&page={page}"
    media_types:
      - video/movie
      - video/show/serie
    row_selector: "article.card"   # à adapter
    entries:
      - name: title
        selector: "h2"
        actions:
          - type: get_text
      - name: link
        selector: "a"
        actions: *actions_get_href_resolve
      - name: web-link
        selector: "a"
        actions: *actions_get_href_resolve
      - name: img/poster > link
        selector: "img"
        actions: *actions_get_src
```

---

## 9. Plan de travail recommandé

### Phase 1 — Sites GET uniquement, complexité faible à moyenne

| # | Site                    | Fichier YAML cible                                          | Statut |
|---|-------------------------|-------------------------------------------------------------|--------|
| 1 | `coflix`                | `server/services/darkstream/coflix.yaml`                    | ✅ Implémenté |
| 2 | `animesultra`           | `server/services/darkstream/animesultra.yaml`               | ⛔ Bloqué POST — voir Phase 3 |
| 3 | `frenchanimes`          | `server/services/darkstream/frenchanimes.yaml`              | ✅ Implémenté |
| 4 | `filmspourenfants`      | `server/services/darkstream/filmspourenfants.yaml`          | 🔲     |
| 5 | `otaku_attitude`        | `server/services/darkstream/otaku-attitude.yaml`            | 🔲     |
| 6 | `skyanimes`             | `server/services/darkstream/skyanimes.yaml`                 | 🔲     |

### Phase 2 — Sites GET, parsing HTML plus complexe

| # | Site                    | Fichier YAML cible                                          | Statut |
|---|-------------------------|-------------------------------------------------------------|--------|
| 7 | `cpasmieux`             | `server/services/darkstream/cpasmieux.yaml`                 | 🔲     |
| 8 | `cpasmal`               | `server/services/darkstream/cpasmal.yaml`                   | 🔲     |
| 9 | `gum_gum_streaming_com` | `server/services/darkstream/gum-gum-streaming.yaml`         | 🔲     |
| 10 | `adkami_com`           | `server/services/darkstream/adkami.yaml`                    | 🔲     |
| 11 | `animeultime`          | `server/services/darkstream/animeultime.yaml`               | 🔲     |

### Phase 3 — Sites bloqués (nécessitent support POST backend)

| # | Site            | Fichier YAML cible                                    | Bloquant                                    |
|---|-----------------|-------------------------------------------------------|---------------------------------------------|
| 12 | `animesultra`  | `server/services/darkstream/animesultra.yaml`         | Recherche POST (`index.php?do=search`)      |
| 13 | `wiflix`       | `server/services/darkstream/wiflix.yaml`              | Recherche POST + cookies                    |
| 14 | `french_stream` | `server/services/darkstream/french-stream.yaml`      | Recherche POST, API JSON (POST)             |

---

## 10. Références croisées

| Fichier                                                                 | Rôle                                                        |
|-------------------------------------------------------------------------|-------------------------------------------------------------|
| `docs/plugin.video.vstream-3.9.2/.../resources/sites/*.py`             | Implémentation vStream (source de vérité pour les patterns) |
| `docs/plugin.video.vstream-3.9.2/.../resources/sites.json`             | URLs courantes et statut actif/inactif                      |
| `server/services/anime-sama.yaml`                                       | **Référence principale** — YAML le plus complet             |
| `server/services/papystreaming.yaml`                                    | Référence YAML simple (film/série, HTML)                    |
| `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_action.rs`        | Toutes les actions disponibles                              |
| `server/services/darkstream/services.json`                              | Fichier de référence standalone (entrées pour services.json)|
| `server/services/darkstream/*.yaml`                                     | **Destination des nouveaux YAML**                           |

### Actions ScraperAction disponibles (depuis `scraper_action.rs`)

- `get_text` — texte d'un élément HTML
- `get_attribut { argument }` — attribut HTML (href, src, alt, class...)
- `split { argument }` — découpage par séparateur
- `map { argument, default? }` — table de correspondance
- `regex_find_all { pattern, format }` — regex avec groupes capturants
- `get_request_url` — URL de la requête courante
- `get_response_body` — corps brut de la réponse
- `suffix { argument }` — ajout de suffixe
- `max` — valeur maximale parmi les résultats
- `resolve_url` — résolution URL relative → absolue
- `resolve_url_from_parent { levels }` — résolution depuis un ancêtre
- `get_url_host` — extraction du nom d'hôte
- `build_nextjs_data_url { data_root?, route_prefix?, page_path_prefix_to_strip? }` — URL Next.js
- `ratio { argument }` — multiplication numérique
- `format_text { argument }` — template avec placeholders `{param}` et `{}`
- `build_url { base, fields }` — construction URL depuis JSON
- `extract_field { path }` — extraction champ JSON (JSON pointer)
- `get_date { format, months? }` — normalisation de date → ISO
- `normalize_duration` — durée humaine → secondes

---

*Ce document doit être mis à jour au fur et à mesure de l'implémentation des YAML.*
