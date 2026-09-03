# Analyse - Généralisation de l'administration des groupes de services

> Date : 2026-09-03
> Statut : analyse révisée après décisions d'architecture
> Périmètre : `front/admin-app`, administration HTTP/Tauri et crates Rust

## Objectif

L'administration ne doit plus exposer uniquement les sources du groupe
`arachnea-stream`. Elle doit permettre de consulter et de modifier les états
d'activation des sources des groupes, dans cet ordre :

1. `arachnea-stream`
2. `arachnea-stream-hoster`
3. `arachnea-proxies`
4. `arachnea-ip-countries`

Le nom canonique retenu est `arachnea-stream-hoster`, déjà utilisé par le
répertoire `server/services/arachnea-stream-hoster/`. La variante
`arachnea-stream-host` mentionnée dans la demande ne correspond à aucun groupe
chargé aujourd'hui et ne doit pas devenir un second identifiant.

Chaque groupe doit être une unité d'affichage du frontend et une partie de
l'identité technique d'une source. Les titres et descriptions de groupe sont
des données exclusivement frontend, localisées dans les fichiers de locale ; le
backend ne doit pas les fournir ni les connaître.

## État actuel

### Catalogues de services

Les quatre manifestes existent déjà :

| Groupe | Manifeste |
| --- | --- |
| `arachnea-stream` | `server/services/arachnea-stream/services.json` |
| `arachnea-stream-hoster` | `server/services/arachnea-stream-hoster/services.json` |
| `arachnea-proxies` | `server/services/arachnea-proxies/services.json` |
| `arachnea-ip-countries` | `server/services/arachnea-ip-countries/services.json` |

Le manifeste `arachnea-stream` est récursif : il importe notamment les
collections `legal-stream` et `dark-stream`. Les autres groupes déclarent
directement leurs fichiers YAML. Les quatre formats s'appuient donc déjà sur
les primitives de catalogue et d'activation de Scrapyfy ; le manque concerne
l'orchestration d'exécution et l'exposition dans l'API d'administration.

### Backend

La logique d'administration est concentrée dans
`server/crates/arachnea-stream/src/admin/` :

- `mod.rs` définit `AdminState`, protège l'enregistrement des opérations et
  enregistre toutes les routes `admin/*` dans `register_admin_service`.
- `ops.rs` rassemble les opérations de session, catalogue de services,
  activation, identifiants, paramètres applicatifs et rechargement.
- `dto.rs` contient à la fois les DTO génériques de services et les DTO propres
  aux paramètres du serveur.
- `auth.rs` contient l'authentification, les sessions et la limitation des
  tentatives de connexion.

`main.rs` assemble ces composants, construit le scraper rechargable,
génère/imprime le mot de passe temporaire et appelle actuellement
`register_admin_service` avant le lancement du contrôleur.

Le groupe et le nom du store de persistance sont aujourd'hui déclarés dans
`stream_scraper.rs` (`STREAM_SERVICE_GROUP_NAME` et
`STREAM_SERVICES_STORE_NAME`). Le store typé `arachnea-services` contient les
enregistrements `SourceServiceRecord`, identifiés uniquement par `source_id` :
un booléen `enabled` et les colonnes `login`/`password`. Ces deux dernières sont
chiffrées par l'adaptateur applicatif
`typed_service_credentials_store.rs`. Cette donnée utilisateur persiste entre
deux lancements.

Scrapyfy possède déjà les abstractions génériques de catalogue, de chargement
de manifestes et de politique d'activation dans ses modules `scrapyfy/*`, ainsi
que le type de persistance des sources. En revanche, le contrat de l'API admin
est encore façonné par le seul facade `ReloadableStreamScraper`.

`RestServerHandle` n'a pas à être déplacé : il se trouve déjà dans
`arachnea-core::controler::rest::supervisor` et est réexporté par
`arachnea_core::controler`. C'est la bonne couche pour un handle de serveur
REST générique. Son utilisation par les opérations de paramètres peut rester
un adaptateur applicatif.

Le générateur aléatoire du mot de passe temporaire doit devenir une primitive
sans état de `arachnea-core`, par exemple dans `crypt`. Scrapyfy appelle cette
primitive au moment de construire son état d'authentification lorsqu'aucun hash
permanent ne lui est fourni. Il retourne ensuite le secret créé à l'exécutable
afin que celui-ci soit affiché une fois sur stdout ; Stream ne porte donc plus
la logique de génération, de stockage en mémoire ni de vérification.

L'intégralité des routes `admin/*` doit devenir un service générique Scrapyfy :
`status`, `login`, `logout`, `services`, `set-service-enabled`,
`reset-service-enabled`, `credentials`, `set-credentials`,
`clear-credentials`, `settings`, `update-settings`, `set-admin-password` et
`reload`. Les opérations qui requièrent une donnée ou une action propre à
l'application l'obtiennent par un adaptateur injecté, au lieu de rester des
routes implémentées dans Stream.

### Limites du modèle de persistance actuel

La collision d'identifiant est traitée par la clé composite retenue dans cette
analyse. Les autres comportements suivants sont indépendants de ce changement
de clé et restent hors du périmètre de la généralisation :

1. `register_defaults()` crée un enregistrement pour toute source au démarrage.
   La présence d'une ligne ne signifie donc pas qu'un administrateur a créé une
   surcharge ; `has_override` est généralement erroné.
2. `clear_enabled()` supprime l'enregistrement complet. La réinitialisation de
   l'activation efface donc aussi le login et le mot de passe chiffrés.
3. L'écriture des identifiants recrée, lorsqu'il n'existe pas de ligne, un
   enregistrement avec `enabled: true`. Après une réinitialisation, renseigner
   des identifiants peut ainsi modifier indirectement l'état effectif.
4. Les mutations d'activation et d'identifiants sont deux séquences distinctes
   lecture-modification-écriture du même enregistrement ; elles peuvent perdre
   une mise à jour concurrente.

Le commentaire qualifie le mot de passe temporaire de « one-shot », mais il
n'est pas consommé lors d'une connexion réussie : il reste valable jusqu'à
l'arrêt du processus. La terminologie doit être corrigée indépendamment de la
généralisation, sauf volonté explicite de rendre ce secret réellement à usage
unique.

### Frontend

`front/admin-app` est une SPA Vue indépendante. Son API est centralisée dans
`src/services/adminApi.ts` et `src/composables/useAdminApi.ts`. La réponse
`services` y est actuellement une liste plate de `AdminServiceEntry`.
`src/views/ServicesView.vue` charge cette liste, affiche les actions
d'activation et les identifiants, puis marque le rechargement comme nécessaire.

Les locales sont des ressources publiques chargées depuis
`front/admin-app/public/locales/`. Les entrées de navigation et le titre de
l'application sont aujourd'hui statiques ; il n'existe pas de ressource de
configuration publique pour définir les groupes administrables.

## Écarts à résoudre

1. Une source est actuellement adressée par son seul identifiant. Cet
   identifiant n'est plus suffisant dès que plusieurs manifestes sont exposés :
   deux groupes peuvent légitimement définir le même identifiant.
2. Les opérations d'administration mélangent les responsabilités génériques
   des sources Scrapyfy et les responsabilités de l'application Stream
   (configuration, serveur REST, façade de rechargement, mode desktop).
3. Le frontend n'a ni la liste ordonnée des groupes, ni leurs textes
   localisés, ni un contrat API contenant le groupe d'une source.
4. La modification de l'état d'une source hors stream doit provoquer le même
   effet de rechargement que pour une source stream, sans rendre le module
   admin dépendant des facades Stream, proxy ou IP-country.

## Architecture cible

### Répartition des responsabilités

| Couche | Responsabilités proposées | Responsabilités exclues |
| --- | --- | --- |
| `arachnea-core` | Contrats de contrôleur, réponses HTTP/Tauri, `RestServerHandle`, primitives de persistance et générateur aléatoire de mot de passe temporaire. | Catalogues Scrapyfy, groupes de services, YAML, politique d'administration et configuration de l'application Stream. |
| `arachnea-scrapyfy` | Service d'administration complet : routes `admin/*`, DTO, sessions, authentification, contrôle des écritures, catalogue multi-groupes, activation/identifiants, rechargement explicite des catalogues, paramètres runtime et décision de redémarrage, `register_admin_service`. | Tauri context, fichiers/configuration propres à une application et implémentations concrètes de runtime. |
| `arachnea-stream` | Composition minimale : ouverture des stores, déclaration des groupes et implémentation des adaptateurs vers `ApplicationConfiguration`, `RestServerHandle`, `ReloadableStreamScraper` et les runtimes proxy/IP-country. Affichage ponctuel du secret temporaire retourné par Scrapyfy. | Routes admin, sessions, authentification, politique de rechargement, sélection/activation des services et décision de redémarrage. |
| `front/admin-app` | Chargement de `config.json`, navigation par groupe, filtrage/ordre d'affichage et résolution localisée des titres/descriptions. | Définition métier ou persistance de l'état d'une source. |

Cette séparation permet à un autre exécutable de réutiliser Scrapyfy et son
administration de sources sans dépendre de `arachnea-stream`. Elle préserve
aussi le fait que le contrôleur REST et le binaire restent réutilisables dans
des projets qui n'emploient pas Scrapyfy.

### Identité stable d'une source

Toute source administrable doit être identifiée par le couple :

```text
(service_store_id, service_id)
```

Les DTO de réponse et les entrées des opérations d'écriture doivent donc
contenir `service_store_id` et `service_id`. Le frontend ne doit jamais déduire
le groupe à partir du nom du service ni d'un chemin YAML.

Exemple de forme cible :

```json
{
  "service_store_id": "arachnea-proxies",
  "service_id": "proxifly",
  "title": "Proxifly",
  "enabled": true,
  "credentials": null
}
```

Le même couple doit être utilisé par les opérations `set-service-enabled`,
`reset-service-enabled`, `credentials`, `set-credentials` et
`clear-credentials`, ainsi que par les erreurs de sources indisponibles.

`arachnea-services` est conservé comme namespace de persistance unique. Son
enregistrement évolue en ajoutant la colonne `service_store_id`, qui forme avec
`source_id` la clé primaire composite :

```text
SourceServiceRecord {
  service_store_id: String, // primary key
  source_id: String,        // primary key
  enabled: bool,
  login: Option<String>,
  password: Option<String>
}
```

Les identifiants chiffrés restent donc sur le même enregistrement que l'état
d'activation. Toutes les lectures et écritures du module Scrapyfy adressent la
ligne par `(service_store_id, source_id)` ; aucune concaténation de clé ne doit
être exposée dans l'API.

Le changement de clé primaire est incompatible avec la base SQLite déjà créée.
Il n'y aura volontairement aucune migration : les données du namespace
`arachnea-services` sont supprimées avant l'initialisation du nouveau schéma,
puis les valeurs par défaut des manifestes sont recréées. Les surcharges
d'activation et les identifiants existants sont donc perdus. La suppression doit
couvrir `records.sqlite3` et ses fichiers SQLite associés (`-wal` et `-shm`) ou
l'équivalent géré par l'implémentation du store ; elle doit être annoncée dans le
`CHANGELOG.md` comme action opérateur destructive.

### Module d'administration Scrapyfy

`arachnea-scrapyfy` doit exposer un module public dédié, par exemple
`arachnea_scrapyfy::admin`, regroupant :

- l'état d'authentification, les sessions opaques, la limitation des tentatives
  et les contrôles de méthode/origine ;
- les DTO et entrées d'opérations propres aux sources ;
- la résolution d'une référence `(groupe, service)` ;
- la lecture/écriture de l'activation et des identifiants dans le store
  `arachnea-services` qualifié par groupe ;
- la construction du catalogue de groupes et de sources déclarées, y compris
  les sources indisponibles ;
- les DTO et opérations de paramètres runtime, de mot de passe et de
  rechargement ;
- `register_admin_service`, qui enregistre toutes les routes `admin/*`.

La configuration d'un groupe doit fournir son identifiant technique, son
catalogue/manifeste et un accès au runtime rechargable qui consomme ce
catalogue. Aucune chaîne de titre ou de description de groupe ne doit figurer
dans cette structure Rust.

Pour éviter une dépendance sur `ReloadableStreamScraper`, Scrapyfy reçoit un
`AdminRuntimeAdapter` limité. Cet adaptateur fournit la lecture/écriture du hash
de mot de passe permanent, les valeurs persistées de port/réseau/racine, les
capacités d'application à chaud et les hooks vers les runtimes concrets. Les
DTO, la validation des demandes, le calcul de `restart_required`, la réponse
`update-settings` et le rapport de rechargement restent gérés par Scrapyfy.

Scrapyfy orchestre lui-même le rechargement des manifestes administrés et de la
politique d'activation. Lorsque le rechargement doit remplacer un composant
spécifique à l'application, il appelle un hook étroit de l'adaptateur. Stream se
limite alors à recréer ou raccorder ses façades Stream, proxy et IP-country,
sans choisir les groupes, appliquer les surcharges ou construire la réponse
admin.

`RestServerHandle` reste dans Core. L'adaptateur Stream peut l'utiliser pour
appliquer effectivement un port, un mode réseau ou une racine ; il renvoie à
Scrapyfy le résultat d'application. Scrapyfy garde ainsi la décision et le
contrat de redémarrage sans dépendre de l'implémentation REST concrète.

L'authentification est également propriété de Scrapyfy. Son état reçoit, par
l'adaptateur, le hash permanent persistant et les règles éventuelles propres à
l'exécutable. La politique standard (desktop/loopback sans session, session
requise à distance, POST et origine contrôlée pour les écritures) est définie
une fois dans Scrapyfy et s'applique à toutes les routes `admin/*`.

La fonction de composition des services de `arachnea-stream` construit les
adaptateurs et les quatre définitions de groupe, puis appelle une seule fois
`arachnea_scrapyfy::admin::register_admin_service(...)` avant
`controler.launch()`.

### Rechargement et effets d'exécution

Une bascule d'activation ne doit pas se limiter à modifier la base : elle doit
être prise en compte par le consommateur du manifeste concerné. Le rechargement
reste explicitement demandé par l'UI après une mutation ; cette décision est
confirmée.

Scrapyfy reconstruit et valide de façon atomique les catalogues des quatre
groupes avant de les publier. Il appelle les hooks applicatifs seulement pour
les composants qui ne sont pas génériques. Un échec d'un groupe est rapporté
avec son identifiant et ne doit ni invalider le runtime actif ni provoquer
l'application partielle de l'état. Cela évite notamment qu'une source proxy ou
IP-country apparaisse activée dans l'UI alors que le fournisseur actif utilise
encore un catalogue ancien.

## Contrat frontend

### Configuration publique

Ajouter `front/admin-app/public/config.json`, copié tel quel par Vite dans le
bundle admin :

```json
{
  "title": "Arachnéa Stream",
  "service-store": [
    "arachnea-stream",
    "arachnea-stream-hoster",
    "arachnea-proxies",
    "arachnea-ip-countries"
  ]
}
```

Le tiret du nom JSON est volontaire. Le type TypeScript le représente comme
`'service-store'`; le code ne doit pas inventer une seconde forme camelCase de
la ressource publiée.

`title` configure directement le titre de la barre applicative. Il est distinct
des titres de groupes et n'est pas localisé par cette proposition. Le fichier
doit être validé au chargement : titre non vide, tableau de chaînes non vides et
sans doublon. Une configuration absente ou invalide doit afficher une erreur
configurée/localisée et ne pas présenter une administration incomplète.

### Locales des groupes

Chaque locale déclare uniquement ses textes :

```json
{
  "service-store": {
    "arachnea-stream": {
      "title": "Arachnéa stream",
      "description": "Services de streaming vidéo."
    },
    "arachnea-stream-hoster": {
      "title": "Hébergeurs de streaming",
      "description": "Services de résolution des lecteurs vidéo."
    }
  }
}
```

La locale anglaise doit définir les quatre groupes configurés. Les autres
locales peuvent ne définir qu'un sous-ensemble.

La résolution ne doit pas dépendre du comportement implicite de la fonction
`t`, car les deux champs ont des règles différentes :

| Champ | Ordre de résolution | Valeur finale |
| --- | --- | --- |
| Titre de groupe | locale sélectionnée, puis `en` | identifiant du groupe |
| Description de groupe | locale sélectionnée, puis `en` | absence de description |

Une valeur absente, non textuelle ou vide est considérée comme absente. Le
fallback ne s'applique pas au titre des sources individuelles déjà fourni par
leur YAML/API : il s'applique seulement à l'en-tête du groupe.

### Navigation et vue des services

La configuration détermine l'ordre et la visibilité des groupes. L'API fournit
les sources et leur groupe, tandis que le frontend forme les sections dans
l'ordre de `service-store`.

La navigation latérale doit créer une entrée par groupe, menant à une vue de
services filtrée sur cet identifiant. Une route paramétrée unique, par exemple
`/services/:serviceStoreId`, évite d'enregistrer dynamiquement des routes pour
chaque valeur de `config.json` et préserve les liens directs. La vue affiche le
titre et, lorsqu'elle existe, la description résolus ci-dessus ; elle conserve
les actions existantes pour les sources du groupe.

Les réponses pour un groupe déclaré mais vide ou entièrement indisponible restent
visibles sous leur entrée de navigation. Les groupes reçus de l'API mais absents
de `config.json` ne sont pas affichés : la configuration est la liste explicite
des groupes administrables par cette distribution. Ils doivent toutefois être
signalés dans la console ou dans les diagnostics de développement afin de
révéler une divergence frontend/backend.

## Approches écartées

### Conserver des identifiants de source globaux

Cette solution paraît minimale, mais elle impose une convention cachée entre des
catalogues indépendants et rend toute collision future destructrice dans le
store d'administration. Le couple groupe/service est nécessaire dès maintenant.

### Définir les titres de groupes dans les manifestes YAML ou l'API

Cela dupliquerait les textes entre backend et frontend et contredirait la
contrainte de localisation. Les manifestes restent des données techniques ; les
textes d'interface restent dans les locales de `front/admin-app`.

### Déplacer toute l'administration dans `arachnea-core`

`arachnea-core` ne dépend pas de Scrapyfy et doit rester exploitable par une
application sans scraper. Le catalogue, les manifests et les états des sources
appartiennent à `arachnea-scrapyfy`, de même que l'authentification du service
admin. Core ne reçoit que des primitives sans connaissance du domaine admin,
comme le générateur de secret temporaire.

### Faire dépendre Scrapyfy de `ReloadableStreamScraper`

Cette inversion de dépendance empêcherait la réutilisation demandée. Scrapyfy
doit dépendre d'un contrat de rechargement limité, fourni par l'application, et
non de la façade Stream.

## Plan d'implémentation proposé

1. [x] Introduire dans Scrapyfy les références de source namespacées et faire évoluer
   `SourceServiceRecord` avec `service_store_id` comme seconde clé primaire dans
   le namespace conservé `arachnea-services`.
2. [x] Déplacer dans `arachnea-scrapyfy::admin` toutes les routes admin, les DTO,
   les sessions, l'authentification, les paramètres runtime et l'orchestration
   de rechargement ; exposer `register_admin_service`.
3. [x] Ajouter dans `arachnea-core` le générateur sans état de mot de passe
   temporaire, puis faire retourner le secret nouvellement créé par le service
   admin Scrapyfy à l'exécutable appelant.
4. [x] Faire construire par la composition Stream les quatre groupes et un
   `AdminRuntimeAdapter` minimal vers sa configuration et ses runtimes concrets.
5. [x] Faire évoluer le contrat API et le frontend simultanément pour transporter
   `service_store_id` dans toutes les opérations de source.

6. [x] Ajouter `config.json`, les clés de locale anglaises et françaises, la route
   paramétrée et les entrées de navigation par groupe.
7. [x] Supprimer les données `arachnea-services` existantes sans migration lors de
   l'introduction du schéma incompatible, puis documenter cette action
   destructive dans le `CHANGELOG.md`.
8. [x] Mettre à jour les tests existants affectés et la documentation publique, puis
   exécuter les vérifications Rust et frontend pertinentes.

La demande ne requiert pas de nouvelle infrastructure de test. Il n'existe pas
de tests Rust dédiés aux routes admin ; les tests existants relatifs à la
persistance d'activation et à la vue Services devront toutefois être adaptés
s'ils couvrent les contrats modifiés.

## Implémentation - Phase 1 backend (2026-09-03)

- Ajout de `SourceServiceKey` dans `arachnea-scrapyfy`, avec une clé primaire
  SQLite composite ordonnée `(service_store_id, source_id)` pour
  `SourceServiceRecord`. `ScraperSourceDescriptor`, les catalogues et la
  politique d'activation propagent désormais le groupe technique.
- Création de `arachnea_scrapyfy::admin` : routes `admin/*`, sessions,
  authentification, DTO, contrôle des écritures, catalogues multi-groupes et
  orchestration de rechargement sont sortis de `arachnea-stream`. Les mutations
  acceptent et renvoient `service_store_id`; l'omission reste tolérée seulement
  lorsqu'un `service_id` est non ambigu entre les groupes.
- Ajout de `arachnea_core::crypt::generate_random_password`. Le service admin
  Scrapyfy crée le secret temporaire lorsqu'aucun hash permanent n'est défini,
  puis le binaire Stream l'affiche une fois sur stdout.
- Ajout de `StreamAdminRuntimeAdapter` et déclaration des groupes, dans l'ordre
  retenu : `arachnea-stream`, `arachnea-stream-hoster`,
  `arachnea-proxies`, `arachnea-ip-countries`. Le runtime Stream concret est
  rechargé via l'adaptateur; les autres groupes sont validés et leurs états
  d'activation/identifiants sont administrables par le service générique en
  attendant le raccordement de leurs consommateurs runtime dédiés.
- Le store `data/persistence/arachnea-services/` antérieur au schéma composite
  est supprimé une fois avant ouverture (répertoire complet, donc
  `records.sqlite3`, `-wal` et `-shm` inclus), puis marqué comme v2. Les
  surcharges et identifiants antérieurs sont volontairement perdus.
- Validation exécutée : `cargo check -p arachnea-stream` et
  `cargo test -p arachnea-stream --lib` (13 réussis, 1 test fournisseur live
  ignoré).

## Implémentation - Phase 2 frontend (2026-09-03)

- Le frontend `front/admin-app` transporte désormais `service_store_id` dans
  toutes les opérations de source : catalogue (`AdminServiceEntry`,
  `AdminServiceStore`, `ServicesResponse`), `set-service-enabled`,
  `reset-service-enabled`, `credentials`, `set-credentials` et
  `clear-credentials`. La recherche d'une entrée `credentials` accepte la clé
  composite `store/service` ou la clé nue `service`, sans jamais déduire le groupe
  d'un jeton.
- Ajout de `public/config.json` (`title` + `service-store` ordonné des quatre
  groupes) et de `services/appConfig.ts`, qui valide la ressource au chargement
  (titre non vide, tableau de chaînes non vides et sans doublon). Une
  configuration absente ou invalide affiche une erreur localisée
  (`config.loadFailed`) au lieu de présenter une administration incomplète. La
  barre applicative tire son titre de `config.title` (repli sur la locale
  statique).
- La locale anglaise définit les quatre groupes; la française en définit
  également quatre. Les titres/descriptions de groupe suivent les deux chaînes
  de fallback (locale sélectionnée puis `en`; titre : identifiant du groupe en
  dernier recours, description : absence) via `serviceStoreTitle` /
  `serviceStoreDescription` exposés par le module i18n, réactifs aux
  changements de langue.
- La navigation latérale crée une entrée par groupe configuré, menant à une route
  paramétrée unique `/services/:serviceStoreId` qui préserve les liens directs.

  La vue Services filtre les sources et les sources indisponibles sur le groupe de
  la route, affiche titre/description résolus et garde le flux d'activation /
  identifiants existant. Les groupes reçus de l'API mais absents de
  `config.json` restent masqués et sont signalés dans la console; une valeur de
  route inconnue redirige vers le premier groupe configuré.
- Validation exécutée : `npm run build` (type-check + build Vite) de
  `front/admin-app`, vérification que `config.json` est bien copié dans le
  bundle `dist/admin/`, et `cargo check -p arachnea-stream` (backend inchangé
  en cette phase, toujours compilable).

## Décisions confirmées

- Le rechargement demandé par l'UI reste explicite après chaque bascule. Il est
  orchestré par Scrapyfy pour les quatre groupes.
- Les services appartenant à un groupe absent de `config.json` restent masqués
  par le frontend.
- L'authentification, les sessions et le contrôle des écritures appartiennent à
  `arachnea-scrapyfy`, pas à `arachnea-stream`.
- La génération sans état du mot de passe temporaire appartient à
  `arachnea-core`; son cycle de vie est géré par le service admin Scrapyfy avec
  les paramètres optionnels fournis par l'application.
- La remise à zéro de `arachnea-services` est volontairement destructive et
  remplace toute migration des anciens identifiants ou surcharges.

## Validation prévue

- Vérifier qu'une source portant le même `service_id` dans deux groupes peut
  être activée, désactivée et recevoir des identifiants sans collision.
- Vérifier qu'une base `arachnea-services` issue de l'ancien schéma est
  supprimée, que le nouveau schéma comporte la clé primaire composite et que les
  valeurs par défaut des quatre manifestes sont recréées sans migration.
- Vérifier que chaque groupe apparaît dans l'ordre de `config.json`, que les
  titres et descriptions respectent les deux chaînes de fallback, et qu'une
  description absente n'affiche aucun substitut.
- Vérifier qu'une mutation suivie du rechargement actualise les consommateurs
  Stream, hoster, proxy et IP-country, sans remplacer un runtime valide en cas
  d'échec de reconstruction.
