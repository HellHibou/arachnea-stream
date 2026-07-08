# Standardisation YAML — Analyse d'impact

## Résumé

Deux changements de nommage dans les fichiers YAML et le frontend, sans rétro-compatibilité :

1. **Dans les catégories : `request > query_url` → `link`**
2. **Dans les sections : `items` → `entries`**

---

## 1. Contexte technique : comment `request > query_url` fonctionne

Dans les entries YAML, le `>` dans le nom d'une entry (`name: request > query_url`) n'est **pas** un simple nom de champ : c'est un chemin hiérarchique. Le résultat de cette entry est placé dans une sous-structure :

```yaml
# YAML
- name: request > query_url
  type: string
  actions:
    - type: format_text
      argument: "{base_url}/wp-json/apiflix/v1/options/?post_type=movies&sort=1"
```

produit un objet catégorie comme :

```json
{
  "key": "movies",
  "label": "Films",
  "request": {
    "query_url": "{base_url}/wp-json/apiflix/v1/options/?post_type=movies&sort=1"
  }
}
```

Ce champ est ensuite lu par les sub-queries via `request_pointer: /request/query_url`.

### Changement

Remplacer `name: request > query_url` par `name: link` dans les entries catégories. L'objet produit deviendra :

```json
{
  "key": "movies",
  "label": "Films",
  "link": "{base_url}/wp-json/apiflix/v1/options/?post_type=movies&sort=1"
}
```

Les sub-queries qui référencent ce champ avec `request_pointer: /request/query_url` devront utiliser `request_pointer: /link`.

---

## 2. Fichiers impactés — Modifications effectuées

### 2.1 YAML services

#### `server/services/darkstream/coflix.yaml`

**3 occurrences** de `request > query_url` → `link` (catégories movies, series, animes).

Aucun `request_pointer` à mettre à jour — ce fichier n'utilise pas de sous-requête liée aux catégories.

#### `server/services/darkstream/anime-sama.yaml`

**6 occurrences** de `request > query_url` → `link` (3 dans `load_home`, 3 dans `get_category`).

Aucun `request_pointer` à mettre à jour.

#### `server/services/darkstream/animeultime.yaml`

**3 occurrences** de `request > query_url` → `link` (catégories animes, dramas, tokusatsu).

Aucun `request_pointer` à mettre à jour.

#### `server/services/darkstream/frenchanimes.yaml`

**3 occurrences** de `request > query_url` → `link` (catégories vf, vostfr, films).

#### `server/services/rtlplay-be.yaml`

**4 occurrences** de `request > query_url` → `link` (2 dans `storefront_entries`, 2 dans `section_entries`).

**`items:` ligne 1140** : conservé tel quel. Il s'agit de la config du post-processeur `append_static_items`, pas d'une structure de données section. Ce champ est propre au format de configuration du post-processeur et n'est pas lié au nommage des sections.

#### `server/services/francetv.yaml`

Pas d'occurrence de `request > query_url` dans les catégories.

#### `server/services/tf1-fr.yaml`

Pas d'occurrence.

#### `server/services/m6play-fr.yaml`

Pas d'occurrence.

#### `server/services/rtbf-auvio-be.yaml`

Pas d'occurrence.

---

### 2.2 Frontend — `front/src/services/rustify.ts`

#### `normalizeHomeSection` (lignes ~806-809)

Actuellement, lecture de `record.items` avec fallback sur `record.entries` :

```typescript
let rawItems = readRecordList(record.items)
if (rawItems.length === 0) {
  rawItems = readRecordList(record.entries)
}
```

Devient (uniquement `entries`) :

```typescript
const rawItems = readRecordList(record.entries)
```

#### `normalizeHomeSectionSources` (ligne ~847)

Actuellement, le `link` de la section est résolu avec fallback sur `query_url` :

```typescript
const link = firstNonEmptyString([record.link, record.query_url, record.queryUrl])
```

Devient (uniquement `link`, plus de `query_url`) :

```typescript
const link = firstNonEmptyString([record.link])
```

---

### 2.3 Frontend — Types (`front/src/types/home.ts`)

Aucun changement nécessaire. Les interfaces `HomeSection` utilisent déjà `items` comme nom de propriété interne au frontend :
- `items: MediaItem[]` dans `HomeSection`
- `link: string` dans `HomeSectionSource`

Ces propriétés sont le résultat de la normalisation (transformation du backend → format frontend). Les noms de champs dans le flux JSON backend/frontend sont traités dans `rustify.ts` uniquement.

---

## 3. Rust backend — Changement nul

Aucun changement dans le backend Rust (`arachnea-scrapyfy`). Le moteur traite les noms d'entries comme des clés de sortie JSON de manière générique.

Aucun fichier ne référençait `/request/query_url` dans un `request_pointer`. Les seuls `request_pointer` existants pointent vers d'autres champs (ex: `/id` dans `coflix.yaml`) et restent valides.

---

## 4. Checklist des changements effectués

### YAML — `request > query_url` → `link` (19 occurrences)

- [x] `server/services/darkstream/coflix.yaml` : 3 entries
- [x] `server/services/darkstream/anime-sama.yaml` : 6 entries
- [x] `server/services/darkstream/animeultime.yaml` : 3 entries
- [x] `server/services/darkstream/frenchanimes.yaml` : 3 entries
- [x] `server/services/rtlplay-be.yaml` : 4 entries

### YAML — `items` → `entries`

- [ ] `server/services/rtlplay-be.yaml` ligne 1140 : **non modifié** — champ du post-processeur `append_static_items`, pas une section

### Frontend

- [x] `front/src/services/rustify.ts` : suppression du fallback `record.items` dans `normalizeHomeSection`
- [x] `front/src/services/rustify.ts` : suppression des fallbacks `record.query_url` / `record.queryUrl` dans `normalizeHomeSectionSources`

---

## 5. Résumé des changements

| Avant | Après | Contexte |
|-------|-------|---------|
| `name: request > query_url` | `name: link` | 19 occurrences dans 5 fichiers YAML |
| `request_pointer: /request/query_url` | — aucun trouvé — | Aucun fichier ne référençait ce chemin |
| `items:` (section) | `entries:` | **Non modifié** — l'unique occurrence (`rtlplay-be.yaml`) est un post-processeur |
| `record.items` (fallback) | — supprimé — | `normalizeHomeSection` (frontend) |
| `record.query_url, record.queryUrl` (fallback) | — supprimé — | `normalizeHomeSectionSources` (frontend) |
