# Anime-Sama — Exemple de sortie JSON

Fichier source : `server/services/arachnea-stream/dark-stream/anime-sama.yaml`

Ce document présente des exemples de sortie JSON **réalistes** pour chaque requête définie dans la configuration YAML d'Anime-Sama, tels que produits par le moteur `arachnea-scrapyfy` après exécution des sélecteurs CSS, actions et post-processus.

---

## 1. `service_stream_metadata`

Requête statique qui décrit le service.

```json
[
  {
    "id": "anime-sama",
    "title": "Anime-Sama",
    "logo": "https://raw.githubusercontent.com/Anime-Sama/IMG/img/autres/logo.png",
    "description": {
      "fr": "Anime-Sama est une plateforme de streaming et de lecture en ligne dédiée aux animés, mangas et webtoons. Elle propose un large catalogue de contenus, allant des dernières sorties aux classiques du genre, avec des options de filtrage par type de média, thème et langue."
    },
    "search_themes": [
      "Action",
      "Adolescence",
      "Friendship",
      "Love",
      "Martial Arts",
      "Assassination",
      "Other World",
      "Adventure",
      "Fighting",
      "Comedy",
      "Crime",
      "Demons",
      "Donghua",
      "Drama",
      "Ecchi",
      "School",
      "Family",
      "Fantasy",
      "Ghibli",
      "War",
      "Harem",
      "Historical",
      "Horror",
      "Isekai",
      "Games",
      "Josei",
      "Magic",
      "Mecha",
      "Military",
      "Monsters",
      "Music",
      "Mystery",
      "Nostalgia",
      "Politics",
      "Psychological",
      "Daily Life",
      "Romance",
      "Samurai",
      "School Life",
      "Science Fiction",
      "Seinen",
      "Shojo",
      "Shonen",
      "Slice of Life",
      "Sports",
      "Supernatural",
      "Thriller",
      "Tournaments",
      "Work",
      "Vampires",
      "Revenge",
      "Time Travel"
    ]
  }
]
```

---

## 2. `load_home`

Page d'accueil avec bannières, catégories et sections de contenu.

```json
[
  {
    "banners": [
      {
        "key": "anime-sama-season-banner",
        "title": "Les animés de l'été 2026",
        "image": "https://anime-sama.to/assets/banners/ete-2026.webp",
        "link": "https://anime-sama.to/catalogue/summer-2026",
        "web-link": "https://anime-sama.to/catalogue/summer-2026",
        "description": "Découvrez les nouveautés de la saison estivale 2026 : nouvelles séries, suites attendues et films inédits."
      }
    ],
    "categories": [
      {
        "key": "Animés",
        "label": "Animés",
        "link": "https://anime-sama.to/catalogue/?type%5B%5D=Anime"
      },
      {
        "key": "manga",
        "label": "Scans",
        "link": "https://anime-sama.to/catalogue/?type%5B%5D=Scans"
      },
      {
        "key": "webtoon",
        "label": "Webtoon",
        "link": "https://anime-sama.to/catalogue/?type%5B%5D=Webtoon"
      }
    ],
    "sections": [
      {
        "label": "Derniers contenus sortis",
        "entries": [
          {
            "title": "Solo Leveling",
            "title/alt": "俺だけレベルアップな件",
            "media-type": "video/show/anime",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/solo-leveling.webp"
            },
            "description": "Depuis qu'il a obtenu le pouvoir de monter de niveau, Sung Jinwoo chasse des monstres dans les donjons pour devenir plus fort. Mais un jour, une mission tourne au drame...",
            "link": "https://anime-sama.to/catalogue/solo-leveling",
            "web-link": "https://anime-sama.to/catalogue/solo-leveling",
            "theme": ["Action", "Fantasy", "Adventure"],
            "lang": ["VF", "VOSTFR"]
          },
          {
            "title": "One Piece",
            "title/alt": null,
            "media-type": "video/show/anime",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/one-piece.webp"
            },
            "description": "Luffy et son équipage poursuivent leur voyage vers le One Piece à travers le Nouveau Monde.",
            "link": "https://anime-sama.to/catalogue/one-piece",
            "web-link": "https://anime-sama.to/catalogue/one-piece",
            "theme": ["Action", "Adventure", "Comedy", "Shonen"],
            "lang": ["VF", "VOSTFR", "VO"]
          },
          {
            "title": "Jujutsu Kaisen",
            "title/alt": "呪術廻戦",
            "media-type": "video/show/anime",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/jujutsu-kaisen.webp"
            },
            "description": "Yuji Itadori avale un doigt maudit pour sauver ses camarades et devient le réceptacle du plus puissant des fléaux.",
            "link": "https://anime-sama.to/catalogue/jujutsu-kaisen",
            "web-link": "https://anime-sama.to/catalogue/jujutsu-kaisen",
            "theme": ["Action", "Supernatural", "Horror", "Shonen"],
            "lang": ["VF", "VOSTFR"]
          }
        ]
      },
      {
        "label": "Derniers épisodes ajoutés",
        "entries": [
          {
            "title": "Solo Leveling",
            "media-type": "video/show/anime",
            "episode": {
              "label": "Épisode 12"
            },
            "release-date": "2026-07-08",
            "lang": ["VOSTFR"],
            "link": "https://anime-sama.to/catalogue/solo-leveling",
            "web-link": "https://anime-sama.to/catalogue/solo-leveling",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/solo-leveling.webp"
            }
          },
          {
            "title": "One Piece",
            "media-type": "video/show/anime",
            "episode": {
              "label": "Épisode 1123"
            },
            "release-date": "2026-07-09",
            "lang": ["VF"],
            "link": "https://anime-sama.to/catalogue/one-piece",
            "web-link": "https://anime-sama.to/catalogue/one-piece",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/one-piece.webp"
            }
          }
        ]
      },
      {
        "label": "Derniers scans ajoutés",
        "entries": [
          {
            "title": "Berserk",
            "media-type": "images/manga",
            "episode": {
              "label": "Chapitre 378"
            },
            "release-date": "2026-07-09",
            "lang": ["VF"],
            "link": "https://anime-sama.to/catalogue/berserk",
            "web-link": "https://anime-sama.to/catalogue/berserk",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/berserk.webp"
            }
          },
          {
            "title": "Solo Leveling",
            "media-type": "images/webtoon",
            "episode": {
              "label": "Chapitre 200"
            },
            "release-date": "2026-07-08",
            "lang": ["VF"],
            "link": "https://anime-sama.to/catalogue/solo-leveling",
            "web-link": "https://anime-sama.to/catalogue/solo-leveling",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/solo-leveling.webp"
            }
          }
        ]
      },
      {
        "label": "Découvrez des pépites",
        "entries": [
          {
            "title": "Made in Abyss",
            "title/alt": "メイドインアビス",
            "media-type": "video/show/anime",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/made-in-abyss.webp"
            },
            "description": "Dans l'immense gouffre appelé Abyss, une jeune orpheline part à la recherche de sa mère.",
            "link": "https://anime-sama.to/catalogue/made-in-abyss",
            "web-link": "https://anime-sama.to/catalogue/made-in-abyss",
            "theme": ["Adventure", "Fantasy", "Mystery", "Drama"],
            "lang": ["VF", "VOSTFR"]
          },
          {
            "title": "Vinland Saga",
            "title/alt": "ヴィンランド・サガ",
            "media-type": "video/show/anime",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/vinland-saga.webp"
            },
            "description": "Le jeune Thorfinn, dont le père a été assassiné, parcourt les mers du Nord en quête de vengeance.",
            "link": "https://anime-sama.to/catalogue/vinland-saga",
            "web-link": "https://anime-sama.to/catalogue/vinland-saga",
            "theme": ["Action", "Adventure", "Historical", "Drama", "Seinen"],
            "lang": ["VF", "VOSTFR"]
          }
        ]
      }
    ]
  }
]
```

---

## 3. `get_category`

Page de catégorie (catalogue) avec pagination.

Requête exécutée avec : `query_url = "https://anime-sama.to/catalogue/?type%5B%5D=Anime"`, `page = 1`

```json
[
  {
    "categories": [
      {
        "key": "anime",
        "label": "Anime",
        "link": "https://anime-sama.to/catalogue/?type%5B%5D=Anime"
      },
      {
        "key": "manga",
        "label": "Scans",
        "link": "https://anime-sama.to/catalogue/?type%5B%5D=Scans"
      },
      {
        "key": "webtoon",
        "label": "Webtoon",
        "link": "https://anime-sama.to/catalogue/?type%5B%5D=Webtoon"
      }
    ],
    "sections": [
      {
        "label": "Catalogue",
        "link": "https://anime-sama.to/catalogue/?type%5B%5D=Anime&page=1",
        "current_page": 1,
        "have_more": true,
        "entries": [
          {
            "source": "anime-sama",
            "title": "Solo Leveling",
            "title/alt": "俺だけレベルアップな件",
            "media-type": "video/show/anime",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/solo-leveling.webp"
            },
            "description": "Depuis qu'il a obtenu le pouvoir de monter de niveau, Sung Jinwoo chasse des monstres...",
            "link": "https://anime-sama.to/catalogue/solo-leveling",
            "web-link": "https://anime-sama.to/catalogue/solo-leveling",
            "theme": ["Action", "Fantasy", "Adventure"],
            "lang": ["VF", "VOSTFR"]
          },
          {
            "source": "anime-sama",
            "title": "One Piece",
            "title/alt": null,
            "media-type": "video/show/anime",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/one-piece.webp"
            },
            "description": "Luffy et son équipage poursuivent leur voyage vers le One Piece à travers le Nouveau Monde.",
            "link": "https://anime-sama.to/catalogue/one-piece",
            "web-link": "https://anime-sama.to/catalogue/one-piece",
            "theme": ["Action", "Adventure", "Comedy", "Shonen"],
            "lang": ["VF", "VOSTFR", "VO"]
          },
          {
            "source": "anime-sama",
            "title": "Jujutsu Kaisen",
            "title/alt": "呪術廻戦",
            "media-type": "video/show/anime",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/jujutsu-kaisen.webp"
            },
            "description": "Yuji Itadori avale un doigt maudit pour sauver ses camarades...",
            "link": "https://anime-sama.to/catalogue/jujutsu-kaisen",
            "web-link": "https://anime-sama.to/catalogue/jujutsu-kaisen",
            "theme": ["Action", "Supernatural", "Horror", "Shonen"],
            "lang": ["VF", "VOSTFR"]
          },
          {
            "source": "anime-sama",
            "title": "Attack on Titan",
            "title/alt": "進撃の巨人",
            "media-type": "video/show/anime",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/attack-on-titan.webp"
            },
            "description": "Pour se venger de l'humanité qui l'a rejeté, Eren déclenche le Grand Terrassement.",
            "link": "https://anime-sama.to/catalogue/attack-on-titan",
            "web-link": "https://anime-sama.to/catalogue/attack-on-titan",
            "theme": ["Action", "Drama", "Fantasy", "Military", "Mystery", "Supernatural"],
            "lang": ["VF", "VOSTFR", "VO"]
          },
          {
            "source": "anime-sama",
            "title": "Demon Slayer",
            "title/alt": "鬼滅の刃",
            "media-type": "video/show/anime",
            "img/landscape": {
              "link": "https://cdn.anime-sama.to/covers/demon-slayer.webp"
            },
            "description": "Tanjiro Kamado cherche un remède pour transformer sa sœur Nezuko en démon...",
            "link": "https://anime-sama.to/catalogue/demon-slayer",
            "web-link": "https://anime-sama.to/catalogue/demon-slayer",
            "theme": ["Action", "Adventure", "Supernatural", "Shonen"],
            "lang": ["VF", "VOSTFR"]
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

Requête exécutée avec : `query_url = "https://anime-sama.to/catalogue/?type%5B%5D=Scans"`, `page = 2`

```json
[
  {
    "current_page": 2,
    "have_more": true,
    "entries": [
      {
        "source": "anime-sama",
        "title": "Berserk",
        "title/alt": "ベルセルク",
        "media-type": "images/manga",
        "img/landscape": {
          "link": "https://cdn.anime-sama.to/covers/berserk.webp"
        },
        "description": "Guts, le guerrier noir, poursuit sa quête de vengeance contre Griffith...",
        "link": "https://anime-sama.to/catalogue/berserk",
        "web-link": "https://anime-sama.to/catalogue/berserk",
        "theme": ["Action", "Adventure", "Dark Fantasy", "Seinen"],
        "lang": ["VF"]
      },
      {
        "source": "anime-sama",
        "title": "One Punch Man",
        "title/alt": "ワンパンマン",
        "media-type": "images/manga",
        "img/landscape": {
          "link": "https://cdn.anime-sama.to/covers/one-punch-man.webp"
        },
        "description": "Saitama est un héros si puissant qu'il vainc tous ses adversaires d'un seul coup.",
        "link": "https://anime-sama.to/catalogue/one-punch-man",
        "web-link": "https://anime-sama.to/catalogue/one-punch-man",
        "theme": ["Action", "Comedy", "Superhero", "Seinen"],
        "lang": ["VF"]
      },
      {
        "source": "anime-sama",
        "title": "Vagabond",
        "title/alt": "バガボンド",
        "media-type": "images/manga",
        "img/landscape": {
          "link": "https://cdn.anime-sama.to/covers/vagabond.webp"
        },
        "description": "Miyamoto Musashi, le plus célèbre épéiste du Japon, cherche la voie du sabre.",
        "link": "https://anime-sama.to/catalogue/vagabond",
        "web-link": "https://anime-sama.to/catalogue/vagabond",
        "theme": ["Historical", "Martial Arts", "Seinen", "Drama"],
        "lang": ["VF"]
      }
    ]
  }
]
```

---

## 5. `search`

Résultats de recherche avec filtres.

Requête exécutée avec : `search_terms = "Solo"`, `media_types = ["video/show/anime"]`, `page = 1`

```json
[
  {
    "source": "anime-sama",
    "current_page": 1,
    "have_more": false,
    "entries": [
      {
        "title": "Solo Leveling",
        "title/alt": "俺だけレベルアップな件",
        "media-type": "video/show/anime",
        "img/poster": {
          "link": "https://cdn.anime-sama.to/covers/solo-leveling.webp"
        },
        "description": "Depuis qu'il a obtenu le pouvoir de monter de niveau, Sung Jinwoo chasse des monstres dans les donjons pour devenir plus fort.",
        "link": "/catalogue/solo-leveling",
        "web-link": "/catalogue/solo-leveling",
        "theme": ["Action", "Fantasy", "Adventure"],
        "lang": ["VF", "VOSTFR"]
      }
    ]
  }
]
```

---

## 6. `get_entry`

Page détaillée d'un programme (avec saisons et bande-annonce).

Requête exécutée avec : `query_url = "https://anime-sama.to/catalogue/solo-leveling"`

```json
[
  {
    "title": "Solo Leveling",
    "title/alt": "俺だけレベルアップな件",
    "img/poster": {
      "link": "https://cdn.anime-sama.to/covers/solo-leveling-poster.webp"
    },
    "description": "Depuis qu'il a obtenu le pouvoir de monter de niveau, Sung Jinwoo chasse des monstres dans les donjons pour devenir plus fort. Cependant, une mission dangereuse pourrait bien changer sa vie à jamais...",
    "theme": ["Action", "Fantasy", "Adventure", "Shonen", "Isekai"],
    "year": "2024",
    "seasons": [
      {
        "label": "Saison 1",
        "link": "https://anime-sama.to/catalogue/solo-leveling/saison-1"
      },
      {
        "label": "Saison 2",
        "link": "https://anime-sama.to/catalogue/solo-leveling/saison-2"
      }
    ],
    "video/trailer": "https://www.youtube.com/embed/_B2n2y1G5cU"
  }
]
```

---

## 7. `get_season`

Liste des épisodes d'une saison avec lecteurs disponibles.

Requête exécutée avec : `query_url = "https://anime-sama.to/catalogue/solo-leveling/saison-1/vf"`

Post-processus appliqués :
1. `fetch_regex_items_from_items` — récupère les tableaux d'URLs depuis les fichiers `episodes.js` pour chaque langue
2. `pivot_items_by_index` — réorganise les listes d'URLs en épisodes avec lecteurs
3. `derive_pagination` — pagination finale

```json
[
  {
    "current_page": 1,
    "have_more": false,
    "link": "https://anime-sama.to/catalogue/solo-leveling/saison-1/vf",
    "episodes": [
      {
        "episode-number": "1",
        "title": "Episode 1",
        "lang": "vf",
        "name": "videobin",
        "players": [
          {
            "name": "videobin",
            "lang": "vf",
            "embed-link": "https://videobin.com/embed/abc123"
          }
        ]
      },
      {
        "episode-number": "2",
        "title": "Episode 2",
        "lang": "vf",
        "name": "videobin",
        "players": [
          {
            "name": "videobin",
            "lang": "vf",
            "embed-link": "https://videobin.com/embed/abc124"
          }
        ]
      },
      {
        "episode-number": "3",
        "title": "Episode 3",
        "lang": "vf",
        "name": "videobin",
        "players": [
          {
            "name": "videobin",
            "lang": "vf",
            "embed-link": "https://videobin.com/embed/abc125"
          }
        ]
      },
      {
        "episode-number": "12",
        "title": "Episode 12",
        "lang": "vf",
        "name": "videobin",
        "players": [
          {
            "name": "videobin",
            "lang": "vf",
            "embed-link": "https://videobin.com/embed/abc134"
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
| `service_stream_metadata` | static | `object[]` | `id`, `title`, `logo`, `description`, `search_themes` |
| `load_home` | html | `object[]` | `banners[]`, `categories[]`, `sections[]` |
| `get_category` | html | `object[]` | `categories[]`, `sections[].entries[]` (avec pagination) |
| `get_section` | html | `object[]` | `current_page`, `have_more`, `entries[]` |
| `search` | html | `object[]` | `current_page`, `have_more`, `entries[]` |
| `get_entry` | html | `object[]` | `title`, `description`, `theme`, `year`, `seasons[]`, `video/trailer` |
| `get_season` | html | `object[]` | `current_page`, `have_more`, `episodes[].players[]` |

---

*Document généré le 09/07/2026 — Basé sur les spécifications `arachnea-stream-en.md` et `arachnea-scrapyfy-en.md`, ainsi que la configuration `anime-sama.yaml`.*