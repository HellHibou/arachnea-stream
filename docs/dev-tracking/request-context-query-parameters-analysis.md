# Analyse — `RequestControlerContext` + `QueryParameters` (sortie des champs ETag des structures de requête)

## 1. État actuel

### 1.1 `arachnea-stream::stream_scraper`

Onze structures de requête (`SearchRequest`, `GetEntryRequest`, `GetSeasonRequest`,
`ListLivesRequest`, `GetLiveRequest`, `LoadHomeRequest`, `GetServiceRequest`,
`GetCategoryRequest`, `GetSectionRequest`, `GetBannersRequest`, `GetPlayersRequest`)
portent chacune deux champs qui ne sont pas de la donnée métier de la requête :

- `arachnea_etag: Option<String>` (`alias = "arachneaEtag"`) — ETag global entrant du client ;
- `enable_etag: bool` (`alias = "enableEtag"`) — activation de la validation conditionnelle.

Le helper `request_arachnea_etag(explicit_etag, headers)` fusionne le champ explicite avec
l'en-tête HTTP `If-None-Match` (le champ explicite est prioritaire ; utile au chemin Tauri).

La méthode privée `StreamScraper::execute_query_with_etag` :

1. normalise l'ETag client (`normalize_client_etag`) ;
2. si `enable_etag == false` : délègue à `ScraperAgregator::execute_query_async` sans fragments,
   retourne `(result, None)` ;
3. sinon : calcule la liste déterministe des services impliqués, décode les fragments clients
   (`decode_client_fragments`), lance la **phase 1** (validation conditionnelle parallèle),
   reconstruit l'ETag global (`build_global_etag`), court-circuite si toutes les sources sont
   stale et que l'ETag global reconstruit correspond à celui du client, puis lance la **phase 2**
   (GET complets restreints aux sources stale) en fusionnant les lignes fraîches.

Chaque méthode publique du facade (`search`, `get_entry`, `get_season`, `list_lives`,
`get_live`, `load_home`, `get_service_with_etag`, `get_category`, `get_section`,
`get_banners`, `get_players`) prend aujourd'hui `arachnea_etag: Option<String>` et
`enable_etag: bool` en paramètres et retourne un couple
`(ScraperAggregationResult<…>, Option<String>)`.

### 1.2 Contrôleurs (`arachnea-core::controler`)

- Le contrat `register_etag_result_function[_with_state]` passe actuellement une carte brute
  `HashMap<String, String>` des en-têtes à la fonction enregistrée :
  `F: Fn(Arc<S>, I, HashMap<String, String>) -> Fut`.
- Le backend **REST (Warp)** construit cette carte depuis `warp::http::HeaderMap`
  (`headers_to_map`) pour les routes GET et POST.
- Le backend **Tauri** passe `Default::default()` (carte vide) — l'IPC ne transporte pas
  d'en-têtes HTTP.
- La comparaison `If-None-Match` ↔ ETag produit et la réponse `304` sont gérées dans
  `ControlerServiceExt::register_etag_result_function`.

### 1.3 Moteur de scraping (`arachnea-scrapyfy`)

- `ScraperAgregator::execute_query_async(&self, group_name, query_name, params, source_params,
  scrapper_list, query_media_type_filter, fields_filters, source_field_name, operation,
  client_fragments)` exécute une seule passe ; il n'a aucune notion d'activation globale ni
  d'ETag global — toute l'orchestration conditionnelle vit dans `arachnea-stream`.
- Il n'existe pas encore de type `QueryParameters`.
- **Point favorable** : `arachnea-scrapyfy` dépend déjà de `arachnea-core`, donc utiliser un
  type défini dans `arachnea_core::controler` depuis scrapyfy ne crée pas de cycle.

### 1.4 Frontend

`front/src/services/rustify.ts` envoie à la fois l'en-tête `If-None-Match` **et** le paramètre
de requête `arachneaEtag` (ETag caché rejoué). Les structs serde backend n'utilisant pas
`deny_unknown_fields`, supprimer les champs `arachneaEtag` / `enableEtag` côté serveur reste
tolérant vis-à-vis du frontend existant.

## 2. Problème à résoudre

Les champs `arachnea_etag` et `enable_etag` sont des **données de transport/contexte**, pas des
paramètres métier : ils polluent chaque structure `*Request` (et donc chaque contrat JSON), se
dupliquent onze fois, et couplent le facade aux détails HTTP. Par ailleurs, la logique
conditionnelle (phase 1 / phase 2 / ETag global) est spécifique au facade `arachnea-stream`
alors qu'elle n'est que l'orchestration générique du moteur.

## 3. Conception cible

### 3.1 `RequestControlerContext` — `arachnea-core/src/controler/mod.rs`

> Nommage — **décision validée** : `RequestControlerContext`, conforme à l'orthographe
> « Controler » utilisée dans tout le dépôt (`ControlerService`, `ControlerJsonInput`), plutôt
> que « RequestCotrollerContext » tel que tapé initialement dans la demande.

```rust
/// Header carrying the client global ETag for conditional validation.
pub const HEADER_IF_NONE_MATCH: &str = "if-none-match";
/// Header carrying the server validator echoed back to clients.
pub const HEADER_ETAG: &str = "etag";

/// Context of an incoming controller request.
///
/// Carries the received HTTP headers so registered commands can access them
/// without depending on a specific backend (REST, Tauri IPC, ...).
pub struct RequestControlerContext {
    headers: HashMap<String, String>,
}

impl RequestControlerContext {
    pub fn new(headers: HashMap<String, String>) -> Self;
    /// Returns the optional value of the given header (case-insensitive lookup).
    pub fn get_header(&self, name: &str) -> Option<&str>;
}
```

D'autres constantes pourront être ajoutées au même endroit quand de nouveaux en-têtes seront
gérés (`if-modified-since`, …).

### 3.2 Contrat contrôleur — fusion des deux contrats en un seul

> **Décision validée** : plutôt qu'un renommage, les deux contrats sont **fusionnés** dans les
> méthodes existantes `register_result_function` / `register_result_function_with_state`, qui
> deviennent header-aware (contexte entrant + gestion ETag / `304`). Les méthodes
> `register_etag_result_function[_with_state]` disparaissent.

Nouveau contrat unique :

```rust
// register_result_function_with_state : la closure reçoit le contexte juste après le state
F: Fn(Arc<S>, RequestControlerContext, I) -> Fut
Fut: Future<Output = Result<(O, Option<String>), E>>   // (payload, ETag global optionnel)
```

- `register_result_function` **absorbe** le corps actuel de `register_etag_result_function` :
  lecture de l'ETag client via `RequestControlerContext::get_header(HEADER_IF_NONE_MATCH)`
  puis normalisation, réponse `304 Not Modified` sans body si correspondance, sinon 200 avec
  payload et en-tête `ETag` quand un ETag est produit. Il délègue désormais à
  `register_json_function` (et non plus à `register_serialized_function`).
- `register_result_function_with_state` conserve son rôle de binding de state et délègue à la
  version fusionnée.
- Les 11 commandes ETag de `stream_scraper.rs` basculent sur
  `register_result_function_with_state`.
- `get_stream` reste sur `register_result_function_with_state` : sa closure reçoit un contexte
  ignoré et retourne `(résultat, None)`.

- **REST** (`controler/rest/service.rs`) : construit le contexte depuis la carte d'en-têtes
  Warp existante (GET et POST).
- **Tauri** (`controler/tauri/mod.rs`) : construit un contexte vide (`Default`), comportement
  inchangé puisque l'IPC n'a pas d'en-têtes.

#### 3.2.1 Fonctions impactées par la fusion

| Fonction | Fichier | Impact |
|---|---|---|
| `ControlerServiceExt::register_result_function` | `controler/mod.rs` | Signature header-aware : `Fn(I, RequestControlerContext) -> Fut<Result<(O, Option<String>), E>>` ; bascule de `register_serialized_function` vers `register_json_function` ; absorption de la logique `304` / en-tête `ETag` de l'actuelle `register_etag_result_function`. |
| `ControlerServiceExt::register_result_function_with_state` | `controler/mod.rs` | Closure étendue à `Fn(Arc<S>, RequestControlerContext, I)` ; délègue à `register_result_function` fusionnée. |
| `ControlerServiceExt::register_etag_result_function` | `controler/mod.rs` | **Supprimée** — corps transféré dans `register_result_function`. |
| `ControlerServiceExt::register_etag_result_function_with_state` | `controler/mod.rs` | **Supprimée.** |
| `ControlerService::register_serialized_function` (+ impls REST `rest/service.rs` et Tauri `tauri/mod.rs`) | 3 fichiers | Perd son dernier appelant interne (l'ancien `register_result_function`). Conservée dans un premier temps ; suppression envisageable dans une passe de nettoyage ultérieure si aucun autre usage n'apparaît. |
| `ControlerService::register_json_function` (+ impls REST/Tauri) | 3 fichiers | Inchangée dans sa signature — devient le canal unique des contrats typés ; le backend Tauri construit déjà un contexte vide côté `register_json_function` (via `ControlerJsonInput`). |
| 11 enregistrements de commandes : `search`, `load_home`, `get_service`, `list_lives`, `get_category`, `get_section`, `get_banners`, `get_players`, `get_entry`, `get_season`, `get_live` | `stream_scraper.rs` (`register_service`) | Bascule de `register_etag_result_function_with_state` → `register_result_function_with_state` ; closures `|scraper, context, input|`. |
| Enregistrement `get_stream` | `stream_scraper.rs` | Même méthode qu'avant (`register_result_function_with_state`) mais closure adaptée : `|scraper, _context, input| … get_stream(...).map(|r| (r, None))` — retour au format tuple du nouveau contrat. |
| `register_stream_function_with_state` (route DRM `get_drm_license`) | `stream_scraper.rs` | Non impactée. |
| `ControlerJsonInput` / `ControlerJsonOutput` / `JsonControlerFunction` / helpers `deserialize_input`, `serialize_output`, `normalize_etag` | `controler/mod.rs` | Conservés tels quels — ils supportent désormais le seul contrat restant. |

### 3.3 `QueryParameters` — `arachnea-scrapyfy`

Nouvelle structure (par exemple dans `scraper_agregator.rs` ou un petit module dédié, ré-exportée
depuis `scrapyfy/mod.rs`) regroupant les options d'exécution d'une requête :

```rust
/// Runtime options controlling how a query is executed.
pub struct QueryParameters {
    /// Whether conditional ETag validation is enabled. Defaults to `true`.
    pub enable_etag: bool,
}

impl Default for QueryParameters {
    fn default() -> Self { Self { enable_etag: true } }
}
```

### 3.4 Nouvelle signature de `ScraperAgregator::execute_query_async`

```rust
pub async fn execute_query_async(
    &self,
    context: &RequestControlerContext,   // 1er paramètre après self
    query_parameters: QueryParameters,   // juste après le contexte
    group_name: &str,
    // ... paramètres inchangés ...
    operation: &str,
) -> ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>
```

La logique de `execute_query_with_etag` y est **transférée** :

- lecture de l'ETag client via `context.get_header(HEADER_IF_NONE_MATCH)` puis normalisation ;
- si `query_parameters.enable_etag == false` : passe unique sans fragments, aucun ETag global ;
- sinon : phase 1 avec fragments décodés, construction de l'ETag global, court-circuit « tout
  stale », phase 2 sur les sources stale uniquement ;
- le paramètre `client_fragments` disparaît de la signature (il est désormais calculé en interne).

**Déplacement associé** : `build_global_etag`, `decode_client_fragments`, `normalize_client_etag`
(quittent `arachnea-stream::stream_etag` pour rejoindre scrapyfy, où ils deviennent génériques).
`arachnea-stream` peut continuer à ré-exporter ce dont il a besoin.

**Retour de l'ETag global au contrôleur** — **option B validée** :

> Ajouter un champ `global_etag: Option<String>` à `ScraperAggregationResult`. Les méthodes
> publiques du facade le lisent et continuent de retourner `(result, etag)` aux fonctions
> enregistrées : le contrat `register_result_function` reste `(O, Option<String>)`, les autres
> appelants d'`execute_query_async` (tests scrapyfy) gardent un retour simple.

### 3.5 `arachnea-stream::stream_scraper`

- Suppression des champs `arachnea_etag` / `enable_etag` des 11 structures `*Request`, ainsi que
  de `default_enable_etag` et du helper `request_arachnea_etag`.
- Suppression de `execute_query_with_etag` et de `get_service_with_etag` (le contenu migre dans
  `execute_query_async`) ; `get_service(context)` redevient la seule variante.
- Chaque méthode publique prend `context: RequestControlerContext` **juste après `&self`** et le
  transmet à `execute_query_async` avec `QueryParameters::default()` :

```rust
pub async fn search(
    &self,
    context: RequestControlerContext,
    query: String,
    media_types: Vec<String>,
    themes: Vec<String>,
    page: usize,
    source_params: ScraperSourceParams,
) -> Result<(ScraperAggregationResult<…>, Option<String>)>
```

- Fonctions enregistrées dans `register_service` : la closure devient
  `|scraper, context, input| scraper.search(context, input.query, …)` — plus aucune extraction
  manuelle d'en-tête dans `stream_scraper.rs`.
- `get_stream` reste sur `register_result_function_with_state` (méthode fusionnée) ; sa closure
  reçoit un contexte ignoré et retourne un tuple avec ETag à `None` :
  `|scraper, _context, input| scraper.get_stream(input.resolver, input.target).map(|r| (r, None))`.

## 4. Impacts par fichier

| Fichier | Changement |
|---|---|
| `server/crates/arachnea-core/src/controler/mod.rs` | Ajout `RequestControlerContext` + constantes `HEADER_IF_NONE_MATCH` / `HEADER_ETAG` ; **fusion des contrats** : `register_result_function[_with_state]` deviennent header-aware (contexte après le state, sortie `(O, Option<String>)`, gestion `304` / en-tête `ETag`), suppression de `register_etag_result_function[_with_state]`. Détail complet en §3.2.1. |
| `.../controler/rest/service.rs` | Construction du contexte depuis les en-têtes Warp (GET/POST). |
| `.../controler/tauri/mod.rs` | Construction d'un contexte vide (IPC sans en-têtes). |
| `server/crates/arachnea-scrapyfy/src/scrapyfy/*` | Nouvelle struct `QueryParameters` (+ `Default`, `enable_etag = true`) ; `execute_query_async` : nouveaux paramètres `context` + `query_parameters`, absorption de la logique phase 1/phase 2 + ETag global ; accueil des helpers ETag globaux ; champ `global_etag` dans `ScraperAggregationResult` (option B). |
| `server/crates/arachnea-stream/src/stream_scraper.rs` | Nettoyage des 11 `*Request`, signatures publiques avec `context` après `self`, suppression `execute_query_with_etag` / `request_arachnea_etag` / `default_enable_etag`, mises à jour des enregistrements. |
| `.../stream_etag.rs` / `lib.rs` | Déplacement des helpers vers scrapyfy (ou ré-export pour compatibilité). |
| Tests (`stream_scraper_tests.rs`, tests scrapyfy) | Adaptation aux nouvelles signatures (aucun nouveau test). |
| `CHANGELOG.md`, `docs/TODO.md` | Entrées correspondantes. |
| Frontend (facultatif, hors périmètre immédiat) | `rustify.ts` peut cesser d'envoyer `arachneaEtag`/`enableEtag` (l'en-tête `If-None-Match` suffit) — compatible sans changement grâce à serde. |

## 5. Comportement conservé / nuances

- Validation conditionnelle activée **par défaut** (`QueryParameters::default()`), comme
  aujourd'hui (`default_enable_etag == true`).
- Chemin REST : identique — l'ETag arrive par `If-None-Match` (le paramètre `arachneaEtag` sera
  simplement ignoré s'il est encore envoyé).
- Chemin Tauri : contexte vide → pas de fragments clients (déjà le cas : « the desktop frontend
  never sends ETags ») ; la réponse conserve ETag / `304` au niveau contrôleur.
- `get_stream` et la route DRM restent hors périmètre.

## 6. Étapes d'implémentation proposées

1. `arachnea-core` : `RequestControlerContext` + constantes ; **fusion des contrats** —
   `register_result_function[_with_state]` absorbent la logique header-aware
   (`register_json_function`, `304`, en-tête `ETag`) et suppriment
   `register_etag_result_function[_with_state]` ; mise à jour REST/Tauri.
2. `arachnea-scrapyfy` : `QueryParameters`, déplacement des helpers ETag, refonte
   d'`execute_query_async` (contexte + paramètres + phases), champ `global_etag` dans
   `ScraperAggregationResult`, adaptation des tests internes.
3. `arachnea-stream` : nettoyage des `*Request`, nouvelles signatures du facade (contexte après
   `&self`), suppression de `execute_query_with_etag`, migration des enregistrements y compris
   `get_stream`, mise à jour des tests.
4. Vérification : `cargo check`/`cargo test` sur les crates touchées ; mise à jour
   `CHANGELOG.md` / `docs/TODO.md`.

## 7. Décisions validées

1. **Nommage** : `RequestControlerContext` (convention « Controler » du dépôt) — validé.
2. **Retour de l'ETag global** : option B — champ `global_etag: Option<String>` dans
   `ScraperAggregationResult` — validé.
3. **Fusion** : les contrats `register_result_function[_with_state]` et
   `register_etag_result_function[_with_state]` sont fusionnés — les méthodes existantes
   `register_result_function[_with_state]` deviennent header-aware (contexte après le state,
   sortie `(O, Option<String>)`, gestion `304` / en-tête `ETag`) et remplacent les variantes
   `register_etag_*` qui disparaissent ; l'unique appelant de l'ancien contrat simple
   (`get_stream`) reste sur la méthode fusionnée avec un contexte ignoré et un ETag à `None` —
   validé. Liste exhaustive des fonctions impactées : §3.2.1.

> **Statut** : implémenté — `RequestControlerContext`, fusion du contrat
> `register_result_function[_with_state]`, `QueryParameters`, absorption de la validation
> conditionnelle par `ScraperAgregator::execute_query_async` et nettoyage des structures
> `*Request` de `arachnea-stream`.


