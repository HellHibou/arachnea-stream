# RTL Play — Exemple de sortie JSON

Fichier source : `server/services/arachnea-stream/legal-stream/rtlplay-be.yaml`

Exemples de sortie JSON **réalistes** pour chaque requête RTL Play, tels que produits par `arachnea-scrapyfy`.

---

## 1. `service_stream_metadata`

```json
[
  {
    "id": "rtlplay-be",
    "title": "RTL Play",
    "logo": "https://www.rtlplay.be/rtlplay/img/rtlplay/logo-large.svg",
    "description": {
      "fr": "RTL Play est la plateforme de streaming du groupe audiovisuel belge RTL Belgium, offrant un large catalogue de programmes télévisés, films, séries, documentaires et contenus exclusifs en ligne."
    }
  }
]
```

---

## 2. `load_home`

```json
[
  {
    "banners": [
      { "key": "prog_001", "title": "Les Apprentis Champions", "subtitle": "Nouvelle saison", "description": "Les célébrités relèvent des défis sportifs.", "image": "https://www.rtlplay.be/images/apprentis-champions/large.jpg", "logo": "https://www.rtlplay.be/images/apprentis-champions/logo.png", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_001", "web-link": "https://www.rtlplay.be/rtlplay/detail/prog_001" }
    ],
    "categories": [
      { "key": "series", "label": "Séries", "image": "https://www.rtlplay.be/images/categories/series.jpg", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/storefronts/series?temsPerSwimlane=20" },
      { "key": "films", "label": "Films", "image": "https://www.rtlplay.be/images/categories/films.jpg", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/storefronts/films?temsPerSwimlane=20" },
      { "key": "divertissement", "label": "Divertissement", "image": "https://www.rtlplay.be/images/categories/divertissement.jpg", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/storefronts/divertissement?temsPerSwimlane=20" }
    ],
    "sections": [
      { "key": "swimlane_001", "label": "Les plus regardés", "entries": [
        { "title": "Les Apprentis Champions", "title/alt": "Saison 2", "description": "Les célébrités s'affrontent.", "key": "prog_001", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_001", "web-link": "https://www.rtlplay.be/rtlplay/detail/prog_001", "media-type": ["video/show/other"], "theme": ["Divertissement"], "year": 2026, "duration": "45 min", "release-date": "2026-07-09", "img/poster": { "link": "https://www.rtlplay.be/images/apprentis-champions/poster.jpg" }, "img/landscape": { "link": "https://www.rtlplay.be/images/apprentis-champions/landscape.jpg" } },
        { "title": "RTL Info", "title/alt": "Journal 19h", "description": "Le journal télévisé de RTL.", "key": "prog_002", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_002", "web-link": "https://www.rtlplay.be/rtlplay/detail/prog_002", "media-type": ["video/news"], "theme": ["Info"], "year": 2026, "duration": "30 min", "img/poster": { "link": "https://www.rtlplay.be/images/rtlinfo/poster.jpg" } }
      ] },
      { "key": "swimlane_002", "label": "Séries à découvrir", "entries": [
        { "title": "Faut pas rêver", "title/alt": null, "description": "Voyage et découverte.", "key": "prog_003", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_003", "web-link": "https://www.rtlplay.be/rtlplay/detail/prog_003", "media-type": ["video/show/documentary"], "theme": ["Documentaire"], "img/poster": { "link": "https://www.rtlplay.be/images/faut-pas-rever/poster.jpg" } }
      ] }
    ]
  }
]
```

---

## 3. `get_category`

Requête : `query_url = "https://lfvp-api.dpgmedia.net/RTL_PLAY/storefronts/series?temsPerSwimlane=20"`

```json
[
  {
    "banners": [
      { "key": "prog_010", "title": "Série du moment", "description": "À ne pas manquer.", "image": "https://www.rtlplay.be/images/serie-moment/large.jpg", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_010", "web-link": "https://www.rtlplay.be/rtlplay/detail/prog_010" }
    ],
    "categories": [
      { "key": "series", "label": "Séries", "image": "https://www.rtlplay.be/images/categories/series.jpg", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/storefronts/series?temsPerSwimlane=20" },
      { "key": "films", "label": "Films", "image": "https://www.rtlplay.be/images/categories/films.jpg", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/storefronts/films?temsPerSwimlane=20" }
    ],
    "sections": [
      { "key": "swimlane_010", "label": "Séries récentes", "entries": [
        { "title": "Faut pas rêver", "key": "prog_003", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_003", "web-link": "https://www.rtlplay.be/rtlplay/detail/prog_003", "media-type": ["video/show/documentary"], "img/poster": { "link": "https://www.rtlplay.be/images/faut-pas-rever/poster.jpg" } }
      ] }
    ]
  }
]
```

---

## 4. `search`

Requête : `search_terms = "Apprentis"`

```json
[
  {
    "source": "rtlplay-be", "current_page": 1, "have_more": false,
    "entries": [
      { "title": "Les Apprentis Champions", "title/alt": "Saison 2", "description": "Les célébrités s'affrontent.", "key": "prog_001", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_001", "web-link": "https://www.rtlplay.be/rtlplay/detail/prog_001", "media-type": ["video/show/other"], "theme": ["Divertissement"], "year": 2026, "duration": "45 min", "release-date": "2026-07-09", "img/poster": { "link": "https://www.rtlplay.be/images/apprentis-champions/poster.jpg" }, "img/landscape": { "link": "https://www.rtlplay.be/images/apprentis-champions/landscape.jpg" } }
    ]
  }
]
```

---

## 5. `get_entry`

Requête : `query_url = "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_001"`

```json
[
  {
    "title": { "label": "Les Apprentis Champions" },
    "title/alt": { "label": "Saison 2" },
    "description": "Des célébrités belges relèvent des défis sportifs pour une bonne cause.",
    "media-type": ["video/show/other"],
    "link": "https://www.rtlplay.be/rtlplay/player/playable_001",
    "web-link": "https://www.rtlplay.be/rtlplay/~prog_001",
    "img/poster": { "link": "https://www.rtlplay.be/images/apprentis-champions/landscape.jpg" },
    "img/landscape": { "link": "https://www.rtlplay.be/images/apprentis-champions/background.jpg" },
    "img/logo": { "link": "https://www.rtlplay.be/images/apprentis-champions/title-art.png" },
    "year": 2026, "duration": "2700", "theme": ["Divertissement"], "genre": ["Divertissement"],
    "casting": ["Catherine", "Thomas", "Sophie"], "director": ["Pierre D."],
    "language": ["VF"], "content-advisor": "Tout public",
    "players": [
      { "name": "RTL Play", "resolver": { "kind": "rtlplay-video", "stream": { "kind": "widevine-license-proxy" }, "target_id": "playable_001" } }
    ],
    "seasons": [
      { "label": "Saison 1", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_001?selectedSeasonIndex=0" },
      { "label": "Saison 2", "link": "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_001?selectedSeasonIndex=1" }
    ]
  }
]
```

---

## 6. `get_season`

Requête : `query_url = "https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/prog_001?selectedSeasonIndex=1"`

```json
[
  {
    "current_page": 1, "have_more": false,
    "episodes": [
      { "title": "Épisode 1 : Le grand départ", "description": "Les célébrités arrivent au camp d'entraînement.", "duration": "2700", "episode-number": "1", "release-date": "2026-07-01", "link": "https://www.rtlplay.be/rtlplay/player/ep_001", "players": [{ "name": "RTL Play", "resolver": { "kind": "rtlplay-video", "stream": { "kind": "widevine-license-proxy" }, "target_id": "ep_001" } }], "img/preview": { "link": "https://www.rtlplay.be/images/apprentis-champions/ep1.jpg" } },
      { "title": "Épisode 2 : Première épreuve", "description": "Les équipes s'affrontent.", "duration": "2700", "episode-number": "2", "release-date": "2026-07-08", "link": "https://www.rtlplay.be/rtlplay/player/ep_002", "players": [{ "name": "RTL Play", "resolver": { "kind": "rtlplay-video", "stream": { "kind": "widevine-license-proxy" }, "target_id": "ep_002" } }], "img/preview": { "link": "https://www.rtlplay.be/images/apprentis-champions/ep2.jpg" } }
    ]
  }
]
```

---

## 7. `list_lives`

```json
[
  { "title": "RTL tvi", "key": "tvi", "channel-id": "1", "media-type": ["video/live"], "link": "tvi", "web-link": "https://www.rtlplay.be/rtlplay/direct/tvi", "img/poster": { "link": "https://www.rtlplay.be/images/live/tvi/poster.jpg" }, "img/landscape": { "link": "https://www.rtlplay.be/images/live/tvi/hero.jpg" } },
  { "title": "RTL club", "key": "club", "channel-id": "2", "media-type": ["video/live"], "link": "club", "web-link": "https://www.rtlplay.be/rtlplay/direct/club", "img/poster": { "link": "https://www.rtlplay.be/images/live/club/poster.jpg" }, "img/landscape": { "link": "https://www.rtlplay.be/images/live/club/hero.jpg" } },
  { "title": "RTL plug", "key": "plug", "channel-id": "3", "media-type": ["video/live"], "link": "plug", "web-link": "https://www.rtlplay.be/rtlplay/direct/plug", "img/poster": { "link": "https://www.rtlplay.be/images/live/plug/poster.jpg" }, "img/landscape": { "link": "https://www.rtlplay.be/images/live/plug/hero.jpg" } },
  { "title": "bel RTL", "key": "bel", "media-type": ["video/live"], "link": "bel", "web-link": "https://www.rtlplay.be/rtlplay/direct/bel" },
  { "title": "Radio Contact", "key": "contact", "media-type": ["video/live"], "link": "contact", "web-link": "https://www.rtlplay.be/rtlplay/direct/contact" },
  { "title": "RTL District", "key": "RTLdistrict", "media-type": ["video/live"], "link": "RTLdistrict", "web-link": "https://www.rtlplay.be/rtlplay/direct/RTLdistrict" }
]
```

---

## 8. `get_live`

```json
[
  {
    "players": [
      { "name": "RTL Play", "resolver": { "kind": "rtlplay-live", "stream": { "kind": "widevine-license-proxy" }, "target_id": "tvi" } }
    ]
  }
]
```

---

## Résumé

| Query | Type | Structure racine | Champs clés |
|---|---|---|---|
| `service_stream_metadata` | static | `object[]` | `id`, `title`, `logo`, `description` |
| `load_home` | json | `object[]` | `banners[]`, `categories[]`, `sections[].entries[]` |
| `get_category` | json | `object[]` | `banners[]`, `categories[]`, `sections[].entries[]` |
| `search` | json | `object[]` | `entries[]` (labels, images multiples) |
| `get_entry` | json | `object[]` | `title`, `players[]` (rtlplay-video), `seasons[]` |
| `get_season` | json | `object[]` | `episodes[].players[]` (rtlplay-video) |
| `list_lives` | json | `object[]` | `entries[]` (via `result_item_field` + `append_static_items`) |
| `get_live` | json | `object[]` | `players[]` (rtlplay-live) |

---

*Document généré le 09/07/2026.*