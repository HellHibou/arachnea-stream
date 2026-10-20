# Analyse — serveur proxy loopback unique pour Scrapyfy

Date : 20 octobre 2026.

Statut : approche B et challenge `407` validés par l’utilisateur le 20 octobre 2026, puis implémentation demandée. Les sections 1 à 11 consignent la conception ; la section 12 décrit la réalisation et ses validations.

## 1. Objectif et périmètre

Remplacer les multiples listeners de compatibilité par un seul serveur proxy HTTP local, partagé par les clients et fallbacks qui utilisent un proxy dans un même runtime applicatif Scrapyfy. Le serveur accepte les requêtes HTTP en forme absolue et les tunnels HTTPS via `CONNECT`, sans interception TLS.

Ajouter une option `--proxy-server-port` dans `SrcapyfyApplicationOptions` : port imposé si non nul, allocation par le système si absent ou égal à zéro, puis journalisation du port réellement lié. Une application locale peut utiliser ce même serveur, notamment un navigateur, avec ses propres paramètres de routage.

« Un seul port » désigne **un seul port d’écoute proxy**. Les connexions clientes et sortantes continuent nécessairement à utiliser des ports sources éphémères. Les listeners API, DNS et les ports de débogage des navigateurs ne font pas partie de cette demande.

Le contrat proposé porte sur un runtime applicatif partagé. Les constructions d’agrégateurs auxiliaires et les snapshots de fournisseurs de ce runtime ne doivent pas créer d’autre listener. Deux processus distincts restent indépendants ; ils ne peuvent pas réserver simultanément le même port fixe.

## 2. État actuel vérifié dans le dépôt

| Emplacement | Comportement observé | Conséquence |
| --- | --- | --- |
| `server/crates/arachnea-proxy/src/connectors/rquest.rs` | `ArachneaRquestLoopback` lie systématiquement `127.0.0.1:0`. Chaque instance possède une tâche d’acceptation et un `JoinSet` de connexions. | Une instance supplémentaire signifie un listener supplémentaire. |
| `server/crates/arachnea-http/src/client.rs`, `PreparedProxyRuntime::new` | Crée un helper général et un autre avec paramètres fixes pour Chaser CF lorsque `HttpProxyConfig::Arachnea` est actif. | Deux listeners peuvent exister avant les tentatives à route épinglée. |
| Même fichier, `execute_one_with_rquest_scoped` | Sélectionne une route, crée un helper à partir de `selected.core`, puis construit un moteur Rquest. Cache borné à 32 entrées hors contexte de recherche ; pendant une recherche, les helpers ne sont pas mis en cache. | Multiplication des listeners selon les clients, destinations, routes et tentatives ; le cache ne constitue pas une mutualisation du serveur. |
| `server/crates/arachnea-scrapyfy/src/scrapyfy/http_client.rs` | Partage la configuration proxy entre clients dérivés et transmet pays, affinité et politiques de réponse au client HTTP. | Le handle partagé est un point d’intégration utile, mais ne partage actuellement pas le listener. |
| `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs` | Initialise un relais système puis un core dynamique ; les snapshots fournisseurs ont leur propre handle système afin d’éviter un cycle core/fournisseur. | La mutualisation doit inclure le bootstrap, sans provoquer de recherche récursive de proxys. |
| `server/crates/arachnea-proxy/src/server/handlers/http.rs` | Extrait les paramètres enregistrés des en-têtes HTTP et CONNECT ; retire les paramètres non transférables ainsi que `Proxy-Authorization` des requêtes HTTP sortantes. | Une partie du protocole est réutilisable. Les paramètres fixes au listener ne conviennent plus au partage concurrent. |
| `server/crates/arachnea-proxy/src/server/auth.rs` | Authentification Basic optionnelle, fondée sur des identifiants statiques. | Aucun décodage actuel des identifiants en paramètres de routage. |
| `server/crates/arachnea-proxy/src/server/server.rs` | `ProxyServer::run` démarre des listeners HTTP, SOCKS et HTTPS selon sa configuration ; pas de handle exposant directement le port éphémère lié et l’arrêt du runtime. | Lancer ce serveur tel quel ne suffit pas ; il faut une API de serveur embarqué avec cycle de vie explicite. |
| `server/crates/arachnea-http/src/engine/chaser_cf.rs` | Lit les identifiants d’une URL proxy et configure l’authentification des pages ; le relais actuel fixe les paramètres faute d’en-têtes CONNECT injectables par ce chemin. | Le canal d’identifiants existe côté adaptateur, mais son fonctionnement complet avec le nouveau protocole reste à qualifier. |
| `server/crates/arachnea-stream/src/main.rs` | Charge les options persistées puis CLI, mais effectue aussi une validation supplémentaire des arguments. | Ajouter uniquement l’option dans Scrapyfy ne garantit pas qu’elle soit utilisable depuis l’exécutable. |
| `server/crates/arachnea-stream/src/stream_scraper.rs` | Construit le core dynamique et active explicitement `with_fallback_direct(true)`. | Préserver cette politique applicative sans l’étendre implicitement aux autres profils. |

La source du nombre de ports se trouve donc dans `arachnea-http` et `arachnea-proxy`, même si l’option et l’orchestration appartiennent à Scrapyfy et à l’application qui l’embarque.

## 3. Contraintes à préserver

- Les pays représentent un ensemble autorisé, normalisé et dédupliqué ; ne pas réintroduire le paramètre singulier `country` ni interpréter leur ordre comme une priorité géographique.
- La sélection, les préférences, les chaînes et les transports restent dans `arachnea-proxy/src/core/`.
- Les classifications de réponse, tokens de sélection, preuves `NotSent`/envoi possible et règles de rejeu restent corrélés à la tentative qui a réellement utilisé le proxy.
- Le contexte `ProxySearchContext` est local à une tâche Tokio. Il ne traverse ni TCP ni un `tokio::spawn` automatiquement. Le listener partagé doit recevoir explicitement le contexte nécessaire.
- Aucun port global avec une variable mutable « pays courants » ou « proxy courant » : deux requêtes simultanées peuvent demander des pays différents.
- Une connexion HTTPS déjà établie conserve sa route. Un changement de paramètres s’applique à une nouvelle connexion ; le serveur ne voit pas les requêtes chiffrées dans le tunnel.
- Le mode direct et les proxys réseau explicitement configurés gardent leurs contrats. Le routage embarqué doit utiliser le serveur partagé partout où un helper de compatibilité est actuellement nécessaire.

## 4. Approches possibles

### A. Mutualiser uniquement le helper existant

Partager un `Arc<ArachneaRquestLoopback>` réduit les ports généraux, mais conserve les listeners à paramètres fixes et à routes épinglées, ou détruit leur isolation si on les supprime sans remplacement. Ne satisfait pas la demande complète.

### B. Serveur partagé avec contextes par connexion — retenue

Créer un runtime de serveur HTTP loopback dans `arachnea-proxy`, injecté dans les clients HTTP par Scrapyfy. Tous les moteurs utilisent la même adresse ; chaque connexion porte un profil de routage ou un identifiant de contexte interne.

Avantages : conserve Rquest et ses caractéristiques TLS, réutilise les handlers actuels, permet l’usage par un navigateur local et évite toute modification du format YAML des fallbacks.

Coût : évolution transversale de trois crates et de l’orchestration Stream ; gestion explicite des contextes et des pools nécessaire.

## 5. Architecture proposée

### Propriété et démarrage

Un objet runtime partagé possède le listener, son adresse effective, la tâche serveur, un registre de contextes et la politique de routage externe. Les clients ne possèdent que des références et des baux sur leurs contextes.

La configuration est définie avant le premier accès réseau. La réservation du socket se fait une seule fois, de façon synchronisée ; l’adresse n’est publiée qu’après un bind réussi. Ne pas rechercher un port libre puis refermer le socket avant de le relier : cela introduirait une course.

Dans l’application, démarrer ce runtime pendant l’initialisation asynchrone, après résolution des options et avant d’accepter les requêtes. Il doit être disponible même sans première requête scraper pour qu’un navigateur local puisse s’y connecter immédiatement. Les constructeurs synchrones de bibliothèques peuvent recevoir le runtime ou un handle initialisable une fois ; ils ne doivent pas lancer chacun un serveur indépendant.

La portée est le runtime applicatif, pas une variable statique globale cachée : les tests existants et les utilisateurs embarquant plusieurs runtimes doivent pouvoir les isoler explicitement.

### Contextes internes et routes épinglées

Conserver la sélection actuelle avant la tentative Rquest. Enregistrer le core sélectionné, la destination attendue, la génération de route et le contexte de recherche dans le runtime ; transmettre un identifiant opaque dans un en-tête de contrôle proxy réservé. Le serveur retrouve ce contexte et l’applique autour de la connexion au core.

L’identifiant ne doit pas encoder une chaîne arbitraire permettant au client de construire des objets internes. Il doit être imprévisible, limité au runtime et vérifié pour la destination autorisée. Un identifiant inconnu, expiré ou incompatible entraîne une erreur explicite, sans repli vers une route générique.

La sélection ne doit pas être refaite par le listener : le token attribué à la réponse doit désigner la route réellement utilisée. La classification HTTP reste côté client, là où TLS est terminé. Pour les clients externes, le serveur ne peut pas classifier les réponses HTTPS chiffrées et ne doit pas annoncer une rotation fondée sur leur statut.

Le cache peut conserver des moteurs Rquest et leurs baux de contexte, **plus aucun listener par entrée**. Clé de pool/session : paramètres canoniques, destination selon le contrat actuel, endpoint sélectionné, génération et portée de recherche. Deux contextes différents ne doivent pas réutiliser le même tunnel par accident.

Le serveur prend une référence au contexte avant de commencer le relais. L’abandon d’une tentative, d’un cache ou d’une session libère son bail ; les relais déjà acceptés gardent leurs références jusqu’à leur fermeture. Borner le registre et supprimer les entrées inutilisées pour éviter une croissance permanente.

### Bootstrap et rechargement

Les fournisseurs de proxys et de géolocalisation doivent employer le même listener avec un contexte système distinct, sans repasser par la recherche dynamique qu’ils alimentent. Ne pas faire détenir au snapshot fournisseur un `Arc` fort vers un runtime qui détient déjà le core et ce fournisseur : prévoir un accès faible ou une séparation des responsabilités pour casser ce cycle.

Lors d’un rechargement des sources, conserver le listener et son port. Les nouvelles connexions utilisent la nouvelle génération de routage ; les tunnels et tentatives en cours conservent leur contexte. Le changement de port est proposé comme option de démarrage nécessitant un redémarrage, hors réglages administratifs à chaud.

## 6. Paramètres utilisables par une application locale

### Clients capables d’envoyer des en-têtes proxy

Conserver les noms publics existants, notamment :

```http
CONNECT example.com:443 HTTP/1.1
Host: example.com:443
Arachnea-Proxy-Countries: ["BE","FR"]
Arachnea-Proxy-Affinity: browser-session-1
```

Les en-têtes appartiennent à la négociation proxy, pas à la requête HTTPS envoyée au site. Ajouter ces champs dans la page ou dans un `fetch()` ordinaire ne suffit pas pour piloter un CONNECT déjà établi.

### Navigateurs et clients à identifiants proxy

Proposer un second transport des paramètres via `Proxy-Authorization: Basic`, car l’adaptateur Chaser dispose déjà d’un chemin pour les identifiants. Il s’agit d’un **nouveau protocole proposé**, absent actuellement, dont l’interopérabilité devra être validée sur les moteurs activés.

Syntaxe humaine proposée pour le nom d’utilisateur : une chaîne de paramètres URL encodés, par exemple `countries=BE,FR&proxy_affinity=browser-session-1`. Mot de passe conventionnel : `local`. Le profil par défaut utilise `default` / `local`. Après décodage, utiliser les mêmes définitions, normalisations et validations que le canal d’en-têtes ; les pays deviennent le tableau JSON canonique attendu par le core.

Le mot de passe conventionnel n’est pas une protection d’accès. Le mode de routage par identifiants doit être explicite dans la configuration du serveur embarqué et ne doit pas modifier l’authentification statique du serveur autonome. Les profils inconnus, paramètres invalides ou doublons contradictoires sont refusés. Ne pas accepter de chemin ou de query string d’URL proxy comme canal implicite : les clients ne garantissent pas leur transmission au serveur.

**Négociation validée :** un navigateur peut attendre une réponse `407` avant d’envoyer ses identifiants. Le serveur demande donc les identifiants de profil sur les connexions sans contexte interne et sans paramètres explicites, puis accepte `default` / `local` pour le profil par défaut. Cela impose ces identifiants aux clients externes ordinaires utilisant ce chemin et évite d’ouvrir une route avant de connaître les pays souhaités. Aucun mode alternatif sans challenge n’est prévu dans ce chantier.

Ne pas promettre qu’une simple URL contenant un utilisateur et un mot de passe suffit dans tous les navigateurs. Documenter séparément la configuration de l’adresse proxy et la saisie des identifiants dans un navigateur compatible. Pour Chaser, vérifier le challenge et l’authentification avant la première navigation et les sous-ressources ; Ghostwire et Tauri doivent être qualifiés selon leurs APIs réellement utilisées. Aucun moteur ne doit recréer un second listener pour contourner une incompatibilité.

### Règles de résolution

1. Un contexte interne valide détermine la route épinglée ou le relais système. Refuser les paramètres externes contradictoires ; ne pas permettre leur utilisation pour modifier cette route.
2. Sinon, décoder le profil externe, puis appliquer les en-têtes reconnus. Refuser les conflits de valeur entre canaux pour éviter un comportement ambigu.
3. Appliquer les valeurs par défaut du core lorsque le client a explicitement choisi le profil par défaut.

Les contextes internes ne sont pas accessibles par les identifiants publics. Les paramètres de contrôle et identifiants ne sont jamais transmis au site distant ; leur valeur n’est pas journalisée.

## 7. Option et restriction loopback

| Cas | Comportement proposé |
| --- | --- |
| Option absente | Bind de `127.0.0.1:0`, puis lecture et log du port effectif. |
| `--proxy-server-port 0` | Même allocation automatique. |
| `--proxy-server-port 18080` | Bind de `127.0.0.1:18080` ; adresse utilisable par les applications locales. |
| Port fixe occupé ou refusé | Échec contextualisé du démarrage ; aucun autre port choisi silencieusement. |
| Valeur invalide ou hors `u16` | Erreur de configuration avant démarrage. |
| Modification du port en cours d’exécution | Redémarrage requis. |

Proposer la clé persistée `proxy-server-port`, conforme au mécanisme d’export existant, avec priorité CLI sur fichier. Ne pas persister le port aléatoire comme configuration voulue : il peut changer au redémarrage. Le log de niveau `info` contient uniquement l’adresse effective et le caractère automatique ou explicite de l’allocation.

Aucune option d’adresse d’écoute n’est nécessaire pour cette demande. `127.0.0.1` satisfait la restriction loopback et évite de créer un second listener IPv6. Les clients utilisent cette adresse littérale ; ne pas annoncer `localhost` comme équivalent garanti si le système le résout d’abord en `::1`. Si un support IPv6 est demandé plus tard, définir le choix d’une seule famille avant d’ajouter un bind supplémentaire.

Le serveur embarqué vérifie aussi les pairs loopback et applique les limites de taille des en-têtes et de connexions. Les possibilités de bind public du serveur autonome ne doivent pas rendre configurable un bind public dans ce chemin Scrapyfy. En conteneur, « local » désigne l’espace réseau du conteneur ; aucun mapping de port ne doit être présenté comme une exposition automatiquement utilisable depuis l’hôte.

Sans feature `arachnea-proxy`, ne pas démarrer de serveur ; une demande explicite de port doit produire une erreur claire plutôt qu’une option acceptée sans effet.

## 8. Fichiers et contrats touchés lors de l’implémentation

- `arachnea-proxy` : runtime embarqué dans les modules serveur existants, adaptation du connecteur Rquest, extraction des profils et contextes dans les handlers, réutilisation du core pour toute décision de route.
- `arachnea-http` : injection du runtime, suppression des créations multiples de helpers dans `PreparedProxyRuntime` et les tentatives épinglées, adaptation des moteurs et de l’identité des sessions/pools.
- `arachnea-scrapyfy` : option, partage dans les handles, agrégateurs et snapshots fournisseurs, gestion du cycle de vie.
- `arachnea-stream` : passage du runtime/configuration et démarrage asynchrone, validation CLI existante, maintien du même listener pendant les reloads. Ces modifications d’orchestration ne déplacent pas le routage dans la façade Stream.
- Documentation : README Proxy, HTTP, Scrapyfy et commandes Stream concernées ; spécifications Scrapyfy FR/EN si elles décrivent les options ou les contrats modifiés. Aucun nouveau champ YAML de requête n’est requis a priori.
- Suivi : TODO réorienté vers ce chantier, CHANGELOG décrivant ensuite le comportement effectivement livré.

Ne pas renommer les types historiques, réorganiser les fichiers ou introduire de dépendance vers Stream dans les crates partagés pour ce chantier.

## 9. Séquence d’implémentation

1. Définir le contrat du runtime, du registre et du canal de profils ; intégrer un serveur HTTP avec bind configurable, adresse effective et arrêt explicite.
2. Injecter ce runtime dans les clients généraux et les snapshots système ; préserver l’absence de cycle de propriété et de bootstrap récursif.
3. Remplacer les listeners des routes épinglées par des contextes enregistrés ; préserver preuves d’envoi, limites temporelles, classification et règles de rejeu.
4. Migrer les fallbacks navigateur et leurs sessions au même port ; qualifier le canal d’identifiants et refuser explicitement les cas non pris en charge.
5. Exposer l’option jusqu’à l’exécutable, vérifier les reloads et mettre à jour les documentations contractuelles.

Ne déclarer le chantier terminé que lorsque tous les chemins de création de helpers utilisés par ce runtime sont migrés, y compris les fournisseurs et les fallbacks rarement activés.

## 10. Validation prévue

Cette étape est une analyse de sources ; aucun test d’exécution n’a été effectué. Conformément aux instructions du dépôt, ne pas créer de nouveaux tests sans demande explicite ; adapter les fixtures et tests existants affectés.

- Réutiliser les tests des handlers HTTP, d’intégration du proxy et `arachnea-http/src/engine/rquest_proxy_tests.rs` pour les cas déjà couverts : paramètres, CONNECT, route épinglée, classification, rejet et preuve d’envoi.
- Exécuter les suites existantes des trois crates et les checks de compilation des combinaisons de features pertinentes, ainsi que l’intégration Stream ; contrôler le formatage des fichiers modifiés.
- Recette manuelle : compter les sockets `LISTEN` avant/après des requêtes parallèles dans des pays différents, changements de routes et fallbacks. Il doit rester exactement un listener proxy pour le runtime.
- Vérifier port automatique, zéro, fixe, occupé, invalides, configuration persistée et priorité CLI ; vérifier le log de l’adresse effective.
- Utiliser une application locale indépendante puis un navigateur avec un profil pays ; observer le challenge éventuel, CONNECT, les sous-ressources, la route retenue et l’absence d’en-têtes de contrôle au serveur de destination.
- Vérifier les connexions sans profil, les profils invalides et contradictions de paramètres, l’isolation des tunnels, le bootstrap à froid, l’annulation d’une recherche, le rechargement des sources et la fermeture du runtime.
- Pour l’arrêt, vérifier que l’abandon du propriétaire arrête l’acceptation et termine les connexions selon une politique explicite. Un reload doit au contraire laisser les connexions existantes finir et conserver le port.

## 11. Décisions validées et détails retenus

L’utilisateur a validé l’approche B et le challenge `407` le 20 octobre 2026 : un listener HTTP `127.0.0.1`, possédé par le runtime applicatif, `--proxy-server-port`/`proxy-server-port`, démarrage avant usage externe, contextes internes opaques et paramètres externes transmis par en-têtes ou identifiants de profil.

Les détails proposés sont conservés comme choix d’implémentation : utilisateur `countries=BE,FR&proxy_affinity=browser-session-1`, mot de passe conventionnel `local`, profil `default` / `local` sans contrainte de pays, erreur au démarrage si le port imposé est indisponible et redémarrage requis pour changer ce port. Aucune décision fonctionnelle supplémentaire n’est bloquante à ce stade.

La qualification des moteurs navigateur et du bootstrap reste un travail technique d’implémentation et de validation, pas une décision utilisateur en suspens. Toute incompatibilité démontrée qui exigerait de changer ce contrat devra être signalée avant de modifier l’approche retenue.

## 12. Réalisation du 20 octobre 2026

### Runtime et intégration

- `arachnea-proxy/src/server/loopback.rs` possède le socket HTTP unique, lié une seule fois via `OnceCell`, avec port fixe ou automatique, adresse effective et log `info`. Le listener et ses pairs sont limités à IPv4 loopback ; limites : 1 024 connexions, en-têtes de 16 KiB lus en 30 secondes, 8 192 contextes vivants.
- Le registre ne retient que des références faibles. Les handles de configuration, helpers et connexions acceptées possèdent les baux ; leur libération permet le nettoyage sans cycle via les fournisseurs. La suppression d’une route par défaut vérifie son identité pour préserver une génération plus récente.
- Rquest, les fallbacks intégrés et les routes épinglées utilisent `start_on_runtime(...)`. Une tentative conserve son core sélectionné, son token et son contexte de recherche ; elle n’ouvre aucun nouveau listener. Les cores épinglés continuent à vérifier la destination.
- Le canal interne utilise aussi Basic, avec utilisateur réservé `arachnea-context` et secret aléatoire opaque, plutôt qu’un en-tête de contrôle supplémentaire. Cela permet au navigateur et aux clients HTTP de sélectionner un bail avec le même mécanisme. Ce canal n’accepte pas de profil public permettant de fabriquer une route interne. Les secrets sont exclus des logs applicatifs et des exemples.
- Les profils externes utilisent la syntaxe validée et sont négociés via `407`. Les paramètres sont validés et fusionnés sans écrasement contradictoire ; un contexte interne expiré renvoie `403`, un profil invalide ou contradictoire `400`. Le serveur n’interprète pas les réponses HTTPS chiffrées.
- Scrapyfy injecte le même runtime dans les handles indépendants de ses clients et snapshots système. Stream le crée selon les options effectives, publie sa route et lie le socket au démarrage ; les rebuilds reçoivent le même runtime et publient le nouveau core après construction réussie.
- Les pages conservées dans le gestionnaire de sessions retiennent aussi leur runtime et leur bail, même si le client qui les a ouvertes est remplacé. La fermeture du dernier propriétaire du runtime termine le listener et ses tâches de connexion. `shutdown()` permet aussi un arrêt terminal explicite malgré les références encore détenues par des clients ou des fournisseurs.

### Limites explicites

Le serveur autonome conserve ses listeners et son authentification statique. Le constructeur de commodité `ArachneaRquestLoopback::start(...)` conserve un runtime indépendant pour les utilisateurs hors application ; le partage nécessite l’injection explicite du même runtime.

La première livraison excluait Tauri/Wry des requêtes proxy et privilégiait Chaser-CF pour celles-ci. Cette limitation est levée par le complément décrit en section 13. Les moteurs fournis par l’appelant restent responsables de leur transport.

Les identifiants conventionnels `default` / `local` ne protègent pas contre un autre processus local. Le port est local à l’espace réseau du processus. Une connexion existante conserve ses paramètres ; les modifications de profil nécessitent une nouvelle connexion.

### Vérifications

Les tests existants ont été adaptés, sans nouveau nom de test ni infrastructure ajoutée au dépôt : réservation concurrente d’un seul socket, port imposé occupé, challenge et profils externes, refus des contextes expirés et paramètres contradictoires, retrait des en-têtes de contrôle, partage du runtime par les fournisseurs et les clients, et conservation des preuves de sélection Rquest.

Résultats :

- Suites réunies Proxy, HTTP et Scrapyfy : **252 tests existants réussis** (125 Proxy, 47 HTTP, 80 Scrapyfy), avec Chaser-CF et la persistance SQLite activés. Commande : `cargo test --offline -p arachnea-proxy -p arachnea-http -p arachnea-scrapyfy --lib --features arachnea-http/chaser-cf,arachnea-scrapyfy/arachnea-proxy,arachnea-scrapyfy/sqlite-persistence` depuis `server/`.
- `cargo check --offline -p arachnea-stream` : compilation de l’intégration applicative réussie.
- `cargo check --offline -p arachnea-scrapyfy --no-default-features` : réussi ; warnings existants de code inutilisé dans les chemins sans persistance/proxy.
- `cargo check --offline -p arachnea-http --features arachnea-proxy,chaser-cf,tauri-cloudflare-solver,ghostwire` : réussi, y compris les branches de sélection des fallbacks navigateur.
- Formatage des fichiers Rust concernés contrôlé de façon ciblée et `git diff --check` réussi. Les fichiers de tests Rquest et le module de sélection des moteurs conservent leur formatage préexistant hors des passages modifiés.
- Recette temporaire hors dépôt : lecture d’un port persisté, priorité CLI sur cette valeur, export du zéro et rejet de ports négatifs, hors plage ou non numériques.
- Client externe utilisant `countries=BE` : routage effectif vers un proxy de fixture local ; absence des identifiants et des paramètres de contrôle au destinataire.
- Chrome réel via Chaser-CF : navigation HTTP puis HTTPS via CONNECT avec le profil interne sur le même port. Le test a aussi retiré la route publique par défaut avant les navigations : le challenge reste disponible et permet au navigateur de présenter son contexte interne. Une fixture TLS auto-signée a nécessité une exception de certificat limitée au processus Chrome de cette recette ; aucune vérification TLS de production n’a été désactivée.

Les ouvertures de sockets des fixtures ont nécessité une exécution hors des restrictions réseau du bac à sable. Aucun nouveau fichier de test ni infrastructure n’est conservé dans le dépôt. La recette couvre Chrome/Chaser ; elle ne constitue pas une qualification de chaque navigateur externe ni un benchmark de charge.


## 13. Complément Tauri/Wry approuvé le 20 octobre 2026

Après validation de l’utilisation du loopback, le solveur Tauri/Wry reçoit l’URL effective du contexte préparé par la façade HTTP. La priorité normale de sélection automatique est rétablie pour les requêtes proxy. Les fenêtres proxy utilisent un stockage isolé et non persistant ; elles restent sur `about:blank` jusqu’à l’installation des paramètres natifs et de l’authentification. La navigation intervient après le retour du callback natif afin d’éviter une réentrée dans le verrou interne de Tauri.

Sur macOS 14+, un `WKWebsiteDataStore` configuré avant la création du WebView utilise Network.framework avec les identifiants du bail et sans failover direct. La recette macOS 14.8 a montré que le chemin HTTP CONNECT natif ne couvrait pas les destinations HTTP ordinaires. Le socket partagé accepte donc aussi SOCKS5, sans autre listener : authentification obligatoire par un bail interne vivant, CONNECT seulement, négociation limitée à 30 secondes. Les profils publics continuent d’utiliser HTTP et le challenge `407`. Les URL HTTP internes sont adaptées en SOCKS5 uniquement pour le WebView macOS ; un proxy réseau HTTP explicite y est refusé au profit de SOCKS5. Les symboles modernes sont chargés à la demande pour conserver le démarrage en mode direct sur les versions antérieures.

Sous Windows, WebView2 reçoit un proxy HTTP et un gestionnaire d’authentification limité à l’autorité du proxy ; SOCKS5 authentifié est refusé explicitement. Sous Linux, WebKitGTK vérifie que la demande d’authentification provient du proxy attendu. Les identifiants ne sont pas ajoutés aux requêtes d’origine. Les erreurs de configuration sont remontées avant navigation.

Les tests existants du serveur SOCKS ont été étendus pour couvrir le même socket partagé, le rejet des accès anonymes et des baux inconnus, le refus UDP et le relais TCP authentifié. Aucun nouveau test ou infrastructure de test n’est ajouté au dépôt.

Validation native et limites : une recette temporaire hors dépôt a vérifié Wry sur macOS pour HTTP puis HTTPS avec validation TLS normale, routage pays BE, corps de page et récupération du cookie vers Rust. La fenêtre applicative Tauri a aussi été exécutée avec le contexte pays BE : chargement via le proxy, contenu et récupération du cookie validés. Les 252 tests existants HTTP, Proxy et Scrapyfy passent avec les deux solveurs activés (47 HTTP, 125 Proxy, 80 Scrapyfy). La branche d’authentification WebView2 a été compilée isolément pour Windows ; les signatures GTK ont été vérifiées dans les sources de la dépendance. Ces vérifications ne remplacent pas une recette native Windows/Linux.
