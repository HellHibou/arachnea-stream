# France TV — Exemple de sortie JSON

Fichier source : `server/services/arachnea-stream/legal-stream/francetv.yaml`

Exemples de sortie JSON **réalistes** pour chaque requête France TV, tels que produits par `arachnea-scrapyfy`.

---

## 1. `service_stream_metadata`

```json
[
  {
    "id": "francetv",
    "title": "France TV",
    "logo": "https://www.france.tv/favicons/favicon-192.png",
    "description": {
      "fr": "France TV est le groupe audiovisuel public français, offrant une large gamme de contenus télévisés, films, séries, documentaires et émissions en ligne..."
    }
  }
]
```

---

## 2. `load_home`

```json
[
  {
    "source": "francetv",
    "categories": [
      { "key": "series-et-fictions", "label": "Séries et fictions", "image": "https://images.france.tv/series/vignette_16x9/w:1024", "request": { "name": "francetv", "source": "francetv", "query_url": "https://api-mobile.yatta.francetv.fr/apps/categories/series-et-fictions?platform=apps&page=0" } },
      { "key": "films", "label": "Films", "image": "https://images.france.tv/films/vignette_16x9/w:1024", "request": { "name": "francetv", "source": "francetv", "query_url": "https://api-mobile.yatta.francetv.fr/apps/categories/films?platform=apps&page=0" } },
      { "key": "documentaires", "label": "Documentaires", "image": "https://images.france.tv/documentaires/vignette_16x9/w:1024", "request": { "name": "francetv", "source": "francetv", "query_url": "https://api-mobile.yatta.francetv.fr/apps/categories/documentaires?platform=apps&page=0" } },
      { "key": "info", "label": "Info", "image": "https://images.france.tv/info/vignette_16x9/w:1024", "request": { "name": "francetv", "source": "francetv", "query_url": "https://api-mobile.yatta.francetv.fr/apps/categories/info?platform=apps&page=0" } },
      { "key": "sport", "label": "Sport", "image": "https://images.france.tv/sport/vignette_16x9/w:1024", "request": { "name": "francetv", "source": "francetv", "query_url": "https://api-mobile.yatta.francetv.fr/apps/categories/sport?platform=apps&page=0" } }
    ],
    "sections": [
      {
        "label": "À la une",
        "have_more": false,
        "entries": [
          {
            "source": "francetv", "title": "Plus belle la vie", "description": "Les habitants du Mistral vivent de nouvelles aventures.", "media-type": ["video/show/serie"],
            "link": "https://api-mobile.yatta.francetv.fr/apps/program/plus-belle-la-vie?platform=apps&page=0",
            "web-link": "https://www.france.tv/plus-belle-la-vie",
            "theme": ["Séries et fictions"], "count-season": 20,
            "img/poster": { "link": "https://images.france.tv/plus-belle-la-vie/vignette_3x4/w:1024" },
            "img/landscape": { "link": "https://images.france.tv/plus-belle-la-vie/background_16x9/w:2500" },
            "img/logo": { "link": "https://images.france.tv/plus-belle-la-vie/logo/w:450" }
          }
        ]
      }
    ]
  }
]
```

---

## 3. `get_category`

Requête : `query_url = "https://api-mobile.yatta.francetv.fr/apps/categories/films?platform=apps&page=0"`

```json
[
  {
    "source": "francetv",
    "sections": [
      {
        "label": "Films à l'affiche",
        "have_more": false,
        "entries": [
          {
            "source": "francetv", "title": "Le Comte de Monte-Cristo", "description": "Une adaptation moderne du chef-d'œuvre d'Alexandre Dumas.",
            "media-type": ["video/movie"], "link": "https://api-mobile.yatta.francetv.fr/apps/program/le-comte-de-monte-cristo?platform=apps&page=0",
            "web-link": "https://www.france.tv/le-comte-de-monte-cristo",
            "theme": ["Films"],
            "img/poster": { "link": "https://images.france.tv/monte-cristo/vignette_3x4/w:1024" }
          }
        ]
      }
    ]
  }
]
```

---

## 4. `search`

Requête : `search_terms = "Monte-Cristo"`

```json
[
  {
    "source": "francetv",
    "have_more": false,
    "entries": [
      {
        "source": "francetv", "title": "Le Comte de Monte-Cristo", "description": "Edmond Dantès, jeune marin, est trahi par ses proches et emprisonné au château d'If.",
        "media-type": ["video/movie"],
        "link": "https://api-mobile.yatta.francetv.fr/apps/program/le-comte-de-monte-cristo?platform=apps&page=0",
        "web-link": "https://www.france.tv/le-comte-de-monte-cristo",
        "theme": ["Films"],
        "img/poster": { "link": "https://images.france.tv/monte-cristo/vignette_3x4/w:1024" }
      }
    ]
  }
]
```

---

## 5. `get_entry`

Requête : `query_url = "https://api-front.yatta.francetv.fr/standard/publish/contents/abc123"`

```json
[
  {
    "title": "Le Comte de Monte-Cristo", "description": "Edmond Dantès, jeune marin, est trahi par ses proches...",
    "media-type": ["video/movie"], "link": "https://www.france.tv/le-comte-de-monte-cristo",
    "web-link": "https://www.france.tv/le-comte-de-monte-cristo",
    "count-season": 1, "theme": ["Films", "Aventure"], "director": ["Alexandre de La Patellière", "Matthieu Delaporte"],
    "casting": ["Pierre Niney", "Bastien Bouillon", "Anaïs Demoustier"],
    "content-advisor": "Tout public", "duration": "10680",
    "players": [
      { "name": "FranceTV", "resolver": { "kind": "francetv-video", "stream": { "kind": "francetv-license-proxy" }, "target_id": "si_12345" } }
    ],
    "img/poster": { "link": "https://images.france.tv/monte-cristo/vignette_3x4/w:1024" },
    "img/landscape": { "link": "https://images.france.tv/monte-cristo/background_16x9/w:2500" },
    "img/logo": { "link": "https://images.france.tv/monte-cristo/logo/w:450" },
    "seasons": [
      {
        "label": "Bande-annonce",
        "episodes": [
          { "title": "Bande-annonce VF", "description": "Découvrez la bande-annonce.", "duration": "150",
            "players": [{ "name": "FranceTV", "resolver": { "kind": "francetv-video", "stream": { "kind": "francetv-license-proxy" }, "target_id": "si_67890" } }],
            "img/preview": { "link": "https://images.france.tv/monte-cristo/vignette_16x9/w:1024" }
          }
        ]
      }
    ]
  }
]
```

---

## 6. `list_lives`

```json
[
  {
    "title": "France 2", "title/alt": "Journal Météo", "description": "Le bulletin météo national.",
    "key": "france2", "media-type": ["video/live"],
    "link": "france2", "web-link": "https://www.france.tv/france2/direct.html",
    "channel": "France 2",
    "img/poster": { "link": "https://images.france.tv/france2/vignette_16x9/w:1024" },
    "img/landscape": { "link": "https://images.france.tv/france2/background_16x9/w:2500" }
  },
  {
    "title": "France 3", "title/alt": "Le 19/20", "description": "Le journal régional.",
    "key": "france3", "media-type": ["video/live"],
    "link": "france3", "web-link": "https://www.france.tv/france3/direct.html",
    "channel": "France 3",
    "img/poster": { "link": "https://images.france.tv/france3/vignette_16x9/w:1024" },
    "img/landscape": { "link": "https://images.france.tv/france3/background_16x9/w:2500" }
  }
]
```

---

## 7. `get_live`

```json
[
  {
    "players": [
      { "name": "FranceTV", "resolver": { "kind": "francetv-live", "stream": { "kind": "francetv-license-proxy" }, "target_id": "france2" } }
    ]
  }
]
```

---

## Résumé

| Query | Type | Structure racine | Champs clés |
|---|---|---|---|
| `service_stream_metadata` | static | `object[]` | `id`, `title`, `logo`, `description` |
| `load_home` | json | `object[]` | `categories[]`, `sections[].entries[]` (program/collection/category/event/video) |
| `get_category` | json | `object[]` | `sections[].entries[]` |
| `search` | json | `object[]` | `entries[]` |
| `get_entry` | json | `object[]` | `title`, `players[]` (francetv-video), `seasons[].episodes[]`, images multiples |
| `list_lives` | json | `object[]` | `entries[]` (via `result_item_field`) |
| `get_live` | json | `object[]` | `players[]` (francetv-live) |

---

*Document généré le 09/07/2026.*