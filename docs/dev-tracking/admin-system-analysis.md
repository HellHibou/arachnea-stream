# Analyse : système d'administration Arachnéa

> Date : 2026-08-28
> Statut : phases 1 à 5 implémentées ; phases 6 et 7 en attente

## Objet

L'évolution doit fournir une administration indépendante du front de consultation, publiée sous /admin. Elle couvre le catalogue et l'activation des services Arachnéa Stream, les identifiants des services, le mot de passe administrateur, le port, le réseau, le rechargement, le systray et une fenêtre desktop dédiée. C'est une évolution structurelle : cette analyse doit être validée avant toute implémentation.

## État actuel

Les services sont déclarés par des manifests JSON récursifs. Le manifest racine server/services/arachnea-stream/services.json importe legal-stream et dark-stream. arachnea-scrapyfy::resolve_manifest_sources aplatit les imports et conserve path, enabled et les paramètres. Au chargement, ScraperAgregator::add_query_collection_from_config ignore définitivement une entrée désactivée.

Les YAML fournissent déjà id, title, logo et description localisée. Un logo peut contenir un placeholder tel que {base_url} ; l'admin devra le résoudre avec les paramètres fusionnés ou utiliser une icône de secours.

arachnea-core fournit une persistance transactionnelle (PersistenceStore, FilePersistenceStore) et un contrat distinct pour les credentials. Le store chiffré existe mais est seulement créé dans main.rs, sans être utilisé par StreamScraper. Il n'y a ni API admin, ni authentification, ni catalogue incluant les services désactivés, ni rechargement atomique. Le front actuel est un seul bundle Vue/Vite ; le packaging Tauri n'intègre qu'un frontendDist. Le systray ne sait actuellement qu'ouvrir le navigateur/logs ou arrêter. Aucun fichier de configuration applicatif ne centralise encore le port, le root, le réseau et l'authentification administrateur.

## Architecture recommandée

Créer un projet Vite autonome admin/, avec ses propres locales, routeur, thème et composants. Il peut utiliser Vue/Vuetify mais ne doit importer aucun module de front/src. Le serveur le monte sous /admin avec fallback de navigation SPA, sans intercepter /api/admin/*. En desktop, une fenêtre Tauri admin est créée à la demande ; la fenêtre principale reste inchangée.

La logique métier et les DTO résident dans un module arachnea-stream::admin. Le core ne reçoit que les abstractions génériques réellement nécessaires : adresse TCP cliente dans le contexte, montage de plusieurs bundles et callbacks systray.

Un fichier de configuration applicatif est ajouté côté arachnea-stream. Il est la source persistante des paramètres de démarrage administrables :

- port du serveur ;
- root public (entrypoint root) ;
- type de réseau : local, private ou public ;
- hash du mot de passe administrateur.

Le fichier est lu avant la construction du contrôleur. Les arguments CLI explicites gardent priorité sur ces valeurs, ce qui permet aux déploiements automatisés de les imposer temporairement. Les modifications depuis l'admin écrivent le fichier de façon atomique ; port, root et réseau ne prennent effet qu'au redémarrage.

## Activation persistante

Le flag JSON est conservé comme défaut fourni par l'application. L'état effectif est :

1. override persistant dans arachnea-services ;
2. sinon enabled du manifest ;
3. un YAML manquant ou invalide demeure indisponible malgré un override.

Une entrée est indexée par l'ID YAML, jamais le chemin, et contient format_version et enabled. Les manifests ne sont jamais modifiés par l'admin ; Réinitialiser supprime l'override. Les IDs doivent être uniques.

Dans arachnea-scrapyfy, ajouter un trait générique, injecté optionnellement dans l'agrégateur : ScraperSourceEnabled avec une méthode asynchrone is_enabled(source). Le descripteur de source contient au moins l'ID, les chemins, le défaut et les paramètres fusionnés. Sans handler, le comportement actuel reste inchangé.

PersistenceSourceEnabled::new(store, namespace) vit dans Scrapyfy et retourne le défaut si l'enregistrement est absent ; une erreur de store refuse la source avec une erreur contextualisée. arachnea-stream lui fournit son store et le namespace constant arachnea-services. Au démarrage et à chaque rechargement, il inventorie tous les YAML et crée les entrées manquantes sans effacer les choix existants. Le catalogue admin est indépendant des sources chargées et retourne aussi les services désactivés.

## YAML et credentials

Étendre les YAML de façon rétrocompatible avec un bloc credentials possédant required et signup_url. Sans ce bloc : required vaut false et signup_url est absent. Seuls les services concernés sont modifiés. Les types Scrapyfy et les spécifications FR/EN doivent être mis à jour dans la même évolution.

Les identifiants sont chiffrés, et l'API de lecture ne retourne que leur présence et un login masqué ; jamais le mot de passe. L'ouverture de signup_url est une action navigateur, pas une tentative backend de créer un compte.

## API

Toutes les opérations HTTP sont sous /api/admin/<OPERATION> :

- status : mode, capacités et auth requise ;
- login et logout : session distante ;
- services : catalogue dans la langue demandée ;
- set-service-enabled et reset-service-enabled : override de service ;
- credentials, set-credentials et clear-credentials : état et gestion des credentials ;
- settings et update-settings : port et réseau ;
- set-admin-password : mot de passe permanent ;
- reload : rechargement serveur.

Les erreurs ont un format stable (code, message) et les réponses sensibles sont no-store. Le frontend doit construire l'URL depuis la base publique afin de supporter entrypoint-root.

## Sécurité

Règle proposée conformément à la demande : desktop sans login ; serveur loopback sans login ; serveur distant avec mot de passe. En l'absence de mot de passe permanent, un mot de passe aléatoire est généré à chaque démarrage et écrit une seule fois dans la console.

La condition loopback dépend de l'adresse TCP distante (127.0.0.0/8, ::1), pas du header Host. Le contrôleur REST devra transmettre le peer address. Le mot de passe permanent est stocké sous forme de hash dans le fichier de configuration.

MD5 ne doit pas être utilisé : il est cryptographiquement cassé et très rapide à attaquer. SHA-256 est le minimum acceptable parmi les deux choix proposés, mais un SHA-256 simple reste vulnérable aux attaques par dictionnaire ou GPU. La solution recommandée est Argon2id avec sel aléatoire, dont la représentation encodée contient le sel et les paramètres ; elle est stockée dans le champ password_hash du fichier. Si SHA-256 est imposé, le fichier doit au minimum contenir un sel aléatoire distinct et un hash dérivé du couple sel/mot de passe, mais ce choix réduit fortement la sécurité.

Après connexion, utiliser une session opaque courte via cookie HttpOnly, SameSite=Strict, avec défense CSRF/origin pour les écritures et limitation des essais.

Le choix Local doit être refusé par le backend sauf en mode serveur, client loopback et processus root (UID 0 sous Unix ; équivalent Windows à définir). Le mode Public sans HTTPS doit afficher un avertissement fort ; une option plus sûre est de le refuser sans mot de passe permanent.

## Rechargement et redémarrage

Le rechargement construit et valide un nouveau StreamScraper, puis échange atomiquement l'instance active. Les routes restent enregistrées une fois et consultent une façade stable, par exemple ReloadableStreamScraper, à chaque requête. Séquence : inventaire, synchronisation du store, application des overrides, construction/validation, swap seulement en cas de succès, résultat détaillé.

Un changement de port, root, bind ou ACL ne peut pas s'appliquer sans rebinder le serveur : update-settings met à jour le fichier de configuration et répond restart_required, sans redémarrage automatique. Les arguments CLI gardent priorité sur les réglages persistés. En desktop, l'UI indique de redémarrer l'application.

Le systray reçoit Ouvrir l'administration et Recharger la configuration via callbacks génériques.

## Interface admin et validation

Le bundle admin emploie Vue 3, Composition API et script setup TypeScript. Les vues restent des surfaces de composition ; les composants utilisent props/événements et les appels API sont placés dans des composables. Le thème propose system, light et dark ; system est le défaut. Les traductions emploient le même principe que le front actuel (index, dictionnaires, fallback, navigateur) mais dans ses propres fichiers.

Le packaging recommandé est un deuxième build admin/dist, intégré aux ressources release/Tauri, avec une évolution localisée des web assets pour plusieurs mounts. Compiler l'admin sous front/dist/admin simplifierait le build mais créerait la dépendance exclue par le besoin.

Valider avec les checks existants : Rust ciblé, build/type-check du nouveau bundle, puis /admin, locales, activation/persistance/rechargement, accès loopback/distant, fenêtre desktop et systray. Ne pas créer de nouvelle infrastructure de tests sans demande explicite.

## Décisions validées

1. Persistance vise bien PersistenceStore et arachnea-services est le namespace attendu.
2. Les manifests restent immuables ; l'admin écrit des overrides.
3. L'administration utilise un mot de passe unique, conservé sous forme de hash dans le fichier de configuration. MD5 est exclu ; Argon2id est retenu afin de résister aux attaques par dictionnaire et GPU.
4. Le fichier de configuration est data/config.json, dans le répertoire de données applicatif. Il est écrit atomiquement et ses permissions sont restreintes lorsque la plateforme le permet.
5. Les arguments CLI ont priorité sur les valeurs de data/config.json.
6. Le réseau public est autorisé en HTTP. L'interface doit afficher un avertissement clair ; l'authentification distante reste obligatoire.

## Plan d'implémentation complet

### Phase 1 — Modèle de configuration applicative — implémentée

1. Créer le module de configuration propre à arachnea-stream.
2. Définir le document versionné lu depuis data/config.json :
   - version de format ;
   - port optionnel ;
   - entrypoint root optionnel ;
   - mode réseau ;
   - password_hash optionnel ;
   - paramètres supplémentaires réservés à des évolutions futures.
3. Implémenter la lecture tolérante de l'absence initiale du fichier, la validation stricte des valeurs et l'écriture atomique.
4. Restreindre les permissions du fichier après écriture sur les systèmes qui le supportent.
5. Fusionner configuration et CLI dans main.rs, avec priorité aux arguments explicitement fournis.
6. Générer au démarrage un mot de passe temporaire lorsque password_hash est absent, le conserver en mémoire uniquement et l'afficher une seule fois dans la console.

#### Vérification de l'implémentation — 2026-08-28

| Élément | État | Implémentation constatée |
|---|---|---|
| Module de configuration | Fait | arachnea-stream expose le module configuration. |
| Chemin de stockage | Fait | Le chemin relatif est data/config.json, résolu dans le répertoire de données applicatif. |
| Document versionné | Fait | format_version, server_port, entrypoint_root, network_mode et password_hash sont sérialisés en JSON. |
| Lecture et validation | Fait | L'absence de fichier emploie les valeurs par défaut ; ports nuls, modes réseau inconnus, roots dangereux et versions non prises en charge sont rejetés. |
| Écriture atomique | Fait | Écriture dans .config.json.tmp, synchronisation, remplacement atomique et permissions 0600 sous Unix. |
| Priorité CLI | Fait | Les marqueurs server_port_specified, network_mode_specified et entrypoint_root_specified empêchent le fichier de remplacer une option CLI explicite. |
| Mot de passe temporaire | Fait pour la génération | Une valeur aléatoire de 24 caractères est imprimée en mode serveur lorsqu'aucun password_hash n'existe. Elle n'est pas persistée. |
| Création/modification via administration | Reporté | La méthode save est prête, mais l'API et l'interface qui la déclencheront appartiennent à la phase 4 et à la phase 5. |
| Hash Argon2id | Reporté | Le champ password_hash est réservé et documenté pour Argon2id ; le hash et sa vérification seront ajoutés avec les endpoints de login/changement de mot de passe. |

La phase 1 fournit donc le format, le chargement et la priorité de configuration. Elle ne rend pas encore le mot de passe temporaire utilisable par une authentification distante : cette application effective dépend volontairement du contrôle d'accès de la phase 4.

### Phase 2 — Activation générique et catalogue YAML — implémentée

1. Ajouter dans arachnea-scrapyfy le descripteur de source et le trait ScraperSourceEnabled.
2. Préserver le flag enabled existant comme comportement par défaut lorsqu'aucun handler n'est configuré.
3. Implémenter PersistenceSourceEnabled, prenant un Arc de PersistenceStore et le namespace à utiliser.
4. Ajouter un chargeur de catalogue capable de parcourir tous les manifests imports et YAML, sans filtrer les services désactivés.
5. Valider les IDs uniques et remonter les erreurs de YAML/manifests avec le chemin précis.
6. Résoudre les logos à partir des paramètres connus et retourner une absence d'URL lorsqu'un placeholder reste non résolu.
7. Ajouter au modèle YAML le bloc optionnel credentials (required et signup_url).
8. Mettre à jour les spécifications Scrapyfy en français et anglais.

#### Vérification de l'implémentation — 2026-08-28

| Élément | État | Implémentation constatée |
|---|---|---|
| Point d'extension | Fait | ScraperSourceEnabled reçoit un ScraperSourceDescriptor stable. |
| Persistance | Fait | PersistenceSourceEnabled lit et écrit les overrides dans le namespace fourni. |
| Compatibilité manifests | Fait | Sans handler, le flag enabled existant reste le comportement par défaut. |
| Priorité override | Fait | L'agrégateur lit le YAML, détermine l'ID puis applique l'override ; un override peut donc réactiver une entrée désactivée dans le manifest. |
| Catalogue complet | Fait | load_service_catalog parcourt les imports, inclut les sources désactivées et rejette les IDs dupliqués. |
| Synchronisation | Fait | StreamScraper enregistre les états absents dans arachnea-services avant de charger les collections, sans écraser les choix existants. |
| Schéma credentials | Fait | credentials.required et credentials.signup_url sont optionnels et rétrocompatibles. |
| Documentation YAML | Fait | Les spécifications Scrapyfy françaises et anglaises décrivent le bloc credentials. |

### Phase 3 — Intégration arachnea-stream et rechargement

1. Conserver le PersistenceStore configuré dans StreamScraper et injecter le handler utilisant arachnea-services.
2. Synchroniser les services découverts dans ce namespace sans écraser un override existant.
3. Utiliser le store chiffré réellement configuré pour les credentials de services.
4. Créer une façade ReloadableStreamScraper, stable pour les routes, contenant l'instance active.
5. Construire le nouveau scraper et son catalogue hors du chemin de requête.
6. Échanger atomiquement l'instance seulement après validation complète.
7. Produire un résultat de rechargement détaillant les services chargés, désactivés, ignorés et en erreur.
8. Faire réutiliser ce flux par le démarrage, l'API admin et le systray.

#### Vérification de l'implémentation — 2026-08-29

| Élément | État | Implémentation constatée |
|---|---|---|
| PersistenceStore conservé | Fait | `StreamScraper` conserve `credentials_store` et `persistence_store` ; `StreamScraperBuildOptions` les porte pour chaque reconstruction. |
| Handler arachnea-services | Fait | `from_options` injecte `PersistenceSourceEnabled` sur le namespace `arachnea-services` dans chaque instance reconstruite. |
| Synchronisation sans écrasement | Fait | `register_defaults` est appelé au démarrage et à chaque rechargement ; les overrides existants ne sont jamais réécrits. |
| Store chiffré des credentials | Fait | `main.rs` construit `EncryptedFileCredentialsStore` (`data/credentials`) et le transmet au scraper ; le store JSON clair n'est plus utilisé par l'exécutable. |
| Façade ReloadableStreamScraper | Fait | Nouveau module `arachnea-stream::reloadable_stream_scraper` ; toutes les routes résolvent `current()` à chaque requête. |
| Construction hors chemin de requête | Fait | Le remplacement est construit dans `tokio::task::spawn_blocking` pendant que l'instance active continue de servir. |
| Swap atomique après validation | Fait | L'échange n'a lieu que si la construction réussit et qu'aucune entrée n'est en erreur ; sinon `applied` reste `false` et l'instance active est conservée. |
| Résultat de rechargement détaillé | Fait | `StreamReloadReport` distingue `loaded`, `disabled`, `ignored` (YAML manquant) et `errors` (YAML invalide, identifiant dupliqué, échec de lecture d'état, échec de construction). |
| Réutilisation du flux | Partiel | Le démarrage et la façade partagent le même chemin (`from_options`) ; l'API admin et le systray appelleront `reload()` aux phases 4 et 6. `reload_blocking` est déjà disponible pour les callbacks systray. |
| Stabilité des commandes proxy/DRM | Fait | Le noyau proxy et les chemins publics capturés à l'enregistrement sont réappliqués aux instances reconstruites. |


### Phase 4 — API admin et sécurité

1. Enrichir le contexte REST avec l'adresse TCP distante.
2. Créer les services de mot de passe, sessions et contrôle d'accès.
3. Hasher les nouveaux mots de passe avec Argon2id et vérifier le hash au login.
4. Appliquer les règles desktop, loopback et accès distant avant toute opération admin.
5. Créer les opérations status, login, logout, services, activation, reset, credentials, paramètres, changement de mot de passe et reload sous /api/admin/<OPERATION>.
6. Ne jamais renvoyer password_hash, session brute ou credentials de services.
7. Ajouter les cookies de session sécurisés, Cache-Control: no-store, la vérification origin/CSRF des écritures et une limitation des tentatives.
8. Valider côté backend l'autorisation de choisir Local : serveur, loopback et privilège root.
9. Retourner les capacités et la provenance des paramètres effectifs depuis status/settings.

#### Vérification de l'implémentation — 2026-08-29

| Élément | État | Implémentation constatée |
|---|---|---|
| Adresse TCP distante | Fait | `RequestControlerContext` porte `remote_addr` et `method` ; le backend REST les remplit depuis `warp::addr::remote()` (repli non-loopback quand absent) ; le contexte Tauri les laisse vides. |
| Services mot de passe / sessions / contrôle d'accès | Fait | `admin::auth` : hachage Argon2id, sessions opaques en mémoire (`SessionStore`, TTL 2 h, cookie HttpOnly/SameSite=Strict), limiteur de tentatives par IP. |
| Règles desktop / loopback / distant | Fait | Desktop toujours autorisé (contexte IPC) ; serveur : clients loopback autorisés, autres soumis à session. `status` expose `auth_required` et `authenticated`. |
| Opérations admin | Fait | status, login, logout, services, set-service-enabled, reset-service-enabled, credentials, set-credentials, clear-credentials, settings, update-settings, set-admin-password et reload sous `admin/<operation>` (montés `api/admin/<operation>`). |
| Confidentialité | Fait | `password_hash`, token de session brut et credentials de services ne sont jamais sérialisés ; les logs ne contiennent aucun secret. |
| Cookies et en-têtes | Fait | Session via `Set-Cookie` HttpOnly/SameSite=Strict ; toutes les réponses admin en `Cache-Control: no-store`. |
| CSRF et limitation | Fait | Les écritures exigent POST et refusent les `Origin`/`Referer` étrangers au `Host` ; le login est limité à 10 échecs / 10 min par IP. |
| Validation Local | Fait | `update-settings` refuse le mode `local` hors (serveur, client loopback, processus privilégié via `application::is_running_elevated`). |
| Capacités et provenance | Fait | `status` expose les capacités ; `settings` renvoie port/root/réseau effectifs avec leur source (CLI, configuration ou défaut) et `public_http_warning`. |
| Mot de passe temporaire | Fait | Conservé en mémoire, imprimé une seule fois, comparé en temps constant ; un mot de passe permanent le neutralise. |
| Rechargement atomique | Fait | L'opération `reload` réutilise `ReloadableStreamScraper::reload` et renvoie un rapport détaillé. |
### Particularités à connaître pour l'interface (phase 5)

Voici les points que le frontend admin doit respecter pour rester aligné sur l'API :

1. **URL de l'API** : le frontend construit l'URL depuis le root public (`entrypoint_root`) et ne suppose jamais que le serveur est monté à la racine. En HTTP, les opérations sont servies sous `{root}/api/admin/<opération>` ; en desktop, le même code passe par le handler Tauri `invoke("admin/<opération>", payload)` qui expose les mêmes noms de commandes.
2. **Écran de connexion piloté par `status`** : l'écran d'accueil appelle `status` et s'affiche seulement si `auth_required` est `true`. `authenticated` indique si le client courant passe déjà le contrôle d'accès (desktop ou loopback, ou session valide).
3. **Motorisation de la connexion** : `login` (POST `{password}`) renvoie un cookie de session `arachnea_admin_session` (`HttpOnly; SameSite=Strict`) posé par le navigateur ; le frontend ne doit jamais stocker le token côté application. `logout` invalide la session côté serveur.
4. **Données sensibles** : l'API ne renvoie jamais `password_hash`, de session brute, ni les mots de passe de services. `credentials` ne fournit que `required`, `signup_url`, `configured` et `login_masked`. Ne jamais tenter de recalculer ou d'afficher le secret.
5. **Ouverture de `signup_url`** : c'est une action navigateur contrôlée (fenêtre/onglet externe), jamais une requête API pour créer un compte.
6. **Activation des services** : `set-service-enabled` / `reset-service-enabled` agissent sur l'override et répondent `reload_required: true`. L'interface doit proposer une action « Recharger » et afficher son résultat ; tant que le rechargement n'est pas fait, l'état effectif affiché peut différer de l'état appliqué au serveur.
7. **Logos** : le backend résout les logos depuis les paramètres fusionnés ; un logo absent signifie une URL non résolue. L'interface doit prévoir une icône de secours.
8. **Descriptions localisées** : `services` accepte un paramètre `lang` ; l'interface choisit la langue de l'utilisateur et affiche un fallback (en/fr/première déclarée) côté backend.
9. **Réglages et redémarrage** : `settings` renvoie les valeurs **effectives** avec leur provenance (`server_port_source`, `network_mode_source`, `entrypoint_root_source` valant `command_line`, `configuration` ou `default`) et `public_http_warning` (réseau public en HTTP). `update-settings` persiste le fichier et répond `restart_required: true` — le port, le root et le réseau ne prennent effet qu'au redémarrage. L'interface doit afficher un avertissement clair pour le mode public en HTTP et une indication de redémarrage pour les changements de réglages.
10. **Restriction du mode Local** : le backend refuse le choix `local` sauf si l'utilisateur est un client loopback **et** que le processus tourne avec des privilèges élevés (root/administrateur). L'interface doit masquer ou désactiver ce choix dans les autres cas.
11. **Changement de mot de passe** : `set-admin-password` exige `current_password` dès qu'un hash permanent existe ; un mot de passe de moins de 8 caractères est refusé.
12. **Rechargement** : `reload` renvoie `applied`, `loaded`, `disabled`, `ignored`, `errors` et `build_error`. L'interface affiche ces états et, quand `applied` est `false`, conserve l'état précédent et propose les erreurs détaillées.
13. **Composants et logs** : les appels API vivent dans des composables, les vues restent des surfaces de composition ; les erreurs portent un format stable `{error:{code,message}}` que le composable réseau doit normaliser pour l'UI.
### Phase 5 — Application web admin

1. Créer le projet admin indépendant et ses scripts Vite.
2. Configurer sa base pour la publication sous /admin et le proxy de développement vers le serveur.
3. Mettre en place l'i18n propre : index de langues, dictionnaires, fallback et sélection navigateur.
4. Mettre en place le thème system/light/dark, avec system par défaut.
5. Implémenter la page de connexion conditionnelle, pilotée par status.
6. Implémenter la liste de services, leurs descriptions localisées, les logos de secours et les actions d'activation.
7. Implémenter les dialogues de credentials et l'ouverture contrôlée de signup_url.
8. Implémenter les paramètres port/root/réseau, les restrictions Local et l'avertissement HTTP public.
9. Afficher les états de rechargement et de redémarrage requis.
10. Construire les appels API dans des composables et conserver les vues comme surfaces de composition.

#### Vérification de l'implémentation — 2026-08-29

| Élément | État | Implémentation constatée |
|---|---|---|
| Projet indépendant | Fait | `front/admin-app/` autonome avec ses propres `package.json`, `vite.config.ts`, `tsconfig.json`, sans import de `front/public-app/src`. |
| Publication sous /admin | Fait | `vite.config.ts` : `base: '/admin/'`, `<base href="{base}">` injecté côté serveur, `router` en `createWebHistory(getAppBasePath())`. |
| Proxy de développement | Fait | Proxy Vite `/api` → `http://127.0.0.1:8080` pour le développement local (port 5174). |
| i18n propre | Fait | `admin-app/src/i18n/` : index (`useI18n`, `t()`, `locale`, `availableLocales`), types, dictionnaires `en.json`/`fr.json`, fallback `en`, détection navigateur. |
| Thème system/light/dark | Fait | `admin-app/src/services/theme.ts` : presets, résolution `system` via `matchMedia('(prefers-color-scheme: dark)')`, persistance localStorage, défaut `system`. |
| Page de connexion conditionnelle | Fait | `LoginView.vue` appelle `status` au montage ; affiche le formulaire seulement si `auth_required && !authenticated` ; bouton de déconnexion sinon. |
| Liste de services | Fait | `ServicesView.vue` : catalogue localisé, logos de secours (initiale ou placeholder), toggle d'activation, badge `reload_required`, bouton « Recharger ». |
| Dialogues de credentials | Fait | `CredentialsDialog.vue` : affichage `login_masked`/`configured`, formulaire login/mot de passe, bouton « Créer un compte » ouvrant `signup_url` via `window.open()`. |
| Paramètres port/root/réseau | Fait | `SettingsView.vue` : port (1-65535), réseau (local/private/public), entrypoint root, avertissements public HTTP et local, indication de provenance (CLI/configuration/défaut), `restart_required` affiché. |
| Rechargement et redémarrage | Fait | Bouton « Recharger la configuration » avec résultat détaillé (`applied`, `loaded`, `disabled`, `ignored`, `errors`, `build_error`), conservation de l'état précédent si `applied: false`. |
| Composables et vues | Fait | `useAdminApi.ts` centralise tous les appels API avec gestion d'erreurs (`AdminApiException`, `{error:{code,message}}`) ; les vues sont des surfaces de composition sans logique métier. |
| Build | Fait | `npm run build` dans `front/` produit `dist/` (public) et `dist/admin/` (admin) ; les deux builds réussissent avec `vue-tsc` type-check. |
| Structure unifiée | Fait | `front/package.json` orchestre `build:public` et `build:admin` ; `public-app/vite.config.ts` sort dans `../../dist`, `admin-app/vite.config.ts` sort dans `../../dist/admin`. |

### Phase 6 — Assets, serveur, desktop et systray

1. Étendre le montage des web assets pour servir simultanément le front principal à la racine et admin à /admin.
2. Prévoir un fallback SPA par mount sans collision avec l'API.
3. Intégrer admin/dist dans le build release et les ressources Tauri.
4. Créer une capability Tauri dédiée à la fenêtre admin, avec uniquement les permissions nécessaires.
5. Créer et réutiliser une fenêtre admin dédiée en mode desktop.
6. Ajouter au systray serveur les entrées Ouvrir l'administration et Recharger la configuration.
7. Afficher les erreurs de rechargement de manière exploitable dans les logs et l'interface.

### Phase 7 — Documentation et validation

1. Mettre à jour les tests existants touchés par le chargement, les manifests et les options de démarrage, sans créer de nouvelle infrastructure.
2. Exécuter cargo check et les tests ciblés des crates modifiées.
3. Exécuter type-check et build du front admin.
4. Vérifier manuellement les modes serveur local, privé et public HTTP.
5. Vérifier l'authentification distante, le mot de passe temporaire et le mot de passe permanent.
6. Vérifier persistance, reset, rechargement sans interruption des requêtes en cours et redémarrage requis.
7. Vérifier /admin derrière un entrypoint root ainsi que la fenêtre desktop et le systray.
8. Mettre à jour les spécifications Stream FR/EN, le changelog et les rustdocs affectés.

L'implémentation devra mettre à jour les spécifications Scrapyfy FR/EN, les spécifications Stream FR/EN, les rustdocs concernés et le changelog applicable.
