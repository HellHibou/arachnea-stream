# Format de réponse du backend Arachnea

Analyse réalisée à partir des fichiers exemples dans `docs/sample_out/`.

---

## 1. `load_home` — Chargement de la page d'accueil

Fichier exemple : `load_home.json`

### Structure générale

```json
[
  {
    "banners": [ /* ... */ ],
    "categories": [ /* ... */ ],
    "sections": [ /* ... */ ],
    "source": [ "identifiant-source" ]
  },
  /* ... chaque élément représente une source configurée */
]
```

### Champs

| Champ | Type | Description |
|-------|------|-------------|
| `banners` | `array` | Bannières promotionnelles de la source (peut être vide). Chaque élément est un objet ou une liste d'objets imbriquée. |
| `categories` | `array` | Catégories disponibles dans la source. |
| `sections` | `array` | Sections de contenu (rangées de la page d'accueil). |
| `source` | `string[]` | Identifiant unique de la source (ex: `"rtlplay-be"`, `"rtbf-auvio-be"`, etc.). |

#### Structure d'une bannière

```json
{
  "description": ["..."]         // optionnel
  "image": ["url"],
  "key": ["id-unique"],
  "link": ["url-detail"],
  "logo": ["url-logo"],          // optionnel
  "subtitle": ["..."],           // optionnel
  "title": ["..."],
  "video": ["..."],              // optionnel
  "web-link": ["url-web"],
  "entryUrl": ["..."],           // optionnel (m6play)
  "expire": ["2026-12-31"],      // optionnel (m6play)
  "year": ["2025"]               // optionnel (m6play)
}
```

#### Structure d'une catégorie

```json
{
  "image": ["url"],              // optionnel
  "description": ["..."],        // optionnel
  "key": ["identifiant"],
  "label": ["Nom affiché"],
  "request": {
    "query_url": ["url-api"]     // optionnel
  }
}
```

#### Structure d'une section

Deux formes possibles :

- **Avec contenu direct (`entries`)** :
  ```json
  {
    "entries": [ /* objet d'entrée */ ],
    "key": ["identifiant"],      // optionnel
    "label": ["Nom de la rangée"],
    "current_page": ["1"],        // optionnel
    "have_more": ["true"]         // optionnel
  }
  ```

- **Lien simple** :
  ```json
  {
    "label": ["Nom"],
    "link": ["url"]
  }
  ```

---

## 2. `get_section` — Contenu paginé d'une section/catégorie

Fichiers exemples : `get_section.json`, `get_section_1.json`, `get_section_2.json`, `get_section_3.json`

### Structure générale

```json
{
  "current_page": ["1"],
  "entries": [ /* ... */ ],
  "have_more": ["true"],
  "source": ["identifiant-source"]
}
```

### Champs racine

| Champ | Type | Description |
|-------|------|-------------|
| `current_page` | `string[]` | Numéro de la page courante. |
| `entries` | `array` | Liste des entrées (contenus). |
| `have_more` | `string[]` | `"true"` s'il y a une page suivante, `"false"` sinon. |
| `source` | `string[]` | Identifiant de la source. |

### Champs d'une entrée

Tous les champs sont des **tableaux de chaînes** (`string[]`) sauf les champs `img/*` qui sont des **objets**.

| Champ | Type | Description | Présence |
|-------|------|-------------|----------|
| `title` | `string[]` | Titre principal. | Toujours |
| `link` | `string[]` | Lien vers la page de détail du contenu. | Toujours |
| `web-link` | `string[]` | Lien web public. | Toujours |
| `source` | `string[]` | Source d'origine. | Toujours |
| `media-type` | `string[]` | Type média (ex: `"video/show/other"`, `"video/show/serie"`, `"video/show/anime"`). | Toujours |
| `img/poster` | `object` | Image poster (vignette). `{ "link": ["url"] }`. | Toujours |
| `img/landscape` | `object` | Image paysage alternative. `{ "link": ["url"] }`. | Parfois |
| `channel` | `string[]` | Chaîne de diffusion. | Parfois |
| `description` | `string[]` | Description longue. | Parfois |
| `duration` | `string[]` | Durée en secondes. | Parfois |
| `expire` | `string[]` | Date d'expiration (format `YYYY-MM-DD`). | Parfois |
| `release-date` | `string[]` | Date de sortie (peut contenir heure, ex: `"2026-05-25 11:52:55"`). | Parfois |
| `theme` | `string[]` | Thème ou genre. | Parfois |
| `title/alt` | `string[]` | Sous-titre ou titre alternatif. | Parfois |
| `year` | `string[]` | Année de production. | Parfois |

> **Note importante** : Les champs `source`, `current_page`, `have_more` apparaissent au niveau racine dans `get_section`, mais aussi parfois dans les `entries` individuelles (héritage de la source).

---

## 3. `search` — Résultats de recherche

Fichier exemple : `search.json`

### Structure générale

```json
[
  {
    "current_page": ["1"],
    "entries": [ /* ... */ ],
    "have_more": ["true"],
    "source": ["identifiant-source"],
    "source_params": [              // optionnel
      { "page": ["2"] }
    ]
  }
  /* ... chaque élément est un bloc de résultats par source */
]
```

### Champs racine

| Champ | Type | Description |
|-------|------|-------------|
| `current_page` | `string[]` | Numéro de page. |
| `entries` | `array` | Liste des entrées trouvées. |
| `have_more` | `string[]` | `"true"` s'il y a plus de résultats. |
| `source` | `string[]` | Identifiant de la source. |
| `source_params` | `array` | Paramètres additionnels (ex: page suivante). Optionnel. |

### Champs d'une entrée (varient selon la source)

Les champs communs à toutes les sources sont omis. Voici les particularités par source :

#### rtlplay-be

```json
{
  "genre": ["Film"],
  "img/poster": { "link": ["url"] },
  "key": ["uuid"],
  "link": ["url"],
  "media-type": ["video/show/other"],
  "theme": ["Film"],
  "title": ["Titre"],
  "web-link": ["url"]
}
```

#### rtbf-auvio-be

```json
{
  "channel": ["La Une"],
  "description": ["..."],
  "duration": ["5404"],
  "embed-link": ["url-embed"],
  "expire": ["2026-06-06"],
  "id": ["3442079"],
  "img/poster": { "link": ["url"] },
  "link": ["url"],
  "media-type": ["video/show/other"],
  "release-date": ["2026-05-07"],
  "subtitle": ["..."]              // optionnel
  "theme": ["Drame"],
  "title": ["Titre"],
  "web-link": ["url"]
}
```

#### m6play-fr

```json
{
  "description": ["..."],
  "img/poster": { "link": ["url"] },
  "link": ["id-programme"],
  "media-type": ["video/other/Divertissement"],
  "release-date": ["2026-05-28 23:30:37"],
  "theme": ["Divertissement", "Emotion"],
  "title": ["Titre"]
}
```

#### tf1-fr

```json
{
  "description": ["..."],
  "img/poster": { "link": ["url"] },
  "link": ["url-smarttv"],
  "media-type": ["video/show/serie"],
  "source": ["tf1-fr"],
  "theme": ["Romance", "Drame"],
  "title": ["Titre"],
  "web-link": ["url"]
}
```

#### francetv

```json
{
  "casting": [""],                   // optionnel
  "channel": ["France 5"],           // optionnel
  "content-advisor": ["TP"],          // optionnel
  "description": [""],
  "director": [""],                  // optionnel
  "duration": ["300"],               // optionnel
  "img/landscape": { "link": ["url1", "url2"] },
  "img/logo": { "link": ["url"] },   // optionnel
  "img/poster": { "link": ["url1", "url2", "url3"] },
  "link": ["url-api"],
  "media-type": ["video/show/other"],
  "source": ["francetv"],
  "theme": ["Enfants"],
  "title": ["Titre principal", "Titre secondaire"],
  "title/alt": ["Titre alternatif"], // optionnel
  "web-link": ["url"]
}
```

#### anime-sama

```json
{
  "description": ["..."],
  "img/poster": { "link": ["url"] },
  "lang": ["VOSTFR"],                // optionnel
  "link": ["url"],
  "media-type": ["video/show/anime"],
  "theme": ["Comédie", "Action"],
  "title": ["Titre"],
  "title/alt": ["Titre alternatif"],
  "web-link": ["url"]
}
```


---

## 4. `get_category` — Contenu d'une catégorie (page catégorie)

Fichier exemple : `get_category.json`

### Structure générale

```json
[
  {
    "banners": [ /* ... */ ],
    "sections": [ /* ... */ ],
    "source": [ "identifiant-source" ]
  },
  /* ... chaque élément représente une source configurée */
]
```

Identique à `load_home` mais sans le champ `categories`. Certaines sources peuvent aussi omettre `banners`.

### Champs

| Champ | Type | Description |
|-------|------|-------------|
| `banners` | `array` | Bannières promotionnelles de la source (peut être vide ou absent). Même structure que dans `load_home`. |
| `sections` | `array` | Sections de contenu (rangées de la page). |
| `source` | `string[]` | Identifiant unique de la source. |

### Structure d'une section

Même structure que `load_home` :

- **Avec contenu direct (`entries`)** — ex: `rtlplay-be`, `francetv` :
  ```json
  {
    "entries": [ /* objet d'entrée */ ],
    "key": ["identifiant"],      // optionnel
    "label": ["Nom de la rangée"],
    "current_page": ["1"],        // optionnel
    "have_more": ["true"]         // optionnel
  }
  ```

- **Lien simple** — ex: `rtbf-auvio-be` :
  ```json
  {
    "label": ["Nom"],
    "link": ["url"]
  }
  ```

### Champs d'une entrée (varient selon la source)

#### rtlplay-be

```json
{
  "description": ["..."],
  "duration": ["105 min"],
  "expire": ["2026-06-07"],
  "img/landscape": { "link": ["url"] },
  "img/logo": { "link": ["url"] },
  "img/poster": { "link": ["url"] },
  "key": ["uuid"],
  "link": ["url"],
  "media-type": ["video/show/other"],
  "release-date": ["2026-05-17"],
  "title": ["Titre"],
  "web-link": ["url"],
  "year": ["2017"]
}
```

#### francetv

```json
{
  "description": ["..."],
  "img/landscape": { "link": ["url"] },
  "img/logo": { "link": ["url"] },
  "img/poster": { "link": ["url"] },
  "link": ["url-api"],
  "media-type": ["video/movie", "video/show/other"],
  "source": ["francetv"],
  "theme": ["Cinéma"],
  "title": ["Titre"]
}
```

> **Note** : Les sections avec `entries` pour `rtbf-auvio-be` dans `get_category` utilisent le format lien simple (`label` + `link`) uniquement. Les entrées de contenu ne sont pas chargées directement dans cette fonction pour cette source.

---

## Règles générales du format

1. **Tous les champs scalaires sont des tableaux** (`string[]`), même les valeurs uniques. Un champ `"title"` contiendra toujours `["valeur"]`, pas `"valeur"`.

2. **Les images** (`img/poster`, `img/landscape`, `img/logo`) sont des objets avec une clé `"link"` contenant un tableau d'URLs :
   ```json
   "img/poster": { "link": ["url1", "url2"] }
   ```

3. **`media-type`** peut contenir un ou plusieurs types, sous forme de tableau : `["video/show/serie"]` ou `["video/other/kids", "video/show/serie"]`.

4. **Les champs manquants sont simplement absents** du JSON — il n'y a pas de valeurs `null` ou de clés vides systématiques.

5. **`source`** est toujours présente et identifie le service ayant produit les données. Elle apparaît au niveau racine de `get_section` et au niveau de chaque bloc de `search`/`load_home`.

6. **`key`** (identifiant unique) n'est présente que pour certaines sources (`rtlplay-be` utilise un UUID).

7. **Pagination** : `current_page` et `have_more` sont présents au niveau racine. Pour la suite de la pagination, `source_params` dans `search` contient `{ "page": ["2"] }`.