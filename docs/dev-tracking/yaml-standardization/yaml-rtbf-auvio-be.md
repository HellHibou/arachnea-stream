# RTBF Auvio — Exemple de sortie JSON

Fichier source : `server/services/arachnea-stream/legal-stream/rtbf-auvio-be.yaml`

Exemples de sortie JSON **réalistes** pour chaque requête RTBF Auvio, tels que produits par `arachnea-scrapyfy`.

---

## 1. `service_stream_metadata`

```json
[
  {
    "id": "rtbf-auvio-be",
    "title": "RTBF Auvio",
    "logo": "https://auvio.rtbf.be/images/icons/auvio_192x192.png",
    "description": {
      "fr": "RTBF Auvio est la plateforme de streaming du groupe audiovisuel belge RTBF, offrant un large catalogue de programmes télévisés, films, séries, documentaires et contenus exclusifs en ligne."
    }
  }
]
```

---

## 2. `load_home`

```json
[
  {
    "sections": [
      { "label": "Nos recommandations", "link": "/recommendations" },
      { "label": "Les plus regardés", "link": "/trending" }
    ],
    "categories": [
      { "key": "series", "label": "Séries", "image": "https://auvio.rtbf.be/images/series/m.jpg", "request": { "query_url": "https://bff-service.rtbf.be/auvio/v1.23/pages/series" } },
      { "key": "films", "label": "Films", "image": "https://auvio.rtbf.be/images/films/m.jpg", "request": { "query_url": "https://bff-service.rtbf.be/auvio/v1.23/pages/films" } },
      { "key": "sport", "label": "Sport", "image": "https://auvio.rtbf.be/images/sport/m.jpg", "request": { "query_url": "https://bff-service.rtbf.be/auvio/v1.23/pages/sport" } },
      { "key": "info", "label": "Info", "image": "https://auvio.rtbf.be/images/info/m.jpg", "request": { "query_url": "https://bff-service.rtbf.be/auvio/v1.23/pages/info" } },
      { "key": "kids", "label": "Kids", "image": "https://auvio.rtbf.be/images/kids/m.jpg", "request": { "query_url": "https://bff-service.rtbf.be/auvio/v1.23/pages/kids" } }
    ],
    "banners": [
      { "key": "media_123", "title": "Les Traîtres", "subtitle": "Nouvelle saison", "description": "Le jeu de trahison revient pour une nouvelle saison palpitante.", "image": "https://auvio.rtbf.be/images/traitres/xl.jpg", "link": "https://bff-service.rtbf.be/auvio/v1.23/pages/traitres", "web-link": "https://auvio.rtbf.be/traitres", "video": "https://exposure.api.redbee.live/v2/customer/RTBF/businessunit/Auvio/entitlement/media_123/play" },
      { "key": "media_456", "title": "Koh-Lanta", "subtitle": "Épisode 7", "description": "Les aventuriers s'affrontent lors de l'épreuve d'immunité.", "image": "https://auvio.rtbf.be/images/kohlanta/xl.jpg", "link": "https://bff-service.rtbf.be/auvio/v1.23/pages/kohlanta", "web-link": "https://auvio.rtbf.be/kohlanta", "video": "https://exposure.api.redbee.live/v2/customer/RTBF/businessunit/Auvio/entitlement/media_456/play" }
    ]
  }
]
```

---

## 3. `get_category`

Requête : `query_url = "https://bff-service.rtbf.be/auvio/v1.23/pages/series"`

```json
[
  {
    "sections": [
      { "label": "Séries du moment", "link": "/series/trending" },
      { "label": "Séries jeunesse", "link": "/series/kids" }
    ],
    "categories": [
      { "key": "series", "label": "Séries", "image": "https://auvio.rtbf.be/images/series/m.jpg", "request": { "query_url": "https://bff-service.rtbf.be/auvio/v1.23/pages/series" } },
      { "key": "films", "label": "Films", "image": "https://auvio.rtbf.be/images/films/m.jpg", "request": { "query_url": "https://bff-service.rtbf.be/auvio/v1.23/pages/films" } },
      { "key": "sport", "label": "Sport", "image": "https://auvio.rtbf.be/images/sport/m.jpg", "request": { "query_url": "https://bff-service.rtbf.be/auvio/v1.23/pages/sport" } },
      { "key": "info", "label": "Info", "image": "https://auvio.rtbf.be/images/info/m.jpg", "request": { "query_url": "https://bff-service.rtbf.be/auvio/v1.23/pages/info" } },
      { "key": "kids", "label": "Kids", "image": "https://auvio.rtbf.be/images/kids/m.jpg", "request": { "query_url": "https://bff-service.rtbf.be/auvio/v1.23/pages/kids" } }
    ],
    "banners": [
      { "key": "banner_001", "title": "Série Tendances", "subtitle": "Découvrez", "description": "Les séries qui font parler.", "image": "https://auvio.rtbf.be/images/series-banner/xl.jpg", "link": "https://bff-service.rtbf.be/auvio/v1.23/pages/series-tendance" }
    ]
  }
]
```

---

## 4. `get_section`

Requête : `query_url = "https://bff-service.rtbf.be/auvio/v1.23/pages/trending"`, `page = 1`, `page_size = 20`

```json
[
  {
    "current_page": 1, "have_more": true,
    "entries": [
      { "source": "rtbf-auvio-be", "title": "Les Traîtres", "description": "Jeu de trahison et de stratégie.", "link": "https://bff-service.rtbf.be/auvio/v1.23/pages/traitres", "web-link": "https://auvio.rtbf.be/traitres", "media-type": ["video/show/other"], "release-date": "2026-07-09", "duration": "3600", "channel": "La Une", "theme": ["Divertissement"], "img/poster": { "link": "https://auvio.rtbf.be/images/traitres/m.jpg" } },
      { "source": "rtbf-auvio-be", "title": "Koh-Lanta", "description": "Aventure et survie.", "link": "https://bff-service.rtbf.be/auvio/v1.23/pages/kohlanta", "web-link": "https://auvio.rtbf.be/kohlanta", "media-type": ["video/show/other"], "release-date": "2026-07-08", "duration": "5400", "channel": "La Une", "theme": ["Divertissement"], "img/poster": { "link": "https://auvio.rtbf.be/images/kohlanta/m.jpg" } }
    ]
  }
]
```

---

## 5. `search`

Requête : `search_terms = "Traîtres"`

```json
[
  {
    "source": "rtbf-auvio-be", "current_page": 1, "have_more": false,
    "entries": [
      { "source": "rtbf-auvio-be", "id": "media_123", "title": "Les Traîtres", "subtitle": "Saison 3 - Épisode 1", "description": "Les candidats découvrent le château.", "link": "https://bff-service.rtbf.be/auvio/v1.23/pages/traitres-s3e1", "web-link": "https://auvio.rtbf.be/traitres-s3e1", "media-type": ["video/show/other"], "release-date": "2026-07-01", "duration": "3600", "channel": "La Une", "theme": ["Divertissement"], "img/poster": { "link": "https://auvio.rtbf.be/images/traitres-ep1/m.jpg" } }
    ]
  }
]
```

---

## 6. `get_entry`

Requête : `query_url = "https://bff-service.rtbf.be/auvio/v1.23/pages/traitres"`

```json
[
  {
    "title": "Les Traîtres", "title/alt": null,
    "description": "Jeu de trahison et de stratégie où les candidats doivent démasquer les traîtres.",
    "img/poster": { "link": "https://auvio.rtbf.be/images/traitres/xl.jpg" },
    "img/logo": { "link": "https://auvio.rtbf.be/images/traitres/logo/m.jpg" },
    "players": [
      { "resolver": { "kind": "rtbf-auvio-video", "stream": { "kind": "redbee-license-proxy" }, "target_id": "asset_123" } }
    ],
    "web-link": "https://auvio.rtbf.be/traitres",
    "duration": "3600", "lang/subtitles": ["VF"], "lang/audio": ["VF"],
    "release-date": "2026-07-09", "expire": "2027-07-09",
    "theme": ["Divertissement"], "content-advisor": "Tout public",
    "seasons": [
      { "label": "Saison 3", "link": "/traitres/saison-3", "episodes": [
        { "link": "/traitres/s3e1", "title": "Épisode 1 : Bienvenue au château", "description": "Les candidats arrivent au château.", "duration": "3600", "release-date": "2026-07-01", "expire": "2027-07-01",
          "players": [ { "resolver": { "kind": "rtbf-auvio-video", "stream": { "kind": "redbee-license-proxy" }, "target_id": "asset_124" } } ],
          "img/poster": { "link": "https://auvio.rtbf.be/images/traitres-ep1/m.jpg" } }
      ] }
    ]
  }
]
```

---

## 7. `get_season`

Requête : `query_url = "https://bff-service.rtbf.be/auvio/v1.23/pages/traitres/saison-3?_page=1&_limit=20"`

```json
[
  {
    "current_page": 1, "have_more": true,
    "episodes": [
      { "link": "/traitres/s3e1", "title": "Épisode 1 : Bienvenue au château", "description": "Les candidats arrivent au château.", "duration": "3600", "release-date": "2026-07-01", "expire": "2027-07-01",
        "players": [ { "resolver": { "kind": "rtbf-auvio-video", "stream": { "kind": "redbee-license-proxy" }, "target_id": "asset_124" } } ],
        "episode-number": "1", "img/poster": { "link": "https://auvio.rtbf.be/images/traitres-ep1/m.jpg" } },
      { "link": "/traitres/s3e2", "title": "Épisode 2 : La première trahison", "description": "Un premier traître est démasqué.", "duration": "3600", "release-date": "2026-07-08", "expire": "2027-07-08",
        "players": [ { "resolver": { "kind": "rtbf-auvio-video", "stream": { "kind": "redbee-license-proxy" }, "target_id": "asset_125" } } ],
        "episode-number": "2", "img/poster": { "link": "https://auvio.rtbf.be/images/traitres-ep2/m.jpg" } }
    ]
  }
]
```

---

## 8. `list_lives`

```json
[
  { "title": "La Une", "key": "la_une", "media-type": ["video/live"], "link": "/la-une", "web-link": "https://auvio.rtbf.be/la-une", "channel": "La Une", "img/poster": { "link": "https://auvio.rtbf.be/images/la-une/m.jpg" }, "img/landscape": { "link": "https://auvio.rtbf.be/images/la-une/xl.jpg" } },
  { "title": "Tipik", "key": "tipik", "media-type": ["video/live"], "link": "/tipik", "web-link": "https://auvio.rtbf.be/tipik", "channel": "Tipik", "img/poster": { "link": "https://auvio.rtbf.be/images/tipik/m.jpg" }, "img/landscape": { "link": "https://auvio.rtbf.be/images/tipik/xl.jpg" } },
  { "title": "La Trois", "key": "la_trois", "media-type": ["video/live"], "link": "/la-trois", "web-link": "https://auvio.rtbf.be/la-trois", "channel": "La Trois", "img/poster": { "link": "https://auvio.rtbf.be/images/la-trois/m.jpg" }, "img/landscape": { "link": "https://auvio.rtbf.be/images/la-trois/xl.jpg" } }
]
```

---

## 9. `get_live`

```json
[
  {
    "players": [
      { "name": "RTBF Auvio", "resolver": { "kind": "rtbf-auvio-live", "stream": { "kind": "redbee-license-proxy" }, "target_id": "la_une" } }
    ]
  }
]
```

---

## Résumé

| Query | Type | Structure racine | Champs clés |
|---|---|---|---|
| `service_stream_metadata` | static | `object[]` | `id`, `title`, `logo`, `description` |
| `load_home` | json | `object[]` | `sections[]`, `categories[]`, `banners[]` (via sub-queries) |
| `get_category` | json | `object[]` | `sections[]`, `categories[]`, `banners[]` (via sub-queries) |
| `get_section` | json | `object[]` | `current_page`, `have_more`, `entries[]` |
| `search` | json | `object[]` | `entries[]` (pagination via `derive_pagination`) |
| `get_entry` | json | `object[]` | `title`, `players[]` (rtbf-auvio-video), `seasons[].episodes[]` |
| `get_season` | json | `object[]` | `episodes[].players[]` (avec resolver + target_id) |
| `list_lives` | json | `object[]` | `entries[]` (via `result_item_field`) |
| `get_live` | json | `object[]` | `players[]` (rtbf-auvio-live) |

---

*Document généré le 09/07/2026.*