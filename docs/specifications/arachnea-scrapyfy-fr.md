# Spécification du format YAML arachnea-scrapyfy

## 1. Structure générale du fichier

Un fichier YAML décrit une **collection de requêtes** pour une source de données. La structure racine est :

```yaml
id: mon-identifiant-source
title: "Titre optionnel"
logo: "https://example.com/logo.png"
description:
  fr: "Description en français"
  en: "Description in English"
parameters:
  - name: base_url
    value: "https://example.com"
    description: "URL de base de la source"
http:
  mode: auto
  user_agent_profile: chrome
queries:
  - name: ma_query
    scraper_type: html
    # ... champs spécifiques au type
```

---

## 2. Champs racine

### `id` (obligatoire)
Identifiant unique de la source. Utilisé comme clé de recherche.
Alias YAML : `name` (interchangeable avec `id`).

```yaml
id: m6play-fr
```

### `title` (optionnel)
Titre lisible de la source. Si absent, la valeur de `id` est utilisée.

```yaml
title: "M6 Play France"
```

### `logo` (optionnel)
URL ou chemin vers le logo de la source.

```yaml
logo: "https://example.com/logo.png"
```

### `description` (optionnel)
Carte multilingue de descriptions, clé = code langue, valeur = texte.

```yaml
description:
  fr: "Service de streaming M6"
  en: "M6 streaming service"
```

### `parameters` (optionnel)
Liste de paramètres par défaut de la collection. Chaque paramètre a :

| Champ | Type | Obligatoire | Description |
|-------|------|-------------|-------------|
| `name` | string | oui | Nom utilisé comme `{placeholder}` dans les templates |
| `value` | string | oui | Valeur par défaut (peut contenir des `{placeholders}`) |
| `description` | string | non | Description du paramètre |
| `actions` | array | non | Actions appliquées à la valeur résolue |

Les paramètres sont résolus séquentiellement dans l'ordre de déclaration YAML. Un paramètre peut référencer les paramètres précédents via `{placeholder}`.

```yaml
parameters:
  - name: base_url
    value: "https://www.m6play.fr"
  - name: query_url
    value: "{base_url}/api/search"
```

**Ajouts automatiques** : Les paramètres suivants sont automatiquement injectés lors de l'exécution :
- `source` — identifiant de la collection (`id`)
- `service_id` — identifiant de la collection
- `service_title` — titre de la collection
- `service_logo` — logo de la collection
- `service_description` — description sérialisée en JSON
- `page_index` — dérivé de `page` (`page - 1`)
- `offset` — dérivé de `page * page_size`
- `query_separator` — `?`, `&` ou `""` selon l'URL

---

## 3. Configuration HTTP (`http`)

Applicable au niveau collection, requête et sous-requête. La configuration de la collection est héritée par chaque requête, et celle de la requête par ses sous-requêtes.

```yaml
http:
  mode: auto                    # auto | direct | cloudflare_smart | cloudflare_browser
  user_agent_profile: chrome    # chrome | chrome_stable | firefox | firefox_stable
  user_agent: "Custom UA"       # optionnel : surcharge complète du User-Agent
  proxy_country: "US"           # optionnel : pays hint pour le proxy
  max_redirects: 5              # optionnel : nombre max de redirections
```

### Modes HTTP (`mode`)

| Valeur | Description |
|--------|-------------|
| `auto` | Mode par défaut, choix automatique |
| `direct` | Requête directe sans contournement |
| `cloudflare_smart` | Tente de contourner Cloudflare intelligemment |
| `cloudflare_browser` | Utilise un navigateur pour contourner Cloudflare |

### Profils User-Agent (`user_agent_profile`)

| Valeur | Description |
|--------|-------------|
| `chrome` | Profile Chrome standard |
| `chrome_stable` | Profile Chrome stable |
| `firefox` | Profile Firefox standard (identique à `firefox_stable` en pratique) |
| `firefox_stable` | Profile Firefox stable |

---

## 4. Type de scraper : `scraper_type`

Chaque requête dans `queries` doit définir `scraper_type` avec l'une des valeurs :
- `html` — extraction depuis du HTML avec des sélecteurs CSS
- `json` — extraction depuis du JSON avec des pointeurs JSON
- `static` — données déclarées statiquement dans le YAML (pas de requête HTTP)
- `text` — extraction depuis du texte brut découpé par délimiteurs

---

## 5. Structure commune à tous les scrapers (sauf static)

Les scrapers `html`, `json` et `text` partagent ces champs via `ScraperQueryCommon` :

```yaml
name: "identifiant-requete"          # obligatoire
scraper_type: html|json|text          # obligatoire
base_url: "https://example.com"       # obligatoire : URL de base, disponible comme {base_url}
media_types:                          # optionnel : types de contenu produits
  - movie
  - series
query_url: "/api/data"                # obligatoire : template d'URL
request_method: get                   # optionnel : get (défaut) | post
request_body_pointer: "/data/payload" # optionnel : pointeur JSON pour le corps POST
request_body_select: first            # optionnel : first | all (défaut: all)
request_body_actions: []              # optionnel : actions sur le corps avant envoi
request_headers:                      # optionnel : en-têtes HTTP
  - name: "Authorization"
    pointer: "/auth/token"
    select: first
    actions: []
http: {}                              # optionnel : configuration HTTP (surcharge)
query_param_mappings: []              # optionnel : mappings de paramètres
result_item_field: "entries"          # optionnel : champ groupe à aplatir en lignes
post_process: []                      # optionnel : post-traitements
```

### `request_headers`

Chaque en-tête est défini par :

| Champ | Type | Description |
|-------|------|-------------|
| `name` | string | Nom de l'en-tête (ex: `Authorization`) |
| `pointer` | string (opt.) | Pointeur JSON pour extraire la valeur du contexte |
| `select` | enum | `first` (défaut) ou `all` (valeurs jointes avec ", ") |
| `actions` | array | Actions appliquées à la valeur résolue |

```yaml
request_headers:
  - name: "Authorization"
    pointer: "/data/token"
    select: first
    actions:
      - type: format_text
        argument: "Bearer {}"
```

### `query_param_mappings`

Transforme les paramètres source en paramètres cible via une table de traduction :

| Champ | Type | Description |
|-------|------|-------------|
| `target_param` | string | Nom du paramètre de sortie |
| `source_param` | string | Nom du paramètre d'entrée (valeurs séparées par virgules) |
| `values` | map | Table de correspondance : clé source → valeur cible. `{}` = valeur encodée URL |
| `item_suffix` | string | Suffixe ajouté à chaque élément traduit |

```yaml
query_param_mappings:
  - target_param: "category_ids"
    source_param: "categories"
    values:
      movie: "cat_01"
      series: "cat_02"
    item_suffix: ","
```

### `result_item_field`

Quand une requête produit un groupe dont les items doivent devenir les lignes de résultat, ce champ indique le nom du groupe à aplatir :

```yaml
result_item_field: "entries"
```

Avec ce réglage, si la requête produit `{ entries: [ { id: 1 }, { id: 2 } ] }`, le résultat sera `[ { id: 1 }, { id: 2 } ]`.

---

## 6. Scraper HTML (`scraper_type: html`)

### Champs spécifiques

```yaml
name: recherche
scraper_type: html
row_selector: "div.result-item"     # obligatoire : sélecteur CSS pour chaque ligne
row_concurrency: 4                  # optionnel : nombre max de lignes traitées en parallèle (défaut: 4)
entries: []                         # obligatoire : extracteurs de champs
sub_queries: []                     # optionnel : sous-requêtes (voir section 10)
```

### Entrées HTML (`entries`)

Chaque entrée peut être un **champ** (feuille) ou un **groupe** (contenant des sous-entrées).

```yaml
entries:
  # Champ simple
  - name: title
    type: string
    selector: "h2 a"                # sélecteur CSS optionnel
    select: first                   # first (défaut) | all
    actions:                        # optionnel : pipeline d'actions
      - type: get_text

  # Groupe
  - name: images
    type: object[]                   # object | object[] (défaut: object[])
    selector: "img.poster"
    select: all
    entries:                         # sous-entrées du groupe
      - name: url
        type: string
        actions:
          - type: get_attribut
            argument: src
          - type: resolve_url
            proxy: true
```

#### `post_build` des groupes

Un groupe objet HTML peut déclarer des transformations ordonnées exécutées après
l'extraction de tous ses champs enfants.

##### `math_formula`

Calcule un champ enfant numérique depuis les champs frères scalaires numériques
du même objet.

| Champ | Type | Description |
|-------|------|-------------|
| `target` | string | Chemin enfant séparé par `>` recevant le nombre calculé |
| `expression` | string | Expression mathématique dont les placeholders `{chemin}` lisent les champs frères scalaires |

Les expressions sont analysées par le moteur mathématique déterministe intégré.
Tous les placeholders doivent résoudre vers des champs frères numériques à
l'exécution, sinon la cible est omise. L'expression est validée au chargement
du YAML.

##### `remove_fields`

Retire des champs frères temporaires après leur consommation par une
transformation post-build précédente.

| Champ | Type | Description |
|-------|------|-------------|
| `fields` | string[] | Chemins enfants séparés par `>` à supprimer |

```yaml
- name: storyboard
  type: object
  select: first
  entries:
    - name: count_per_image
      type: number
      actions: [ ... ]
    - name: count_per_row
      type: number
      actions: [ ... ]
  post_build:
    - type: math_formula
      target: rows
      expression: "{count_per_image} / {count_per_row}"
    - type: remove_fields
      fields: [count_per_image, count_per_row]
```

Si une entrée de formule est absente ou non numérique à l'exécution, sa cible est omise.

#### Règles de validation des entrées HTML :

- Un champ doit définir `actions` ou `sub_queries` (ou les deux), mais pas `entries`.
- Un groupe doit définir `entries` mais pas `actions`.
- `post_build` est pris en charge uniquement par les groupes HTML, jamais par les champs feuilles.
- `type` est obligatoire pour les deux.
- Pour un groupe :
  - `type: object` avec `select: first` → un objet unique
  - `type: object` avec `select: all` → tableau d'objets
  - `type: object[]` → tableau d'objets (incompatible avec `select: first`)
- Les noms supportent les chemins hiérarchiques avec `>` :
  ```yaml
  - name: parent>enfant>champ
    type: string
  ```

### `select`

| Valeur | Description |
|--------|-------------|
| `first` (défaut pour certains contextes) | Seulement le premier élément correspondant |
| `all` (défaut général) | Tous les éléments correspondants |

---

## 7. Scraper JSON (`scraper_type: json`)

### Champs spécifiques

```yaml
name: recherche-api
scraper_type: json
row_pointer: "/data/results/*"           # obligatoire : pointeur JSON pour chaque ligne
extract_next_data: false                 # optionnel : parser le HTML comme Next.js __NEXT_DATA__
filters:                                 # optionnel : filtres racine
  status: ["active", "published"]
sibling_sub_query_concurrency: 4         # optionnel : parallélisme sous-requêtes (défaut: 4)
sub_query_context_concurrency: 4         # optionnel : parallélisme contextes (défaut: 4)
sub_query_fetch_concurrency: 8           # optionnel : parallélisme requêtes HTTP (défaut: 8)
entries: []                              # obligatoire : extracteurs de champs
sub_queries: []                          # optionnel : sous-requêtes
```

### Syntaxe des pointeurs JSON

Le système de pointeurs JSON supporte :

| Syntaxe | Description |
|---------|-------------|
| `/data/title` | Chemin simple dans un objet |
| `/data/*` | Tous les éléments d'un tableau ou toutes les valeurs d'un objet |
| `/data/0` | Index numérique dans un tableau |
| `/data/*[role=mea]` | Filtre : éléments du tableau dont `role` = `"mea"` |
| `/data/0[status=active]` | Filtre sur un index spécifique |
| `/data/*/external_key` | Descend dans tous les sous-éléments |

```yaml
# Exemple : filtrer par role=cover
row_pointer: "/images/*[role=cover]"
```

### Entrées JSON (`entries`)

Chaque entrée peut être un **champ** ou un **groupe**.

```yaml
entries:
  # Champ simple
  - name: title
    type: string
    pointer: "/title"                    # pointeur JSON optionnel
    select: first                        # first | all
    actions:
      - type: get_text

  # Groupe
  - name: episodes
    type: object[]
    pointer: "/episodes/*"
    select: all
    entries:
      - name: id
        type: string
        pointer: "/id"
      - name: title
        type: string
        pointer: "/title"
```

**Contrainte** : En JSON, l'entrée `type` est **obligatoire** pour tous les champs et groupes.

**Pointeur absent** : Si `pointer` n'est pas défini, l'extraction opère sur la ligne elle-même.

---

## 8. Scraper Static (`scraper_type: static`)

Ne fait **aucune requête HTTP**. Les données sont déclarées dans le YAML et les templates `{placeholders}` sont résolus au moment de l'exécution.

### Champs spécifiques

```yaml
name: metadata
scraper_type: static
media_types:                            # optionnel
  - metadata
entries: []                             # obligatoire : entrées statiques
```

### Entrées statiques (`entries`)

Chaque entrée peut avoir :

| Champ | Type | Description |
|-------|------|-------------|
| `name` | string | Nom du champ (supporte `>` pour chemins hiérarchiques) |
| `type` | string | Type de sortie JSON (`string`, `number`, `boolean`, `object`, `object[]`) |
| `value` | YAML value | Valeur scalaire ou structure (template `{placeholder}` résolu) |
| `actions` | array | Actions appliquées après résolution du template |
| `items` | array | Liste d'objets statiques (template `{placeholder}` dans chaque valeur) |

**Règles** :
- `type` est obligatoire
- `value` et `items` sont mutuellement exclusifs
- Si `items` est présent, `type` doit être `object` ou `object[]`
- Les champs `value` et les valeurs dans `items` sont rendus via les paramètres d'exécution

```yaml
entries:
  # Valeur scalaire avec template
  - name: logo
    type: string
    value: "{base_url}/logo.png"
    actions:
      - type: resolve_url
        proxy: true

  # Valeur objet
  - name: platform
    type: object
    value:
      name: "M6 Play"
      url: "{base_url}"

  # Liste d'items statiques
  - name: quick_links
    type: object[]
    items:
      - label: "Accueil"
        url: "{base_url}/home"
      - label: "Recherche"
        url: "{base_url}/search"
```

---

## 9. Scraper Text (`scraper_type: text`)

Extrait des données d'un corps de réponse texte en découpant par lignes (`row_delimiter`) puis en champs (`field_delimiter`).

### Champs spécifiques

```yaml
name: fichiers-csv
scraper_type: text
row_delimiter: "\n"                    # obligatoire : délimiteur de lignes
field_delimiter: "|"                   # optionnel : délimiteur de champs
entries: []                            # obligatoire : extracteurs de champs
```

### Entrées text (`entries`)

| Champ | Type | Description |
|-------|------|-------------|
| `name` | string | Nom du champ (supporte `>` pour chemins hiérarchiques) |
| `type` | string | Type de sortie JSON (obligatoire pour les champs) |
| `field` | int (opt.) | Index 1-based du champ après split par `field_delimiter` |
| `actions` | array | Pipeline d'actions appliquées à la valeur extraite |
| `entries` | array | Sous-entrées (groupe) |

```yaml
row_delimiter: "\n"
field_delimiter: "|"
entries:
  - name: id
    type: number
    field: 1
  - name: title
    type: string
    field: 2
  - name: duration
    type: number
    field: 3
    actions:
      - type: normalize_duration
```

Avec une ligne comme `42|Mon Film|1h30`, cela produit :
```json
{ "id": 42, "title": "Mon Film", "duration": 5400 }
```

---

## 10. Sous-requêtes (`sub_queries`)

Les sous-requêtes permettent d'enchaîner des requêtes HTTP supplémentaires à partir des valeurs extraites. Elles sont disponibles dans les scrapers `html` et `json`.

### Structure commune (`SubQueryCommon`)

```yaml
sub_queries:
  - scraper_type: html|json            # obligatoire : type de la sous-requête
    # Champs communs :
    context_pointer: "/data/items/*"   # optionnel : pointeur pour les lignes de contexte
    context_select: all                # optionnel : first | all
    filters:                           # optionnel : filtres sur le contexte
      type: ["movie", "series"]
    row_filters:                       # optionnel : filtres sur les lignes récupérées
      lang: ["fr", "en"]
    target: "parent>details"           # optionnel : chemin de destination dans le résultat
    request_pointer: "/url"            # optionnel : pointeur pour l'URL de la sous-requête
    request_select: first              # optionnel : first | all
    request_actions: []                # optionnel : actions sur l'URL avant requête
    request_method: get                # optionnel : get | post
    request_headers: []                # optionnel : en-têtes de la sous-requête
    http: {}                           # optionnel : config HTTP
    post_process: []                   # optionnel : post-traitements
    # Champs spécifiques selon scraper_type
```

### `target`

Chemin où les résultats de la sous-requête sont imbriqués. Format : `"parent>enfant"` (séparé par `>`).
Si absent, les résultats sont fusionnés au niveau racine.

```yaml
target: "details>episodes"
```

### Sous-requête HTML

```yaml
  - scraper_type: html
    row_selector: "div.episode"
    entries:
      - name: title
        type: string
        selector: "h3"
        actions:
          - type: get_text
    context_entries:                   # optionnel : extrait aussi du contexte parent
      - name: season
        type: number
        pointer: "/season_number"
```

### Sous-requête JSON

```yaml
  - scraper_type: json
    row_pointer: "/data/*"
    entries:
      - name: id
        type: string
        pointer: "/id"
    request_body_pointer: "/payload"   # optionnel : corps POST depuis le contexte
    request_body_select: first
    request_body_actions: []
    extract_next_data: false           # optionnel : parser Next.js
```

### Sous-requêtes d'entrée (`sub_queries` sur une `entry`)

Les entrées HTML et JSON peuvent aussi porter des sous-requêtes. La syntaxe est identique :

```yaml
entries:
  - name: url
    type: string
    selector: "a"
    actions:
      - type: get_attribut
        argument: href
    sub_queries:
      - scraper_type: html
        row_selector: "div.detail"
        entries:
          - name: description
            type: string
            selector: "p.desc"
            actions:
              - type: get_text
```

---

## 11. Actions (`actions`)

Les actions sont un pipeline de transformations appliquées aux valeurs extraites. Elles sont taggées par `type` en snake_case.

### `get_text`
Lit le contenu textuel concaténé de l'élément HTML sélectionné.

```yaml
- type: get_text
```

### `html_to_text`
Convertit le contenu HTML en texte brut via `quick_html2md`.

```yaml
- type: html_to_text
```

### `get_attribut`
Lit un attribut HTML (ex: `href`, `src`, `content`).

```yaml
- type: get_attribut
  argument: "href"
```

### `split`
Divise chaque valeur par le séparateur et remplace la liste par les fragments.

```yaml
- type: split
  argument: ","
```

### `map`
Réécrit les valeurs via une table de correspondance.

| Champ | Type | Description |
|-------|------|-------------|
| `argument` | mapping | Table clé → valeur. Une clé `null` correspond à une entrée nulle |
| `default` | scalar (opt.) | Valeur de repli quand la clé est absente. `{}` = valeur originale |

```yaml
- type: map
  argument:
    movie: "film"
    series: "série"
  default: "autre"
```

Les valeurs mappées à `null` sont supprimées de la liste de sortie.

### `regex_find_all`
Applique une regex et remplace la liste par les groupes capturés.

| Champ | Type | Description |
|-------|------|-------------|
| `pattern` | string | Expression régulière |
| `format` | string | Template de sortie : `{1}` = groupe 1, `{request_url}`, `{base_url}`, etc. |

```yaml
- type: regex_find_all
  pattern: "id=(\\d+)"
  format: "{1}"
```

### `get_request_url`
Ajoute l'URL actuelle de la requête à la liste des valeurs.

```yaml
- type: get_request_url
```

### `get_response_body`
Ajoute le corps brut de la réponse HTTP à la liste des valeurs.

```yaml
- type: get_response_body
```

### `suffix`
Ajoute un suffixe constant à chaque valeur.

```yaml
- type: suffix
  argument: ".jpg"
```

### `max`
Garde seulement la plus grande valeur entière positive.

```yaml
- type: max
```

### `resolve_url`
Résout chaque valeur comme une URL relative par rapport à l'URL de la page.

| Champ | Type | Description |
|-------|------|-------------|
| `proxy` | bool | Si true, enveloppe les URLs HTTP(S) via le proxy public |
| `proxy_headers` | object | En-têtes HTTP intégrés à l'URL proxy et envoyés en amont ; exige `proxy: true` ; les valeurs supportent les placeholders de requête, `{request_url}` et `{request_origin}` |
| `proxy_replace_all` | object[] | Règles `ReplaceAll` ordonnées jointes à l'URL proxy ; exige `proxy: true` |

```yaml
- type: resolve_url
  proxy: true
  proxy_headers:
    Referer: "{request_origin}/"
  proxy_replace_all:
    - pattern: '(?m)^(https?://[^\r\n]+)'
      replacement: '{proxy}/$1'
      content_types: [text/vtt]
```

Les valeurs de `proxy_headers` supportent les placeholders nommés de requête, `{request_url}` et `{request_origin}`. Chaque règle contient `pattern` (regex obligatoire), `replacement` (template obligatoire avec
captures `$1`, `$2`, etc. et variables proxy comme `{proxy}` et `{proxy_inherited}`) et `content_types` (liste optionnelle
de MIME types). Le proxy applique les règles au corps textuel dans l'ordre déclaré.

`{proxy}` se résout en chemin proxy public (ex. `/api/proxy`). `{proxy_inherited}` se résout en ce même chemin mais
inclut le segment `opts_…` de l'URL de requête courante lorsqu'il est présent (ex. `/api/proxy/opts_ABCD`), ce qui
permet aux règles ReplaceAll de préserver les options proxy héritées dans les URLs réécrites.

### `resolve_url_from_parent`
Résout les URLs relatives par rapport à un ancêtre de l'URL de la page.

| Champ | Type | Description |
|-------|------|-------------|
| `levels` | usize | Nombre de segments de chemin à remonter |
| `proxy` | bool | Envelopper via le proxy public |
| `proxy_headers` | object | En-têtes HTTP intégrés à l'URL proxy et envoyés en amont ; exige `proxy: true` ; les valeurs supportent les placeholders de requête, `{request_url}` et `{request_origin}` |
| `proxy_replace_all` | object[] | Règles `ReplaceAll` ordonnées jointes à l'URL proxy ; exige `proxy: true` |

```yaml
- type: resolve_url_from_parent
  levels: 2
  proxy: false
```

### `get_url_host`
Remplace chaque valeur par le hostname de l'URL parsée (supprime `www.`).

```yaml
- type: get_url_host
```

### `build_nextjs_data_url`
Convertit chaque valeur (page path) en URL `/_next/data/...json`.

| Champ | Type | Description |
|-------|------|-------------|
| `data_root` | string (opt.) | Préfixe avant `/_next/data` (ex: `/rtlplay`) |
| `route_prefix` | string (opt.) | Préfixe entre le build id et le path |
| `page_path_prefix_to_strip` | string (opt.) | Préfixe à retirer du path courant |

```yaml
- type: build_nextjs_data_url
  data_root: "/site"
  route_prefix: "detail"
```

### `ratio`
Multiplie les valeurs numériques par un facteur.

```yaml
- type: ratio
  argument: 0.5
```

### `replace_text`
Remplace toutes les occurrences d'une chaîne par une autre.

```yaml
- type: replace_text
  search: "&amp;"
  replace: "&"
```

### `format_text`
Formate chaque valeur via un template.

Placeholders disponibles :
- `{}` = valeur courante
- `{request_url}` = URL de la requête
- `{base_url}`, `{locale}`, etc. = paramètres d'exécution

```yaml
- type: format_text
  argument: "{base_url}/image/{}.jpg"
```

### `build_url`
Construit une URL à partir de plusieurs champs JSON de la réponse.

| Champ | Type | Description |
|-------|------|-------------|
| `base` | string | Template d'URL avec placeholders nommés |
| `fields` | mapping | Mapping : placeholder → pointeur JSON |

```yaml
- type: build_url
  base: "{base_url}/{program_code}-p_{program_id}/{clip_code}-c_{clip_id}"
  fields:
    program_code: "/program/code"
    program_id: "/program/id"
    clip_code: "/clip/code"
    clip_id: "/clip/id"
```

### `extract_field`
Extrait un champ du corps JSON de la réponse par pointeur JSON.

```yaml
- type: extract_field
  path: "/data/build_id"
```

### `get_date`
Normalise les dates au format `YYYY-MM-DD`.

| Champ | Type | Description |
|-------|------|-------------|
| `format` | string ou array | Format(s) d'entrée essayés dans l'ordre |
| `months` | mapping (opt.) | Table de correspondance des mois (nécessaire pour `dd_month_yyyy`) |

Formats supportés (`GetDateSource`) :

| Valeur YAML | Description |
|-------------|-------------|
| `yyyy_mm_dd` | `2026-03-25` ou `2026-03-25T20:15:00+01:00` |
| `dd_mm_yyyy` | `25/03/2026` ou `25.03.2026` |
| `dd_mm_yy` | `25/03/26` ou `25.03.26` (pivot 70 ans : 70-99→19xx, 0-69→20xx) |
| `dd_month_yyyy` | `mar. 25 mars 2026` (nécessite `months`) |
| `days_from_today` | `"3"` → aujourd'hui + 3 jours |
| `yyyy_mm_dd_hh_mm_ss` | `2026-03-25T20:15:00+01:00` ou `2026-03-25 20:15:00` |

```yaml
- type: get_date
  format: yyyy_mm_dd

# Plusieurs formats essayés dans l'ordre
- type: get_date
  format:
    - yyyy_mm_dd
    - dd_mm_yyyy
    - dd_month_yyyy
  months:
    janv: 1
    févr: 2
    mars: 3
    avril: 4
    mai: 5
    juin: 6
    juil: 7
    août: 8
    sept: 9
    oct: 10
    nov: 11
    déc: 12
```

### `normalize_duration`
Parse les durées lisibles (ex: `39 min`, `1 h 05 min`) en secondes.

```yaml
- type: normalize_duration
```

### `base64_decode`
Décode chaque valeur en Base64 (alphabet standard). Les valeurs non décodables sont conservées telles quelles.

```yaml
- type: base64_decode
```

### `unpack_packer`
Dépaquette le format déterministe Dean Edwards Packer sans exécuter de JavaScript. L'action
accepte uniquement les appels dont le payload, le radix, le compteur de symboles, le dictionnaire
et le séparateur de `split` sont littéraux. Une valeur non conforme ou malformée est supprimée.

Les radices de 2 à 62 sont supportés.

```yaml
- type: unpack_packer
```

---

## 12. Post-traitements (`post_process`)

Transformations appliquées après l'extraction initiale des champs. Disponibles sur les requêtes `html` et `json`, ainsi que sur les sous-requêtes.

Tagguées par `type` en snake_case.

### `extract_regex_items`
Construit des items de groupe à partir de correspondances regex répétées dans un champ texte.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ source contenant le texte à analyser |
| `target` | string | Champ cible recevant les items extraits |
| `pattern` | string | Expression régulière avec groupes capturants |
| `entries` | array | Définition des champs extraits de chaque match |

```yaml
- type: extract_regex_items
  source: "raw_html"
  target: "episodes"
  pattern: '<a href="(.*?)">(.*?)</a>'
  entries:
    - name: url
      type: string
      capture_group: 1
      actions:
        - type: resolve_url
          proxy: true
    - name: title
      type: string
      capture_group: 2
```

Chaque `entry` dans `entries` :

| Champ | Type | Description |
|-------|------|-------------|
| `name` | string | Nom du champ de sortie (supporte `>`) |
| `type` | string (opt.) | Type JSON de sortie |
| `capture_group` | int (opt.) | Index du groupe de capture (1-based) |
| `actions` | array | Actions appliquées à la valeur extraite |

### `filter_items`
Filtre les items d'un groupe en testant un champ scalaire avec une regex.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source |
| `field` | string | Champ scalaire à tester dans chaque item |
| `pattern` | string | Regex : si `keep_matching: true`, garde les items qui matchent |
| `keep_matching` | bool | `true` (défaut) = garde les matchs, `false` = garde les non-matchs |

```yaml
- type: filter_items
  source: "videos"
  field: "format"
  pattern: "mp4|hls"
  keep_matching: true
```

### `fetch_regex_items_from_items`
Pour chaque item d'un groupe, fait une requête HTTP, applique une regex sur la réponse, et ajoute les résultats au chemin cible.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source |
| `request_field` | string | Champ de l'item contenant l'URL à fetch |
| `request_actions` | array | Actions appliquées à l'URL avant la requête |
| `target` | string | Chemin cible pour les items extraits |
| `pattern` | string | Regex appliquée sur le corps de la réponse |
| `entries` | array | Définition des champs extraits |
| `copy_item_fields` | array | Champs à copier de l'item source vers l'item cible |

```yaml
- type: fetch_regex_items_from_items
  source: "episodes"
  request_field: "detail_url"
  request_actions:
    - type: resolve_url
  target: "episodes>sources"
  pattern: '<source src="(.*?)" type="(.*?)">'
  entries:
    - name: url
      type: string
      capture_group: 1
    - name: format
      type: string
      capture_group: 2
  copy_item_fields:
    - source: "id"
      target: "episode_id"
```

### `pivot_items_by_index`
Réorganise des listes de valeurs alignées en items indexés.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source |
| `target` | string | Champ cible |
| `values_field` | string | Champ contenant les listes de valeurs |
| `nested_field` | string | Champ dans values_field à utiliser comme pivot |
| `nested_value_field` | string | Sous-champ de la valeur à pivoter |
| `sort_by` | string (opt.) | Champ de tri des items résultants |
| `copy_item_fields` | array | Champs à copier de l'item source |
| `copy_root_fields` | array | Champs racine à copier |
| `promote_first_nested_fields` | array | Champs à promouvoir du premier item |
| `copy_target_fields` | array | Champs à copier dans l'item cible |
| `generated_fields` | array | Champs générés (index 1-based) |

Structure `ScraperFieldMapping` (pour `copy_item_fields`, `copy_root_fields`, `copy_target_fields`) :

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Chemin source |
| `target` | string | Chemin destination |

Structure `ScraperGeneratedField` :

| Champ | Type | Description |
|-------|------|-------------|
| `name` | string | Nom du champ |
| `type` | string (opt.) | Type JSON |
| `format` | string | Template où `{}` = index 1-based |

```yaml
- type: pivot_items_by_index
  source: "qualities"
  target: "sources"
  values_field: "values"
  nested_field: "format"
  nested_value_field: "url"
  sort_by: "height"
  generated_fields:
    - name: "label"
      type: string
      format: "Source {}"
  copy_item_fields:
    - source: "id"
      target: "item_id"
```

### `compute_items_field`
Calcule un champ scalaire via une expression mathématique par item.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source |
| `nested_source` | string (opt.) | Sous-champ du groupe |
| `target` | string | Champ cible pour le résultat |
| `expression` | string | Expression mathématique |
| `variables` | mapping | Variables disponibles dans l'expression |

Structure `ScraperComputedFieldVariable` :

| Champ | Type | Description |
|-------|------|-------------|
| `path` | string (opt.) | Chemin de résolution (défaut : nom de la variable) |
| `scope` | enum | `auto` (défaut), `current`, `parent`, `root`, `params` |

```yaml
- type: compute_items_field
  source: "qualities"
  target: "height"
  expression: "{height} * 2"
  variables:
    height:
      path: "height"
      scope: current
```

Portées des variables :

| Valeur | Description |
|--------|-------------|
| `auto` (défaut) | Cherche dans : item courant → item parent → racine → paramètres |
| `current` | Item en cours de traitement |
| `parent` | Item parent du `nested_source` |
| `root` | Nœud racine de la requête |
| `params` | Paramètres d'exécution |

### `derive_pagination`
Dérive les métadonnées de pagination à partir des champs extraits.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string (opt.) | Champ source (optionnel) |
| `entries_field` | string | Champ contenant les entrées (défaut: `"entries"`) |
| `current_page_field` | string | Champ contenant la page courante (défaut: `"current_page"`) |
| `total_pages_field` | string (opt.) | Champ contenant le nombre total de pages |
| `have_more_field` | string | Champ booléen "a plus" (défaut: `"have_more"`) |
| `next_value_field` | string (opt.) | Champ contenant la valeur de la prochaine page |
| `next_param` | string | Nom du paramètre pour la prochaine page (défaut: `"page"`) |
| `source_params_target` | string | Champ cible pour les paramètres source (défaut: `"source_params"`) |
| `page_size_field` | string (opt.) | Champ contenant la taille de page |
| `infer_from_full_page_field` | string (opt.) | Champ pour inférer depuis une page complète |
| `remove_fields` | array | Champs à supprimer après dérivation |

```yaml
- type: derive_pagination
  current_page_field: "page"
  have_more_field: "has_next"
  next_param: "page"
  remove_fields:
    - "page"
    - "has_next"
```

### `append_static_items`
Ajoute des items statiques à un groupe, avec déduplication optionnelle.

| Champ | Type | Description |
|-------|------|-------------|
| `target` | string | Champ groupe cible |
| `items` | array | Liste d'items (chaque item est une map string→string) |
| `unique_field` | string (opt.) | Champ utilisé pour dédupliquer |

```yaml
- type: append_static_items
  target: "links"
  items:
    - label: "Accueil"
      url: "{base_url}"
    - label: "Contact"
      url: "{base_url}/contact"
  unique_field: "url"
```

### `set_nested_fields`
Définit un ou plusieurs champs sur chaque item d'un sous-tableau, avec une
valeur statique ou une copie depuis un champ voisin.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source contenant les items |
| `nested_source` | string | Sous-champ tableau à l'intérieur de chaque item source |
| `fields` | array | Définitions de champs à appliquer |

Structure `ScraperNestedFieldDefinition` :

| Champ | Type | Description |
|-------|------|-------------|
| `target` | string | Chemin de destination (supporte `>` pour les chemins hiérarchiques) |
| `value` | string (opt.) | Valeur statique à écrire (mutuellement exclusif avec `copy_from` et `host_from`) |
| `copy_from` | string (opt.) | Champ source à copier depuis le même item (mutuellement exclusif avec `value` et `host_from`) |
| `host_from` | string (opt.) | Extrait le nom d'hôte de l'URL contenue dans ce champ voisin (mutuellement exclusif avec `value` et `copy_from`) |

```yaml
- type: set_nested_fields
  source: "episodes"
  nested_source: "players"
  fields:
    - target: resolver > kind
      value: "stream-resolver"
    - target: resolver > target_id
      copy_from: web-link
    - target: name
      host_from: web-link
```

---

## 13. Types de sortie (`type` / `ScraperOutputType`)

| Valeur YAML | Description |
|-------------|-------------|
| `string` | Chaîne de caractères |
| `number` | Nombre (entier ou flottant) |
| `boolean` | Booléen |
| `object` | Objet JSON |
| `string[]` | Tableau de chaînes |
| `number[]` | Tableau de nombres |
| `boolean[]` | Tableau de booléens |
| `object[]` | Tableau d'objets |

---

## 14. Modèle de données de sortie (`ScraperDataNode`)

Le résultat d'une requête est un arbre de nœuds. Chaque nœud peut contenir :

- `values` — valeurs scalaires
- `children` — sous-nœuds nommés (chemins hiérarchiques)
- `items` — items explicites (tableaux d'objets)
- `output_type` — type JSON déclaré

Les chemins hiérarchiques sont exprimés avec `>` dans les noms :
```yaml
name: "media>images>poster"
```

Ceci crée la structure :
```json
{ "media": { "images": { "poster": "..." } } }
```

---

## 15. Exemple complet

```yaml
id: example-source
title: "Source d'exemple"
description:
  fr: "Une source d'exemple complète"
parameters:
  - name: base_url
    value: "https://www.example.com"
  - name: api_url
    value: "{base_url}/api"

http:
  mode: auto
  user_agent_profile: chrome

queries:
  # Requête statique pour les métadonnées
  - name: service_metadata
    scraper_type: static
    media_types: [metadata]
    entries:
      - name: name
        type: string
        value: "Example Source"
      - name: logo
        type: string
        value: "{base_url}/logo.png"
        actions:
          - type: resolve_url
            proxy: true

  # Requête JSON pour la recherche
  - name: search
    scraper_type: json
    base_url: "{base_url}"
    media_types: [movie, series]
    query_url: "{api_url}/search?q={query}&page={page}"
    row_pointer: "/results/*"
    entries:
      - name: id
        type: string
        pointer: "/id"
      - name: title
        type: string
        pointer: "/title"
      - name: type
        type: string
        pointer: "/type"
      - name: year
        type: number
        pointer: "/year"
      - name: poster
        type: string
        pointer: "/poster"
        actions:
          - type: resolve_url
            proxy: true
    sub_queries:
      - scraper_type: json
        context_pointer: "/results/*"
        target: "details"
        request_pointer: "/detail_url"
        request_actions:
          - type: resolve_url
        row_pointer: "/data/*"
        entries:
          - name: synopsis
            type: string
            pointer: "/synopsis"
          - name: duration
            type: number
            pointer: "/duration"
            actions:
              - type: normalize_duration
          - name: genres
            type: string[]
            pointer: "/genres/*"
    post_process:
      - type: derive_pagination
        current_page_field: "page"
        have_more_field: "has_next"
        next_param: "page"

  # Requête HTML pour les détails
  - name: details
    scraper_type: html
    base_url: "{base_url}"
    media_types: [movie, series]
    query_url: "/program/{id}"
    row_selector: "main.content"
    entries:
      - name: title
        type: string
        selector: "h1.title"
        actions:
          - type: get_text
      - name: description
        type: string
        selector: "div.description"
        actions:
          - type: html_to_text
      - name: duration
        type: number
        selector: "span.duration"
        actions:
          - type: get_text
          - type: normalize_duration
      - name: thumbnail
        type: string
        selector: "meta[property='og:image']"
        actions:
          - type: get_attribut
            argument: content
      - name: images
        type: object[]
        selector: "img.gallery"
        entries:
          - name: url
            type: string
            actions:
              - type: get_attribut
                argument: src
              - type: resolve_url
                proxy: true
          - name: alt
            type: string
            actions:
              - type: get_attribut
                argument: alt

  # Requête texte (CSV)
  - name: episodes_csv
    scraper_type: text
    base_url: "{base_url}"
    media_types: [episodes]
    query_url: "/episodes.csv"
    row_delimiter: "\n"
    field_delimiter: ","
    entries:
      - name: id
        type: string
        field: 1
      - name: season
        type: number
        field: 2
      - name: episode
        type: number
        field: 3
      - name: title
        type: string
        field: 4
      - name: duration_min
        type: number
        field: 5
        actions:
          - type: ratio
            argument: 60
```
