# Analyse — erreurs par source dans les requêtes agrégées

## Demande

Les requêtes exécutées contre plusieurs sources ne doivent plus perdre les
données valides lorsqu'une seule source échoue. L'erreur doit être retournée
avec l'identité de la source concernée et le frontend doit l'afficher dans une
popup pour `load_home`, `search`, `get_banners`, `get_category` et
`get_section`.

Cette analyse décrit l'état actuel et la modification proposée. Elle ne change
pas encore le comportement applicatif.

## État actuel

### Exécution backend

`ScraperQueryCollection::execute_query` dans
`server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_query_collection.rs`
exécute une requête pour **une** collection, donc pour une source. Sa signature
actuelle est :

```rust
Result<Vec<HashMap<String, ScraperDataNode>>>
```

Cette fonction doit continuer à échouer normalement lorsqu'une requête propre à
la source est invalide ou que son HTTP/parsing échoue : elle constitue la limite
technique où l'erreur est produite.

Le comportement bloquant se situe juste au-dessus, dans
`ScraperAgregator::execute_query_async` (`scraper_agregator.rs`) :

1. les collections sélectionnées sont filtrées ;
2. chaque source est exécutée en parallèle ;
3. `futures::future::try_join_all` rassemble les résultats ;
4. le premier `Err` fait retourner `Err` à l'agrégateur ;
5. les données éventuellement déjà produites par les autres sources sont
   abandonnées.

La chaîne de contrôleurs transforme ensuite cet `Err` en erreur globale :

- le contrôleur sérialise les `Result` de commandes ;
- REST renvoie une réponse HTTP 400 ;
- Tauri rejette l'invocation ;
- `call_api` lève une exception côté frontend ;
- les écrans affichent leur état d'erreur global et les données ne sont jamais
  normalisées.

### Points d'entrée concernés

`StreamScraper` appelle l'agrégateur pour les requêtes demandées :

| Endpoint | Méthode backend | Forme actuelle | Particularité |
| --- | --- | --- | --- |
| `load_home` | `load_home` | tableau de lignes | toutes les sources configurées |
| `search` | `search` | tableau de groupes/lignes | toutes les sources, ou seulement celles paginées |
| `get_category` | `get_category` | tableau de lignes | sources définies dans la catégorie |
| `get_banners` | `get_banners` | objet fusionné | souvent une source, mais plusieurs si `source` est vide |
| `get_section` | `get_section` | objet fusionné | un appel par source, plusieurs appels parallèles côté frontend |

`get_entry`, `get_season`, les résolveurs et les fournisseurs proxy/IP utilisent
également l'agrégateur. La demande étend désormais le contrat d'enveloppe à
toutes les commandes JSON de `StreamScraper`; seuls les endpoints binaires et
les contrats internes non exposés au frontend gardent leur forme spécialisée.

### Frontend actuel

Le frontend attend directement les données :

- `loadHomeCatalog` et `getCategoryCatalog` appellent `normalizeHomeCatalog` ;
- `searchMediaItemsPage` lit les groupes source et calcule la pagination ;
- `getBanners` renvoie le payload brut, puis `normalizeBannersResponse` le
  convertit ;
- les composables `homeCatalogData` et `mediaSearchResults` transforment toute
  exception en erreur de page/recherche.

Il n'existe pas de composant de dialogue générique. Les messages d'état actuels
(`RouteStateMessage`, erreurs de chargement) sont intégrés aux écrans et ne
conviennent pas à une réussite partielle : dans ce cas, les résultats doivent
rester visibles pendant que l'utilisateur est averti des sources indisponibles.

## Contrat de réponse proposé (révisé)

La normalisation demandée s'applique à **toutes les commandes JSON enregistrées
par `StreamScraper`** (`search`, catalogue, détails, lives, résolveurs et
metadata), et non seulement aux quatre endpoints initiaux. Les endpoints de
flux binaires (`proxy`, licence DRM) restent hors de ce contrat, puisqu'ils ne
retournent pas de JSON métier.

Chaque commande JSON retournera une enveloppe JSON stable,
`ScraperAggregationResult<T>` :

```json
{
  "data": [/* données, dans le format propre à la commande */],
  "errors": [
    {
      "code": "ARACHNEA_E172147680012300",
      "source": "example-source",
      "operation": "get_category",
      "message": "Request failed: …"
    }
  ]
}
```

Pour `get_banners`, `data` conserve sa forme objet actuelle après la fusion :

```json
{
  "data": { "banners": [] },
  "errors": []
}
```

`data` conserve exactement la forme précédente de la commande : tableau pour
`search` ou `load_home`, objet pour `get_banners`, objet de détail pour
`get_entry`, etc. Les consommateurs qui ont besoin du contenu ne doivent donc
pas connaître la nature agrégée ou mono-source de l'appel.

Principes du contrat :

- `data` est toujours présent, y compris si toutes les sources échouent ;
- `errors` est toujours présent et vaut `[]` en cas de succès complet ;
- chaque erreur contient obligatoirement un `code`, l'`operation`, `source`
  lorsqu'elle est connue, et un `message` technique ;
- l'ordre des données réussies reste l'ordre des sources configurées ;
- l'ordre des erreurs suit ce même ordre, indépendamment de l'ordre de fin des
  futures ;
- une erreur de source n'est jamais convertie en HTTP 400/Tauri reject : le
  résultat est sérialisé avec succès, même si `data` est vide ;
- les erreurs de validation avant exécution et les défaillances ne pouvant pas
  être rattachées à une source doivent elles aussi être représentées dans
  `errors`, avec `source: null`. Cela permet à `call_api` de traiter toutes les
  réponses métier selon la même règle ; les erreurs de protocole (JSON invalide,
  route inconnue, panne réseau avant toute réponse) sont normalisées côté
  frontend dans la même représentation d'erreur et la même pile de
  notifications.

Le `message` technique est nécessaire au diagnostic, mais n'est pas affiché
par défaut. L'UI présentera d'abord un message localisé construit à partir de
`operation`, `source` et du code. Le détail est révélé volontairement par
l'utilisateur. La chaîne d'erreur complète est aussi tracée côté Rust avec le
même code, le nom de source et l'opération.

## Modification Rust proposée

### Types et code de corrélation

Introduire des types sérialisables génériques partagés (dans `arachnea-core`,
ou dans un module commun dépendance de `arachnea-scrapyfy`), par exemple :

```rust
#[derive(Serialize)]
pub struct ScraperExecutionError {
    pub code: String,
    pub operation: String,
    pub source: Option<String>,
    pub origin: ScraperErrorOrigin,
    pub message: String,
}

#[derive(Serialize)]
pub struct ScraperAggregationResult<T> {
    pub data: T,
    pub errors: Vec<ScraperExecutionError>,
}
```

Le générateur de code sera placé dans `arachnea-core`, seul crate déjà partagé
par `arachnea-scrapyfy`, `arachnea-stream` et les autres services. Son API sera
petite et indépendante du scraping, par exemple
`next_error_code() -> ArachneaErrorCode`. Cela permet de la réutiliser dans un
autre service sans lui faire dépendre de types de scraper.

Format retenu : `ARACHNEA_E{timestamp_millis_utc}{sequence:02}`. Le générateur
centralisé conserve atomiquement le dernier milliseconde et le compteur `00..99`.
S'il reçoit plus de 100 demandes pendant la même milliseconde, il avance au
prochain créneau logique avant de générer le code suivant ; cela évite une
collision dans le processus. Le code est lisible et correspond à la convention
proposée. La valeur sera écrite comme champ structuré des logs, pas seulement
intégrée dans le message.

`ScraperAgregator::execute_query_async` deviendrait le point de conversion des
erreurs de `ScraperQueryCollection::execute_query` en `ScraperExecutionError`.
Il utiliserait `join_all` (ou un équivalent qui conserve tous les résultats)
plutôt que `try_join_all`, puis n'agrégerait que les lignes `Ok`.

Cette approche garde les requêtes concurrentes. Elle ne transforme pas une
erreur de parsing ou de transport en faux succès : elle la rend simplement
locale à la source qui l'a produite.

À chaque conversion, le backend écrira un log `tracing::error!` comprenant au
minimum `error_code`, `operation`, `source` et la chaîne complète de l'erreur.
Le code affiché dans le navigateur permet ainsi de retrouver exactement cette
entrée dans les logs serveur.

`origin` vaut `backend` pour toute erreur produite par le serveur. Il permet au
frontend de ne pas prétendre qu'un code généré localement correspond à une ligne
de log backend.

### Adaptation de `StreamScraper`

Toutes les méthodes exposées comme commandes JSON retourneraient l'enveloppe.
Les quatre méthodes demandées et `get_section` reçoivent une attention
fonctionnelle particulière :

- `search`, `load_home` et `get_category` transmettent directement le tableau
  `data` de l'agrégateur ;
- `get_banners` fusionne uniquement les lignes réussies dans son objet actuel,
  puis conserve `errors` dans l'enveloppe.
- `get_section` fusionne uniquement les lignes réussies dans sa section, puis
  conserve les erreurs. C'est indispensable car `loadHomeSectionPage` lance
  aujourd'hui plusieurs appels `get_section` en parallèle, un par source.

Les méthodes mono-source (`get_entry`, `get_players`, `get_season`, `get_live`,
`get_stream`) enveloppent leur valeur existante dans `data`. Elles utilisent le
même champ `errors`, même si celui-ci est généralement vide. Les fournisseurs
internes (proxy/IP) peuvent extraire `data` lorsqu'ils ne sont pas des commandes
UI, afin que leur contrat interne reste limité.

Les rustdocs actuellement inexacts (« échoue si une source échoue ») seront
mis à jour. Le changement de contrat public du serveur sera ajouté à
`server/CHANGELOG.md` (ou au `CHANGELOG.md` applicable après vérification de
son emplacement).

## Modification frontend proposée

### Adaptateur de contrat universel dans `call_api`

Dans `front/src/services/rustify.ts`, ajouter les types génériques miroir :

```ts
interface ScraperExecutionError {
  code: string
  operation: string
  source: string | null
  origin: 'backend' | 'frontend'
  message: string
}

interface ScraperAggregationResult<T> {
  data: T
  errors: ScraperExecutionError[]
}
```

`call_api<T>` deviendra la frontière unique. Il suit ce protocole :

1. après une réponse REST/Tauri valide, il valide l'enveloppe, envoie chaque
   élément de `errors` à la file globale de notifications, écrit le détail dans
   `console.error`, puis retourne seulement `data` comme `Promise<T>` ;
2. en cas d'erreur technique (HTTP non-2xx sans enveloppe, rejet Tauri, échec
   réseau, JSON invalide ou enveloppe absente/malformée), il crée une
   `ScraperExecutionError` synthétique avec `origin: 'frontend'`,
   `source: null`, `operation: fct_name` et le message technique disponible ;
3. cette erreur synthétique est ajoutée à la même file et loggée avec
   `console.error`, puis l'appel est rejeté afin que le flux de chargement en
   cours cesse d'utiliser une donnée inconnue.

Un petit générateur frontend, distinct mais de même format que celui du backend,
attribue un code `ARACHNEA_E{timestamp_millis_utc}{sequence:02}` aux erreurs
synthétiques. Son code est un identifiant de diagnostic navigateur : la popup
peut indiquer qu'il est d'origine frontend et les développeurs le retrouvent
dans la console, mais il n'est pas censé exister dans les logs serveur lorsqu'il
n'a pas été possible de joindre le backend. Si le backend fournit déjà une
erreur structurée avec code dans une réponse non-2xx, `call_api` réutilise ce
code et conserve `origin: 'backend'`.

Les fonctions de normalisation existantes ne changent donc pratiquement pas et
les erreurs de tous les endpoints reçoivent exactement le même affichage.
Pendant le déploiement, le lecteur peut accepter temporairement un payload nu
comme valeur `data` avec `errors: []`.

### Pile de popups accessible et flux de données

Créer un module `useErrorNotifications` (source unique de vérité) qui maintient
une file réactive de notifications, et deux composants ciblés :

- `ErrorNotificationStack.vue` : rend la pile dans un `Teleport` fixé en bas à
  droite ; reçoit une liste en lecture seule et réémet la fermeture ;
- `ErrorNotification.vue` : rend une erreur, le message localisé, le code de
  corrélation, le bouton fermer et le détail technique extensible.

`App.vue` reste une surface de composition : il lit la file et connecte
`dismiss(code)`, sans connaître les appels backend. `call_api` est le seul
producteur automatique de notifications ; chaque erreur produit donc une popup
et l'ordre de la pile suit l'ordre de réception. Une limite explicite (par
exemple 5 visibles, les suivantes restant en file) évitera qu'un lot de sources
masque l'interface.

Chaque notification sera un `role="alert"` non modal pour ne pas interrompre
la navigation ni le chargement partiel. Elle disposera d'un bouton accessible,
d'un détail replié par défaut (`<details>` ou bouton `aria-expanded`) et de la
fermeture par `Escape` pour la notification active. Les valeurs techniques seront
interpolées en texte uniquement, jamais via `v-html`.

Pour une erreur `origin: 'frontend'`, le message localisé indiquera une erreur
technique lors de l'opération demandée, et le détail comportera explicitement
« origine : frontend ». Pour une erreur `origin: 'backend'`, le message reste
centré sur la source lorsque celle-ci est connue, et le détail affiche
« origine : backend ». Dans les deux cas, le code est visible et copiable.

Les émissions seront branchées comme suit :

| Flux | Producteur | Traitement |
| --- | --- | --- |
| toutes les commandes JSON | `call_api` | une notification et un `console.error` par élément de `errors` |
| `load_home`, `search`, `get_category` | leurs services existants | les données valides sont normalisées comme avant |
| `get_section` | `loadHomeSectionPage` | les sources valides mettent à jour la section ; les autres génèrent leurs notifications sans interrompre `Promise.all` |
| `get_banners` | `deferredBannerLoader` | les bannières valides sont ajoutées au fil de l'eau ; chaque erreur est déjà notifiée par `call_api` |

Une réponse sans erreur ne déclenche rien. Une réponse avec données et erreurs
ne renseigne pas `errorMessage` : les cartes et sections valides restent donc
visibles. Une erreur globale de transport/API garde le comportement actuel
d'état d'erreur, car aucune donnée exploitable n'est disponible.

Les libellés du dialogue seront ajoutés aux deux catalogues existants
`front/public/locales/fr.json` et `front/public/locales/en.json`.

## Plan de développement détaillé

1. **Établir les types backend communs.** ✅ Terminé
   - [x] `arachnea-core::error_code::ErrorCodeGenerator` — générateur thread-safe de codes au format `ARACHNEA_E{millis}{seq:02}`
   - [x] `arachnea-core::error_code::ArachneaErrorCode` — newtype sérialisable
   - [x] `arachnea-core::scraper_result::ScraperAggregationResult<T>` — enveloppe `data`/`errors`
   - [x] `arachnea-core::scraper_result::ScraperExecutionError` — champs `code`, `operation`, `source` (Option), `origin`, `message`
   - [x] `arachnea-core::scraper_result::ScraperErrorOrigin` — enum `Backend`/`Frontend` sérialisée en `snake_case`
   - Voir `server/crates/arachnea-core/src/error_code.rs` et `server/crates/arachnea-core/src/scraper_result.rs`
2. **Rendre l'agrégateur tolérant aux erreurs.** ✅ Terminé
   - [x] `try_join_all` → `join_all` dans `execute_query_async`
   - [x] Ajout du paramètre `operation: &str` pour les codes d'erreur
   - [x] Collecte complète : chaque `Err` produit un `ScraperExecutionError` avec code, log `tracing::error!`, source nommée
   - [x] Résultats `Ok` conservés dans l'ordre de configuration
   - [x] Nouveau type de retour : `ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>`
   - [x] Générateur de codes centralisé : `static ERROR_CODE_GEN` via `LazyLock<ErrorCodeGenerator>`
   - [x] Mise à jour des appelants internes : `proxy_provider.rs`, `ip_country_provider.rs`, `stream_resolver.rs`, `stream_scraper.rs`
   - Voir `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs`
3. **Propager l'enveloppe dans `StreamScraper`.** ✅ Terminé
   - [x] Toutes les méthodes de commande JSON retournent `Result<ScraperAggregationResult<T>>`
   - [x] `search`, `load_home`, `get_service`, `list_lives`, `get_category` : retour direct de `ScraperAggregationResult<Vec<...>>`
   - [x] `get_entry`, `get_season`, `get_live` : extraction d'une entrée unique, `ScraperAggregationResult::new(data, errors)`
   - [x] `get_section`, `get_banners`, `get_players` : fusion des lignes réussies, conservation des erreurs dans l'enveloppe
   - [x] `get_stream` : enveloppe `ScraperAggregationResult::ok(ResolvedStream)`
   - [x] Mise à jour des rustdocs (suppression des `# Errors` obsolètes mentionnant l'échec source)
   - [x] Mise à jour des tests : extraction de `.data` pour compatibilité avec `test_query`
   - Voir `server/crates/arachnea-stream/src/stream_scraper.rs`
4. **Vérifier le contrat serveur.** ✅ Terminé
   - [x] Ajout de tests directs dans `scraper_agregator.rs` : succès complet,
     succès partiel, échec complet, ordre configuré, code de corrélation et
     erreur pré-exécution avec `source: null`.
   - [x] Les erreurs pré-exécution (groupe absent ou sélection sans source
     configurée) produisent désormais une enveloppe avec code, log
     `tracing::error!` et `source: null`.
   - [x] Les tests existants de `StreamScraper` vérifient que les enveloppes de
     succès n'ont aucune erreur avant d'extraire `data`.
   - [x] `CHANGELOG.md` mis à jour avec le contrat d'erreurs par source.
   - Validation : `cargo test -p arachnea-scrapyfy scraper_agregator::tests --lib`
     et `cargo check --tests`.
5. **Centraliser la lecture frontend.** ✅ Terminé
   - [x] Types partagés `ScraperAggregationResult<T>`,
     `ScraperExecutionError` et `ScraperErrorOrigin` dans
     `front/src/types/scraperError.ts`, réexportés par `rustify.ts`.
   - [x] `call_api` valide l'enveloppe pour REST et Tauri, notifie chaque
     élément de `errors`, puis retourne uniquement `data` aux normaliseurs.
   - [x] Compatibilité transitoire : un payload nu est lu comme
     `{ data: payload, errors: [] }`; une enveloppe partielle ou invalide est
     une erreur frontend.
   - [x] Erreurs REST non-2xx, réponses JSON invalides ou vides, rejet Tauri
     et échecs réseau deviennent des erreurs structurées frontend, sont
     journalisées, ajoutées à la file, puis relancées.
   - [x] Générateur frontend monotone de codes
     `ARACHNEA_E{millis}{sequence:02}`.
   - [x] File réactive minimale `useErrorNotifications` ajoutée comme source
     de vérité; le rendu accessible de la pile reste le point 6.
   - Validation : `npm run type-check`.
6. **Construire la pile UI.** ✅ Terminé
   - [x] `useErrorNotifications` fournit la file réactive globale, en lecture
     seule, avec les actions `enqueue` et `dismiss`.
   - [x] `ErrorNotification.vue` rend une erreur non modale avec `role="alert"`,
     message localisé, code copiable, détails techniques repliés, origine et
     bouton de fermeture accessible.
   - [x] `ErrorNotificationStack.vue` téléporte la pile en bas à droite,
     limite l'affichage à cinq notifications et ferme l'élément actif avec
     Escape.
   - [x] `App.vue` raccorde la file et le rendu global, indépendamment des
     changements de route.
   - [x] Traductions ajoutées dans les catalogues `en` et `fr`.
   - Validation : `npm run type-check`.
7. **Adapter les chargements concurrents.** ✅ Terminé
   - [x] `loadHomeSectionPage` utilise `Promise.allSettled` et ne fusionne que
     les sources résolues; une erreur technique ne fait plus perdre les pages
     réussies concurrentes.
   - [x] Les données vides ou partielles retournées par une enveloppe restent
     des réponses résolues et mettent à jour les sections normalement.
   - [x] Les workers de bannières différées conservent leur isolation par
     source : un rejet technique est déjà notifié par `call_api` et ne bloque
     pas les autres travailleurs.
   - Validation : `npm run type-check`.
8. **Validation finale.** ⏳ Partiellement terminée
   - [x] `cargo check -p arachnea-scrapyfy -p arachnea-stream`
   - [x] `cargo test -p arachnea-scrapyfy scraper_agregator::tests --lib`
     couvre succès total/partiel, échec total, ordre et codes de corrélation.
   - [x] `cargo fmt --check`, `npm run type-check`, `npm run build`,
     `npm run lint:css` et `git diff --check`.
   - [ ] Scénario manuel avec serveur et source réellement défaillante pour
     `load_home`, `search`, `get_category`, `get_banners` et `get_section` :
     non exécuté, aucun serveur local ni configuration de source défaillante
     n'étant disponible dans cette session.

## Décisions validées

1. **Toutes les sources échouent.** La proposition retourne `data` vide et une
   notification par erreur. L'écran conserve son état « aucun contenu » ou son
   état dédié existant, plutôt qu'une panne globale.
2. **Une requête mono-source échoue.** Elle utilise aussi l'enveloppe et crée
   une notification ; le traitement est ainsi vraiment uniforme pour tous les
   appels backend.
3. **`get_banners` différé.** Ses échecs ne doivent plus être silencieux ; ils
   sont notifiés automatiquement par `call_api`, sans interrompre les autres
   chargements concurrents.
4. **`get_section` concurrent.** Chaque appel est aujourd'hui mono-source mais
   plusieurs appels sont lancés simultanément par section. `Promise.all` doit
   aboutir avec les sections vides/partielles retournées par l'enveloppe, afin
   de préserver les données des autres sources.
5. **Pagination de recherche.** Les sources en erreur ne doivent pas produire
   de paramètres de page suivante. Les groupes réussis continuent de déterminer
   `haveMore` et `sourceParams` comme aujourd'hui.
6. **Messages d'erreur.** La décision est désormais : message localisé par
   défaut dans la notification, détail technique visible sur demande et présent
   dans la console/logs avec le code de corrélation.

Ces règles sont validées pour l'implémentation. Elles signifient notamment que
les échecs scraper, y compris ceux d'un appel mono-source, ne doivent plus être
convertis en exception frontend : `call_api` traite leur enveloppe, conserve
`data` et crée les notifications nécessaires.

## Limites de contrat restantes

Aucun choix produit supplémentaire n'est bloquant. Les limites techniques
suivantes sont établies afin de ne pas confondre erreur scraper et erreur de
transport :

- les commandes JSON exposées par `StreamScraper` retournent toujours
  `ScraperAggregationResult<T>` ;
- les endpoints binaires (`proxy`, licence DRM) conservent leur protocole
  spécifique ;
- une réponse HTTP non-2xx, une invocation Tauri rejetée avant sérialisation,
  une réponse JSON invalide ou une enveloppe absente/malformée sont converties
  par `call_api` en erreurs standardisées `origin: frontend`, affichées dans la
  même pile puis relancées vers le flux appelant ;
- pour un échec sans source identifiable, `source` vaut `null` : la
  notification localisée indique alors l'opération et le code de corrélation,
  sans inventer de source.

## Validation prévue après implémentation

- vérification de compilation Rust ciblée pour `arachnea-scrapyfy` et
  `arachnea-stream` ;
- vérification TypeScript/Vue du frontend avec les scripts existants ;
- scénario manuel ou test existant : deux sources, une qui réussit et une qui
  échoue, pour chacun des cinq endpoints UI ;
- contrôle que les données de la source valide restent rendues, qu'une
  notification par erreur apparaît et que le code correspond aux logs backend.

Conformément aux règles du dépôt, aucun nouveau test ne sera créé sans demande
explicite. Les tests existants touchés seront mis à jour si nécessaire.

## Impact et compromis

Il s'agit d'une évolution de contrat backend/frontend, pas d'un correctif
strictement local : toutes les commandes JSON de `StreamScraper` changent d'un
payload nu vers une enveloppe `data`/`errors`. L'enveloppe est préférable à
l'injection de fausses lignes d'erreur dans les données, car elle garde les
normaliseurs, schémas YAML et la pagination indépendants des erreurs
opérationnelles. Le traitement central dans `call_api` réduit fortement les
risques d'oublier une nouvelle commande ; la file globale est justifiée car les
notifications doivent survivre aux changements de vues et aux chargements
concurrents.
