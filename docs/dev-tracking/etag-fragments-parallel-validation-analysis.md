# Analyse : Validation conditionnelle parallèle (ETag / Fragments) et logique de rattrapage

> Généré le 2026-08-23 — Pré-analyse prospective (v2, révisée)  
> Portée : `server/crates/arachnea-stream` + `server/crates/arachnea-scrapyfy` + `front/src/services/rustify.ts`  
> Statut : **Implémenté** (voir §12) — document d'architecture et de conception

---

## 1. Contexte et objectif

Le serveur Arachnea agrège des catalogues depuis plusieurs sources distantes
(`load_home`, `get_category`, `search`, etc.). Chaque requête client peut
déclencher **N appels HTTP parallèles** vers N sources différentes.

L'objectif de cette pré-analyse est de définir un mécanisme de **validation
conditionnelle** (ETag / `If-None-Match`, `If-Modified-Since` + hash) qui
permette de :

- **Éviter le parsing** des pages distantes inchangées (économie de CPU).
- **Répondre rapidement** au client avec un `304 Not Modified` ou un diff vide
  lorsque rien n'a changé.
- **Garantir un ETag global cohérent** construit à partir des fragments
  individuels de chaque source.

> **Note d'architecture** : le code actuel (`stream_scraper.rs`) utilise
> `execute_query_async` de `ScraperAgregator` qui lance déjà les requêtes en
> parallèle (async/await, pas de threads explicites). Le mécanisme décrit ici
> est une **évolution prospective** qui s'insérerait dans cette couche.

---

## 2. État actuel de l'implémentation

### 2.1 Ce qui existe déjà

| Élément | Emplacement | Rôle |
|---|---|---|
| `StreamScraper` | `server/crates/arachnea-stream/src/stream_scraper.rs` | Façade haute niveau, agrège les appels aux sources |
| `ScraperAgregator::execute_query_async` | `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs` | Exécute une requête sur N sources en parallèle |
| `ScraperAggregationResult` | `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_result.rs` | Structure de retour (données + erreurs) |
| `ScraperDataNode` | `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_data_node.rs` | Arbre de données sérialisé en JSON |
| `ScraperSourceParams` | `server/crates/arachnea-scrapyfy` | Paramètres runtime par source (pagination, etc.) |

### 2.2 Ce qui n'existe pas encore

- **Aucun mécanisme d'ETag / validation conditionnelle** (`If-None-Match`,
  `If-Modified-Since`) dans le client HTTP ou le scraper.
- **Aucun concept de fragment** individuel par URL source.
- **Aucune notion de statut `fresh` / `stale`** dans les résultats.
- **Aucune logique de "seconde passe"** (rattrapage) pour les données manquantes.
- **Aucun cache serveur** (Redis / mémoire) pour les réponses parsées.

Le présent document décrit donc une **architecture cible** à implémenter.

---

## 3. Contrat de retour des workers (Tâches asynchrones)

> **Correction terminologique** : le code actuel utilise **async/await** (tokio),
> pas des threads OS explicites. On parlera donc de **tâches asynchrones**
> (workers) plutôt que de threads.

Pour chaque URL source, une tâche asynchrone est lancée. Elle doit retourner au
thread principal une structure contenant :

| Champ | Type | Description |
|---|---|---|
| `etag` | `string` | Fragment ETag individuel **à jour** pour cette URL (ex: `E:w456` ou `C:def-123`). Calculé même si le statut est `stale` (dans ce cas, identique au fragment entrant). |
| `status` | `enum` | `fresh` : données modifiées (ou première requête) — le worker a parsé le HTML et retourne les données. `stale` : données inchangées (304 ou hash identique) — le worker **n'a pas parsé** et **ne retourne pas de données**. |
| `data` | `Option<Value>` | Données parsées (objet JSON) si `status == fresh` ; `None` si `status == stale`. |

### 3.1 Sémantique des fragments

Le fragment encode le **mécanisme de validation** utilisé pour la source :

| Préfixe | Mécanisme | Exemple |
|---|---|---|
| `E:` | ETag HTTP (`If-None-Match`) | `E:w456` |
| `C:` | `If-Modified-Since` + hash de contenu | `C:def-123` |

Le fragment est **stable** tant que le contenu ne change pas. Il sert à :
- Construire l'**ETag global** de la réponse agrégée.
- Permettre au client de renvoyer son ETag global lors de la requête suivante.

### 3.2 Construction de l'ETag global

L'ETag global est une suite de blocs **séparés par `;`**. Un bloc optionnel
`S:<hash>` précède l'ensemble pour les **requêtes multi-sources** (N > 1) :

```
ETag global = [S:<hash_services>;]<etag_1>;<etag_2>;...;<etag_N>
```

| Bloc | Description | Présence |
|---|---|---|
| `S:<hash_services>` | Hash de la **concaténation ordonnée** des noms de services (retourne la liste des services impliqués dans la requête, triée par ordre alphabétique) | Uniquement si N > 1 |
| `<etag_i>` | Fragment ETag individuel de la source `i` (ex: `E:w456` ou `C:def-123`) | Toujours |

#### Rôle du bloc `S:` (multi-sources uniquement)

Pour une requête multi-sources (ex: `load_home`, `get_category`), la **liste
des services** peut changer au fil du temps. Le bloc `S:` permet de détecter ce
changement **sans décomposer le JSON** :

- `S:<hash>` est le **hash XXH3-128 encodé intégralement en base 62**
  (jusqu'à 22 caractères) de la concaténation des **noms de services** de la
  requête, **triés par ordre alphabétique**. Le hash est **stable entre
  requêtes** car il ne dépend que des noms de services (pas des URLs, qui
  changent souvent).
- Si le `S:<hash>` reçu ne correspond pas au hash calculé côté serveur, la liste
  des services a changé → on considère que **tout a été modifié** (retour du
  JSON complet, pas de `304`).
- Si le `S:<hash>` correspond, on peut analyser les fragments suivants pour
  comparer les etag individuels.

> **Cas mono-source (N == 1)** : pas besoin de bloc `S:`. Le ETag global est
> simplement le fragment individuel de l'unique source.

#### Décodage côté serveur

Pour traiter un ETag entrant, le serveur :

1. Détermine si la requête est multi-sources.
2. Si oui, vérifie la présence et la validité du bloc `S:` ; en cas
   d'inadéquation → retour direct du JSON complet.
3. Découpe la partie restante sur le séparateur `;`.
4. Associe chaque fragment à chaque URL source **par position** (l'ordre est
   déterministe : tri alphabétique des noms de services).

---

## 4. Algorithme de traitement (Côté serveur) — Version parallèle

Le traitement d'une requête client se décompose en **deux phases distinctes** :
une phase de validation parallèle, et une phase de rattrapage optionnelle.

### 4.1 Phase 1 : Lancement des workers en parallèle

Le thread principal lance **simultanément** les `N` workers (via
`Promise.all`, `CompletableFuture`, ou `tokio::join!` / `FuturesUnordered`).

Chaque worker exécute l'algorithme de validation **en utilisant le fragment
client** pour sa propre URL :

1. Il interroge la source distante :
   - Fragment `E:` → requête avec `If-None-Match: <fragment>`.
   - Fragment `C:` → requête avec `If-Modified-Since: <date>` + comparaison de hash.
2. Si la source répond `304 Not Modified` (ou hash identique) →
   retourne `{ status: 'stale', etag: etag_entrant, data: None }`.
3. Si la source répond `200 OK` (ou hash différent) →
   génère les données, calcule le nouveau fragment ETag,
   retourne `{ status: 'fresh', etag: nouvel_etag, data: données }`.

**À l'issue de cette phase**, le thread principal dispose de :
- La liste des fragments ETag **à jour** pour toutes les URLs (`etag`),
  qu'elles soient `fresh` ou `stale`.
- Les données parsées **uniquement** pour les URLs `fresh`.

### 4.2 Phase 2 : Décision et rattrapage (seconde passe)

Le thread principal examine les statuts retournés.

#### Cas A : Toutes les URLs ont le statut `fresh`

- **Action** : Aucune donnée manquante. Le thread principal agrège directement
  toutes les données reçues (celles des workers).
- **Construction du JSON** : JSON complet ou Diff (selon le protocole client).
- **ETag global** : Construit en concaténant tous les `etag` reçus.
- **Résultat** : Réponse immédiate (pas de seconde passe).

#### Cas B : Au moins une URL a le statut `stale` (données manquantes)

- **Action** : Le thread principal identifie toutes les URLs ayant retourné `stale`.
- **Seconde passe (forcée)** : Pour **ces URLs uniquement**, le thread principal
  effectue de nouvelles requêtes HTTP **sans** utiliser de mécanisme de
  validation (ni `If-None-Match`, ni `If-Modified-Since`). Il s'agit de
  **GET complets** forcés.
  - *Raison* : La première passe a prouvé que le contenu n'avait **pas changé**,
    mais le worker n'a pas extrait les données pour économiser du CPU. Il faut
    maintenant les récupérer pour construire la réponse agrégée.
- Pour chaque URL de cette seconde passe :
  - Téléchargement du HTML complet.
  - **Parsing** (on subit le coût CPU, inévitable ici).
  - Extraction des données.
- **Reconstruction de l'agrégat** : Le thread principal fusionne :
  - Les données `fresh` de la phase 1.
  - Les données fraîchement extraites lors de la phase 2 (pour les URLs `stale`).
- **ETag global** : Le thread principal **utilise les `etag` de la PHASE 1**
  pour construire l'ETag global (car ils représentent l'état actuel et à jour
  de chaque source). **Il ne recalcule pas** de nouveaux fragments ETag lors
  de la seconde passe, car ils seraient identiques.

#### Cas particulier : Toutes les URLs sont `stale`

- La phase 1 n'a retourné **aucune donnée** (que des `None`).
- La phase 2 devra donc charger **intégralement toutes les URLs**
  (GET complets + parsing pour toutes).
- C'est le pire cas en termes de performance, mais il est identique à un
  système sans ETag. Il ne se produit que lorsque toutes les sources sont
  simultanément inchangées. Dans ce scénario, le serveur répondra plus
  lentement, mais le client bénéficiera d'un `304` ou d'un diff vide.

### 4.3 Exemple concret (3 URLs)

| URL | Phase 1 (validation) | Résultat Phase 1 | Phase 2 (rattrapage) |
| :--- | :--- | :--- | :--- |
| URL 1 | Hash différent (modifiée) | `status: 'fresh'`, data: `{...}` | Pas de rattrapage |
| URL 2 | 304 Not Modified | `status: 'stale'`, data: `None` | GET complet + parsing |
| URL 3 | 304 Not Modified | `status: 'stale'`, data: `None` | GET complet + parsing |

- **ETag global final** : Concaténation des `etag` de la Phase 1 pour
  les 3 URLs.
- **JSON final** : data(URL1) + data_phase2(URL2) + data_phase2(URL3).

---

## 5. Points d'insertion dans l'architecture actuelle

### 5.1 Couche concernée

Le mécanisme s'insérerait **entre** `StreamScraper` et `ScraperAgregator` :

```
Client
  │
  ▼
StreamScraper (façade)          ← point d'entrée actuel
  │
  ▼
[NOUVEAU] ValidationLayer       ← validation conditionnelle + seconde passe
  │
  ▼
ScraperAgregator::execute_query_async
  │
  ▼
Sources distantes (N appels parallèles)
```

### 5.2 Option d'activation (paramètre)

Chaque requête de `StreamScraper` doit pouvoir **activer ou désactiver** le
mécanisme ETag (ex: `enable_etag: bool`). **Activé par défaut** (`true`), il est
appliqué aux requêtes GET :

```rust
pub async fn load_home(
  &self,
  arachnea_etag: Option<&str>, // ETag entrant du client (si activé)
  enable_etag: bool,            // option d'activation de la validation
) -> Result<ScraperAggregationResult<...>>
```

Si `enable_etag == false` → le serveur se comporte comme aujourd'hui (aucun
calcul de fragments, aucune validation conditionnelle). Si `enable_etag == true`
→ on applique les phases décrites en section 4.

### 5.3 Modifications nécessaires

| Composant | Modification |
|---|---|
| `ScraperAgregator` | Exposer les en-têtes de réponse HTTP (ETag, Last-Modified) et le statut (200/304) dans le résultat. |
| `ScraperAggregationResult` | Ajouter un champ `etags: HashMap<String, String>` (source → etag) et `statuses: HashMap<String, WorkerStatus>`. |
| `StreamScraper` | Accepter l'EN (Arachnea ETag) et le paramètre `enable_etag`, découper (`;`) par URL, orchestrer phase 1 + phase 2. |
| Client HTTP (`arachnea-http`) | Support des en-têtes `If-None-Match` / `If-Modified-Since` et détection du statut `304`. |
| Contrôleur REST | Accepter l'en-tête `If-None-Match` entrant, retourner `304` si l'ETag global est inchangé. |
| `call_api` (`rustify.ts`) | Modifier pour utiliser **GET** au lieu de POST (tout param dans la query string). |

### 5.4 Contrat de requête/réponse (proposition)

**Requête client** (GET — cas multi-sources avec ETag activé) :

```
GET /api/load_home?page=1
If-None-Match: "S:3f8a9b2c1d;E:w456;E:w457;C:def-123"
```

**Réponse serveur** :

- Si l'`If-None-Match` entrant satisfait (même liste `S:` + etag identiques
  individuellement) → **code HTTP `304 Not Modified`** (aucun body).
- Sinon → `200 OK` avec le JSON complet (ou diff) et l'en-tête
  `ETag: "S:3f8a9b2c1d;E:w456;E:w457;C:def-123"`.

---

## 6. Bascule frontend vers GET

La transformation s'effectue **uniquement dans `call_api`** (`rustify.ts`),
sans passer par toutes les fonctions qui l'utilisent :

```ts
export async function call_api<T>(fct_name: string, params: Record<string, unknown>): Promise<T> {
  // path Tauri — inchangé
  // ...
  // path REST : GET au lieu de POST
  const query = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    query.append(key, JSON.stringify(value)) // sérialisation uniforme
  }
  const response = await fetch(`${restApiBaseUrl}/${fct_name}?${query.toString()}`, {
    method: 'GET',
    headers: {
      ...(etagResolver?.(fct_name) ? { 'If-None-Match': etagResolver(fct_name) } : {}),
    },
  })
}
```

- Le corps JSON disparaît ; les paramètres sont encodés dans la query string,
  conformément au support déjà en place du contrôleur REST
  (`ControlerFunctionInput::Query`).
- Toute fonction qui utilise `call_api` devient un GET **sans modifier l'appel
  côté backend** (le backend désérialise la query vers le même type
  `DeserializeOwned`).
- Les requêtes **POST** internes (non GET) ne sont **pas** sujettes à la
  validation ETag et conservent leur comportement actuel.

## 7. Implications pour l'architecture future (Cache serveur optionnel)

Cette logique de "seconde passe" est précisément l'endroit où un cache serveur
futur apportera le plus de valeur.

**Avec cache serveur (évolution future) :**

Lors de la Phase 2, au lieu d'effectuer un **GET complet + parsing** coûteux
vers l'externe, le thread principal pourra :

1. Vérifier si les données pour l'URL `stale` sont présentes dans le cache
   serveur local (Redis / mémoire).
2. **Si oui** : Les récupérer instantanément (lecture mémoire) → zéro réseau
   externe, zéro parsing.
3. **Si non** : Revenir au GET complet externe (fallback), puis alimenter le
   cache serveur pour les prochaines fois.

**Avantage** : Avec un cache serveur bien dimensionné, le scénario "toutes les
URLs sont `stale`" deviendra **ultra-rapide**, car la phase 2 se résumera à des
lectures en cache, sans aucun appel réseau externe. L'ETag global (basé sur les
fragments de la Phase 1) aura déjà validé que les données sont à jour, et le
cache serveur fournira le contenu sans délai.

---

## 8. Résumé des avantages

| Avantage | Explication |
| :--- | :--- |
| **Minimisation du CPU** | Les workers ne parsent que les pages modifiées (Phase 1). Les pages inchangées ne sont parsées que si une **seconde passe** est déclenchée (et encore, uniquement pour reconstruire l'agrégat). |
| **Traitement parallèle** | Les N appels distants sont lancés simultanément. Le temps d'attente total est celui du plus lent des workers, et non la somme des N. |
| **Flexibilité pour le cache futur** | La Phase 2 est le point d'insertion idéal pour un cache serveur. Elle transformera un appel réseau coûteux en une lecture mémoire, sans changer le protocole client. |
| **Pas d'impact client** | Le client reçoit toujours un ETag global cohérent (basé sur la Phase 1) et un JSON complet (ou diff), quelle que soit la stratégie de rattrapage interne. |

---

## 9. Risques et points d'attention

### 9.1 Déterminisme de l'ETag global

L'ordre de concaténation des fragments doit être **stable** entre deux requêtes
identiques. Si l'ordre dépend de l'ordre d'arrivée des workers (non-déterministe
en async), l'ETag global varierait à chaque requête, rendant la validation
conditionnelle inutile.

**Solution** : trier les fragments par URL (ou par identifiant de source) avant
concaténation.

### 9.2 Sources sans support de validation conditionnelle

Certaines sources distantes ne supportent ni `If-None-Match` ni
`If-Modified-Since` (ou répondent toujours `200`). Dans ce cas :

- Le fragment `C:` (hash de contenu) est la seule option.
- Le worker doit **télécharger le HTML complet** pour calculer le hash, mais
  peut **éviter le parsing** si le hash est identique.

### 9.3 Coût de la seconde passe

Le scénario "toutes les URLs sont `stale`" est le pire cas : il double le
nombre de requêtes HTTP (phase 1 + phase 2). C'est acceptable car :

- Il ne se produit que lorsque tout est inchangé.
- Le client reçoit un `304` ou un diff vide (bénéfice réseau).
- Le cache serveur futur éliminera ce coût.

### 9.4 Compatibilité avec `execute_query_async`

Le mécanisme actuel d'agrégation (`execute_query_async`) ne retourne pas les
en-têtes HTTP ni le statut de chaque source. Une **extension du contrat de
retour** est nécessaire (voir section 5.2).

---

## 10. Conclusion

Le mécanisme est **parfaitement fonctionnel** dans un contexte multi-thread /
asynchrone, à condition que chaque worker retourne explicitement son `status`
(`fresh`/`stale`) et son `etag`.

La "seconde passe" est le coût à payer pour maintenir un serveur **stateless**
tout en profitant des optimisations de la Phase 1. Elle sera naturellement
atténuée, voire supprimée, lorsque le cache serveur optionnel sera implémenté
ultérieurement.

## 11. Décisions de conception (validées)

Les options suivantes sont **tranchées** et servent de référence pour
l'implémentation :

- **Transport de l'ETag** : en-tête HTTP standard `If-None-Match` / `ETag`.
- **Format du hash `S:`** : **XXH3-128 encodé intégralement en base 62**
  (sans troncature), calculé sur les **noms de services uniquement** (stable
  entre requêtes). Le choix d'un hash non cryptographique (`xxhash-rust`, algo
  `xxh3`) est délibéré : les fragments ne servent que de détecteur de
  changement rapide et stable, pas de garantie anti-falsification. L'encodage
  base62 réutilisable vit dans `arachnea-core::crypt::base62`.
- **Réponse « pas de changement »** : **code HTTP `304 Not Modified`** (sans
  body).
- **`enable_etag` par défaut** : **`true`** pour l'instant ; désactivé
  uniquement si nécessaire.
- **Paramètres GET volumineux** : format `sourceParams=` avec **JSON encodé**
  dans la query string.

**Points de vigilance restants (non bloquants) :**

- Limitation de la taille des URLs/query strings des serveurs en GET longue
  (encodage JSON des `sourceParams`).
- Gestion de l'échappement du `;` dans la valeur ETag (rare, mais à prévoir
  dans le parseur).
- Alignement `call_api` : détection du `304` et gestion du cache local ETag
  côté front.


**Prochaines étapes suggérées :**

1. Étendre `ScraperAggregationResult` avec les fragments et statuts par source.
2. Ajouter le support des en-têtes conditionnels dans `arachnea-http`.
3. Implémenter la `ValidationLayer` entre `StreamScraper` et `ScraperAgregator`.
4. Ajouter le support de l'en-tête `If-None-Match` / `ETag` dans le contrôleur REST.
5. (Futur) Implémenter le cache serveur (Redis / mémoire) pour la Phase 2.

---

## 12. Journal d'implémentation (2026-08-23)

Le mécanisme décrit ci-dessus est implémenté avec les adaptations suivantes :

| Point de l'analyse | Implémentation réelle |
|---|---|
| Fragments `E:` / `C:` | `arachnea-scrapyfy::conditional` — `E:<etag>` (composant percent-encodé), `C:<last_modified_unix>-<hash>` où `hash` = **XXH3-128 encodé intégralement en base 62** (`xxhash-rust` feature `xxh3` + `arachnea-core::crypt::base62`), plus un préfixe additionnel `N:<hash(source)>` pour les sources sans fetch (fragments stables, jamais validés). |
| Exposition statut/en-têtes HTTP | Le chemin conditionnel est implémenté directement dans le fetch racine du `query_executor` (`fetch_responses_with_validation`) via `HttpClient::send_for_request` qui expose déjà statut + en-têtes ; pas besoin d'étendre `FetchedResponse` au-delà de la variante marqueur `NotModified`. |
| Phase 1 / Phase 2 | `StreamScraper::execute_query_with_etag` : phase 1 parallèle avec fragments décodés, court-circuit 304 quand toutes les sources sont stale et que l'ETag global reconstruit correspond, phase 2 (GET complets) restreinte aux sources stale uniquement. |
| ETag global | `arachnea-stream::stream_etag` : `[S:<hash>;]<fragments>` sur noms de services triés alphabétiquement (hash `S:` calculé par le même helper `hash62` XXH3/base62 complet) ; décodage par position avec rejet en cas de mismatch du bloc `S:` ou de compte de fragments incorrect. |
| Contrôleur REST | Nouveau contrat `register_json_function` / `register_etag_result_function[_with_state]` dans `arachnea-core` : accès aux en-têtes entrants (`If-None-Match` normalisé), réponse `304` sans body, en-tête `ETag` sinon. Implémenté côté Warp et Tauri. |
| Bascule GET frontend | `call_api` émet des GET avec paramètres JSON-encodés dans la query string ; le désérialiseur backend (`query_string_to_json_value`) JSON-décode les valeurs commençant par `[`, `{`, `"` et agrège les clés répétées en tableaux. |
| Cache ETag front | Cache mémoire clé par commande + query string sérialisée ; rejeu `If-None-Match`, résolution du `304` depuis l'enveloppe cachée. |
| Commandes couvertes | `search`, `load_home`, `get_service`, `list_lives`, `get_category`, `get_section`, `get_banners`, `get_players`, `get_entry`, `get_season`, `get_live` (paramètres optionnels `arachneaEtag` / `enableEtag`, validation activée par défaut). `get_stream` et la route DRM restent hors périmètre. |

**Écarts connus / suivis :**

- La phase 2 refait un GET complet externe pour les sources stale (le cache serveur futur de §7 reste à implémenter).
- Les tests unitaires existants du workspace ne compilent pas indépendamment de ce changement (références préexistantes à une crate `resources` non liée dans les blocs `test-support` de `scraper_manager.rs`) ; les tests `arachnea-core` passent (20/20).
- Le cache ETag frontal est en mémoire (perte au rechargement) ; une persistance IndexedDB pourrait être ajoutée ultérieurement.

*Document mis à jour lors de l'implémentation.*
