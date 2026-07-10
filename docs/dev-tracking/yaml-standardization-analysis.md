# Standardisation YAML — Analyse d'impact

## Résumé

Analyse des structures JSON produites par les 9 sources YAML de `server/services/arachnea-stream/`, basée sur les fichiers d'exemples générés dans `docs/dev-tracking/yaml-standardization/`.

Trois chantiers de standardisation identifiés :
1. **`request > query_url` → `link`** dans les catégories
2. **`items` → `entries`** dans les sections
3. **Suppression des champs catégories non standards** : `description` dans m6play-fr et `request > channel_label` dans tf1-fr

---

## 1. Contexte technique

### 1.1 `request > query_url` dans les catégories

Le `>` dans `name: request > query_url` crée un chemin hiérarchique :

```yaml
- name: request > query_url
  type: string
  value: "{base_url}/catalogue/?type%5B%5D=Anime"
```

produit :

```json
{
  "key": "anime",
  "label": "Animés",
  "request": {
    "query_url": "https://anime-sama.to/catalogue/?type%5B%5D=Anime"
  }
}
```

### Changement

Remplacer `name: request > query_url` par `name: link` dans les entries catégories. L'objet produit deviendra :

```json
{
  "key": "anime",
  "label": "Animés",
  "link": "https://anime-sama.to/catalogue/?type%5B%5D=Anime"
}
```

### 1.2 `items` → `entries` dans les sections

Remplacer `name: items` par `name: entries` dans les groupes d'éléments des sections.

### 1.3 Champs catégories non standards à supprimer

Deux champs sont produits par un seul service et dupliquent une information déjà disponible ailleurs :

- `categories[] > description` dans m6play-fr : à supprimer des catégories. Le libellé suffit pour la navigation.
- `categories[] > request > channel_label` dans tf1-fr : à supprimer. La valeur du label est déjà portée par `categories[] > label`.

---

## 2. État des lieux par service

### 2.1 `request > query_url` → `link` dans les catégories

| Service | Statut | Occurrences |
|---------|--------|-------------|
| anime-sama | **À faire** | 6 (3 load_home + 3 get_category) |
| animeultime | **À faire** | 3 (load_home) |
| coflix | **À faire** | 3 (load_home) |
| frenchanimes | **À faire** | 3 (load_home) |
| rtlplay-be | **À faire** | 4 (2 storefront_entries + 2 section_entries) |
| francetv | N/A | Utilise `request > query_url` via sub-queries |
| m6play-fr | N/A | Utilise `request > query_url` via sub-queries |
| rtbf-auvio-be | N/A | Utilise `request > query_url` via sub-queries |
| tf1-fr | N/A | Utilise `request > query_url` via sub-queries |

**Note** : francetv, m6play-fr, rtbf-auvio-be et tf1-fr utilisent `request > query_url` dans des sous-requêtes (sub-queries) et non dans les catégories directement. Le changement ne les impacte pas de la même manière.

### 2.2 `items` → `entries` dans les sections

| Service | Statut | Contexte |
|---------|--------|----------|
| anime-sama | **OK** | Utilise déjà `entries` |
| animeultime | **À faire** | Utilise `items` dans `load_home` (sections > items) |
| coflix | **OK** | Utilise déjà `entries` |
| frenchanimes | **À faire** | Utilise `items` dans `load_home` (sections > items) |
| francetv | **OK** | Utilise déjà `entries` |
| m6play-fr | **OK** | Utilise déjà `entries` |
| rtbf-auvio-be | **OK** | Utilise déjà `entries` |
| rtlplay-be | **OK** | Utilise déjà `entries` (sauf `append_static_items` post-processeur) |
| tf1-fr | **OK** | Utilise déjà `entries` |

### 2.3 Champs catégories non standards à supprimer

| Service | Champ | Statut | Contexte |
|---------|-------|--------|----------|
| m6play-fr | `categories[] > description` | **À faire** | Présent uniquement dans les catégories `load_home` |
| tf1-fr | `categories[] > request > channel_label` | **À faire** | Présent uniquement dans les paramètres de requête des catégories |

---

## 3. Incohérences structurelles identifiées

### 3.1 Structure des `categories`

| Propriété | Services |
|-----------|----------|
| `key` + `label` + `link` (plat) | anime-sama, animeultime, coflix, rtlplay-be |
| `key` + `label` + `request > query_url` (hiérarchique) | francetv, m6play-fr, rtbf-auvio-be, tf1-fr |
| `key` + `label` + `link` + `image` | rtbf-auvio-be, rtlplay-be |
| `key` + `label` + `request > name/source/channel/...` | francetv, tf1-fr |
| `key` + `label` + `description` | m6play-fr uniquement |
| `request > channel_label` | tf1-fr uniquement |

**Incohérence** : Mélange entre format plat (`link`) et format hiérarchique (`request > query_url`). Certains services ajoutent `image`, d'autres non.

**À supprimer** : `description` dans les catégories m6play-fr et `request > channel_label` dans les catégories tf1-fr, car ces champs sont spécifiques à un seul service et n'apportent pas de contrat frontend distinct.

### 3.2 Structure des `sections`

| Variante | Services |
|----------|----------|
| `label` + `entries` (MediaItem[]) | anime-sama, coflix, francetv, m6play-fr, rtbf-auvio-be, tf1-fr |
| `label` + `items` (MediaItem[]) | animeultime, frenchanimes |
| `label` + `entries` + `banners` + `categories` | rtlplay-be |

**Incohérence** : `items` vs `entries` pour le même concept (liste d'éléments média).

### 3.3 Structure des `players`

| Variante | Services |
|----------|----------|
| `name` + `embed-link` (direct) | anime-sama, coflix, frenchanimes |
| `name` + `resolver > kind/stream/target_id` | francetv, m6play-fr, rtbf-auvio-be, rtlplay-be, tf1-fr |
| `name` + `direct-link` (HD/SD) | animeultime |
| `name` + `embed-link` + `resolver` | tf1-fr |

**Décision** : On conserve les deux formats (`embed-link` et `resolver`). Chaque service utilise celui qui correspond à son fonctionnement (embed direct pour les sources dark, resolver avec DRM pour les sources légales).

### 3.4 Structure des `seasons`

| Variante | Services |
|----------|----------|
| `label` + `link` (simple) | anime-sama, coflix, m6play-fr, rtlplay-be |
| `label` + `episodes[]` (imbriqué) | animeultime, frenchanimes, francetv, rtbf-auvio-be |
| `languages[]` + `episodes[]` | anime-sama (unique) |

**Incohérence** : Certains services pré-chargent les épisodes dans `get_entry`, d'autres nécessitent un appel `get_season` séparé.

### 3.5 Structure des `episodes`

| Propriété | Services |
|-----------|----------|
| `title` + `episode-number` + `players[]` | anime-sama, animeultime, coflix, frenchanimes, rtlplay-be |
| `title` + `description` + `duration` + `players[]` + `img/preview` | francetv, m6play-fr, rtbf-auvio-be, rtlplay-be |
| `season-name` | francetv, m6play-fr |
| `name` (hostname du player) | anime-sama (unique) |
| `id` | coflix (unique) |

### 3.6 Champs spécifiques à certains services

| Champ | Services |
|-------|----------|
| `video/trailer` | anime-sama uniquement |
| `img/portrait` | tf1-fr uniquement |
| `img/logo` | francetv, m6play-fr, rtbf-auvio-be, rtlplay-be |
| `content-advisor` | francetv, m6play-fr, rtlplay-be |
| `rating` | animeultime, coflix |
| `director` + `casting` | coflix, frenchanimes, francetv, rtlplay-be |
| `storyboard` | m6play-fr uniquement |
| `search_themes` | anime-sama uniquement |
| `languages` (get_season) | anime-sama uniquement |

---

## 4. Standardisation des types

Les types standards suivants sont définis pour chaque propriété. Les services qui ne respectent pas ce standard doivent être corrigés.

### 4.1 `media-type` : doit être `string[]`

| Service | Query | Type actuel | À corriger |
|---------|-------|-------------|------------|
| anime-sama | entries_catalog_cards | `string` | **Oui** |
| anime-sama | entries_catalog_cards_with_other | `string` | **Oui** |
| anime-sama | search entries | `string` | **Oui** |
| anime-sama | load_home ajouts animes | `string` | **Oui** |
| animeultime | entries_top_item | `string` | **Oui** |
| animeultime | load_home items | `string` | **Oui** |
| frenchanimes | entries_catalog_row | `string` | **Oui** |
| coflix | get_category entries | `string` | **Oui** |
| coflix | search entries | `string[]` | **OK** |

**Impact frontend** : `readStringList()` (rustify.ts l.1585) gère déjà les deux formes. Aucun changement nécessaire dans le frontend après la correction des YAML.

### 4.2 `year` : doit être `number`

| Service | Query | Type actuel | À corriger |
|---------|-------|-------------|------------|
| anime-sama | get_entry | `string` (regex) | **Oui** |

Tous les autres services utilisent déjà `number`.

**Impact frontend** : `firstNumber(record.year)` (rustify.ts l.1171) ignore les strings. La correction YAML d'anime-sama réglera le problème.

### 4.3 `episode-number` : doit être `number`

| Service | Query | Type actuel | À corriger |
|---------|-------|-------------|------------|
| anime-sama | get_season generated_fields | `string` | **Oui** |
| animeultime | get_entry | `number` | **OK** |
| coflix | get_season sub_queries | `string` | **Oui** |
| frenchanimes | get_entry | `string` | **Oui** |
| francetv | get_entry episode fields | `string` | **Oui** |
| rtbf-auvio-be | get_season | `string` | **Oui** |
| rtlplay-be | get_season | `string` | **Oui** |
| tf1-fr | get_season | `string` | **Oui** |

**Impact frontend** : `episode-number` n'est pas utilisé directement par le frontend (il normalise via `normalizeEntryEpisode` qui lit `entry.title`). Aucun changement nécessaire.

### 4.4 `description` : doit être `string`

Sauf pour `service_stream_metadata` où c'est un `object` multilingue.

| Service | Query | Type actuel | À corriger |
|---------|-------|-------------|------------|
| Tous | service_stream_metadata | `object` | **OK** (cas particulier) |
| Tous | Autres queries | `string` | **OK** |

**Impact frontend** : `firstNonEmptyString([record.description])` (rustify.ts l.1107) gère les strings. Le cas `object` est géré séparément par `normalizeServiceDescription()`. Aucun changement nécessaire.

### 4.5 `title` : doit être `string`

| Service | Query | Type actuel | À corriger |
|---------|-------|-------------|------------|
| rtlplay-be | get_entry | `object` (pointer /title/label) | **Oui** |
| Tous les autres | Toutes queries | `string` | **OK** |

**Impact frontend** : `firstNonEmptyString([record.title])` (rustify.ts l.1161) ne traverse pas les objets. Si le backend rtlplay-be envoie `{ "label": "..." }`, le frontend recevra `null`. La correction YAML est nécessaire (pointer directement `/title` ou `/label` selon la structure réelle).

### 4.6 `current_page` : doit être `number`

| Service | Query | Type actuel | À corriger |
|---------|-------|-------------|------------|
| anime-sama | get_category | `string` (format_text `"{page}"`) | **Oui** |
| coflix | get_section | `string` (format_text `"{page}"`) | **Oui** |
| frenchanimes | get_section | `string` (format_text `"{page}"`) | **Oui** |
| m6play-fr | get_section | `string` (format_text `"{page}"`) | **Oui** |
| rtbf-auvio-be | get_section | `number` (pointer) | **OK** |

**Impact frontend** : `firstNumber()` (rustify.ts l.1812) ignore les strings et retourne `null`. Le fallback `1` est alors utilisé. La correction YAML (utiliser `number` au lieu de `string` via `format_text`) réglera le problème. Si le backend sérialise toujours le résultat de `format_text` en string, il faudra peut-être une action de conversion dans le YAML ou un changement côté Rust.

### 4.7 `total_pages` : doit être `number`

| Service | Query | Type actuel | À corriger |
|---------|-------|-------------|------------|
| anime-sama | get_category | `number` (via regex) | **OK** (résultat numérique) |
| frenchanimes | get_category | `number` (via regex) | **OK** (résultat numérique) |
| coflix | get_section | `number` (pointer) | **OK** |
| m6play-fr | search | `number` (pointer) | **OK** |
| rtbf-auvio-be | search | `number` (pointer) | **OK** |

**Impact frontend** : Même problème que `current_page`. À vérifier côté Rust si les regex produisent des strings ou des nombres. Aucun changement YAML nécessaire si le résultat est déjà numérique.

### 4.8 `episode` : parfois `object`, parfois absent — **OK**

| Service | Query | Type actuel | À corriger |
|---------|-------|-------------|------------|
| anime-sama | load_home ajouts animes | `object` avec `label` | **OK** |

**Impact frontend** : `firstNonEmptyString([readPath(record, 'episode', 'label')])` (rustify.ts l.1108) gère ce cas. Aucun changement nécessaire.

---

## 5. Vérification du frontend (`rustify.ts`)

### 5.1 Traces de `items` dans le frontend

| Emplacement | Ligne | Code | Statut |
|-------------|-------|------|--------|
| `normalizeHomeSection` | 839 | `readRecordList(record.entries)` | **Déjà migré** |
| `normalizeHomeSection` | 855 | `rawItems.map(...)` | **OK** — variable interne |
| `loadHomeSectionPage` | 462 | `section.items` | **OK** — propriété interne |
| `loadHomeSectionPage` | 462 | `normalizedSection.items` | **OK** — propriété interne |
| `mergeHomeSections` | 920 | `section.items` | **OK** — propriété interne |
| `mergeHomeSections` | 920 | `existingSection.items` | **OK** — propriété interne |
| `HomeSection` (home.ts:81) | | `items: MediaItem[]` | **OK** — nom interne |
| `SearchMediaItemsPage` (rustify.ts:69) | | `items: MediaItem[]` | **OK** — nom interne |

**Conclusion : Aucune trace résiduelle de `record.items`.**

### 5.2 Traces de `query_url` dans le frontend

| Emplacement | Ligne | Code | Statut |
|-------------|-------|------|--------|
| `normalizeHomeSectionSources` | 877 | `firstNonEmptyString([record.link])` | **Déjà migré** |
| `normalizeHomeCategorySource` | 805 | `readStringParamMap(record.request)` | **Conservé** — nécessaire pour 4 services |

### 5.3 Impact des champs catégories supprimés sur le frontend

| Champ supprimé | Lecture frontend | Impact |
|----------------|------------------|--------|
| `categories[] > description` (m6play-fr) | Supprimé de `HomeCategory`, de `normalizeHomeCategory`, du `HomeCategoryStrip` et de l'en-tête de catégorie | **Aligné** : les catégories frontend ne modélisent plus ce champ non standard |
| `categories[] > request > channel_label` (tf1-fr) | Aucune lecture directe. `normalizeHomeCategorySource` copie `record.request` dans `HomeCategorySource` (`Record<string, string>`) | **Aucun impact frontend direct** : la clé n'est pas typée ni affichée. Vérifier seulement côté backend si une action YAML ou un service l'utilisait implicitement |

**Conclusion** : `HomeCategory.description` est supprimé du contrat frontend. `HomeCategorySource` reste un dictionnaire générique pour transporter les paramètres de requête nécessaires au backend.

### 5.4 Modifications déjà effectuées

| Fichier | Ligne | Avant | Après |
|---------|-------|-------|-------|
| `rustify.ts` | 839 | `readRecordList(record.items)` avec fallback `record.entries` | `readRecordList(record.entries)` uniquement |
| `rustify.ts` | 877 | `firstNonEmptyString([record.link, record.query_url, record.queryUrl])` | `firstNonEmptyString([record.link])` uniquement |

### 5.5 Impact des corrections de type sur le frontend

| Correction YAML | Impact frontend |
|----------------|-----------------|
| `media-type` : `string` → `string[]` | Aucun — `readStringList()` gère les deux |
| `year` : `string` → `number` (anime-sama) | Le champ `year` sera correctement lu par `firstNumber()` |
| `episode-number` : `string` → `number` | Aucun — champ backend uniquement |
| `description` : `object` → `string` (metadata) | Aucun — contextes distincts |
| `title` : `object` → `string` (rtlplay-be) | Le titre sera correctement lu par `firstNonEmptyString()` |
| `current_page` : `string` → `number` | La pagination utilisera la bonne valeur au lieu du fallback `1` |
| `total_pages` : aucun changement nécessaire | — |

### 5.6 Types (`front/src/types/home.ts`)

**Aucun changement nécessaire.**

---

## 6. Checklist des actions restantes

### YAML — `request > query_url` → `link`

- [x] `server/services/arachnea-stream/dark-stream/anime-sama.yaml` : déjà aligné dans l'état courant
- [x] `server/services/arachnea-stream/dark-stream/animeultime.yaml` : déjà aligné dans l'état courant
- [x] `server/services/arachnea-stream/dark-stream/coflix.yaml` : déjà aligné dans l'état courant
- [x] `server/services/arachnea-stream/dark-stream/frenchanimes.yaml` : déjà aligné dans l'état courant
- [x] `server/services/arachnea-stream/legal-stream/rtlplay-be.yaml` : déjà aligné dans l'état courant

### YAML — `items` → `entries`

- [x] `server/services/arachnea-stream/dark-stream/animeultime.yaml` : sections > items → entries
- [x] `server/services/arachnea-stream/dark-stream/frenchanimes.yaml` : sections > items → entries

### YAML — Suppression des champs catégories non standards

- [x] `server/services/arachnea-stream/legal-stream/m6play-fr.yaml` : supprimer `categories[] > description`
- [x] `server/services/arachnea-stream/legal-stream/tf1-fr.yaml` : supprimer `categories[] > request > channel_label`

### YAML — Correction des types

- [x] `anime-sama.yaml` : `media-type` (`string` → `string[]`)
- [x] `animeultime.yaml` : `media-type` (`string` → `string[]`)
- [x] `frenchanimes.yaml` : `media-type` (`string` → `string[]`)
- [x] `coflix.yaml` : get_category entries `media-type` (`string` → `string[]`)
- [x] `anime-sama.yaml` : get_entry `year` (`string` → `number`)
- [x] `anime-sama.yaml` : get_season `episode-number` (`string` → `number`)
- [x] `coflix.yaml` : get_season `episode-number` (`string` → `number`)
- [x] `frenchanimes.yaml` : get_entry `episode-number` (`string` → `number`)
- [x] `francetv.yaml` : get_entry `episode-number` (`string` → `number`)
- [x] `rtbf-auvio-be.yaml` : get_season `episode-number` (`string` → `number`)
- [x] `rtlplay-be.yaml` : get_season `episode-number` (`string` → `number`)
- [x] `tf1-fr.yaml` : get_season `episode-number` (`string` → `number`)
- [x] `rtlplay-be.yaml` : get_entry `title` (`object` → `string`) — déjà aligné dans l'état courant
- [x] `anime-sama.yaml` : get_category `current_page` (`string` → `number`) — déjà aligné dans l'état courant
- [x] `coflix.yaml` : get_section `current_page` (`string` → `number`) — déjà aligné dans l'état courant
- [x] `frenchanimes.yaml` : get_section `current_page` (`string` → `number`) — déjà aligné dans l'état courant
- [x] `m6play-fr.yaml` : get_section `current_page` (`string` → `number`) — déjà aligné dans l'état courant

### Frontend — `rustify.ts`

- [x] Suppression du fallback `record.items` dans `normalizeHomeSection` (ligne 839)
- [x] Suppression des fallbacks `record.query_url` / `record.queryUrl` dans `normalizeHomeSectionSources` (ligne 877)

---

## 7. Résumé des changements

| Avant | Après | Contexte |
|-------|-------|---------|
| `name: request > query_url` | `name: link` | 19 occurrences dans 5 fichiers YAML |
| `items:` (section) | `entries:` | 2 fichiers YAML (animeultime, frenchanimes) |
| `categories[].description` | — supprimé — | m6play-fr uniquement |
| `categories[].request.channel_label` | — supprimé — | tf1-fr uniquement |
| `media-type: string` | `media-type: string[]` | 4 fichiers YAML (anime-sama, animeultime, frenchanimes, coflix) |
| `year: string` (anime-sama) | `year: number` | 1 fichier YAML |
| `episode-number: string` | `episode-number: number` | 7 fichiers YAML (anime-sama, coflix, frenchanimes, francetv, rtbf-auvio-be, rtlplay-be, tf1-fr) |
| `title: object` (rtlplay-be) | `title: string` | 1 fichier YAML |
| `current_page: string` (format_text) | `current_page: number` | 4 fichiers YAML (anime-sama, coflix, frenchanimes, m6play-fr) |
| `record.items` (fallback) | — supprimé — | `normalizeHomeSection` (frontend) — **fait** |
| `record.query_url, record.queryUrl` (fallback) | — supprimé — | `normalizeHomeSectionSources` (frontend) — **fait** |
| `record.request` (fallback catégories) | — conservé — | `normalizeHomeCategorySource` (frontend) — nécessaire pour 4 services |

---

*Document mis à jour le 10/07/2026 — Types standards définis, corrections listées avec impact frontend.*
