# Format de réponse du backend Arachnea

Le contrat de sortie du moteur `arachnea-scrapyfy` est défini dans les YAML via le champ `type` des entries. Le backend ne renvoie plus les valeurs applicatives sous forme générique `string[]` : il sérialise directement les champs en JSON typé.

## Types supportés

| Type YAML | Forme JSON |
|---|---|
| `string` | chaîne ou `null` |
| `number` | nombre ou `null` |
| `boolean` | booléen ou `null` |
| `string[]` | tableau de chaînes ou `null` |
| `number[]` | tableau de nombres ou `null` |
| `boolean[]` | tableau de booléens ou `null` |
| `object` | objet ou `null` |
| `object[]` | tableau d'objets ou `null` |

Un champ déclaré dans le YAML mais non trouvé est conservé avec la valeur `null`. Un cast impossible échoue côté backend avec un message d'erreur détaillé.

## Règles objet

`object` et `object[]` s'appliquent aux sous-groupes de `ScraperDataNode`, pas à un cast implicite d'une valeur JSON brute.

- Un groupe YAML est un tableau d'objets par défaut.
- `select: first` force un objet simple.
- `object[]` est l'alias explicite pour une sortie tableau et ne doit pas être combiné avec `select: first`.
- Les groupes implicites créés par des chemins comme `players > direct-link` sont sérialisés en objet, ou en tableau d'objets lorsque plusieurs champs enfants produisent des valeurs alignées.

## Endpoints principaux

| Méthode | Forme de réponse |
|---|---|
| `get_service` | `ServiceMetadata[]` |
| `load_home` | `HomeSourceGroup[]` |
| `get_category` | `HomeSourceGroup[]` |
| `get_section` | `HomeSectionPayload` |
| `search` | `SearchSourceGroup[]` |
| `get_entry` | `EntryDetailsPayload` |
| `get_season` | `SeasonPayload` |
| `list_lives` | `MediaEntry[]` |
| `get_live` | `LivePayload` |
| `resolve_player_stream` | objet spécifique au resolver |

## Champs courants

| Champ | Type attendu |
|---|---|
| `source` | `string` |
| `current_page`, `total_pages`, `page_size` | `number` |
| `have_more`, `infer_have_more_from_full_page` | `boolean` |
| `title`, `description`, `link`, `web-link`, `duration`, `release-date`, `expire` | `string` |
| `year`, `count-season`, `count-episodes`, `rating` | `number` |
| `theme`, `genre`, `casting`, `director`, `media-type`, `lang`, `language` | `string[]` |
| `img/poster`, `img/landscape`, `img/logo`, `storyboard`, `resolver`, `resolver.stream` | `object` |
| `banners`, `categories`, `sections`, `entries`, `season`, `episode`, `episodes`, `players`, `source_params` | `object[]` |

Les paramètres de requête renvoyés au front, notamment `source_params` et les descripteurs `request`, restent consommés comme `Record<string, string>` par le front lorsqu'ils doivent être renvoyés au backend.
