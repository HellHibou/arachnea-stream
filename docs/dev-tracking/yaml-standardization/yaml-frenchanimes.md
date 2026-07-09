# French Animes — Exemple de sortie JSON

Fichier source : `server/services/arachnea-stream/dark-stream/frenchanimes.yaml`

Ce document présente des exemples de sortie JSON **réalistes** pour chaque requête définie dans la configuration YAML de French Animes, tels que produits par le moteur `arachnea-scrapyfy` après exécution des sélecteurs HTML, actions et post-processus.

---

## 1. `service_stream_metadata`

Requête statique qui décrit le service.

```json
[
  {
    "id": "frenchanimes",
    "title": "French Animes",
    "logo": "https://french-anime.com/templates/franime/images/favicon3.png",
    "description": {
      "fr": "Animés VF et VOSTFR en streaming."
    }
  }
]
```

---

## 2. `load_home`

Page d'accueil avec catégories, carrousel et sections de contenu.

```json
[
  {
    "source": "frenchanimes",
    "categories": [
      {
        "key": "vf",
        "label": "Animés VF",
        "link": "https://french-anime.com/animes-vf/"
      },
      {
        "key": "vostfr",
        "label": "Animés VOSTFR",
        "link": "https://french-anime.com/animes-vostfr/"
      },
      {
        "key": "films",
        "label": "Films Animés",
        "link": "https://french-anime.com/films-vf-vostfr/"
      }
    ],
    "sections": [
      {
        "id": "top-carousel",
        "label": "Top Animés",
        "items": [
          {
            "title": "Solo Leveling",
            "title/alt": "俺だけレベルアップな件",
            "link": "https://french-anime.com/solo-leveling",
            "web-link": "https://french-anime.com/solo-leveling",
            "img/poster": {
              "link": "https://french-anime.com/uploads/posters/solo-leveling.jpg"
            },
            "media-type": "video/show/anime"
          },
          {
            "title": "One Piece",
            "title/alt": "ワンピース",
            "link": "https://french-anime.com/one-piece",
            "web-link": "https://french-anime.com/one-piece",
            "img/poster": {
              "link": "https://french-anime.com/uploads/posters/one-piece.jpg"
            },
            "media-type": "video/show/anime"
          }
        ]
      },
      {
        "id": "latest-added",
        "label": "Derniers ajouts",
        "items": [
          {
            "title": "Solo Leveling Episode 13 VOSTFR",
            "link": "https://french-anime.com/solo-leveling-episode-13-vostfr",
            "web-link": "https://french-anime.com/solo-leveling-episode-13-vostfr",
            "media-type": "video/show/anime",
            "lang": ["vostfr"]
          },
          {
            "title": "One Piece Episode 1123 VF",
            "link": "https://french-anime.com/one-piece-episode-1123-vf",
            "web-link": "https://french-anime.com/one-piece-episode-1123-vf",
            "media-type": "video/show/anime",
            "lang": ["vf"]
          }
        ]
      },
      {
        "id": "latest-added-vf",
        "label": "Derniers Animes VF ajoutées",
        "items": [
          {
            "title": "Demon Slayer Épisode 55 VF",
            "link": "https://french-anime.com/demon-slayer-episode-55-vf",
            "web-link": "https://french-anime.com/demon-slayer-episode-55-vf",
            "media-type": "video/show/anime",
            "lang": ["vf"]
          }
        ]
      },
      {
        "id": "top-vostfr",
        "label": "Top VOSTFR",
        "items": [
          {
            "source": "frenchanimes",
            "title": "Attack on Titan",
            "link": "https://french-anime.com/attack-on-titan",
            "web-link": "https://french-anime.com/attack-on-titan",
            "title/alt": "進撃の巨人",
            "img/poster": {
              "link": "https://french-anime.com/uploads/posters/aot.jpg"
            },
            "media-type": "video/show/anime",
            "lang": ["vostfr"]
          }
        ]
      }
    ]
  }
]
```

---

## 3. `get_category`

Page de catégorie paginée.

Requête exécutée avec : `query_url = "https://french-anime.com/animes-vf/"`, `page = 1`

```json
[
  {
    "source": "frenchanimes",
    "sections": [
      {
        "current_page": 1,
        "page_size": 12,
        "have_more": true,
        "link": "https://french-anime.com/animes-vf/page/1/",
        "entries": [
          {
            "source": "frenchanimes",
            "title": "One Piece",
            "link": "https://french-anime.com/one-piece",
            "web-link": "https://french-anime.com/one-piece",
            "img/poster": {
              "link": "https://french-anime.com/uploads/posters/one-piece.jpg"
            },
            "media-type": "video/show/anime",
            "lang": ["vf"]
          },
          {
            "source": "frenchanimes",
            "title": "Solo Leveling",
            "link": "https://french-anime.com/solo-leveling",
            "web-link": "https://french-anime.com/solo-leveling",
            "img/poster": {
              "link": "https://french-anime.com/uploads/posters/solo-leveling.jpg"
            },
            "media-type": "video/show/anime",
            "lang": ["vf"]
          },
          {
            "source": "frenchanimes",
            "title": "Demon Slayer",
            "link": "https://french-anime.com/demon-slayer",
            "web-link": "https://french-anime.com/demon-slayer",
            "img/poster": {
              "link": "https://french-anime.com/uploads/posters/demon-slayer.jpg"
            },
            "media-type": "video/show/anime",
            "lang": ["vf"]
          }
        ]
      }
    ]
  }
]
```

---

## 4. `get_section`

Section paginée (même structure que le catalogue).

Requête exécutée avec : `query_url = "https://french-anime.com/animes-vostfr/"`, `page = 2`

```json
[
  {
    "source": "frenchanimes",
    "current_page": 2,
    "page_size": 12,
    "have_more": true,
    "link": "https://french-anime.com/animes-vostfr/page/2/",
    "entries": [
      {
        "source": "frenchanimes",
        "title": "Jujutsu Kaisen",
        "link": "https://french-anime.com/jujutsu-kaisen",
        "web-link": "https://french-anime.com/jujutsu-kaisen",
        "img/poster": {
          "link": "https://french-anime.com/uploads/posters/jujutsu-kaisen.jpg"
        },
        "media-type": "video/show/anime",
        "lang": ["vostfr"]
      }
    ]
  }
]
```

---

## 5. `search`

Résultats de recherche.

Requête exécutée avec : `search_terms = "Solo"`, `page = 1`

```json
[
  {
    "source": "frenchanimes",
    "current_page": 1,
    "page_size": 10,
    "have_more": false,
    "entries": [
      {
        "source": "frenchanimes",
        "title": "Solo Leveling",
        "link": "https://french-anime.com/solo-leveling",
        "web-link": "https://french-anime.com/solo-leveling",
        "img/poster": {
          "link": "https://french-anime.com/uploads/posters/solo-leveling.jpg"
        },
        "media-type": "video/show/anime",
        "lang": ["vf", "vostfr"]
      }
    ]
  }
]
```

---

## 6. `get_entry`

Page détaillée d'un programme avec épisodes et lecteurs.

Requête exécutée avec : `query_url = "https://french-anime.com/solo-leveling"`

Post-processus : `extract_regex_items` extrait les épisodes depuis le bloc `div.eps`.

```json
[
  {
    "title": "Solo Leveling",
    "title/alt": "俺だけレベルアップな件",
    "description": "Depuis qu'il a obtenu le pouvoir de monter de niveau, Sung Jinwoo chasse des monstres dans les donjons pour devenir plus fort.",
    "img/poster": {
      "link": "https://french-anime.com/uploads/posters/solo-leveling.jpg"
    },
    "year": 2024,
    "theme": ["Action", "Fantasy", "Aventure", "Shônen"],
    "director": ["Shunsuke Nakashige"],
    "casting": ["Taito Ban", "Genta Nakamura", "Haruka Tomatsu"],
    "lang/audio": ["VF", "VOSTFR"],
    "duration": "24 min",
    "seasons": [
      {
        "episodes": [
          {
            "episode-number": "1",
            "title": "Episode 1",
            "players": [
              {
                "embed-link": "https://embed1.example.com/solo-leveling-ep1",
                "name": "embed1.example.com"
              },
              {
                "embed-link": "https://embed2.example.com/solo-leveling-ep1",
                "name": "embed2.example.com"
              }
            ]
          },
          {
            "episode-number": "2",
            "title": "Episode 2",
            "players": [
              {
                "embed-link": "https://embed1.example.com/solo-leveling-ep2",
                "name": "embed1.example.com"
              }
            ]
          },
          {
            "episode-number": "12",
            "title": "Episode 12",
            "players": [
              {
                "embed-link": "https://embed1.example.com/solo-leveling-ep12",
                "name": "embed1.example.com"
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
| `load_home` | html | `object[]` | `categories[]`, `sections[].items[]` (carrousel, listes) |
| `get_category` | html | `object[]` | `sections[].entries[]` (pagination, lang) |
| `get_section` | html | `object[]` | `current_page`, `have_more`, `entries[]` |
| `search` | html | `object[]` | `entries[]` (poster, media-type, lang) |
| `get_entry` | html | `object[]` | `title`, `year`, `theme`, `director`, `casting`, `seasons[].episodes[].players[]` |

---

*Document généré le 09/07/2026 — Basé sur les spécifications `arachnea-stream-en.md` et `arachnea-scrapyfy-en.md`, ainsi que la configuration `frenchanimes.yaml`.*