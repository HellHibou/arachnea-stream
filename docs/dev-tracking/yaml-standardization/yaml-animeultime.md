# Anime Ultime — Exemple de sortie JSON

Fichier source : `server/services/arachnea-stream/dark-stream/animeultime.yaml`

Ce document présente des exemples de sortie JSON **réalistes** pour chaque requête définie dans la configuration YAML d'Anime Ultime, tels que produits par le moteur `arachnea-scrapyfy` après exécution des sélecteurs HTML, actions et post-processus.

---

## 1. `service_stream_metadata`

Requête statique qui décrit le service.

```json
[
  {
    "id": "animeultime",
    "title": "Anime Ultime",
    "logo": "http://www.anime-ultime.net/favicon.ico",
    "description": {
      "fr": "Animés, Dramas et Tokusatsu en streaming et Direct Download."
    }
  }
]
```

---

## 2. `load_home`

Page d'accueil avec catégories, derniers ajouts et classements.

```json
[
  {
    "source": "animeultime",
    "categories": [
      {
        "key": "animes",
        "label": "Animés",
        "link": "http://www.anime-ultime.net/series-0-1/anime/0---"
      },
      {
        "key": "dramas",
        "label": "Dramas",
        "link": "http://www.anime-ultime.net/series-0-1/drama/0---"
      },
      {
        "key": "tokusatsu",
        "label": "Tokusatsu",
        "link": "http://www.anime-ultime.net/series-0-1/tokusatsu/0---"
      }
    ],
    "sections": [
      {
        "label": "Derniers ajouts",
        "id": "latest-additions",
        "items": [
          {
            "title": "One Piece Episode 1123",
            "link": "http://www.anime-ultime.net/read-12345",
            "web-link": "http://www.anime-ultime.net/read-12345",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/one-piece.jpg"
            },
            "media-type": "video/show/anime",
            "source": "animeultime"
          },
          {
            "title": "Solo Leveling Episode 13",
            "link": "http://www.anime-ultime.net/read-12346",
            "web-link": "http://www.anime-ultime.net/read-12346",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/solo-leveling.jpg"
            },
            "media-type": "video/show/anime",
            "source": "animeultime"
          },
          {
            "title": "Kamen Rider Gotchard Episode 45",
            "link": "http://www.anime-ultime.net/read-12347",
            "web-link": "http://www.anime-ultime.net/read-12347",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/kamen-rider.jpg"
            },
            "media-type": "video/show/serie",
            "source": "animeultime"
          }
        ]
      },
      {
        "label": "Top animes",
        "id": "top-animes",
        "items": [
          {
            "title": "One Piece",
            "link": "http://www.anime-ultime.net/serie-1234",
            "web-link": "http://www.anime-ultime.net/serie-1234",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/one-piece.jpg"
            },
            "source": "animeultime",
            "media-type": "video/show/serie"
          },
          {
            "title": "Attack on Titan",
            "link": "http://www.anime-ultime.net/serie-1235",
            "web-link": "http://www.anime-ultime.net/serie-1235",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/aot.jpg"
            },
            "source": "animeultime",
            "media-type": "video/show/serie"
          },
          {
            "title": "Demon Slayer",
            "link": "http://www.anime-ultime.net/serie-1236",
            "web-link": "http://www.anime-ultime.net/serie-1236",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/demon-slayer.jpg"
            },
            "source": "animeultime",
            "media-type": "video/show/serie"
          }
        ]
      },
      {
        "label": "Top dramas",
        "id": "top-dramas",
        "items": [
          {
            "title": "Crash Landing on You",
            "link": "http://www.anime-ultime.net/serie-5678",
            "web-link": "http://www.anime-ultime.net/serie-5678",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/crash-landing.jpg"
            },
            "source": "animeultime",
            "media-type": "video/show/serie"
          }
        ]
      },
      {
        "label": "Top tokusatsu",
        "id": "top-tokusatsu",
        "items": [
          {
            "title": "Kamen Rider Revice",
            "link": "http://www.anime-ultime.net/serie-9012",
            "web-link": "http://www.anime-ultime.net/serie-9012",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/kamen-rider-revice.jpg"
            },
            "source": "animeultime",
            "media-type": "video/show/serie"
          }
        ]
      }
    ]
  }
]
```

---

## 3. `get_category`

Page de catégorie — liste paginée des entrées.

Requête exécutée avec : `query_url = "http://www.anime-ultime.net/series-0-1/anime/0---"`

```json
[
  {
    "source": "animeultime",
    "sections": [
      {
        "entries": [
          {
            "title": "One Piece",
            "description": "Luffy et son équipage poursuivent leur aventure",
            "link": "http://www.anime-ultime.net/serie-1234",
            "web-link": "http://www.anime-ultime.net/serie-1234",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/one-piece.jpg"
            },
            "media-type": "video/show/anime",
            "rating": 9.2,
            "source": "animeultime"
          },
          {
            "title": "Attack on Titan",
            "description": "Pour se venger de l'humanité, Eren déclenche le Grand Terrassement",
            "link": "http://www.anime-ultime.net/serie-1235",
            "web-link": "http://www.anime-ultime.net/serie-1235",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/aot.jpg"
            },
            "media-type": "video/show/anime",
            "rating": 9.0,
            "source": "animeultime"
          },
          {
            "title": "Demon Slayer: Kimetsu no Yaiba",
            "description": "Tanjiro cherche un remède pour sa sœur devenue démon",
            "link": "http://www.anime-ultime.net/serie-1236",
            "web-link": "http://www.anime-ultime.net/serie-1236",
            "img/poster": {
              "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/demon-slayer.jpg"
            },
            "media-type": "video/show/anime",
            "rating": 8.8,
            "source": "animeultime"
          }
        ]
      }
    ]
  }
]
```

---

## 4. `search`

Résultats de recherche.

Requête exécutée avec : `search_terms = "Solo Leveling"`

```json
[
  {
    "source": "animeultime",
    "entries": [
      {
        "title": "Solo Leveling",
        "link": "http://www.anime-ultime.net/serie-7890",
        "web-link": "http://www.anime-ultime.net/serie-7890",
        "img/poster": {
          "link": "http://www.anime-ultime.net/img_resize.php?img=/covers/solo-leveling.jpg"
        },
        "media-type": "video/show/anime",
        "source": "animeultime"
      }
    ]
  }
]
```

---

## 5. `get_entry`

Page détaillée d'un programme avec playlist XML (épisodes avec liens direct download).

Requête exécutée avec : `query_url = "http://www.anime-ultime.net/serie-1234"`

Post-processus : `fetch_regex_items_from_items` extrait les épisodes depuis le flux XML de la playlist.

```json
[
  {
    "title": "One Piece",
    "title/alt": "ワンピース",
    "year": 1999,
    "director": ["Toei Animation"],
    "theme": ["Action", "Aventure", "Shônen"],
    "description": "Luffy, un jeune garçon doté d'un corps élastique, rêve de devenir le Roi des Pirates. Il parcourt les mers en quête du One Piece, le trésor légendaire.",
    "img/poster": {
      "link": "http://www.anime-ultime.net/covers/one-piece.jpg"
    },
    "seasons": [
      {
        "episodes": [
          {
            "title": "1 - Le départ",
            "episode-number": 1,
            "release-date": "2026-07-09",
            "duration": "1440",
            "img/preview": {
              "link": "http://www.anime-ultime.net/thumbs/op-001.jpg"
            },
            "link": "http://www.anime-ultime.net/info-0-1/12345#stream",
            "players": [
              {
                "name": "HD",
                "direct-link": "https://vid.example.com/op001-hd.mp4"
              },
              {
                "name": "SD",
                "direct-link": "https://vid.example.com/op001-sd.mp4"
              }
            ]
          },
          {
            "title": "2 - Le chapeau de paille",
            "episode-number": 2,
            "release-date": "2026-07-09",
            "duration": "1440",
            "img/preview": {
              "link": "http://www.anime-ultime.net/thumbs/op-002.jpg"
            },
            "link": "http://www.anime-ultime.net/info-0-1/12346#stream",
            "players": [
              {
                "name": "HD",
                "direct-link": "https://vid.example.com/op002-hd.mp4"
              },
              {
                "name": "SD",
                "direct-link": "https://vid.example.com/op002-sd.mp4"
              }
            ]
          }
        ]
      }
    ]
  }
]
```

---

## Résumé des structures produites

| Query | Type | Structure racine | Champs clés |
|---|---|---|---|
| `service_stream_metadata` | static | `object[]` | `id`, `title`, `logo`, `description` |
| `load_home` | html | `object[]` | `categories[]`, `sections[].items[]` |
| `get_category` | html | `object[]` | `sections[].entries[]` (rating, media-type) |
| `search` | html | `object[]` | `entries[]` (poster, media-type) |
| `get_entry` | html | `object[]` | `title`, `year`, `director`, `theme`, `seasons[].episodes[].players[]` (HD/SD direct-link) |

---

*Document généré le 09/07/2026 — Basé sur les spécifications `arachnea-stream-en.md` et `arachnea-scrapyfy-en.md`, ainsi que la configuration `animeultime.yaml`.*