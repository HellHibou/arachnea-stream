# M6 Play — Exemple de sortie JSON

Fichier source : `server/services/arachnea-stream/legal-stream/m6play-fr.yaml`

Exemples de sortie JSON **réalistes** pour chaque requête M6 Play, tels que produits par `arachnea-scrapyfy`.

---

## 1. `service_stream_metadata`

```json
[
  {
    "id": "m6play-fr",
    "title": "M6 Play",
    "logo": "https://www.groupem6.fr/app/uploads/sites/3/cache/2024/07/logo-m6-groupe/883637834.png",
    "description": {
      "fr": "M6 Play est la plateforme de streaming du groupe audiovisuel français M6, offrant un large catalogue de programmes télévisés, films, séries, documentaires et contenus exclusifs en ligne."
    }
  }
]
```

---

## 2. `load_home`

```json
[
  {
    "categories": [
      { "key": "m6", "label": "M6", "description": "Replay catalogue for M6.", "request": { "query_url": "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/m6replay/folders?limit=999&offset=0" } },
      { "key": "w9", "label": "W9", "description": "Replay catalogue for W9.", "request": { "query_url": "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/w9replay/folders?limit=999&offset=0" } },
      { "key": "6ter", "label": "6ter", "description": "Replay catalogue for 6ter.", "request": { "query_url": "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/6terreplay/folders?limit=999&offset=0" } },
      { "key": "gulli", "label": "Gulli", "description": "Replay catalogue for Gulli.", "request": { "query_url": "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/gulli/folders?limit=999&offset=0" } }
    ],
    "sections": [
      { "label": "Mes programmes", "link": "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/6play/folders/123/programs?csa=6&with=parentcontext" },
      { "label": "Les nouveautés", "link": "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/6play/folders/456/programs?csa=6&with=parentcontext" }
    ],
    "banners": [
      { "key": "prog_789", "title": "Le Meilleur Pâtissier", "description": "Les candidats s'affrontent autour de créations sucrées.", "image": "https://images.6play.fr/v1/images/abc123/raw", "logo": "https://images.6play.fr/v1/images/logo456/raw", "year": 2026, "release-date": "2026" }
    ]
  }
]
```

---

## 3. `get_category`

Requête : `query_url = "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/m6replay/folders?limit=999&offset=0"`

```json
[
  {
    "sections": [
      { "label": "Séries", "link": "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/6play/folders/111/programs?csa=6&with=parentcontext" },
      { "label": "Films", "link": "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/6play/folders/222/programs?csa=6&with=parentcontext" }
    ]
  }
]
```

---

## 4. `get_section`

Requête : `query_url = "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/6play/folders/111/programs?csa=6&with=parentcontext"`, `page = 1`

```json
[
  {
    "current_page": 1, "have_more": true, "page_size": 18,
    "entries": [
      {
        "source": "m6play-fr", "title": "Scènes de ménages", "description": "Le quotidien humoristique de couples.", "duration": "1500",
        "release-date": "2026-07-09", "media-type": ["video/show/serie"],
        "link": "prog_001", "web-link": "prog_001",
        "img/poster": { "link": "https://images.6play.fr/v1/images/poster001/raw" },
        "img/landscape": { "link": "https://images.6play.fr/v1/images/vignette001/raw" },
        "year": 2026
      },
      {
        "source": "m6play-fr", "title": "Le Meilleur Pâtissier", "description": "Concours de pâtisserie emblématique.", "duration": "5400",
        "release-date": "2026-07-08", "media-type": ["video/show/other"],
        "link": "prog_002", "web-link": "prog_002",
        "img/poster": { "link": "https://images.6play.fr/v1/images/poster002/raw" },
        "img/landscape": { "link": "https://images.6play.fr/v1/images/vignette002/raw" },
        "year": 2026
      }
    ]
  }
]
```

---

## 5. `search`

Requête : `search_terms = "Scènes de ménages"`

```json
[
  {
    "source": "m6play-fr", "current_page": 1, "have_more": false,
    "entries": [
      {
        "title": "Scènes de ménages", "description": "Le quotidien humoristique des couples en appartement.",
        "media-type": ["video/show/serie"], "link": "prog_001",
        "img/poster": { "link": "https://images.6play.fr/v1/images/poster001/raw" },
        "theme": ["Comédie | Humour"], "release-date": "2026-07-09"
      }
    ]
  }
]
```

---

## 6. `get_entry`

Requête : `query_url = "prog_001"`

```json
[
  {
    "title": "Scènes de ménages", "description": "Le quotidien humoristique des couples.",
    "media-type": ["video/show/serie"],
    "img/poster": { "link": "https://images.6play.fr/v1/images/poster001/raw" },
    "img/landscape": { "link": "https://images.6play.fr/v1/images/box001/raw" },
    "img/logo": { "link": "https://images.6play.fr/v1/images/logo001/raw" },
    "year": 2026, "content-advisor": "Tout public",
    "release-date": "2026-01-01", "duration": "1500",
    "link": "https://www.6play.fr/scenes-de-menages-p_prog_001",
    "web-link": "https://www.6play.fr/scenes-de-menages-p_prog_001",
    "seasons": [
      { "label": "Saison 18", "link": "prog_001/videos?subcat=18" },
      { "label": "Saison 19", "link": "prog_001/videos?subcat=19" }
    ]
  }
]
```

---

## 7. `get_season`

Requête : `query_url = "prog_001/videos?subcat=18"`

```json
[
  {
    "current_page": 1, "have_more": false,
    "episodes": [
      {
        "title": "Le retour des voisins", "description": "Les voisins emménagent dans le nouvel appartement.", "duration": "420",
        "season-name": "Saison 18", "release-date": "2026-07-01",
        "players": [
          { "name": "M6+", "resolver": { "kind": "m6play-video", "stream": { "kind": "widevine-license-proxy" }, "target_id": "clip_001" } },
          { "storyboard": { "link": "https://images.6play.fr/v1/images/story001/raw", "width": 200, "height": 112, "columns": 300, "interval": 5 } }
        ],
        "link": "https://www.m6.fr/smd-p_prog_001/smd-c_clip_001",
        "players > embed-link": "https://www.m6.fr/smd-p_prog_001/smd-c_clip_001",
        "img/preview": { "link": "https://images.6play.fr/v1/images/preview001/raw" }
      },
      {
        "title": "Anniversaire surprise", "description": "Catherine organise une fête surprise.", "duration": "420",
        "season-name": "Saison 18", "release-date": "2026-07-02",
        "players": [
          { "name": "M6+", "resolver": { "kind": "m6play-video", "stream": { "kind": "widevine-license-proxy" }, "target_id": "clip_002" } }
        ],
        "link": "https://www.m6.fr/smd-p_prog_001/smd-c_clip_002",
        "players > embed-link": "https://www.m6.fr/smd-p_prog_001/smd-c_clip_002",
        "img/preview": { "link": "https://images.6play.fr/v1/images/preview002/raw" }
      }
    ]
  }
]
```

---

## 8. `list_lives`

```json
[
  {
    "title": "M6", "description": "Direct M6", "key": "M6",
    "link": "M6", "web-link": "https://www.6play.fr/M6/direct",
    "img/poster": { "link": "https://images.6play.fr/v1/images/m6-live/raw" },
    "release-date": "2026-07-09T20:00:00", "expire": "2026-07-09T22:00:00"
  },
  {
    "title": "W9", "description": "Direct W9", "key": "W9",
    "link": "W9", "web-link": "https://www.6play.fr/W9/direct",
    "img/poster": { "link": "https://images.6play.fr/v1/images/w9-live/raw" },
    "release-date": "2026-07-09T20:00:00", "expire": "2026-07-09T22:00:00"
  }
]
```

---

## 9. `get_live`

```json
[
  {
    "players": [
      { "embed-link": "https://www.6play.fr/M6/direct", "resolver": { "kind": "m6play-live", "stream": { "kind": "widevine-license-proxy" }, "target_id": "M6" } }
    ]
  }
]
```

---

## Résumé

| Query | Type | Structure racine | Champs clés |
|---|---|---|---|
| `service_stream_metadata` | static | `object[]` | `id`, `title`, `logo`, `description` |
| `load_home` | json | `object[]` | `categories[]`, `sections[]`, `banners[]` |
| `get_category` | json | `object[]` | `sections[]` (folders) |
| `get_section` | json | `object[]` | `current_page`, `have_more`, `page_size`, `entries[]` |
| `search` | json | `object[]` | `entries[]` (Algolia) |
| `get_entry` | json | `object[]` | `title`, `seasons[]`, images multiples |
| `get_season` | json | `object[]` | `episodes[].players[]` (storyboard, resolver) |
| `list_lives` | json | `object[]` | `entries[]` (via `result_item_field`) |
| `get_live` | json | `object[]` | `players[]` (m6play-live) |

---

*Document généré le 09/07/2026.*