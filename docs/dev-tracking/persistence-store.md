# Analyse d’architecture — persistence store typé

**Date** : 31/08/2026

**Statut** : phases 0 à 3 terminées — prête pour l’implémentation de la phase 4 (bascule applicative).

## Avancement

| Phase | État | Résultat |
|---|---|---|
| 0 — contrats | Terminée | Stores initiaux, clés, TTL et schémas des trois entités définis. |
| 1 — socle typé | Terminée | Contrats de schéma, stores typés mémoire/fichier et validation ajoutés. |
| 2 — repositories | Terminée | Proxys, sessions Cloudflare et activations de sources migrés hors de `PersistedRecord`. |
| 3 — SQLite | Terminée | Backend SQLite, métadonnées, évolution de schéma et requêtes SQL implémentés derrière la feature `sqlite-persistence`. |
| 4 — bascule | À faire | Composition SQLite applicative et suppression des adaptateurs legacy. |
| 5 — validation | À faire | Couverture des backends et de l’évolution de schéma. |

## Décision

Faire évoluer la persistance applicative vers un système **typé par entité** et **déclaré par schéma**. Le code métier ne manipule ni `PersistedRecord`, ni `Map<String, Value>`, ni des chemins de champ textuels. Il utilise des repositories dédiés qui retournent et enregistrent directement ses entités.

SQLite est le backend de production visé. Chaque store nommé porte un unique schéma et une unique table ; SQLite crée automatiquement les colonnes et index manquants, et refuse toute incompatibilité de type. Les backends fichier et mémoire restent compatibles avec le même contrat typé : ils ne créent pas de colonnes physiques, mais stockent et retournent les mêmes entités.

Il n’y a aucune migration des fichiers JSON existants vers SQLite. La base SQLite démarre vide ; les anciens fichiers restent intacts et inutilisés après la bascule.

## Objectifs et limites

- Données métier toujours représentées par des entités Rust typées.
- Évolution du schéma pilotée par une configuration Rust explicite.
- Recherches métier explicites et indexées lorsqu’elles sont nécessaires.
- Compatibilité fonctionnelle avec SQLite, fichier et mémoire.
- Aucune API SQL exposée aux domaines.
- Aucune migration automatique de données JSON existantes.

Les credentials chiffrés et les snapshots IP → pays restent hors de cette évolution : ils disposent déjà de contrats typés et ont des contraintes de stockage distinctes.

## État actuel

Le contrat legacy `PersistenceStore` stocke encore un `PersistedRecord` générique. Les trois usages métier ont toutefois été migrés vers des repositories typés ; `LegacyTypedEntityStore` isole provisoirement l’adaptation au contrat historique dans `arachnea-core`.

| Usage | Chemin actuel | Décision cible |
|---|---|---|
| Inventaire de proxys | `ProxyRecord → ProxyRepository → TypedEntityStore<ProxyRecord>` | Migration terminée. |
| Session Cloudflare | `CachedChaserSession → CloudflareSessionRepository → TypedEntityStore<CachedChaserSession>` | Migration terminée. |
| Activation de source | `SourceEnabledOverride → SourceEnabledRepository → TypedEntityStore<SourceEnabledOverride>` | Migration terminée. |

`CredentialsStore` et `IpCountryStore` sont déjà des interfaces typées. Ils ne doivent pas être forcés dans le nouveau store générique lors de cette première étape.

## Entités persistées

Une entité persistée déclare sa clé et son schéma. Son store nommé est fourni explicitement à la composition de l’application. Le backend l’écrit et la lit sans passer par une map dans l’API de domaine.

```rust
pub trait PersistentEntity: Send + Sync + Sized + 'static {
    type Key: EntityKey;

    fn key(&self) -> Self::Key;
    fn schema() -> EntitySchema;
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()>;
    fn read_from(reader: &EntityReader<'_>) -> Result<Self>;
}
```

`EntityWriter` et `EntityReader` sont des adaptateurs internes, typés par le schéma. Ils peuvent être implémentés par un binder SQL, un codec de fichier ou une représentation mémoire, sans exposer de map JSON aux domaines.

`EntityKey` est limité dans la première version aux clés scalaires `String`, `Integer` et `Uuid`. Chaque clé est nommée et typée par le schéma de son entité ; elle n’est pas une colonne universelle `key TEXT` du store.

Une macro `derive(PersistentEntity)` pourra réduire le code répétitif après stabilisation de plusieurs entités. La première version doit utiliser des implémentations manuelles : elles rendent les colonnes, valeurs optionnelles et règles d’expiration explicites.

### Liste explicite des fusions, créations et retraits

| Objets/fichiers actuels | Action cible | Objet final | Motif |
|---|---|---|---|
| `ProxyRecord` + `PersistedRecord` + conversions `From<&ProxyRecord>` / `TryFrom<&PersistedRecord>` de `proxy_persistence.rs` | Fusion et retrait des conversions. | `ProxyRecord` implémente `PersistentEntity`; sa clé primaire obligatoire `authority: String` est dérivée de l’hôte et du port, et son repository calcule son expiration. | Les données sont identiques ; l’aller-retour JSON/map est redondant. |
| `CachedChaserSession` + enveloppe `PersistedRecord` créée dans `ChaserSessionCache` | Fusion et retrait de l’enveloppe. | `CachedChaserSession` implémente `PersistentEntity`, porte `origin`, un éventuel `schema_version` et son expiration déclarée. | La session est déjà le format persistant minimal. |
| `bool` d’activation + `Map { enabled }` | Création d’une entité. | `SourceEnabledOverride { source_id: String, enabled: bool }`. | Supprime une map monofield et permet une évolution future de l’état. |
| `PersistenceSourceEnabled` | Conservation, mais remplacement de son accès direct au store. | La policy dépend de `SourceEnabledRepository`. | La policy de valeurs par défaut est une responsabilité métier distincte du stockage. |
| `ChaserSessionCache` | Conservation, mais remplacement de son accès direct au store. | Le cache dépend de `CloudflareSessionRepository`. | Il conserve la logique de fraîcheur et de génération des en-têtes. |
| `PersistedRecord`, `PersistenceKey`, `PersistenceTransaction`, `PersistenceBackend`, `record_matches_filters` | Retrait après migration complète des trois consommateurs génériques. | `PersistentEntity`, `TypedEntityStore<E>` et repositories métier. | Ces objets ne servent qu’au format map et au filtrage générique abandonnés. |
| `WafSession` et `CachedChaserSession` | Aucune fusion. | Deux objets conservés. | `WafSession` appartient au runtime du solveur ; la session en cache choisit explicitement ce qui peut être persisté. |
| `StoredCredentials` / `CredentialsStore` | Aucune fusion. | Store spécialisé conservé. | Secrets, chiffrement et permissions de fichier. |
| `IpCountryRecord` / `IpCountryStore` | Aucune fusion. | Store spécialisé conservé. | Chargement/remplacement complet d’un snapshot, différent d’un CRUD par entité. |

## Repositories métier

Les domaines dépendent de repositories spécifiques à leurs opérations, jamais d’une recherche générique par champ :

```rust
#[async_trait]
pub trait ProxyRepository: Send + Sync {
    async fn find_by_country(&self, country: &str) -> Result<Vec<ProxyRecord>>;
    async fn save_many(&self, records: &[ProxyRecord]) -> Result<()>;
    async fn delete(&self, authority: &str) -> Result<()>;
}
```

Les interfaces prévues à court terme sont :

- `ProxyRepository` : lecture par pays, sauvegarde groupée, suppression par autorité ;
- `CloudflareSessionRepository` : lecture, écriture et suppression par origine ;
- `SourceEnabledRepository` : lecture, écriture et suppression par identifiant de source ;
- futur `UserProfileRepository` : lecture, écriture, suppression et requêtes métier telles que `get_by_email`.

Les repositories isolent le domaine du moteur et évitent qu’une évolution de la structure SQL impose des modifications aux appelants.

## Contrat de stockage commun

Le `PersistenceStore` actuel, utilisé derrière `Arc<dyn PersistenceStore>`, ne doit pas recevoir de méthodes génériques du type `get<E>()` : elles ne sont pas compatibles avec cette utilisation en trait object.

Le type est porté par un contrat interne dédié :

```rust
#[async_trait]
pub trait TypedEntityStore<E>: Send + Sync
where
    E: PersistentEntity,
{
    async fn get(&self, key: &E::Key) -> Result<Option<E>>;
    async fn put(&self, entity: &E) -> Result<()>;
    async fn put_all(&self, entities: &[E]) -> Result<()>;
    async fn delete(&self, key: &E::Key) -> Result<()>;
    async fn find(&self, query: &EntityQuery<E>) -> Result<Vec<E>>;
}
```

Chaque repository reçoit un `Arc<dyn TypedEntityStore<E>>`. Un store concret est lié à une seule entité et à un seul schéma ; il ne sert pas plusieurs entités.

`EntityQuery<E>` exprime une ou plusieurs égalités typées, combinées par `AND`. Un repository conserve une API métier telle que `ProxyRepository::find_by_country`, mais la traduit vers cette requête interne :

```rust
EntityQuery::<ProxyRecord>::new()
    .where_eq(ProxyFields::country(), "FR")
    .where_eq(ProxyFields::supports_https(), true);
```

Les descripteurs `ProxyFields::…` sont générés ou définis avec le schéma de l’entité. Les appelants ne fournissent ni map, ni chemin textuel libre, ni valeur JSON non typée. SQLite produit un `WHERE` paramétré sur les colonnes ; fichier et mémoire évaluent les mêmes prédicats sur les entités pendant un scan.

## Schéma déclaré et évolution automatique

Chaque `PersistentEntity` fournit un `EntitySchema`. Celui-ci est la référence de l’application pour les colonnes de son store dédié.

```rust
EntitySchema::new()
    .primary_key(Field::string("authority"))
    .field(Field::string("host"))
    .field(Field::integer("port"))
    .field(Field::string("country").indexed())
    .field(Field::date_time("expires_at").expiration())
    .field(Field::integer("schema_version"));
```

Chaque schéma déclare exactement une clé primaire scalaire par son nom et son type. Un champ ordinaire décrit son chemin logique, son type (`String`, `Integer`, `Boolean`, `DateTime` ou `Json`), son caractère nullable, son index éventuel et son rôle optionnel. Une sous-structure ou collection reste possible avec un champ `Json` explicitement déclaré, sans réintroduire de document libre.

Tous les backends reçoivent les schémas à leur construction par une configuration commune :

```rust
pub struct PersistenceStoreConfig {
    pub name: String,
    pub schema: EntitySchema,
}
```

`name` est l’identifiant stable d’un store et de l’entité qu’il contient. Il sert aux diagnostics, aux métadonnées et au rangement des fichiers. Le nom `proxy-inventory`, par exemple, désigne un store de `ProxyRecord`; les profils utilisateur auront un autre store explicitement nommé.

`PersistenceStoreConfig::name` et `PersistenceStoreConfig::schema` sont obligatoires : il n’existe ni nom ni liste de schémas par défaut. Tous les constructeurs de store reçoivent donc explicitement une configuration, y compris les stores fichier et mémoire. Cette règle rend chaque emplacement et chaque diagnostic non ambigus. `SqlitePersistenceStore::new(..., config)` applique la configuration physiquement ; les autres backends l’emploient pour la validation et les diagnostics. Le repository vérifie que le schéma de son entité correspond à celui du store reçu.

À la première ouverture d’un store SQLite :

1. Créer la table si elle n’existe pas, avec la colonne primaire déclarée par l’entité, par exemple `authority TEXT PRIMARY KEY` ou `id BLOB PRIMARY KEY`.
2. Lire la structure existante et les métadonnées de schéma détenues par Arachnéa.
3. Pour chaque champ déclaré absent, ajouter automatiquement sa colonne nullable.
4. Pour chaque index déclaré absent, le créer automatiquement.
5. Pour chaque champ existant, comparer son type logique mémorisé avec la configuration Rust ; échouer avec une erreur contextualisée en cas d’incompatibilité.
6. Vérifier également le nom et le type de la clé primaire ; échouer en cas d’incompatibilité.
7. Ne jamais modifier automatiquement un type, une clé primaire, ou supprimer une colonne.

SQLite ayant un typage permissif, `PRAGMA table_info` ne suffit pas. Le backend conserve une table interne de métadonnées des colonnes logiques et utilise des tables `STRICT` ou des contraintes `CHECK` appropriées. Un booléen est stocké comme `INTEGER CHECK (value IN (0, 1))`.

### Réconciliation des index

Les index gérés par Arachnéa sont réconciliés dans les deux sens à l’ouverture contrôlée du store :

- index déclaré et absent : création ;
- index géré par Arachnéa mais retiré de la configuration : suppression ;
- définition modifiée : création du nouvel index, puis suppression de l’ancien ;
- index interne SQLite (`PRIMARY KEY`, `UNIQUE`) ou non référencé dans les métadonnées Arachnéa : jamais supprimé.

La table interne de métadonnées enregistre la signature de chaque index géré (colonnes, ordre et nom physique). Elle évite toute suppression accidentelle d’un index externe. Une colonne n’est jamais supprimée automatiquement, même si elle est retirée de la configuration : elle peut contenir des données à préserver.

L’ajout de colonne est léger. La création ou suppression d’un index peut verrouiller le schéma et la création parcourt les données existantes ; ces opérations doivent donc être effectuées à l’ouverture contrôlée du store, jamais dans un chemin de requête chaud.

## Métadonnées de cycle de vie

Les métadonnées ne sont pas globales : elles sont des champs déclarés de l’entité.

- Une entité avec TTL déclare un `DateTime` ayant le rôle `expiration`. SQLite l’indexe et purge les entrées expirées ; fichier et mémoire appliquent la même règle à la lecture.
- `updated_at` n’est pas généré automatiquement : une entité le déclare uniquement lorsqu’elle en a un besoin métier.
- Une version de structure est un champ `schema_version` optionnel de l’entité concernée. Elle reste utile aux proxys et sessions Cloudflare, mais ne doit pas être imposée aux profils ou aux activations de source.

## Backends

| Backend | Représentation interne | Lecture et recherche | Rôle |
|---|---|---|---|
| SQLite | Colonne primaire nommée et typée, puis colonnes définies par `EntitySchema`. | Construction directe de l’entité depuis une ligne ; requêtes de repository sur colonnes indexées. | Production et données durables. |
| Fichier | Entité sérialisée directement dans le fichier du store. | Désérialisation directe ; scan dans le repository lorsqu’une recherche est nécessaire. | Compatibilité et simplicité. |
| Mémoire | `HashMap<E::Key, E>` typée. | Retour direct de l’entité ; scan ou index mémoire ultérieur. | Tests et constructeurs de compatibilité. |

SQLite supprime ainsi les étapes actuelles `entité → JSON/map → entité`. Le backend fichier sérialise une entité une fois au disque, et le backend mémoire peut éviter toute sérialisation. Les objets intermédiaires ne sont conservés que lorsqu’ils ont une responsabilité distincte, comme `CachedChaserSession` face à `WafSession`.

## Intégration SQLite

SQLite est fourni par `rusqlite = 0.40.2` avec `default-features = false` et `features = ["bundled"]`, derrière une feature Cargo optionnelle `sqlite-persistence` de `arachnea-core`, activée par `arachnea-stream`. Le module `sqlite_store.rs` et sa réexportation sont protégés par cette feature ; les autres crates ne compilent donc pas SQLite par défaut.

Chaque store possède son répertoire, par exemple `data/persistence/<store-name>/`, contenant `records.sqlite3`. Ce choix est nécessaire car le mode WAL ajoute les fichiers auxiliaires `-wal` et `-shm`.

`rusqlite` étant synchrone, chaque opération est exécutée dans `tokio::task::spawn_blocking`; la connexion est protégée par un mutex acquis dans cette tâche. À l’ouverture de la base :

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA busy_timeout = 5000;
```

Les écritures SQLite sont validées par opération. Il n’y a pas de cache d’écriture à doubler : l’équivalent de l’ancien `commit` est un no-op pour ce backend. Les écritures groupées passent par `put_all` et sont validées dans une unique transaction SQLite.

## Performances attendues

La cible à colonnes typées améliore principalement les chemins proxys et Cloudflare : moins d’allocations, aucun aller-retour `serde_json::Value`, liaison SQL directe, et filtres SQL sur des colonnes indexées.

Le coût d’écriture augmente avec chaque index, quel que soit le format. Les index doivent donc être limités aux requêtes exposées par les repositories. Le gain d’architecture et de validation est la raison principale de cette évolution ; les gains de performance devront être mesurés avec des données représentatives avant toute optimisation supplémentaire.

## Plan d’implémentation proposé

### Phase 0 — contrats et décisions finales

**✅ Phase validée.**

1. ✅ Stores initiaux : `proxy-inventory`, `cloudflare-session`, `arachnea-services`.
2. ✅ `ProxyRecord` : clé primaire obligatoire `authority: String`, index unitaire `country`, expiration calculée depuis son état runtime.
3. ✅ `CachedChaserSession` : clé primaire obligatoire `origin: String`, expiration issue de `clearance_expires_at`.
4. ✅ `SourceEnabledOverride` : clé primaire obligatoire `source_id: String`, champ `enabled: Boolean`.
5. ✅ Toute entité CRUD possède une clé primaire scalaire. Les entités sans clé métier reçoivent une UUID ; les événements append-only sont hors du contrat de cette première version.

### Phase 1 — socle typé sans changement de backend

**✅ Phase implémentée.**

1. ✅ Ajout dans `arachnea-core` de `PersistentEntity`, `EntitySchema`, `Field`, `EntityQuery<E>`, `EntityReader`, `EntityWriter`, `TypedEntityStore<E>` et `PersistenceStoreConfig`.
2. ✅ `MemoryPersistenceStore<E>` et `FilePersistenceStore<E>` exigent une configuration explicite, avec un unique schéma validé. Les anciennes implémentations nécessaires aux consommateurs non encore migrés sont exposées sous `LegacyMemoryPersistenceStore` et `LegacyFilePersistenceStore`.
3. ✅ Le store mémoire conserve directement les entités ; le store fichier écrit un document d’entités validé par schéma, avec `put_all` validé avant une unique écriture atomique.
4. ✅ Les deux backends valident les champs déclarés, leur type, la clé primaire, les champs obligatoires et le rôle d’expiration.

### Phase 2 — repositories et suppression des conversions domaine

**✅ Phase implémentée.**

1. ✅ `ProxyRepository` et `TypedProxyRepository` encapsulent l’inventaire ; `ProxyRecord` est une `PersistentEntity` et `proxy_persistence.rs` a été retiré.
2. ✅ Le cache Chaser utilise un repository de sessions typées ; `CachedChaserSession` est persistée séparément de `WafSession`.
3. ✅ `SourceEnabledOverride` et `SourceEnabledRepository` remplacent la map `{ enabled }`; `PersistenceSourceEnabled` dépend du repository.
4. ✅ Les trois domaines ne manipulent plus `PersistedRecord` ni `Map<String, Value>`. L’adaptateur `LegacyTypedEntityStore` garde temporairement cette compatibilité à l’intérieur de `arachnea-core`, jusqu’au backend SQLite de la phase 3.

### Phase 3 — backend SQLite et évolution de schéma

**✅ Phase implémentée.**

1. ✅ Feature Cargo `sqlite-persistence` et `rusqlite` 0.40.2 avec `bundled`, activée par `arachnea-stream`.
2. ✅ `SqliteEntityStore` (`sqlite_store.rs`) : répertoire par store (`<root>/<store-name>/records.sqlite3`), WAL, `synchronous = FULL`, `busy_timeout`, opérations en `spawn_blocking` derrière un mutex, `put_all` validé puis commité en une transaction unique.
3. ✅ Table `STRICT` par store avec clé primaire typée et booléens `INTEGER CHECK (IN (0, 1))`, plus les tables internes de métadonnées `arachnea_columns` et `arachnea_indexes`.
4. ✅ Évolution à l’ouverture : création de la table absente, ajout des colonnes déclarées manquantes (toujours nullables), création/suppression réconciliée des index, échec contextualisé en cas de changement de type, de clé primaire ou de colonne/index non géré.
5. ✅ `get`, `put`, `put_all`, `delete` et `find` opèrent directement sur les colonnes typées ; l’expiration est purgée par SQL dans `find` et filtrée par `get`. `EntityReader` gagne des lectures optionnelles (`optional_string`, etc.) pour les champs nullables. Les tests unitaires du backend (roundtrip, TTL, batch, évolution, conflit de type) sont en place.

### Phase 4 — bascule applicative et nettoyage

**⏳ À faire.** L’application reste composée autour des stores legacy au travers de l’adaptateur typé transitoire ; aucune donnée JSON existante ne sera migrée.

1. Construire le store applicatif explicitement nommé avec les schémas initiaux dans `arachnea-stream`.
2. Injecter les repositories SQLite dans l’inventaire de proxys, Chaser-CF et les activations de sources.
3. Conserver les backends fichier et mémoire comme implémentations compatibles pour les constructeurs de compatibilité et les tests.
4. Retirer `PersistedRecord`, `PersistenceKey`, `PersistenceTransaction`, `PersistenceBackend`, `record_matches_filters` et les adaptateurs temporaires uniquement après migration de tous les consommateurs.

### Phase 5 — validation

**⏳ À faire.** Les vérifications réalisées à ce stade sont `cargo check --workspace`, les tests de `arachnea-core` et les tests ciblés du cache Cloudflare. Les scénarios SQLite restent à créer après la phase 3.

1. Adapter les tests existants : lecture/écriture directe d’entités, requête sur un ou plusieurs champs, expiration, redémarrage, SQLite/fichier/mémoire.
2. Vérifier l’évolution du schéma : ajout de colonne, ajout/suppression d’index, incompatibilité de type et index externe préservé.
3. Exécuter `cargo fmt --check`, les tests concernés de `arachnea-core` avec SQLite et `cargo check -p arachnea-stream`.

## Passation pour la suite

### État réel après les phases 1 et 2

- Les APIs métier ne manipulent plus `PersistedRecord` : `ProxyInventory` dépend de `ProxyRepository`, le cache Chaser dépend d’un repository de sessions et `PersistenceSourceEnabled` dépend de `SourceEnabledRepository`.
- `LegacyTypedEntityStore<E>` est le seul adaptateur entre ces repositories et l’ancien `Arc<dyn PersistenceStore>`. Il vit dans `arachnea-core::persistence::typed_store`; il convertit encore les documents au format map exclusivement à cette frontière transitoire.
- La composition de `arachnea-stream` transmet encore le store legacy. Elle n’instancie pas encore les trois `FilePersistenceStore<E>` ou les futurs `SqlitePersistenceStore<E>` explicitement nommés.
- `MemoryPersistenceStore<E>` et `FilePersistenceStore<E>` sont les noms publics des stores typés. Les implémentations historiques sont désormais `LegacyMemoryPersistenceStore` et `LegacyFilePersistenceStore`.
- `proxy_persistence.rs` a été supprimé. Ne pas le réintroduire : les règles de TTL proxy sont maintenant portées par l’implémentation `PersistentEntity` de `ProxyRecord`.

### Ordre de travail recommandé

1. Implémenter et valider `SqlitePersistenceStore<E>` sans modifier les repositories métier.
2. Dans `arachnea-stream`, construire un store SQLite distinct et explicitement configuré pour chaque entité : `proxy-inventory`, `cloudflare-session` et `arachnea-services`.
3. Injecter ces stores dans `TypedProxyRepository`, le repository Cloudflare et `TypedSourceEnabledRepository`; retirer les créations de `LegacyTypedEntityStore` des chemins applicatifs.
4. Faire passer les constructeurs de compatibilité et les tests aux stores mémoire/fichier typés lorsque cela est nécessaire.
5. Seulement après qu’aucun appelant applicatif ne dépend plus de `Arc<dyn PersistenceStore>`, retirer les types et modules legacy.

### Subtilités à préserver

- Ne pas migrer les fichiers JSON existants vers SQLite. Une base SQLite nouvelle et vide est le comportement attendu.
- Les schémas actuels de `ProxyRecord` et `CachedChaserSession` utilisent un champ `Json` pour leur charge complexe, avec des colonnes séparées pour la clé, le pays ou l’expiration. La phase SQLite doit conserver cette compatibilité de schéma ; une normalisation plus fine des colonnes est un changement distinct qui ne doit pas être introduit implicitement.
- L’expiration est déterminée par les entités : proxy = minimum entre la fenêtre de fraîcheur de probe et le cooldown ; session Cloudflare = expiration de `cf_clearance`, ou TTL de repli lorsque celle-ci est absente. Le backend SQLite doit appliquer ces mêmes règles à la lecture et aux recherches.
- Les repositories Cloudflare et activation sont actuellement construits à partir du store legacy pour préserver les chemins existants. Lors de la bascule, injecter les repositories/stores typés au lieu de recréer un adaptateur legacy.
- Ne supprimer `PersistedRecord`, `PersistenceKey`, `PersistenceTransaction`, `PersistenceBackend`, `record_matches_filters`, les codecs et les stores legacy qu’après une recherche globale confirmant l’absence d’import ou de constructeur encore utilisé. Les tests historiques du store legacy devront être remplacés ou retirés dans le même changement.
- Ne pas supprimer les stores typés mémoire et fichier : ils restent utiles aux constructeurs de compatibilité et aux tests, même après la bascule SQLite.

## Risques et points à valider

- Cette évolution touche `arachnea-core`, `arachnea-proxy`, `arachnea-http` et `arachnea-scrapyfy` : elle doit être menée progressivement.
- Une abstraction d’entité trop générique peut cacher les besoins réels ; les recherches restent dans les repositories métier.
- L’ajout futur d’un champ est automatique, mais un changement de type doit rester une erreur nécessitant une migration explicite décidée par le projet.
- Les anciens JSON ne sont volontairement pas repris : la bascule réinitialise les caches et préférences stockées dans l’ancien backend.

## Décisions confirmées

Les décisions suivantes sont actées :

1. ✅ **Nom de store obligatoire** — chaque `PersistenceStoreConfig` reçoit un nom explicite ; il n’existe aucun nom par défaut.
2. ✅ **Champs ajoutés nullable** — l’entité applique ses règles de champs obligatoires à la lecture et à l’écriture.
3. ✅ **Pas d’index composite dans la première version** — uniquement des index unitaires configurés.
4. ✅ **Conflit de type = erreur** — aucun outil ni mécanisme de migration de données n’est prévu à ce stade.
5. ✅ **Écritures groupées** — `put_all` est atomique pour tous les backends.
6. ✅ **Clé primaire par entité** — chaque `EntitySchema` définit le nom et le type de sa clé primaire ; aucune colonne `key TEXT` universelle n’existe. La première version supporte `String`, `Integer` et `Uuid`, sans clé composite.
7. ✅ **UUID SQLite** — les UUID sont stockés comme `BLOB` sur 16 octets.
8. ✅ **Stores initiaux** — `proxy-inventory`, `cloudflare-session` et `arachnea-services` portent respectivement `ProxyRecord`, `CachedChaserSession` et `SourceEnabledOverride`.
9. ✅ **Configuration** — chaque point de composition construit explicitement son `PersistenceStoreConfig { name, schema }`.
10. ✅ **Clé des proxys** — `authority: String` est la clé primaire obligatoire de `ProxyRecord`.

✅ Il ne reste aucun point d’architecture bloquant. La phase 3 doit encore confirmer les choix physiques SQLite, notamment la représentation exacte de `DateTime`, des UUID et du JSON.

## Recommandation

La prochaine étape est la phase 3 : introduire SQLite et la réconciliation de schéma, puis remplacer l’adaptateur legacy à la phase 4.
