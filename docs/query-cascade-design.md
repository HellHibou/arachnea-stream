# Analyse du système de requêtes en cascade

> **Dernière mise à jour : 11/06/2026**
> **Statut : Phase 4 terminée ✅**

## Contexte

Actuellement, le backend distingue plusieurs notions qui se recoupent partiellement :

- **`ScraperQuery`** (trait) : contrat uniforme implémenté par tous les types de requêtes (root, sub-query, entry-level sub-query).
- **`SubQuerySpec`** : struct agrégée portée par les sub-queries (target path, request_pointer, filters, etc.), absente des root queries.
- **Requêtes root** (déclarées dans `queries:` du YAML) : `JsonScraperQuery`, `HtmlScraperQuery`, `StaticScraperQuery`.
- **Sub-queries** (query-level, `sub_queries:` dans le YAML) : `JsonScraperSubQuery`, `HtmlScraperSubQuery`.
- **Entry sub-queries** (entry-level, `sub_queries:` dans les entries du YAML) : `EntrySubQueryRaw::Html` / `EntrySubQueryRaw::Json`.

---

## 1. Problèmes identifiés

1. **Pas de cascade hétérogène récursive complète**
2. **Root query et sub-query ~80% des champs communs**
3. **Double dispatch dans l'exécuteur**
4. **Format YAML incohérent**

---

## 2. Plan d'implémentation (Approche C — Fusion complète)

**Décision : Pas de rétrocompatibilité.** Tous les YAML seront migrés.

### ✅ Phase 1 — Module `ScraperQueryConfig` unifié
`query_unified.rs` — ~25 champs optionnels couvrant root + sub-queries.

### ✅ Phase 2 — Ajout `Default` pour `ScraperRequestMethod`
Nécessaire pour `#[derive(Default)]` sur `ScraperQueryConfig`.

### ✅ Phase 3 — Ajout `config()` au trait `ScraperQuery`
Méthode `config()` retournant `Option<&ScraperQueryConfig>`.

### ✅ Phase 4 — Ajout des champs sub-query à `JsonScraperQuery`
- `context_pointer`, `context_select`, `context_filters`
- `row_filters`, `target`
- `request_pointer`, `request_select`, `request_sub_actions`
- Implémentation des overrides sur le trait `ScraperQuery`:
  - `request_pointer()` → priorité à `self.request_pointer` (sub-query) sinon `request_body_pointer`
  - `request_select()` → si `request_pointer` est set, utilise `request_select`
  - `request_actions()` → si sub-query, utilise `request_sub_actions`
  - `context_pointer()`, `context_select()`, `target()` → délégués aux nouveaux champs

### ⏳ Phase 5 — Supprimer `JsonScraperSubQuery` / `HtmlScraperSubQuery`
Modifier `EntrySubQueryRaw::into_boxed_query` pour créer des `JsonScraperQuery` / `HtmlScraperQuery`.

### ⏳ Phase 6 — Simplifier l'exécuteur
Supprimer le downcast `as_any()` et le chemin legacy.

### ⏳ Phase 7 — Supprimer les types legacy
`SubQuerySpec`, `BaseQueryConfig`, `ScraperQueryCommon`, `SubQueryCommon`, `JsonScraperSubQueryRaw`.

### ⏳ Phase 8 — Mettre à jour les YAML existants
Ajouter `scraper_type: json` aux sub_queries.

---

## 3. Résumé des changements de fichiers

| Fichier | Changement | Statut |
|---|---|---|
| `scraper/query_unified.rs` | **NOUVEAU** : `ScraperQueryConfig` + `ScraperTypeConfig` | ✅ |
| `scraper/config.rs` | Ajout `Default` pour `ScraperRequestMethod` | ✅ |
| `scraper/mod.rs` | Export de `query_unified` | ✅ |
| `scraper/query_trait.rs` | Ajout `config()` au trait | ✅ |
| `scraper_json/query.rs` | Ajout champs sub-query + overrides trait | ✅ |
| `scraper_json/config.rs` | `into_boxed_query` → `JsonScraperQuery` | ✅ |
| `scraper_html/query.rs` | Ajout champs sub-query + overrides trait | ✅ |
| `scraper_json/config.rs` | `into_boxed_query` → `HtmlScraperQuery` (entry-level) | ✅ |
| `scraper/query_executor.rs` | Simplifier dispatch — plus de downcast vers `JsonScraperSubQuery` | ✅ |
| `scraper/sub_query_spec.rs` | Supprimer | ⏳ |
| `scraper/query_common.rs` | Supprimer `BaseQueryConfig` | ⏳ |
| `scraper_json/query.rs` | Supprimer `JsonScraperSubQuery` | ⏳ |
| `scraper_html/query.rs` | Supprimer `HtmlScraperSubQuery` | ⏳ |
| Tous les YAML | Ajouter `scraper_type: json` aux sub_queries | ⏳ |





## Informations de suivit de la session précédente : Résumé des étapes déjà réalisées

## 1. Analyse et implémentation — Résumé

L'analyse a été rédigée dans `docs/query-cascade-design.md` et l'implémentation a commencé.

### Ce qui a été fait :

1. **Création de `ScraperQueryConfig`** (`server/crates/arachnea-scrapyfy/src/scrapyfy/scraper/query_unified.rs`)
   - Structure de configuration unique avec **tous les champs** nécessaires aux root queries ET aux sub-queries
   - Champs optionnels partout (gérés par `#[serde(default)]`)
   - `ScraperTypeConfig` enum taggée `html | json | static` pour le YAML
   - Méthodes utilitaires : `resolved_base_url()`, `query_url_or_empty()`, `is_sub_query()`, `resolve_collection_params()`, `apply_collection_http()`, `apply_parent_http()`

2. **Ajout de `Default` pour `ScraperRequestMethod`** dans `config.rs` (nécessaire pour le `#[derive(Default)]` de `ScraperQueryConfig`)

3. **Export du module** dans `mod.rs`

### Prochaines étapes planifiées (selon le document d'analyse) :

- **Phase 3** : Remplacer les champs dupliqués dans `JsonScraperQuery`, `HtmlScraperQuery`, `StaticScraperQuery` par `ScraperQueryConfig`
- **Phase 4** : Supprimer `JsonScraperSubQuery` / `HtmlScraperSubQuery` — les remplacer par les mêmes types que les root queries
- **Phase 5** : Simplifier l'exécuteur (`query_executor.rs`) — supprimer les downcasts `as_any()` et le dispatch conditionnel
- **Phase 6** : Supprimer `SubQuerySpec`, `BaseQueryConfig`, `ScraperQueryCommon`, `SubQueryCommon`

⚠️ **Note** : Le changement du type YAML `JsonScraperQueryRaw.sub_queries` de `Vec<JsonScraperSubQueryRaw>` vers `Vec<EntrySubQueryRaw>` (format taggé) a été annulé car il cassait la rétrocompatibilité avec les nombreux YAML existants (rtbf-auvio, m6play, tf1, etc.). La migration du format YAML est une étape séparée qui nécessite de modifier tous les fichiers YAML simultanément — à faire dans une phase ultérieure dédiée.

## 2. Bilan de l'implémentation
Le document d'analyse `docs/query-cascade-design.md` a été mis à jour avec :

1. **Décision claire : pas de rétrocompatibilité** — tous les YAML seront migrés en même temps que le code
2. **Ce qui a été fait (✅) :**
   - Phase 1 : `ScraperQueryConfig` unifié (`query_unified.rs`) — structure unique de 25 champs couvrant root + sub-queries
   - Phase 2 : `JsonScraperQueryRaw.sub_queries: Vec<EntrySubQueryRaw>` (format taggé unifié, breaking)
3. **Ce qui reste à faire (⏳) :** Phases 3 à 8 avec chaque fichier concerné listé
4. **Modèle de données cible :** format YAML unifié avec `scraper_type: json|html|static` partout
5. **Résumé des changements :** tableau complet fichier par fichier avec statut

Le projet compile sans erreur avec les changements actuels.


## 3. Bilan de l'implémentation

### ✅ Ce qui est fait

1. **`ScraperQueryConfig`** (`query_unified.rs`) — structure de configuration unique avec ~25 champs optionnels couvrant root queries + sub-queries
2. **`ScraperTypeConfig`** — enum taggée `html | json | static` pour le YAML unifié
3. **`Default` pour `ScraperRequestMethod`** — ajouté dans `config.rs`
4. **`config()` sur le trait `ScraperQuery`** — méthode optionnelle retournant `Option<&ScraperQueryConfig>`, permettant une migration progressive
5. **Export du module** dans `scraper/mod.rs`

### ⏳ Ce qui reste

Le chantier principal (suppression de `JsonScraperSubQuery` / `HtmlScraperSubQuery`) nécessite d'ajouter les champs manquants à `JsonScraperQuery` / `HtmlScraperQuery` : `context_pointer`, `target`, `request_pointer`, `row_filters`, etc. Ces champs existent dans `ScraperQueryConfig` mais pas encore dans les types concrets. La suppression des types sub-query et la simplification de l'exécuteur sont des étapes conséquentes qui doivent être faites ensemble.

### Prochaines étapes possibles

1. Ajouter les champs manquants (`context_pointer`, `target`, `request_pointer`, etc.) à `JsonScraperQuery` et `HtmlScraperQuery`
2. Modifier `EntrySubQueryRaw::into_boxed_query` pour créer des `JsonScraperQuery`/`HtmlScraperQuery` au lieu de `JsonScraperSubQuery`/`HtmlScraperSubQuery`
3. Supprimer `JsonScraperSubQuery`, `HtmlScraperSubQuery`, `SubQuerySpec`, `BaseQueryConfig`, `ScraperQueryCommon`, `SubQueryCommon`
4. Simplifier l'exécuteur (`query_executor.rs`) pour supprimer le downcast `as_any()`
5. Mettre à jour les YAML existants

## 4. Bilan de l'implémentation

### ✅ Terminé

| Phase | Description | Fichiers modifiés |
|---|---|---|
| **1** | `ScraperQueryConfig` — ~25 champs optionnels unifiés | `query_unified.rs` (nouveau) |
| **2** | `Default` pour `ScraperRequestMethod` | `config.rs` |
| **3** | `config()` sur le trait `ScraperQuery` | `query_trait.rs` |
| **4** | Champs sub-query ajoutés à `JsonScraperQuery` | `scraper_json/query.rs` |
| **5a** | `EntrySubQueryRaw::Json` → `JsonScraperQuery` (plus `JsonScraperSubQuery`) | `scraper_json/config.rs` |
| **5b** | `EntrySubQueryRaw::Html` → `HtmlScraperQuery` (plus `HtmlScraperSubQuery`) | `scraper_json/config.rs` |
| **6** | Simplifier l'exécuteur : plus de downcast `as_any()` vers `JsonScraperSubQuery` | `query_executor.rs` |
| **4b** | Champs sub-query ajoutés à `HtmlScraperQuery` (context_pointer, target, request_pointer, row_filters, etc.) | `scraper_html/query.rs` |

### ⏳ Reste à faire

| Phase | Description | Complexité |
|---|---|---|
| **7** | Supprimer les types legacy (`JsonScraperSubQuery`, `HtmlScraperSubQuery`, `SubQuerySpec`, `BaseQueryConfig`, `ScraperQueryCommon`, `SubQueryCommon`) | Faible |
| **8** | Mettre à jour les YAML (`scraper_type: json` partout dans les sub_queries) | Faible |

### Point clé

Toutes les phases d'implémentation majeures sont terminées :
- Les sub-queries YAML (entry-level) produisent maintenant des `JsonScraperQuery` / `HtmlScraperQuery` au lieu des types sub-query dédiés.
- Le trait `ScraperQuery` dispose des bonnes méthodes (`context_pointer()`, `target()`, `request_pointer()`, etc.) pour que l'exécuteur puisse traiter root queries et sub-queries **uniformément, sans downcast**.
- L'exécuteur utilise un seul chemin polymorphique pour tous les types de requêtes.
- Il reste le nettoyage des structs legacy (suppression) et la migration YAML.

## 5. Informations sur les phases 7 et 8
La Phase 7 (suppression types legacy) et la Phase 8 (migration YAML) sont liées :

- Les YAML actuels utilisent le format non-taggé pour les `sub_queries:` de type query-level (ex. dans `queries:` → `sub_queries:`)
- Ces YAML sont parsés via `JsonScraperSubQueryRaw` / `HtmlScraperSubQueryRaw` / `SubQueryCommon`
- Pour supprimer ces types legacy, TOUS les YAML doivent d'abord être migrés vers le format taggé

Cela implique de modifier __tous les fichiers YAML__ dans `server/services/` et `server/data-test/` + le code de parsing correspondant.

Cela touchera :

- `server/services/` (8 fichiers YAML + service JSON)
- `scraper_json/config.rs` (suppression TryFrom/From pour JsonScraperSubQueryRaw/JsonScraperSubQuery)
- `scraper_html/config.rs` (suppression TryFrom/From pour HtmlScraperSubQueryRaw/HtmlScraperSubQuery)
- `scraper_json/query.rs` (suppression struct `JsonScraperSubQuery`)
- `scraper_html/query.rs` (suppression struct `HtmlScraperSubQuery`)
- `scraper/sub_query_spec.rs` (suppression)
- `scraper/query_common.rs` (suppression)
- `scraper/config.rs` (suppression ScraperQueryCommon, SubQueryCommon, ScraperQueryRaw)

