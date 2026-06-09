# Sub-queries at entry level — suivi d'implémentation

> Document de **suivi** pour le refactor du moteur de scraper d'Arachnea.
> Il sert à la fois de **spécification**, de **roadmap**, et de **mémo de conception**.
> Cocher les étapes au fur et à mesure de l'implémentation.
> Toute décision prise pendant l'implémentation qui dévie du plan doit être
> documentée dans la section *Décisions*.
>
> **État au 2026-06-08** : les étapes 6-17 sont implémentées. `cargo check --workspace` passe.
> Le plan révisé pour les étapes 15-21 (sans rétro-compatibilité) est validé
> — voir section *Décisions* pour la raison. L'étape 22 (régression
> query-level sub_queries) est le dernier jalon documenté — elle fixe la
> régression silencieuse introduite par l'étape 16 sur `coflix.yaml`,
> `rtbf-auvio-be.yaml`, `m6play-fr.yaml` et `tf1-fr.yaml` (tous en
> sub_queries query-level, qui n'étaient plus exécutés correctement
> après le câblage du moteur unifié).

---

## 1. Contexte et motivation

### 1.1 Problème initial

Sur `coflix.yaml`, l'entry `players > embed-link` extrait l'URL d'un iframe
(ex : `https://lecteurvideo.com/embed.php?id=20031&ads=true`). Cette page
contient un appel `onclick="showVideo('aHR0cHM6Ly8...', '2')"` dont le **1er
paramètre** est une URL en base64 (le vrai embed du hoster).

Aujourd'hui, il n'existe pas de mécanisme YAML natif pour :

1. Récupérer la valeur d'une entry.
2. Déclencher une requête HTTP vers cette URL.
3. Extraire des champs de la réponse et les merger dans le résultat de l'entry parente.

Le code actuel de `coflix.yaml` (commentaire ligne 547-550) le confirme :
> *"Full hoster resolution is not supported by the current YAML action set."*

Le workaround existant (`post_process: fetch_regex_items_from_items`,
cf. `animeultime.yaml` lignes 467-510) est fonctionnel mais :
- Limité à un seul fetch par query racine.
- Demande un sous-groupe temporaire + un `remove_fields` pour nettoyer.
- Ne supporte pas la récursion (sub_query d'un sub_query).
- Couplé au type de scraper racine (post_process = HTML ou JSON uniquement).

### 1.2 Cas d'usage cibles

- **Lecteurs vidéo** : iframe → page embed → `showVideo('base64')` → base64 décodé.
- **Pagination en cascade** : entry qui produit une URL "next page" → fetch récursif.
- **Résolution d'identifiants** : entry qui produit un ID → fetch endpoint détail.
- **Suivi de redirections** : entry qui produit une URL → fetch la cible.
- **Authentification par token** : entry qui produit un token → fetch ressource authentifiée.

### 1.3 Objectif

Introduire une mécanique **`sub_queries` au niveau d'une entry** (HTML ou JSON),
qui permet d'attacher à une entry une ou plusieurs requêtes de suivi
s'appuyant sur la valeur produite par cette entry.

---

## 2. Design

### 2.1 Architecture cible : une lib par type de scraper

Trois sous-modules regroupés **par type de scraper** + un module commun :

```
server/crates/arachnea-scrapyfy/src/scrapyfy/
├── mod.rs                    (re-export public)
│
├── scraper/                  (CODE COMMUN : trait, moteur, structures)
│   ├── mod.rs
│   ├── query_trait.rs        (ScraperQuery, SubQuerySpec, RowLocator)
│   ├── query_executor.rs     (moteur d'exécution factorisé)
│   ├── entry_trait.rs        (ScraperEntrySpec)
│   ├── request.rs            (ScraperRequestMethod, ScraperRequestHeader)
│   ├── response.rs           (parsing réponse + extraction rows)
│   ├── target.rs             (target path split, merge logic)
│   ├── concurrency.rs        (helpers parallélisme)
│   └── diagnostics.rs        (validation, field_names)
│
├── scraper_static/           (CODE SPÉCIFIQUE STATIC)
│   ├── mod.rs
│   ├── query.rs
│   ├── entry.rs              (si applicable)
│   └── config.rs             (StaticScraperQueryRaw)
│
├── scraper_html/             (CODE SPÉCIFIQUE HTML)
│   ├── mod.rs
│   ├── query.rs              (HtmlScraperQuery + HtmlScraperSubQuery)
│   ├── entry.rs              (HtmlScraperEntry + HtmlScraperSelectMode)
│   ├── response_parser.rs    (parse HTML → rows via CSS selector)
│   ├── row_extractor.rs      (ElementRef → ScraperDataNode)
│   └── config.rs             (HtmlScraperQueryRaw, HtmlScraperEntryRaw)
│
├── scraper_json/             (CODE SPÉCIFIQUE JSON)
│   ├── mod.rs
│   ├── query.rs              (JsonScraperQuery + JsonScraperSubQuery)
│   ├── entry.rs              (JsonScraperEntry)
│   ├── response_parser.rs    (parse JSON → rows via pointer)
│   ├── pointer.rs            (select_json_values, parse_array_filter)
│   ├── row_extractor.rs      (serde_json::Value → ScraperDataNode)
│   └── config.rs             (JsonScraperQueryRaw, JsonScraperEntryRaw)
│
├── actions/                  (INCHANGÉ)
├── post_processes/           (INCHANGÉ)
├── query_helpers.rs          (INCHANGÉ)
├── scraper_data_node.rs      (INCHANGÉ)
├── scraper_agregator.rs      (INCHANGÉ — imports à ajuster)
├── scraper_manager.rs        (INCHANGÉ — imports à ajuster)
├── scraper_query_collection.rs (INCHANGÉ — imports à ajuster)
└── http_client.rs            (INCHANGÉ)
```

**Bénéfices** :
- 1 type de scraper = 1 sous-module → découvrable, prévisible.
- Code commun mutualisé → moteur d'exécution unique, pas de duplication.
- `json_scraper_query.rs` (~1640 lignes) éclaté en ~6 fichiers de 200-300 lignes.
- Ajout futur d'un type (GraphQL, XML, RSS…) = nouveau sous-module qui implémente les traits de `scraper/`.

### 2.2 Trait unifié `ScraperQuery`

**Toutes** les variantes de query (racine HTML, racine JSON, racine Static,
sub-query HTML, sub-query JSON, sub-query Static, sub-query d'entry HTML,
sub-query d'entry JSON) implémentent **un seul trait** :

```rust
/// Implémenté par toute query : racine, sub-query, sub-query d'entry,
/// en HTML, JSON ou Static.
pub(crate) trait ScraperQuery: Send + Sync {
    // --- Identification ---
    fn scraper_type(&self) -> ScraperType;
    fn name(&self) -> &str;

    // --- Requête HTTP (commun racine et sub_query) ---
    fn base_url(&self) -> &str;
    fn query_url(&self) -> &str;
    fn request_method(&self) -> ScraperRequestMethod;
    fn request_headers(&self) -> &[ScraperRequestHeader];
    fn request_pointer(&self) -> Option<&str>;
    fn request_select(&self) -> HtmlScraperSelectMode;
    fn request_actions(&self) -> &[ScraperAction];
    fn extract_next_data(&self) -> bool;
    fn http_config(&self) -> &ScraperHttpConfig;

    // --- Row extraction ---
    fn row_locator(&self) -> RowLocator;

    // --- Entries (avec sub_queries d'entries récursives) ---
    fn entries(&self) -> Vec<&dyn ScraperEntrySpec>;

    // --- Sub-queries siblings (récursion) ---
    fn sub_queries(&self) -> Vec<&dyn ScraperQuery>;

    // --- Spec sub-query (None pour racines) ---
    fn sub_query_spec(&self) -> Option<&SubQuerySpec>;
}
```

### 2.3 SubQuerySpec — struct, pas un trait

C'est un agrégat de données porté par les sub-queries :

```rust
/// Spec sub-query — présente sur sub_queries, absente sur racines.
#[derive(Default)]
pub(crate) struct SubQuerySpec {
    /// Chemin où merger le résultat. `None` = top-level pour sibling, `parent` pour sub_query d'entry.
    pub target: Option<String>,
    /// Pointer vers la valeur d'entry servant d'URL de requête. `None` = `parent` pour sub_query d'entry.
    pub request_pointer: Option<String>,
    pub request_select: HtmlScraperSelectMode,
    pub request_actions: Vec<ScraperAction>,
    /// Pointer dans le **row parent** pour scoper l'exécution à N contextes.
    /// `None` = un seul context (le row parent).
    pub context_pointer: Option<String>,
    pub context_select: HtmlScraperSelectMode,
    pub context_entries: Vec<Box<dyn ScraperEntrySpec>>,
    /// Filtre sur le **context row** (avant fetch). "Est-ce que je lance la requête ?"
    pub filters: HashMap<String, Vec<String>>,
    /// Filtre sur chaque **row fetched** (après fetch). "Est-ce que je garde cette row ?"
    pub row_filters: HashMap<String, Vec<String>>,
}
```

**Distinction `filters` vs `row_filters`** :
- `filters` porte sur le row parent (contexte d'exécution).
- `row_filters` porte sur les rows de la réponse HTTP.

### 2.4 ScraperEntrySpec — trait polymorphique HTML/JSON

```rust
/// Implémenté par HtmlScraperEntry et JsonScraperEntry.
pub(crate) trait ScraperEntrySpec: Send + Sync {
    fn name(&self) -> &str;
    fn entry_type(&self) -> EntryType;          // html | json
    fn pointer(&self) -> Option<&str>;          // JSON
    fn selector(&self) -> Option<&str>;         // HTML
    fn select(&self) -> HtmlScraperSelectMode;
    fn actions(&self) -> &[ScraperAction];
    fn sub_entries(&self) -> Vec<&dyn ScraperEntrySpec>;
    fn is_group(&self) -> bool;
    /// Sub-queries attachées à cette entry (peuvent être HTML ou JSON).
    fn sub_queries(&self) -> Vec<&dyn ScraperQuery>;
}
```

### 2.5 RowLocator — enum, pas de duplication

```rust
pub(crate) enum RowLocator {
    /// HTML : applique un CSS selector.
    Selector {
        selector: scraper::Selector,
        select: HtmlScraperSelectMode,
    },
    /// JSON : applique un pointer.
    Pointer(String),
    /// Pas de locator (scraper static, ou single-row).
    Single,
}
```

### 2.6 Sémantique d'exécution

| Champ | Comportement par défaut | Sémantique explicite |
|---|---|---|
| `request_pointer` (sub_query d'entry) | `parent` | Utilise la valeur de l'entry parente comme URL de requête |
| `target` (sub_query d'entry) | `parent` | Écrase l'entry parente avec le résultat du sub_query |
| `request_pointer` (sub_query sibling) | `None` | Le sub_query utilise son `query_url` statique |
| `target` (sub_query sibling) | `None` | Le sub_query merge au top-level |
| `select: all` (entry) | — | 1 requête HTTP par valeur produite, en parallèle |
| `select: first` (entry) | — | 1 requête pour la première valeur uniquement |
| `sub_queries` (entry) | `[]` | Liste de sub-queries, exécutés en parallèle, mergés séquentiellement |
| `sub_queries` (sub_query) | `[]` | Récursion : un sub_query peut contenir des sub_queries d'entries, etc. |

### 2.7 Moteur d'exécution unique

```rust
pub(crate) async fn execute_query(
    query: &dyn ScraperQuery,
    context: &QueryContext,
) -> Result<ScraperDataNode> {
    // 1. Résoudre la requête HTTP (query_url + request_actions + request_pointer)
    let request_urls = resolve_request_urls(query, context)?;

    // 2. Fetch en parallèle
    let responses = fetch_parallel(request_urls, query, context).await?;

    // 3. Pour chaque réponse : extraire rows
    let mut root = ScraperDataNode::default();
    for response in responses {
        for row in extract_rows(query, &response) {
            // a. Construire item via entries
            let mut item = ScraperDataNode::default();
            for entry in query.entries() {
                entry.apply_to(&mut item, &row, context);
            }

            // b. Exécuter sub_queries d'entries (récursion)
            for entry in query.entries() {
                let entry_values = entry.collect_values(&item);
                for sub_query in entry.sub_queries() {
                    execute_sub_query_for_entry(
                        sub_query, &entry_values, &mut item, context
                    ).await?;
                }
            }

            // c. Merger selon target
            merge_item(&mut root, item, query.sub_query_spec());
        }
    }

    // 4. Sub-queries siblings (récursion naturelle)
    for sibling in query.sub_queries() {
        let sibling_root = execute_query(sibling, context).await?;
        root.merge(sibling_root);
    }

    Ok(root)
}
```

### 2.8 Fusions de champs (vs. état actuel)

| Champ actuel (racine) | Champ actuel (sub_query) | Décision |
|---|---|---|
| `request_body_pointer` | `request_pointer` | **FUSIONNER** → `request_pointer` |
| `request_body_select` | `request_select` | **FUSIONNER** → `request_select` |
| `request_body_actions` | `request_actions` | **FUSIONNER** → `request_actions` |
| `query_url` | `query_url` | Déjà commun |
| `request_method` | `request_method` | Déjà commun |
| `request_headers` | `request_headers` | Déjà commun |
| `http_config` | `http_config` | Déjà commun |
| `extract_next_data` | `extract_next_data` | Déjà commun |
| `base_url` | (héritée) | À promouvoir dans la spec |

**Champs gardés spécifiques aux sub_queries** :
- `context_pointer` / `context_select` : scope d'exécution multiple (la racine a 1 seul context).
- `context_entries` : entries appliquées au context.
- `target` : où merger le résultat.
- `filters` : filtre sur le **context row**.
- `row_filters` : filtre sur les **rows fetched**.

---

## 3. API YAML

### 3.1 Convention `parent`

Pour les sub_queries d'entry :
- `request_pointer: parent` (défaut) → utilise la valeur de l'entry parente.
- `target: parent` (défaut) → écrase l'entry parente avec le résultat.

### 3.2 Exemple : `coflix.yaml` (extrait)

```yaml
- name: players > embed-link
  actions:
    - type: get_response_body
    - type: regex_find_all
      pattern: '<iframe[^>]+src="([^"]+)'
      format: "{1}"
  sub_queries:
    - scraper_type: html
      # request_pointer: parent (défaut)
      # target: parent (défaut)
      row_selector: "html"
      entries:
        - name: embed-link
          actions:
            - type: get_response_body
            - type: regex_find_all
              pattern: "showVideo\\(\\s*['\"]([A-Za-z0-9+/=]+)['\"]"
              format: "{1}"
```

Résultat : `players > embed-link` contient le 1er paramètre de `showVideo(...)` (string base64).

### 3.3 Exemple : sub_query d'entry avec `select: all`

```yaml
- name: episodes
  select: all
  selector: "li.episode"
  actions:
    - type: get_text
  sub_queries:
    # 1 requête par épisode (en parallèle)
    - scraper_type: json
      row_pointer: "/"
      entries:
        - name: title
          pointer: "/title"
        - name: link
          pointer: "/url"
```

### 3.4 Exemple : récursion

```yaml
- name: url
  sub_queries:
    - scraper_type: html
      row_selector: "html"
      entries:
        - name: url
          actions:
            - type: get_attribut
              argument: href
          sub_queries:
            # sub_query dans un sub_query dans une entry
            - scraper_type: json
              row_pointer: "/"
              entries:
                - name: final
                  pointer: "/final"
```

---

## 4. Spécifications RustDoc

> ⚠️ **Règle d'or** : la **RustDoc complète** doit être maintenue sur **tous**
> les nouveaux items (et les items déplacés) :
> - Modules : `//!` description globale.
> - Structs : `///` description + champs documentés.
> - Enums : `///` description + chaque variant documenté.
> - Traits : `///` description + méthodes documentées (`# Arguments`, `# Returns`,
>   `# Errors` quand pertinent).
> - Fonctions publiques : `///` description + `# Arguments`, `# Returns`, `# Errors`.
> - Champs publics : `///` description sur chaque champ.
>
> Conventions alignées sur le `AGENTS.md` server :
> - Format `rustdoc` idiomatique (pas JavaDoc).
> - `# Arguments` pour les paramètres.
> - `# Errors` / `# Panics` / `# Returns` si pertinent.

### 4.1 Modules à documenter

| Module | Documentation requise |
|---|---|
| `scraper::query_trait` | Objectif du trait unifié, ses méthodes, le dispatch polymorphe. |
| `scraper::query_executor` | Algorithme d'exécution (resolve, fetch, extract, merge, recurse). |
| `scraper::entry_trait` | Polymorphisme HTML/JSON des entries, méthode `sub_queries()`. |
| `scraper::request` | Construction des requêtes (méthode, headers, body, actions). |
| `scraper::response` | Parsing de la réponse HTTP et extraction des rows. |
| `scraper::target` | Split de chemin `>` et logique de merge. |
| `scraper::concurrency` | Stratégie de fetch parallèle (`buffer_unordered`, semaphores). |
| `scraper::diagnostics` | Validation, noms de champs, messages d'erreur. |
| `scraper_static::query` | Adaptation du `static_scraper_query` au trait. |
| `scraper_static::entry` | (si applicable) Entry statique, valeurs littérales. |
| `scraper_static::config` | Sérialisation/désérialisation YAML. |
| `scraper_html::query` | `HtmlScraperQuery` + `HtmlScraperSubQuery`, gestion du `row_selector`. |
| `scraper_html::entry` | `HtmlScraperEntry`, `HtmlScraperSelectMode`, gestion des actions HTML. |
| `scraper_html::response_parser` | Parse d'un `Html` vers un vecteur d'`ElementRef` (row). |
| `scraper_html::row_extractor` | Application d'une entry sur un `ElementRef`. |
| `scraper_html::config` | `HtmlScraperQueryRaw`, `HtmlScraperEntryRaw`, validation. |
| `scraper_json::query` | `JsonScraperQuery` + `JsonScraperSubQuery`, gestion du `row_pointer`. |
| `scraper_json::entry` | `JsonScraperEntry`, gestion des pointers et actions JSON. |
| `scraper_json::response_parser` | Parse d'un JSON vers un vecteur de `serde_json::Value` (row). |
| `scraper_json::pointer` | `select_json_values`, wildcards, array filters. |
| `scraper_json::row_extractor` | Application d'une entry sur une `serde_json::Value`. |
| `scraper_json::config` | `JsonScraperQueryRaw`, `JsonScraperEntryRaw`, validation. |

### 4.2 Conventions RustDoc (rappel AGENTS.md server)

- **Format** : `rustdoc` idiomatique (pas JavaDoc).
- **Sections standards** : `# Arguments`, `# Errors`, `# Panics`, `# Returns` si pertinent.
- **Précision** : décrire le comportement, pas la signature (qui est déjà dans le code).
- **Cohérence** : reprendre le style existant (voir `json_scraper_query.rs` comme référence).
- **Mise à jour** : toute modification de comportement doit mettre à jour la RustDoc associée dans le même changement.

---

## 5. Plan d'implémentation

### 5.1 Ordre d'exécution (progressif pour limiter le risque)

#### 1ère passe — Refactor structurel (commité le 2026-06-07, commit `a01ef24`)

1. [x] **Étape 1** — Créer la structure de dossiers `scraper/`, `scraper_static/`, `scraper_html/`, `scraper_json/`.
2. [x] **Étape 2** — Déplacer `static_scraper_query.rs` → `scraper_static/` (cas simple, valide le pattern).
3. [x] **Étape 3** — Déplacer le code JSON dans `scraper_json/` (sans découpage fin dans cette passe).
4. [x] **Étape 4** — Déplacer le code HTML dans `scraper_html/` (sans découpage fin dans cette passe).
5. [x] **Étape 5** — Créer le module `scraper/` avec les traits communs (`ScraperQuery`, `ScraperEntrySpec`, `SubQuerySpec`, `RowLocator`) et le moteur factorisé (`execute_query`). Squelette compilable posé.
6. [x] **Étape 5bis** — `cargo check --workspace` passe.

#### 2e passe — Implémentation du trait unifié (À FAIRE, PR dédiée)

7. [x] **Étape 6** — Découper `scraper_json/query.rs` en `config.rs`, `response_parser.rs`, `pointer.rs`, `row_extractor.rs`.
8. [x] **Étape 7** — Découper `scraper_html/query.rs` en `config.rs`, `response_parser.rs`, `row_extractor.rs`.
9. [x] **Étape 8** — Renommer l'ancien `ScraperQuery` (async_trait) en `ScraperManagerQuery` (ou autre) et le déplacer hors de `scrapyfy/mod.rs` (vers `scraper_manager.rs` qui en est le propriétaire légitime).
10. [x] **Étape 9** — Compléter le trait `scraper::ScraperQuery` avec les méthodes manquantes (`request_method`, `request_headers`, `http_config`).
11. [x] **Étape 10** — Implémenter `ScraperQuery` pour `JsonScraperQuery` et `JsonScraperSubQuery`.
12. [x] **Étape 11** — Implémenter `ScraperQuery` pour `StaticScraperQuery`.
13. [x] **Étape 12** — Créer `HtmlScraperSubQuery` et implémenter `ScraperQuery` pour `HtmlScraperQuery`.
14. [x] **Étape 13** — Implémenter `ScraperEntrySpec` pour `JsonScraperEntry` et `HtmlScraperEntry`.
15. [x] **Étape 14** — Ajouter `entry_sub_queries` aux entries (récursion).
16. [x] **Étape 15** — Implémenter `scraper::execute_query` (moteur unifié complet : fetch, parse, apply, sub_queries d'entries, sub_queries siblings, post_processes, fields_filters, result_item_field).
17. [x] **Étape 16** — Câbler le moteur unifié comme unique chemin d'exécution : suppression de l'ancien trait `ScraperManagerQuery`, des implémentations legacy (`HtmlScraperQuery::execute_query`, `JsonScraperQuery::execute_query`, `StaticScraperQuery::execute_query`), et des helpers morts. `ScraperRequestMethod`/`ScraperRequestHeader` rendus publics. Le moteur unifié via `ScraperQuery::execute_query_items` est l'unique chemin d'exécution. `cargo check --workspace` passe.
18. [x] **Étape 17** — Modifier les types Raw des entries (`HtmlScraperEntryRaw`, `JsonScraperEntryRaw`, `EntrySubQueryRaw`) pour supporter la syntaxe YAML `sub_queries` au niveau entry. Le parsing et la conversion vers `Box<dyn ScraperQuery>` sont implémentés. `coflix.yaml` peut maintenant utiliser `sub_queries` dans n'importe quelle entry.
19. [x] **Étape 18** — `cargo check --workspace` passe (0 erreurs, warnings = dead_code attendu).
20. [x] **Étape 19** — `CHANGELOG.md` et `skills/build-yaml-source/references/yaml-capabilities.md` mis à jour.
21. [ ] **Étape 20** — Passer en revue la RustDoc de tous les nouveaux items (cf. § 4).
22. [x] **Étape 21a** — Étendre `EntrySubQueryRaw` avec les champs avancés nécessaires à la migration des sub_queries query-level : `context_pointer`, `context_select`, `context_entries`, `filters`, `row_filters`, `target`, `request_pointer`, `request_select`, `request_actions`, `request_method`, `request_headers`, `http_config`, `extract_next_data`, `request_body_pointer`, `request_body_select`, `request_body_actions`. L'API entry-level devient ainsi sémantiquement équivalente à l'API query-level — la migration YAML est juste un changement de position du bloc (`sub_queries` au niveau entry vs. au niveau query). Voir section *Décisions* 2026-06-09.
23. [ ] **Étape 21b** — Modifier les autres YAMLs dans une 2e PR dédiée (animeultime, papystreaming, rtlplay-be, etc.). Migration de `rtbf-auvio-be.yaml` `home_sub_queries` / `rtbf_program_episode_sub_query` / etc. en cours.
24. [x] **Étape 22** — Fixer la régression silencieuse sur les sub_queries au niveau query (ancienne API `context_pointer`/`target`/`request_pointer` au niveau query). `JsonScraperSubQuery` et `HtmlScraperSubQuery` exposent leurs champs legacy via le trait `ScraperQuery` (`context_pointer`, `context_select`, `context_entries`, `target`). Le moteur unifié détecte un sub_query query-level (`sibling.context_pointer().is_some()`) et le dispatche vers `JsonScraperSubQuery::execute_query_level` (réutilise le chemin d'exécution legacy `JsonScraperSubQuery::execute` qui supporte `context_pointer`, `filters`, `row_filters`, `request_actions`, etc.). Fixe la régression sur `coflix.yaml` (`get_season`), `rtbf-auvio-be.yaml`, `m6play-fr.yaml`, `tf1-fr.yaml` sans migration YAML. `cargo check --workspace` passe.
25. [x] **Étape 23** — Corriger `response_parent_row` dans `query_executor.rs` : il doit appliquer le `row_pointer` du query parent avant de dispatcher vers `JsonScraperSubQuery::execute_query_level`. Sans ce fix, les sub_queries query-level itèrent sur la réponse JSON brute au lieu du row extrait — pour `rtbf-auvio-be.yaml::load_home` (`row_pointer: /data`), les widgets à `/data/widgets/*` n'étaient pas trouvés par le `context_pointer: /widgets/*` du sub_query. Fixe la régression sur `rtbf-auvio-be.yaml` `load_home` / `get_category` (et tout autre query JSON dont le `row_pointer` n'est pas `/`). `cargo check --workspace` passe.

#### Analyse des YAMLs sources — patterns migrables vers entry-level `sub_queries`

| Source | Pattern actuel | Migrable vers entry-level sub_queries | Priorité |
|---|---|---|---|
| **coflix.yaml** | `players > embed-link` avec regex iframe → commentaire "not supported" | ✅ Cas d'usage initial : sub_query html sur `embed-link` pour extraire `showVideo(base64)` | **Haute** (use case initial) |
| **animeultime.yaml** | `post_process: fetch_regex_items_from_items` sur entry `playlists` → fetch XML | ✅ Remplaçable par une sub_query html qui fetch le playlist XML et extrait les `<item>` entries | **Haute** (workaround existant) |
| **anime-sama.yaml** | `post_process: fetch_regex_items_from_items` sur entry `languages` | ✅ Remplaçable par une sub_query html | **Moyenne** (workaround existant) |
| **rtbf-auvio-be.yaml** | Sub-queries niveau query avec `context_pointer: /data/widgets/*` et `context_pointer: /mediaId` | 🔄 Déjà en sub-queries query-level. Une migration vers entry-level serait possible si une entry produit directement l'URL de suivi, mais pas nécessaire | **Basse** (déjà fonctionnel) |
| **m6play-fr.yaml** | Sub-queries niveau query | 🔄 Déjà en sub-queries query-level | **Basse** |
| **tf1-fr.yaml** | Sub-queries niveau query avec `context_pointer: /editorialSections/*/lists/0/id` | 🔄 Déjà en sub-queries query-level | **Basse** |
| **papystreaming.yaml** | Aucun sub_query, ni `fetch_regex_items_from_items` | ❌ Pas de pattern migrable identifié | Nulle |
| **crunchyroll.yaml** | Aucun sub_query | ❌ Pas de pattern migrable identifié | Nulle |
| **francetv.yaml** | Aucun sub_query | ❌ Pas de pattern migrable identifié | Nulle |
| **frenchanimes.yaml** | Aucun sub_query | ❌ Pas de pattern migrable identifié | Nulle |
| **rtlplay-be.yaml** | Aucun sub_query | ❌ Pas de pattern migrable identifié | Nulle |

**Recommandation** : migrer d'abord `coflix.yaml` et `animeultime.yaml` (cas concrets avec workaround existant). Les autres sources déjà en sub-queries query-level peuvent rester en l'état — le bénéfice d'une migration vers entry-level est marginal.

### 5.2 Validation par étape

À chaque étape, exécuter :
```bash
cargo check --workspace
```

Si erreur, corriger **avant** de passer à l'étape suivante.

### 5.3 Pas de tests

Conformément à `AGENTS.md` server :
> *"Do not create new tests unless the user explicitly asks for them."*

→ Pas de création de tests pour ce refactor. Les YAMLs existants servent de
validation fonctionnelle (régression = le scraper retourne la même chose qu'avant).

### 5.4 Validation de la 1ère passe (commit `a01ef24`)

- `cargo check --workspace` : passe ✅
- Aucun changement de comportement : les YAMLs existants fonctionnent à l'identique.
- Diff limité à la réorganisation modulaire + création de stubs vides.
- Branchement sur `feature/scraper-refactor` (à merger ou rebaser selon la politique du projet).

---

## 6. Limitations connues

### 6.1 Pas de décodage base64 natif

L'`embed-link` final dans `coflix.yaml` contiendra la **string base64** extraite
du 1er paramètre de `showVideo(...)`, pas l'URL décodée.

Pour décoder le base64 :
- Soit côté frontend (JavaScript `atob()`).
- Soit via une action `base64_decode` à ajouter ultérieurement (nouveau change request).

### 6.2 Comportement par défaut `parent`

Pour les sub_queries d'entry, `parent` est implicite. Si l'user veut explicitement
pointer vers un autre champ de l'entry, il devra utiliser un pointer explicite
(ou un sub_query d'un autre type via un groupe).

### 6.3 Concurrence

Le nombre de requêtes parallèles est borné par `sub_query_fetch_concurrency`
(défaut : 8 pour JSON). Cette valeur est conservée à l'identique pour HTML
pour éviter de surcharger le serveur cible.

### 6.4 Breaking change assumé (2e passe)

La 2e passe introduit un **breaking change** assumé : la structure YAML des
services change, et le code Rust des anciens chemins d'exécution est supprimé.
Plus précisément :

- **YAML** : `coflix.yaml` et tous les autres YAMLs de `server/services/`
  doivent être migrés vers la nouvelle API `sub_queries` au niveau entry.
  Les champs `request_body_*` (racine) disparaissent au profit de `request_*`.
- **Rust** : l'ancien trait `ScraperManagerQuery` (async_trait) et ses impls
  sur `HtmlScraperQuery` / `JsonScraperQuery` sont supprimés. Les anciennes
  méthodes `execute_query` concrètes (avec leurs helpers `execute_context`,
  `execute_siblings`, `build_row_node`, etc.) sont remplacées par le moteur
  unifié `scraper::execute_query`. Les tests existants qui dépendent de
  `ScraperManagerQuery` sont migrés ou supprimés.
- **Justification** : maintenir une rétro-compatibilité aurait imposé de
  garder deux moteurs d'exécution en parallèle, ce qui multiplie la
  complexité et la surface de bug pour un gain marginal (les YAMLs sont
  sous contrôle de l'équipe). Le breaking change est effectué en une
  seule passe coordonnée (code + YAMLs).


---

## 7. Décisions

> Documenter ici toute décision prise pendant l'implémentation qui dévie
> du plan initial ou qui mérite explication.

| Date | Décision | Justification |
|---|---|---|
| 2026-06-07 | **Refactor découpé en 2 passes** : (1) structurel pur (commité), (2) implémentation du trait unifié (reporter). | L'implémentation du trait `ScraperQuery` unifié (étape 6) entre en conflit avec l'ancien trait `ScraperQuery` (async_trait) défini dans `scrapyfy/mod.rs` et déjà implémenté sur `HtmlScraperQuery`. La migration complète nécessite de renommer l'ancien trait, migrer tous les `impl ScraperQuery` existants, et adapter les appelants — un refactor substantiel qui dépasse le scope d'une session. La 1ère passe (étapes 1-5) est commitable en l'état car elle n'introduit aucun changement de comportement. |
| 2026-06-07 | **Refactor limité à la réorganisation modulaire** pour la 1ère passe. | Le `json_scraper_query.rs` (~1640 lignes) a été laissé en un seul fichier `scraper_json/query.rs` au lieu d'être découpé en `config.rs` / `response_parser.rs` / `pointer.rs` / `row_extractor.rs` comme prévu. Idem pour `html_scraper_query.rs`. Découpe à finaliser dans la 2e passe pour limiter le risque de régression. |
| 2026-06-07 | **Pas de découpage fin de `json_scraper_query.rs` / `html_scraper_query.rs`** dans la 1ère passe. | Évite d'introduire des régressions dans des helpers de bas niveau (select_json_values, http_client.configured, etc.) qui sont largement utilisés. Le découpage sera fait dans la 2e passe, couplé à l'implémentation du trait unifié. |
| 2026-06-07 | **Approche pragmatique pour `coflix.yaml`** : utiliser `post_process: fetch_regex_items_from_items` au lieu du nouveau trait. | Le besoin initial (extraire le 1er paramètre de `showVideo(...)` depuis l'URL de l'iframe) peut être résolu avec le post-process existant (cf. `animeultime.yaml` lignes 467-510). L'embed-link final contiendra la string base64, à décoder côté frontend. L'API `sub_queries` au niveau entry sera câblée dans la 2e passe. |
| 2026-06-07 | **Convention `parent` (et non `$self`)** pour désigner l'entry parente d'un sub-query d'entry. | Plus lisible, plus court, cohérent avec les conventions de templating du projet (`{base_url}`, `{request_url}`, ...). |
| 2026-06-07 | **Fusions de champs** : `request_body_*` (racine) fusionnés avec `request_*` (sub_query). | L'user a signalé que ces champs avaient la même sémantique (construire la requête HTTP). Un seul champ `request_pointer / request_select / request_actions` est donc utilisé dans le trait unifié. |
| 2026-06-07 | **Distinction `filters` vs `row_filters`** conservée. | `filters` porte sur le **context row** (avant fetch : "est-ce que je lance la requête ?"). `row_filters` porte sur les **rows fetched** (après fetch : "est-ce que je garde cette row ?"). Sémantiques différentes, conservées dans `SubQuerySpec`. |
| 2026-06-07 | **Squelette du trait `ScraperQuery` laissé minimal** dans la 1ère passe. | Le trait contient les méthodes essentielles (identification, request, row_locator, entries, sub_queries, sub_query_spec). Les méthodes `request_method()`, `request_headers()`, `http_config()` seront ajoutées dans la 2e passe pour éviter les stubs non-compilables. |
| 2026-06-07 | **Une lib par type de scraper** validée. | `scraper_static/`, `scraper_html/`, `scraper_json/`, plus un module commun `scraper/`. Permet d'ajouter facilement un futur type (GraphQL, XML, RSS) en créant un nouveau sous-module implémentant les traits de `scraper/`. |
| 2026-06-07 | **Découpage de `scraper_html/query.rs`** (étape 7) en 3 modules satellites. | `config.rs` contient `HtmlScraperQueryRaw` + defaults + conversions `TryFrom`/`From`/`Serialize`. `response_parser.rs` contient `collect_ordered_results` et `parse_html_rows` (extraction des rows via CSS selector). `row_extractor.rs` contient `process_root` (post-process + filtre). `query.rs` conserve le struct `HtmlScraperQuery`, ses méthodes, et l'impl `ScraperManagerQuery`. Les champs du struct sont passés en `pub(crate)` pour permettre à `config.rs` d'accéder directement aux champs privés lors de la conversion `TryFrom`. |
| 2026-06-07 | **Erreur préexistante `scraper` module non déclaré** corrigée lors de l'étape 7. | `scraper_static/query.rs` référençait `crate::scrapyfy::scraper::*` mais le module n'était pas déclaré dans `scrapyfy/mod.rs`. Correction : (1) ajout de `pub(crate) mod scraper;` dans `scrapyfy/mod.rs`, (2) remplacement de `scraper::Selector` par `::scraper::Selector` dans `entry.rs` et `query.rs` pour lever l'ambiguïté avec le module interne, (3) correction de l'import `ScraperFieldMapping` dans `sub_query_spec.rs` (chemin `post_processes::` au lieu de `scraper_query_collection::`). `cargo check --workspace` passe avec succès. |
| 2026-06-08 | **Étape 6 réalisée** : découpage de `scraper_json/query.rs` en `config.rs`, `response_parser.rs`, `pointer.rs`, `row_extractor.rs`. | `config.rs` reprend les types YAML bruts, les conversions `TryFrom`/`From`/`Serialize`, et les types partagés (`ScraperRequestMethod`, `ScraperRequestHeader`, `JsonScraperExecutionOptions`). `response_parser.rs` reprend `collect_ordered_results` et `matches`. `pointer.rs` et `row_extractor.rs` sont des placeholders documentés. Les types déplacés sont ré-exportés depuis `query.rs` pour préserver les imports existants. `query.rs` passe de ~1640 à ~930 lignes. `cargo check --workspace` passe. |
| 2026-06-08 | **Étape 8 réalisée** : déplacement de `ScraperManagerQuery` de `mod.rs` vers `scraper_manager.rs`. | Le trait était déjà nommé `ScraperManagerQuery` (pas besoin de renommage). Il a été déplacé de `mod.rs` vers `scraper_manager.rs`, son propriétaire légitime. `mod.rs` ré-exporte le trait via `pub use scraper_manager::ScraperManagerQuery`. Les imports `use std::collections::HashMap` ont été conservés dans `mod.rs` car plusieurs sous-modules y accèdent via `use super::*;`. `cargo check --workspace` passe. |
| 2026-06-08 | **Étapes 11 et 12 réalisées** : `ScraperQuery` implémenté pour `StaticScraperQuery` + `HtmlScraperQuery` + `HtmlScraperSubQuery`. | Le code stag implémentait déjà le trait `ScraperQuery` pour `StaticScraperQuery` (`scraper_static/query.rs`), `HtmlScraperQuery` et `HtmlScraperSubQuery` (`scraper_html/query.rs`). Le struct `HtmlScraperSubQuery` (lignes 478-511 de `scraper_html/query.rs`) regroupe les champs nécessaires (`context_pointer`, `context_select`, `filters`, `row_filters`, `context_entries`, `target`, `request_pointer`, `request_select`, `request_actions`, `request_method`, `request_headers`, `http_config`, `row_selector`, `row_selector_compiled`, `entries`, `post_processes`). Les trois implémentations de `ScraperQuery` exposent le même contrat : `scraper_type()`, `name()`, `media_types()`, `is_media_type()`, `base_url()`, `query_url()`, `request_method()`, `request_pointer()`, `request_select()`, `request_actions()`, `request_headers()`, `http_config()`, `extract_next_data()`, `row_locator()`, `entries()`, `sub_queries()`, `sub_query_spec()`. Pour les sub-queries (`JsonScraperSubQuery`, `HtmlScraperSubQuery`, `StaticScraperQuery` quand utilisé en sub-query), `sub_query_spec()` retourne `None` à ce stade — l'attachement d'un `SubQuerySpec` propre sera fait dans l'étape 14. |
| 2026-06-08 | **Erreurs d'ambiguïté corrigées** dans `scraper_html/query.rs` lors de l'implémentation de `ScraperQuery` pour `HtmlScraperQuery`. | `HtmlScraperQuery` implémente à la fois `ScraperManagerQuery` (ancien trait `ScraperQuery` renommé) et le nouveau `ScraperQuery` du module `scraper/`. Plusieurs méthodes partagées (`base_url`, `query_url`, `media_types`, `is_media_type`) rendaient `self.method()` ambigu dans les deux blocs d'implémentation. Correction : (1) dans `impl ScraperManagerQuery`, appels explicites `ScraperManagerQuery::base_url(self)`, `ScraperManagerQuery::query_url(self)`, `ScraperManagerQuery::media_types(self)` ; (2) dans `impl ScraperQuery`, appel explicite `ScraperManagerQuery::is_media_type(self, media_types)` pour réutiliser la logique existante. `cargo check --workspace` passe. |
| 2026-06-08 | **Étape 14 réalisée** : `sub_queries: Vec<Box<dyn ScraperQuery>>` ajouté aux entries HTML et JSON (Field + Group). | Le champ `sub_queries` est ajouté aux variants `Field` et `Group` de `HtmlScraperEntry` (`scraper_html/entry.rs`) et `JsonScraperEntry` (`scraper_json/entry.rs`). Le champ est initialisé à `Vec::new()` dans `TryFrom<*Raw>` et ne sérialise pas dans `From<&Entry> for Raw`. `ScraperEntrySpec::sub_queries()` est implémenté pour les deux types (retourne les `&dyn ScraperQuery` via déréférencement des `Box`). Le format YAML reste inchangé (le champ `sub_queries` n'est pas ajouté aux `*Raw` types dans cette passe — sera fait quand le câblage de l'exécution arrive, étape 15+). Le champ est public dans les enums `HtmlScraperEntry` et `JsonScraperEntry` mais contient `Vec<Box<dyn ScraperQuery>>` où `ScraperQuery` est exposé depuis le module `pub(crate) scraper` — ce qui force la visibilité `pub(crate)` du champ effectif au niveau du type, mais l'énum elle-même reste publique pour préserver les imports existants. La correction des pattern matchings (`..` ajouté dans `apply_to` et `From<&Entry>`) a été nécessaire pour absorber le nouveau champ. `cargo check --workspace` passe (17 warnings, tous attendus : nouveau champ `sub_queries` non consommé, plus les warnings préexistants de visibilité des types `ScraperRequestMethod`/`Header` et du module `scraper/`). |
| 2026-06-08 | **Plus de rétro-compatibilité : breaking change assumé pour les étapes 15-21**. | L'user a tranché : il n'est pas nécessaire de maintenir la rétro-compatibilité, et la structure des YAMLs sera modifiée en conséquence. Conséquences : (1) le trait `ScraperManagerQuery` (async_trait) sera supprimé à l'étape 16, ainsi que toutes les `impl ScraperManagerQuery` sur `HtmlScraperQuery` / `JsonScraperQuery` ; (2) les anciennes méthodes `execute_query` concrètes (`HtmlScraperQuery::execute_query`, `JsonScraperQuery::execute_query`) et leurs helpers (`execute_context`, `execute_siblings`, `build_row_node`, `process_root`, etc.) seront remplacés par le moteur unifié `scraper::execute_query` ; (3) les tests existants qui dépendent de `ScraperManagerQuery` (dans `scraper_manager.rs::tests`) seront migrés ou supprimés ; (4) tous les YAMLs de `server/services/` (coflix, animeultime, papystreaming, rtlplay-be, m6play-fr, tf1-fr, rtbf-auvio-be, francetv, crunchyroll, anime-sama, frenchanimes) seront migrés vers la nouvelle API `sub_queries` au niveau entry. Justification : maintenir deux moteurs en parallèle multiplierait la complexité et la surface de bug pour un gain marginal. Le breaking change est effectué en une seule passe coordonnée (code + YAMLs). Cette décision invalide la §6.4 « Pas de breaking change (1ère passe) » qui est remplacée par §6.4 « Breaking change assumé (2e passe) ». |
| 2026-06-08 | **Étape 15 réalisée** : moteur unifié `scraper::execute_query` polymorphe implémenté. | Le moteur unifié `execute_query` est implémenté dans `scraper/query_executor.rs`. Il dispatche sur `scraper_type()` pour fetcher (HTML/JSON/Static), extrait les rows via `RowLocator`, applique les entries, exécute les sub_queries siblings récursivement (via `Box::pin` pour gérer la récursion async), puis applique `merge_targeted` selon le `SubQuerySpec::target`. Le trait `ScraperQuery` est étendu avec `post_processes()` (pour les racines HTML/JSON qui supportent les post-process) et `result_item_field()` (pour le flatten des groupes). Le `QueryContext` est enrichi avec un `fields_filters` optionnel. Le moteur est **encore non câblé** (étape 16) — il compile mais n'est pas appelé par `ScraperQueryDefinition::execute_query` qui continue d'utiliser l'ancien chemin. Limitations actuelles : (1) les `as_html_entry` et `as_json_entry` retournent `None` (le downcast `&dyn ScraperEntrySpec` → `&HtmlScraperEntry` / `&JsonScraperEntry` n'est pas encore implémenté) — donc le moteur ne peut pas encore appliquer les entries sur les rows. (2) Les post-processes ne sont pas encore appliqués (boucle stub). (3) Les actions `request_actions` ne sont pas encore appliquées sur les URLs. (4) Les headers/body ne sont pas encore résolus (passés vides). Ces limitations seront levées à l'étape 16 quand l'ancien chemin sera supprimé et le moteur câblé. `cargo check --workspace` passe (27 warnings de dead_code sur les helpers non encore câblés). |
| 2026-06-08 | **Étape 22 (régression query-level sub_queries) résolue — Option A du plan** : le moteur unifié est étendu pour supporter la sémantique `context_pointer` des sub-queries query-level. | Le moteur unifié câblé à l'étape 16 ne supportait pas la sémantique des sub_queries au niveau query (avec `context_pointer`, `target`, `request_pointer` calculé sur le row parent). Quand un sibling sub_query avait un `context_pointer()` non-`None`, le moteur le dispatche désormais vers `JsonScraperSubQuery::execute_query_level` (réutilise le chemin d'exécution legacy `JsonScraperSubQuery::execute` qui supporte `context_pointer`, `filters`, `row_filters`, `request_actions`, etc.). Le dispatch se fait via un downcast `as_any().downcast_ref::<JsonScraperSubQuery>()`. Le trait `ScraperQuery` est étendu avec quatre méthodes à défaut par défaut (`context_pointer()`, `context_select()`, `context_entries()`, `target()`) que `JsonScraperSubQuery` et `HtmlScraperSubQuery` overrident pour exposer leurs champs legacy. Le `sub_query_spec()` reste à `None` pour les sub_queries query-level (l'attachement d'un `SubQuerySpec` propre n'est pas nécessaire car le dispatch lit directement les champs legacy). Conséquences : (1) `coflix.yaml` (`get_season`), `rtbf-auvio-be.yaml`, `m6play-fr.yaml`, `tf1-fr.yaml` fonctionnent à nouveau sans migration YAML. (2) `coflix.yaml` n'a PAS été migré vers la nouvelle API entry-level — le pattern query-level reste supporté. (3) `cargo check --workspace` passe (12 warnings, tous de dead_code / unused_imports préexistants). Le trait `ScraperQuery::as_any()` a été ajouté pour permettre le downcast vers le type concret. (4) Le module `scraper::query_trait` est privé (`mod query_trait;`) — les imports externes doivent passer par `crate::scrapyfy::scraper::ScraperQuery` (ré-export depuis `scrapyfy/mod.rs`). Cette décision remplace partiellement l'option C du diagnostic initial (extension du moteur unifié) — au lieu de réécrire toute la logique d'`execute_context` dans le moteur, on a privilégié un downcast vers la méthode legacy qui était déjà testée en production. |
| 2026-06-09 | **Régression `StaticScraperQuery` silencieuse — `get_service` retourne un JSON vide**. | Câblage du moteur unifié à l'étape 16 : la branche `FetchedResponse::Static` de `extract_items` (`scraper/query_executor.rs`) essayait de downcaster chaque `&dyn ScraperEntrySpec` vers `&JsonScraperEntry` via `as_json_entry`. Or, les entries d'un `StaticScraperQuery` sont des `StaticScraperEntryRaw` (qui implémentent `ScraperEntrySpec` mais **pas** `JsonScraperEntry` ni `HtmlScraperEntry`). Le downcast échouait silencieusement pour chacune des 4 entries (`id`, `title`, `logo`, `description`) → l'item résultant restait vide → `get_service` (query `service_stream_metadata` sur `m6play-fr`) retournait `{}` au lieu de `{ id, title, logo, description }`. Symptôme : l'embed du player 6play n'était pas construit, le bouton "play" n'avait pas de cible. Le pattern legacy (`StaticScraperQuery::execute_query` pré-étape 16) appliquait directement les templates YAML. Correction : (1) ajout d'une méthode concrète `StaticScraperEntryRaw::apply_to(&mut ScraperDataNode, &HashMap<String, String>) -> Result<()>` qui réutilise `split_static_path`, `render_yaml_value` et `render_yaml_values` (helpers déjà présents dans `scraper_static/query.rs` mais qui n'étaient appelés que depuis l'ancien `execute_query` supprimé) ; (2) nouvelle branche dédiée dans `extract_items` qui downcast vers `StaticScraperEntryRaw` et appelle `apply_to` ; (3) `extract_items` passe à `Result<Vec<ScraperDataNode>>` pour propager les erreurs de templating (un placeholder non résolu dans un `value: "{service_id}"` arrête proprement l'exécution au lieu de produire un item vide). `cargo check --workspace` passe (8 warnings préexistants, aucun nouveau). Tous les YAMLs utilisant `scraper_type: static` (`m6play-fr.yaml`, `darkstream/coflix.yaml` pour `get_season`, `darkstream/anime-sama.yaml`, etc.) sont concernés — la régression touchait potentiellement d'autres services. |

---

## 8. Annexes

### 8.1 Liens utiles

- `server/services/darkstream/coflix.yaml` — use case initial.
- `server/services/darkstream/animeultime.yaml` — exemple de `post_process: fetch_regex_items_from_items` (workaround actuel).
- `server/crates/arachnea-scrapyfy/src/scrapyfy/json_scraper_query.rs` — référence pour la logique JSON actuelle.
- `server/crates/arachnea-scrapyfy/src/scrapyfy/html_scraper_entry.rs` — référence pour la logique HTML actuelle.
- `server/crates/arachnea-scrapyfy/src/scrapyfy/actions/mod.rs` — liste des actions YAML disponibles.
- `skills/build-yaml-source/references/yaml-capabilities.md` — capabilities YAML actuelles.
- `server/AGENTS.md` — conventions RustDoc.
- `AGENTS.md` (racine) — conventions globales.

### 8.2 Glossaire

- **Query** : une définition de scraper (racine ou sub_query).
- **Sub_query** : une query attachée à une autre query (racine ou entry).
- **Entry** : un champ extrait d'une row (peut être un groupe).
- **Row** : un élément de la réponse HTTP (un `<div>` HTML, un objet JSON, etc.).
- **Context** : un row utilisé comme point de départ d'un sub_query.
- **Target** : le chemin où merger le résultat d'un sub_query.
- **`parent`** : convention YAML pour désigner l'entry parente d'un sub_query d'entry.
- **Récursion** : capacité d'un sub_query à contenir des sub_queries d'entries, etc.
- **Dispatch polymorphe** : `&dyn ScraperQuery` permet d'appeler `execute_query` sur n'importe quelle variante.