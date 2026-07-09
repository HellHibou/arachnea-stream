# TF1+ — Exemple de sortie JSON

Fichier source : `server/services/arachnea-stream/legal-stream/tf1-fr.yaml`

Exemples de sortie JSON **réalistes** pour chaque requête TF1+, tels que produits par `arachnea-scrapyfy`.

---

## 1. `service_stream_metadata`

```json
[
  {
    "id": "tf1-fr",
    "title": "TF1+",
    "logo": "https://www.tf1.fr/_next/static/media/tf1-plus-logo-white.107m~.6am38d6.svg",
    "description": {
      "fr": "TF1+ est la plateforme de streaming du groupe audiovisuel français TF1, offrant un large catalogue de programmes télévisés, films, séries, documentaires et contenus exclusifs en ligne."
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
      { "key": "prog_001", "title": "Koh-Lanta", "description": "Les aventuriers s'affrontent.", "image": "https://photos.tf1.fr/kohlanta/banner.jpg", "logo": "https://photos.tf1.fr/kohlanta/logo.png", "link": "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_001%22%7D&programSlug=koh-lanta&channel=tf1", "web-link": "https://www.tf1.fr/tf1/koh-lanta" },
      { "key": "prog_002", "title": "HPI", "description": "Saison 4 de la série à succès.", "image": "https://photos.tf1.fr/hpi/banner.jpg", "logo": "https://photos.tf1.fr/hpi/logo.png", "link": "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_002%22%7D&programSlug=hpi&channel=tf1", "web-link": "https://www.tf1.fr/tf1/hpi" }
    ],
    "categories": [
      { "key": "tf1", "label": "TF1", "request": { "name": "tf1-fr", "source": "tf1-fr", "channel": "tf1", "channel_label": "TF1", "page_size": 10, "query_url": "https://www.tf1.fr/graphql/fr-fr/web?id=483ce0f&variables=%7B%22context%22%3A%7B%22persona%22%3A%22PERSONA_2%22%2C%22application%22%3A%22WEB%22%2C%22device%22%3A%22DESKTOP%22%2C%22os%22%3A%22WINDOWS%22%7D%2C%22filter%22%3A%7B%22channel%22%3A%22tf1%22%7D%2C%22offset%22%3A0%2C%22limit%22%3A10%7D" } },
      { "key": "tmc", "label": "TMC", "request": { "name": "tf1-fr", "source": "tf1-fr", "channel": "tmc", "channel_label": "TMC", "page_size": 10, "query_url": "https://www.tf1.fr/graphql/fr-fr/web?id=483ce0f&variables=%7B%22context%22%3A%7B%22persona%22%3A%22PERSONA_2%22%2C%22application%22%3A%22WEB%22%2C%22device%22%3A%22DESKTOP%22%2C%22os%22%3A%22WINDOWS%22%7D%2C%22filter%22%3A%7B%22channel%22%3A%22tmc%22%7D%2C%22offset%22%3A0%2C%22limit%22%3A10%7D" } },
      { "key": "tfx", "label": "TFX", "request": { "name": "tf1-fr", "source": "tf1-fr", "channel": "tfx", "channel_label": "TFX", "page_size": 10, "query_url": "https://www.tf1.fr/graphql/fr-fr/web?id=483ce0f&variables=%7B%22context%22%3A%7B%22persona%22%3A%22PERSONA_2%22%2C%22application%22%3A%22WEB%22%2C%22device%22%3A%22DESKTOP%22%2C%22os%22%3A%22WINDOWS%22%7D%2C%22filter%22%3A%7B%22channel%22%3A%22tfx%22%7D%2C%22offset%22%3A0%2C%22limit%22%3A10%7D" } },
      { "key": "tf1-series-films", "label": "TF1 Séries Films", "request": { "name": "tf1-fr", "source": "tf1-fr", "channel": "tf1-series-films", "channel_label": "TF1 Séries Films", "page_size": 10, "query_url": "https://www.tf1.fr/graphql/fr-fr/web?id=483ce0f&variables=%7B%22context%22%3A%7B%22persona%22%3A%22PERSONA_2%22%2C%22application%22%3A%22WEB%22%2C%22device%22%3A%22DESKTOP%22%2C%22os%22%3A%22WINDOWS%22%7D%2C%22filter%22%3A%7B%22channel%22%3A%22tf1-series-films%22%7D%2C%22offset%22%3A0%2C%22limit%22%3A10%7D" } },
      { "key": "lci", "label": "LCI", "request": { "name": "tf1-fr", "source": "tf1-fr", "channel": "lci", "channel_label": "LCI", "page_size": 10, "query_url": "https://www.tf1.fr/graphql/fr-fr/web?id=483ce0f&variables=%7B%22context%22%3A%7B%22persona%22%3A%22PERSONA_2%22%2C%22application%22%3A%22WEB%22%2C%22device%22%3A%22DESKTOP%22%2C%22os%22%3A%22WINDOWS%22%7D%2C%22filter%22%3A%7B%22channel%22%3A%22lci%22%7D%2C%22offset%22%3A0%2C%22limit%22%3A10%7D" } },
      { "key": "all", "label": "ALL", "request": { "name": "tf1-fr", "source": "tf1-fr", "channel": "", "channel_label": "ALL", "page_size": 10, "query_url": "https://www.tf1.fr/graphql/fr-fr/web?id=483ce0f&variables=%7B%22context%22%3A%7B%22persona%22%3A%22PERSONA_2%22%2C%22application%22%3A%22WEB%22%2C%22device%22%3A%22DESKTOP%22%2C%22os%22%3A%22WINDOWS%22%7D%2C%22filter%22%3A%7B%22channel%22%3A%22%22%7D%2C%22offset%22%3A0%2C%22limit%22%3A10%7D" } }
    ],
    "sections": [
      { "label": "Nos programmes", "entries": [
        { "source": "tf1-fr", "title": "Koh-Lanta", "media-type": ["video/show/other"], "theme": ["Aventure", "Sport"], "link": "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_001%22%7D&programSlug=koh-lanta&channel=tf1", "web-link": "https://www.tf1.fr/tf1/koh-lanta", "img/poster": { "link": "https://photos.tf1.fr/kohlanta/poster.jpg" }, "img/portrait": { "link": "https://photos.tf1.fr/kohlanta/portrait.jpg" } },
        { "source": "tf1-fr", "title": "HPI", "media-type": ["video/show/serie"], "theme": ["Comédie", "Policier"], "link": "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_002%22%7D&programSlug=hpi&channel=tf1", "web-link": "https://www.tf1.fr/tf1/hpi", "img/poster": { "link": "https://photos.tf1.fr/hpi/poster.jpg" }, "img/portrait": { "link": "https://photos.tf1.fr/hpi/portrait.jpg" } }
      ] }
    ]
  }
]
```

---

## 3. `get_category`

Requête exécutée via `query_url` GraphQL avec filtre canal.

```json
[
  {
    "sections": [
      { "label": "Populaires sur TF1", "entries": [
        { "source": "tf1-fr", "title": "Koh-Lanta", "media-type": ["video/show/other"], "theme": ["Aventure"], "link": "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_001%22%7D&programSlug=koh-lanta&channel=tf1", "web-link": "https://www.tf1.fr/tf1/koh-lanta", "img/poster": { "link": "https://photos.tf1.fr/kohlanta/poster.jpg" }, "img/portrait": { "link": "https://photos.tf1.fr/kohlanta/portrait.jpg" }, "img/landscape": { "link": "https://photos.tf1.fr/kohlanta/thumbnail.jpg" } }
      ] }
    ]
  }
]
```

---

## 4. `search`

Requête : `search_terms = "Koh-Lanta"`

```json
[
  {
    "have_more": false,
    "entries": [
      {
        "source": "tf1-fr", "title": "Koh-Lanta", "description": "Les aventuriers s'affrontent pour la victoire.",
        "theme": ["Aventure"], "media-type": ["video/show/other"],
        "link": "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_001%22%7D&programSlug=koh-lanta&channel=tf1",
        "web-link": "https://www.tf1.fr/tf1/koh-lanta",
        "img/poster": { "link": "https://photos.tf1.fr/kohlanta/poster.jpg" },
        "img/portrait": { "link": "https://photos.tf1.fr/kohlanta/portrait.jpg" },
        "img/landscape": { "link": "https://photos.tf1.fr/kohlanta/thumbnail.jpg" }
      }
    ],
    "entries": [
      {
        "source": "tf1-fr", "title": "Koh-Lanta : L'épreuve ultime", "description": "Dernière épreuve avant la finale.",
        "theme": ["Aventure"], "media-type": ["video/show/other"],
        "link": "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_001%22%7D&programSlug=koh-lanta&channel=tf1",
        "web-link": "https://www.tf1.fr/tf1/koh-lanta/videos/epreuve-finale.html",
        "img/poster": { "link": "https://photos.tf1.fr/kohlanta/epreuve-finale/poster.jpg" }
      }
    ]
  }
]
```

---

## 5. `get_entry`

Requête : `query_url = "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_001%22%7D&programSlug=koh-lanta&channel=tf1"`

```json
[
  {
    "title": "Koh-Lanta", "description": "Les aventuriers s'affrontent dans des épreuves extrêmes.",
    "link": "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_001%22%7D&programSlug=koh-lanta&channel=tf1",
    "web-link": "https://www.tf1.fr/tf1/koh-lanta",
    "media-type": ["video/show/other"], "theme": ["Aventure", "Sport"],
    "seasons": [
      { "label": "Saison 1", "link": "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_001%22%7D&programSlug=koh-lanta&channel=tf1" },
      { "label": "Saison 2", "link": "https://smart-tv.tf1.fr/catalog/fr-fr/smarttv?id=379fec96081ab1a2&variables=%7B%22programId%22%3A%22prog_001%22%7D&programSlug=koh-lanta&channel=tf1" }
    ],
    "players": [
      { "name": "TF1+", "embed-link": "https://www.tf1.fr/tf1/koh-lanta/videos/epreuve-finale.html", "resolver": { "kind": "tf1-video", "stream": { "kind": "tf1-license-proxy" }, "target_id": "clip_001" } }
    ]
  }
]
```

---

## 6. `list_lives`

```json
[
  { "title": "TF1", "key": "tf1", "media-type": ["video/live"], "link": "tf1", "web-link": "https://www.tf1.fr/tf1/direct", "channel": "TF1", "img/poster": { "link": "https://photos.tf1.fr/live/tf1/poster.jpg" }, "img/landscape": { "link": "https://photos.tf1.fr/live/tf1/landscape.jpg" } },
  { "title": "TMC", "key": "tmc", "media-type": ["video/live"], "link": "tmc", "web-link": "https://www.tf1.fr/tmc/direct", "channel": "TMC", "img/poster": { "link": "https://photos.tf1.fr/live/tmc/poster.jpg" } },
  { "title": "TFX", "key": "tfx", "media-type": ["video/live"], "link": "tfx", "web-link": "https://www.tf1.fr/tfx/direct", "channel": "TFX", "img/poster": { "link": "https://photos.tf1.fr/live/tfx/poster.jpg" } },
  { "title": "LCI", "key": "lci", "media-type": ["video/live"], "link": "lci", "web-link": "https://www.tf1.fr/lci/direct", "channel": "LCI", "img/poster": { "link": "https://photos.tf1.fr/live/lci/poster.jpg" } }
]
```

---

## 7. `get_live`

```json
[
  {
    "players": [
      { "name": "TF1+", "resolver": { "kind": "tf1-live", "stream": { "kind": "tf1-license-proxy" }, "target_id": "tf1" } }
    ]
  }
]
```

---

## Résumé

| Query | Type | Structure racine | Champs clés |
|---|---|---|---|
| `service_stream_metadata` | static | `object[]` | `id`, `title`, `logo`, `description` |
| `load_home` | json | `object[]` | `banners[]` (via Android API), `categories[]`, `sections[].entries[]` |
| `get_category` | json | `object[]` | `sections[].entries[]` (programmes par chaîne) |
| `search` | json | `object[]` | `entries[]` (programmes + vidéos, GraphQL) |
| `get_entry` | json | `object[]` | `title`, `players[]` (tf1-video), `seasons[]` |
| `list_lives` | json | `object[]` | `entries[]` (via `result_item_field`) |
| `get_live` | json | `object[]` | `players[]` (tf1-live) |

---

*Document généré le 09/07/2026.*