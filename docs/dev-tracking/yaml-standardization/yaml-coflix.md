# Coflix — Exemple de sortie JSON

Fichier source : `server/services/arachnea-stream/dark-stream/coflix.yaml`

Ce document présente des exemples de sortie JSON **réalistes** pour chaque requête définie dans la configuration YAML de Coflix, tels que produits par le moteur `arachnea-scrapyfy` après exécution des sélecteurs HTML/JSON, actions et post-processus.

---

## 1. `service_stream_metadata`

Requête statique qui décrit le service.

```json
[
  {
    "id": "coflix",
    "title": "Coflix",
    "logo": "https://coflix.trade/wp-content/uploads/2023/01/cropped-coflix.png",
    "description": {
      "fr": "Films, Séries et Animés en streaming VF/HD."
    }
  }
]
```

---

## 2. `load_home`

Page d'accueil avec bannières, catégories et sections de contenu.

```json
[
  {
    "source": "coflix",
    "banners": [
      {
        "title": "Deadpool & Wolverine",
        "image": "https://coflix.trade/uploads/banners/deadpool-wolverine.jpg",
        "link": "https://coflix.trade/movies/deadpool-wolverine",
        "web-link": "https://coflix.trade/movies/deadpool-wolverine",
        "description": "Deadpool voyage dans le multivers pour recruter Wolverine et sauver son univers.",
        "theme": ["Action", "Comédie", "Science-Fiction"]
      },
      {
        "title": "House of the Dragon Saison 2",
        "image": "https://coflix.trade/uploads/banners/hotd-s2.jpg",
        "link": "https://coflix.trade/series/house-of-the-dragon",
        "web-link": "https://coflix.trade/series/house-of-the-dragon",
        "description": "La guerre civile fait rage à Westeros entre les Noirs et les Verts.",
        "theme": ["Drame", "Fantastique", "Guerre"]
      }
    ],
    "categories": [
      {
        "key": "movies",
        "label": "Films",
        "link": "https://coflix.trade/wp-json/apiflix/v1/options/?post_type=movies&sort=1"
      },
      {
        "key": "series",
        "label": "Séries",
        "link": "https://coflix.trade/wp-json/apiflix/v1/options/?post_type=series&sort=1"
      },
      {
        "key": "animes",
        "label": "Animés",
        "link": "https://coflix.trade/wp-json/apiflix/v1/options/?post_type=animes&sort=1"
      }
    ],
    "sections": [
      {
        "label": "Derniers films ajoutés",
        "entries": [
          {
            "title": "Deadpool & Wolverine",
            "link": "https://coflix.trade/movies/deadpool-wolverine",
            "web-link": "https://coflix.trade/movies/deadpool-wolverine",
            "img/poster": {
              "link": "http://coflix.trade/uploads/posters/deadpool-wolverine.jpg"
            },
            "year": 2024,
            "rating": 8.5
          },
          {
            "title": "Dune: Part Two",
            "link": "https://coflix.trade/movies/dune-part-two",
            "web-link": "https://coflix.trade/movies/dune-part-two",
            "img/poster": {
              "link": "http://coflix.trade/uploads/posters/dune-2.jpg"
            },
            "year": 2024,
            "rating": 8.9
          }
        ]
      },
      {
        "label": "Dernières séries ajoutées",
        "entries": [
          {
            "title": "House of the Dragon S2",
            "link": "https://coflix.trade/series/house-of-the-dragon",
            "web-link": "https://coflix.trade/series/house-of-the-dragon",
            "img/poster": {
              "link": "http://coflix.trade/uploads/posters/hotd.jpg"
            },
            "year": 2024,
            "rating": 8.2
          }
        ]
      },
      {
        "label": "Derniers animés ajoutés",
        "entries": [
          {
            "title": "Solo Leveling Saison 2",
            "link": "https://coflix.trade/animes/solo-leveling-s2",
            "web-link": "https://coflix.trade/animes/solo-leveling-s2",
            "img/poster": {
              "link": "http://coflix.trade/uploads/posters/solo-leveling-s2.jpg"
            },
            "year": 2026,
            "rating": 9.1
          }
        ]
      }
    ]
  }
]
```

---

## 3. `get_category`

Page de catégorie — API JSON paginée.

Requête exécutée avec : `query_url = "https://coflix.trade/wp-json/apiflix/v1/options/?post_type=movies&sort=1"`, `page = 1`

```json
[
  {
    "source": "coflix",
    "sections": [
      {
        "current_page": 1,
        "have_more": true,
        "page_size": 20,
        "link": "https://coflix.trade/wp-json/apiflix/v1/options/?post_type=movies&sort=1&page=1",
        "entries": [
          {
            "title": "Deadpool & Wolverine",
            "link": "https://coflix.trade/movies/deadpool-wolverine",
            "web-link": "https://coflix.trade/movies/deadpool-wolverine",
            "img/poster": {
              "link": "https://coflix.trade/uploads/posters/deadpool-wolverine.jpg"
            },
            "media-type": "video/movie",
            "year": 2024,
            "description": "Deadpool voyage dans le multivers pour recruter Wolverine et sauver son univers.",
            "rating": 8.5,
            "director": ["Shawn Levy"],
            "casting": ["Ryan Reynolds", "Hugh Jackman", "Emma Corrin"]
          },
          {
            "title": "Dune: Part Two",
            "link": "https://coflix.trade/movies/dune-part-two",
            "web-link": "https://coflix.trade/movies/dune-part-two",
            "img/poster": {
              "link": "https://coflix.trade/uploads/posters/dune-2.jpg"
            },
            "media-type": "video/movie",
            "year": 2024,
            "description": "Paul Atreides rejoint les Fremen pour mener la révolte contre l'Empire.",
            "rating": 8.9,
            "director": ["Denis Villeneuve"],
            "casting": ["Timothée Chalamet", "Zendaya", "Rebecca Ferguson"]
          }
        ]
      }
    ]
  }
]
```

---

## 4. `get_section`

Section paginée (API JSON).

Requête exécutée avec : `query_url = "https://coflix.trade/wp-json/apiflix/v1/options/?post_type=animes&sort=1"`, `page = 2`

```json
[
  {
    "total_pages": 5,
    "source": "coflix",
    "current_page": 2,
    "have_more": true,
    "page_size": 20,
    "entries": [
      {
        "source": "coflix",
        "title": "Attack on Titan",
        "link": "https://coflix.trade/animes/attack-on-titan",
        "web-link": "https://coflix.trade/animes/attack-on-titan",
        "img/poster": {
          "link": "https://coflix.trade/uploads/posters/aot.jpg"
        },
        "media-type": "video/show/anime",
        "year": 2013,
        "description": "Pour se venger de l'humanité qui l'a rejeté, Eren déclenche le Grand Terrassement.",
        "rating": 9.0,
        "director": ["Hajime Isayama"],
        "casting": ["Yuki Kaji", "Marina Inoue", "Yui Ishikawa"]
      },
      {
        "source": "coflix",
        "title": "Demon Slayer",
        "link": "https://coflix.trade/animes/demon-slayer",
        "web-link": "https://coflix.trade/animes/demon-slayer",
        "img/poster": {
          "link": "https://coflix.trade/uploads/posters/demon-slayer.jpg"
        },
        "media-type": "video/show/anime",
        "year": 2019,
        "description": "Tanjiro Kamado cherche un remède pour transformer sa sœur Nezuko en démon.",
        "rating": 8.8,
        "director": ["Haruo Sotozaki"],
        "casting": ["Natsuki Hanae", "Akari Kito", "Hiro Shimono"]
      }
    ]
  }
]
```

---

## 5. `search`

Résultats de recherche (API JSON).

Requête exécutée avec : `search_terms = "Dune"`

```json
[
  {
    "source": "coflix",
    "current_page": 1,
    "have_more": false,
    "entries": [
      {
        "title": "Dune: Part Two",
        "link": "https://coflix.trade/movies/dune-part-two",
        "web-link": "https://coflix.trade/movies/dune-part-two",
        "img/poster": {
          "link": "https://coflix.trade/uploads/posters/dune-2.jpg"
        },
        "media-type": ["video/movie"],
        "year": 2024,
        "description": "Paul Atreides rejoint les Fremen pour mener la révolte contre l'Empire.",
        "rating": 8.9,
        "director": ["Denis Villeneuve"],
        "casting": ["Timothée Chalamet", "Zendaya"]
      },
      {
        "title": "Dune",
        "link": "https://coflix.trade/movies/dune",
        "web-link": "https://coflix.trade/movies/dune",
        "img/poster": {
          "link": "https://coflix.trade/uploads/posters/dune.jpg"
        },
        "media-type": ["video/movie"],
        "year": 2021,
        "description": "Paul Atreides, un jeune homme brillant, doit voyager sur la planète la plus dangereuse de l'univers.",
        "rating": 8.0,
        "director": ["Denis Villeneuve"],
        "casting": ["Timothée Chalamet", "Rebecca Ferguson", "Oscar Isaac"]
      }
    ]
  }
]
```

---

## 6. `get_entry`

Page détaillée d'un programme avec saisons et lecteurs.

Requête exécutée avec : `query_url = "https://coflix.trade/series/house-of-the-dragon"`

```json
[
  {
    "source": "coflix",
    "title": "House of the Dragon",
    "description": "L'histoire de la maison Targaryen, 200 ans avant les événements de Game of Thrones.",
    "img/poster": {
      "link": "https://coflix.trade/uploads/posters/hotd.jpg"
    },
    "year": 2022,
    "seasons": [
      {
        "label": "Saison 1"
      },
      {
        "label": "Saison 2"
      }
    ],
    "players": [
      {
        "embed-link": "https://embed.example.com/player?id=abc123",
        "name": "example.com"
      }
    ],
    "rating": 8.2,
    "casting": ["Emma D'Arcy", "Matt Smith", "Olivia Cooke", "Rhys Ifans"],
    "director": ["Ryan Condal", "George R.R. Martin"],
    "theme": ["Drame", "Fantastique", "Guerre"]
  }
]
```

---

## 7. `get_season`

Liste des épisodes d'une saison avec lecteurs.

Requête exécutée avec : `query_url = "https://coflix.trade/wp-json/apiflix/v1/series/123/1"`

```json
[
  {
    "current_page": 1,
    "have_more": false,
    "episodes": [
      {
        "id": "456",
        "title": "The Heirs of the Dragon",
        "episode-number": "1",
        "img/preview": {
          "link": "https://coflix.trade/uploads/thumbs/hotd-s1e1.jpg"
        },
        "players": [
          {
            "embed-link": "https://embed.example.com/player?id=ep1",
            "name": "example.com"
          }
        ],
        "release-date": "2022-08-21"
      },
      {
        "id": "457",
        "title": "The Rogue Prince",
        "episode-number": "2",
        "img/preview": {
          "link": "https://coflix.trade/uploads/thumbs/hotd-s1e2.jpg"
        },
        "players": [
          {
            "embed-link": "https://embed.example.com/player?id=ep2",
            "name": "example.com"
          }
        ],
        "release-date": "2022-08-28"
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
| `load_home` | html | `object[]` | `banners[]`, `categories[]`, `sections[].entries[]` |
| `get_category` | json | `object[]` | `sections[].entries[]` (rating, director, casting, year) |
| `get_section` | json | `object[]` | `current_page`, `have_more`, `page_size`, `entries[]` |
| `search` | json | `object[]` | `entries[]` (poster, media-type, rating, director, casting) |
| `get_entry` | html | `object[]` | `title`, `seasons[]`, `players[]`, `casting`, `director`, `theme` |
| `get_season` | json | `object[]` | `episodes[].players[]` (embed-link via base64) |

---

*Document généré le 09/07/2026 — Basé sur les spécifications `arachnea-stream-en.md` et `arachnea-scrapyfy-en.md`, ainsi que la configuration `coflix.yaml`.*