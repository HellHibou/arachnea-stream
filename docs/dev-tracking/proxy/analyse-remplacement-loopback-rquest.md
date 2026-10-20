# Analyse du routage interne via arachnea-proxy sans loopback

Date : 14 octobre 2026.

Statut : inspection des sources uniquement. Aucun changement du transport de production, aucune migration de dépendance, aucun nouveau test ni benchmark réseau.

Révision : l’objectif porte sur l’ensemble du trafic concerné lorsque le proxy est activé, et non sur le seul moteur Rquest. Le mode sans proxy reste un accès direct explicite et normal.

## Objectif et contrat des modes

Le choix existant entre utilisation du proxy et accès direct doit être conservé :

| Mode choisi | Parcours attendu | Contraintes |
| --- | --- | --- |
| Proxy activé | Appel interne à `arachnea-proxy`, puis transport sélectionné par son core | Aucun listener ni relais loopback de compatibilité ; aucun moteur ne contourne le core pour joindre lui-même le proxy distant |
| Proxy désactivé | Accès direct par le moteur HTTP ou navigateur | Aucun adaptateur proxy initialisé, aucune sélection de proxy ni création de listener de compatibilité |

« Tout le trafic » désigne ici tous les échanges concernés par une configuration en mode proxy : requêtes principales, redirections, sous-requêtes, flux médias, renouvellements de session et trafic des solveurs/navigateurs associés. Cela ne signifie ni imposer le proxy aux appels explicitement configurés en direct, ni intercepter tout le trafic du système d’exploitation.

La suppression du loopback n’est pas une optimisation facultative dans le mode proxy cible : c’est une exigence d’architecture. Conserver le loopback pour un moteur ou lui donner directement l’URL du proxy distant ne satisfait pas cette exigence. Un moteur non intégrable doit être identifié comme incompatible avec ce mode jusqu’à adaptation, plutôt que basculer silencieusement en direct ou ouvrir un proxy local.

La configuration actuelle de `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/src/config.rs` distingue `HttpProxyConfig::Disabled`, `Network(String)` et `Arachnea(ArachneaProxyCore)`. Le contrat utilisateur comporte deux modes fonctionnels, mais l’API possède trois variantes techniques : `Disabled` reste direct ; `Arachnea` doit devenir interne sans loopback ; `Network` devra être traduit en configuration d’un nœud/transport du core si cette variante participe au mode proxy cible. Cette traduction doit préserver les options et l’authentification supportées ; ne pas la réaliser implicitement pendant l’analyse.

Passer par `arachnea-proxy` ne signifie pas obligatoirement utiliser un proxy distant pour toutes les destinations : le core peut posséder des routes directes explicitement prévues par sa politique. Il reste alors propriétaire de la décision et de l’ouverture du transport. Les replis actuels du core, notamment la poursuite d’une chaîne sans proxy géographique disponible, doivent être audités et documentés séparément ; la clarification utilisateur n’autorise ni leur suppression générale ni leur extension. Un échec technique d’adaptation ne doit pas être interprété comme le choix « proxy désactivé ».

## Conclusion

L’architecture cible est un transport interne fourni par `arachnea-proxy` pour tous les moteurs utilisés en mode proxy. La dépendance Rquest installée n’expose pas le point d’injection de stream nécessaire : un handler de requête ou `connector_layer` seul ne suffit pas. Pour conserver son comportement HTTP/TLS, l’approche recommandée est une petite extension du projet amont, ou un patch maintenu de la dépendance, permettant d’injecter un flux d’octets dont le routage est déjà établi **avant le TLS de destination**. Exposer uniquement un constructeur de connexion déjà établie ne suffit pas.

Un moteur HTTP interne indépendant constitue une autre option, mais il modifie le comportement HTTP/TLS et doit lui aussi utiliser le core en mode proxy. Son utilisation doit être explicite, et non constituer un repli silencieux. Chromium et les WebViews constituent un blocage distinct avec les APIs actuellement utilisées : un proxy local n’est plus une solution finale acceptable. Une intégration réseau complète sans listener doit être démontrée, ou le moteur doit être remplacé/adapté ou déclaré non supporté dans ce mode. Le mode direct reste disponible indépendamment.

## 1. État local vérifié

- `/Users/jdecker/Downloads/arachnea/server/Cargo.toml` utilise le package `newwreq` version `5.1.7` sous l’alias `rquest` ; `/Users/jdecker/Downloads/arachnea/server/Cargo.lock` verrouille cette version.
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/connectors/rquest.rs` écoute sur `127.0.0.1:0`, accepte les connexions au proxy HTTP et les délègue au core. Il lance une tâche d’acceptation et des tâches distinctes pour les connexions. La destruction de l’adaptateur annule la tâche d’acceptation, mais pas explicitement toutes les tâches de relais déjà acceptées.
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/src/client.rs` crée des adaptateurs de compatibilité généraux et pour les navigateurs, ainsi qu’un cache borné d’adaptateurs Rquest à route épinglée. Supprimer uniquement ce cache ne supprime pas tous les listeners.
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/connectors/tower.rs` implémente déjà `Service<http::Uri>` et retourne un stream interne. Il appelle directement le core proxy, sans listener.
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/connectors/hyper.rs` est seulement une enveloppe autour de ce service Tower, et non une implémentation complète de transport Hyper.
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/http/client.rs` effectue déjà des échanges HTTP/1.1 internes via le core, avec TLS de destination facultatif et corps streamés. Il envoie explicitement `Connection: close` ; il ne remplace pas les pools de connexions, HTTP/2, la compression et le comportement TLS de Rquest.

## 2. Pourquoi le middleware de connecteur ne suffit pas

Sources installées examinées dans `/Users/jdecker/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/newwreq-5.1.7/` :

- `src/client/client/mod.rs:1331` : `ClientBuilder::connector_layer` accepte une couche Tower autour du connecteur existant. Son service doit conserver les types de requête `Unnameable` et de réponse `Conn`.
- `src/lib.rs` : les modules `connect` et `core` sont privés.
- `src/connect.rs:644` : l’interface de connexion se trouve dans un module `sealed` privé au crate ; le contenu du descripteur de requête et les champs de connexion ne peuvent pas être construits depuis un crate externe.
- `src/tls/mod.rs` : le connecteur TLS est exporté uniquement avec la visibilité `pub(crate)`.
- `src/tls/conn/boring.rs` : le TLS de destination peut envelopper des entrées/sorties génériques en interne, mais cette fonctionnalité n’est pas un point d’extension public du transport client.

Une couche générique peut déléguer au service existant, mesurer, limiter ou annuler son opération. Cela ne lui permet pas d’inspecter le descripteur privé et de construire une nouvelle connexion à partir de `ProxyStream`. Il ne faut pas contourner cette interface par des manipulations non sûres de la représentation mémoire, du parsing de chaînes de debug ou de l’interception TCP/DNS.

Les sources de `wreq 6.0.0-rc.29` présentes dans le cache local comportent également un module de connexion privé et le même type de point d’extension sous forme de middleware. Leur présence dans le cache Cargo ne signifie pas que cette version est une dépendance d’Arachnea. La lecture de la branche `main` du projet amont via les sources brutes officielles sur GitHub n’a pas non plus établi l’existence d’un point d’injection public de stream utilisable ; cela ne prouve pas que toutes les versions publiées ou futures en sont dépourvues. Une simple mise à jour de dépendance n’est pas une solution validée.

## 3. Options et compromis

| Option | Listener supprimé | Conservation du comportement actuel | Coût principal |
| --- | --- | --- | --- |
| Handler de requête ou middleware de connecteur uniquement | Non | Le transport existant reste inchangé | Ne satisfait pas l’objectif ; ne permet pas d’injecter le stream |
| Point d’injection du transport avant TLS dans le projet amont / patch maintenu de la dépendance | Oui pour les requêtes du moteur Rust Rquest intégré | Meilleure possibilité de conserver le TLS, ALPN, HTTP/2 et les pools de Rquest | Maintenance de la dépendance et revue par le projet amont |
| Moteur HTTP interne explicite utilisant le client simple existant | Oui pour ce moteur | HTTP/1.1 uniquement dans l’implémentation actuelle ; pas de pool équivalent | Adaptation des en-têtes, cookies, compression, règles de corps, erreurs et configuration |
| Moteur interne complet basé sur Hyper | Oui pour ce moteur | HTTP/1.1 et HTTP/2 possibles avec des adaptateurs concrets ; comportement TLS non automatiquement équivalent à Rquest | Travail structurel sur le moteur et validation d’une nouvelle dépendance directe |
| Configuration du proxy distant sélectionné directement dans Rquest | Oui pour les cas simples à un seul saut pris en charge | TLS de destination de Rquest conservé | Hors cible : le trafic ne traverse plus le transport du core |

Des crates liés à Hyper apparaissent comme dépendances transitives dans `Cargo.lock`, mais ils ne sont pas des dépendances directes confirmées du workspace pour un moteur HTTP interne. Leur ajout nécessite une validation explicite des dépendances et du périmètre. Ne pas implémenter cette alternative au seul motif qu’ils apparaissent dans le fichier de verrouillage.

La configuration directe du proxy distant dans un moteur est exclue comme solution à cet objectif : elle supprime un listener mais contourne `arachnea-proxy`. Le mode direct explicitement choisi reste autorisé ; un échec d’intégration ou de connexion en mode proxy ne doit pas le sélectionner silencieusement.

## 4. Interface de transport recommandée

Contrat proposé, et non API Rust implémentée :

```text
choix de configuration
  ├─ proxy désactivé -> moteur -> connexion directe
  └─ proxy activé -> transport interne arachnea-proxy :
préparation de la requête -> SelectedProxy { core épinglé, jeton, identité de route }
fabrique de connexion Rquest -> core épinglé.connect_request(destination)
                            -> flux d’octets routé + métadonnées
TLS de destination Rquest -> validation des certificats + SNI + ALPN
moteur HTTP Rquest -> requête / réponse / pool de connexions
observateur de réponse -> jeton + observations par phase + preuves d’envoi
```

Exigences :

- Injecter le stream à une frontière de routage connaissant l’URI, avant tout repli DNS/TCP vers l’origine et avant le TLS de destination, et non simplement dans une fabrique TCP ne connaissant qu’une adresse.
- Pour HTTPS, le core établit le TLS du proxy et le tunnel ; Rquest enveloppe le stream obtenu dans le TLS de destination exactement une fois. Conserver sa politique de certificats, SNI et le protocole HTTP négocié.
- Conserver le hostname jusqu’à sa résolution par le transport approprié du core ; ne pas effectuer de résolution DNS directe ni de connexion directe spéculative.
- Pour HTTP, choisir explicitement entre une requête en forme origine à travers un tunnel et le forwarding en forme absolue. Certains proxies HTTP ne prennent pas en charge CONNECT vers le port 80. Conserver la sémantique de `connect_http_request` du core lorsque nécessaire et séparer les credentials du proxy des en-têtes envoyés à l’origine.
- Adapter les entrées/sorties Tokio et les métadonnées aux traits internes de connexion de la dépendance. `ProxyStream` encapsule actuellement des entrées/sorties `Send` ; la connexion Rquest examinée exige `Send + Sync`. Résoudre cet écart de manière sûre, sans promettre que l’adaptateur existant convient sans modification.
- Conserver l’isolation des clients réutilisables et des pools par inventaire, origine exacte et identité de route/endpoint. Une réponse tardive doit conserver son propre endpoint ; le multiplexage HTTP/2 ne doit pas déduire une sélection à partir de la dernière connexion.
- Conserver des erreurs typées et contextualisées. Une erreur du connecteur avant toute écriture HTTP peut établir `NotSent` ; une erreur d’écriture ou de lecture du stream peut encore impliquer un envoi incertain.
- La suppression des relais locaux améliore la maîtrise du cycle de vie des tâches, mais ne supprime pas toutes les tâches pilotant les connexions HTTP. L’annulation et l’arrêt doivent rester explicites.
- Fournir un contrat de transport injectable aux moteurs et solveurs utilisés en mode proxy, plutôt qu’un handler recevant uniquement le corps ou le statut d’une réponse. Ce contrat transporte la destination, le contexte de routage et les métadonnées de connexion.
- Propager le mode choisi aux sous-requêtes, redirections et renouvellements de session. Séparer les pools et sessions entre accès direct et accès via le core, et ne pas réutiliser un client direct pour une demande proxifiée.
- Ne créer aucun listener de compatibilité à l’initialisation, même lorsqu’aucune requête n’a encore été émise. Les tâches HTTP nécessaires ne doivent pas réintroduire un relais réseau local.

L’adaptateur Tower existant est une brique utile, pas un connecteur directement interchangeable pour cette version de Rquest. Un nouveau point d’extension public générique dans la dépendance doit rester indépendant d’Arachnea ; seul l’adaptateur Arachnea appelle le core.

## 5. Frontière avec les navigateurs et les solveurs

Ghostwire accepte actuellement des URLs de proxy via son builder ; une extension interne de Rquest devrait également être exposée à travers cette dépendance pour supprimer son adaptateur local.

Chaser-CF délègue les opérations de navigateur à Chromium et reçoit une URL de proxy. Un `ProxyStream` Rust ne peut pas simplement être passé comme handler réseau du navigateur à travers les APIs actuellement utilisées. Le listener de compatibilité actuel est donc un obstacle à supprimer, pas une exception à conserver dans l’architecture cible.

L’adaptateur Tauri/Wry ouvre également des pages via la pile réseau de la WebView ; aucun connecteur interne vers le core n’a été identifié dans le fichier examiné. Il ne faut pas assimiler le traitement des navigations à l’interception complète du réseau du navigateur.

Une interception des requêtes de page constitue seulement une piste à étudier : elle doit couvrir navigations, scripts, images, iframes, requêtes JavaScript, redirections, WebSocket et éventuels transports UDP, ainsi que les cookies, les en-têtes et les erreurs. Elle doit démontrer l’absence de connexion autonome restante. Remplacer les échanges du navigateur par des réponses obtenues en Rust peut changer TLS, les sessions et le comportement des challenges ; ce n’est pas une solution équivalente déjà validée.

Trois voies restent à concevoir pour ces moteurs : injection réseau native/IPC sans port réseau local, interception exhaustive prouvée, ou remplacement par un moteur compatible. Si aucune ne peut être fournie, signaler explicitement l’incompatibilité du moteur avec le mode proxy sans loopback ; ne pas exécuter automatiquement le solveur en direct. Ces pistes ne sont pas déclarées faisables à partir de la seule lecture des wrappers actuels.

Changer la pile TLS/HTTP vers l’origine peut affecter les services sensibles à l’empreinte du client et le transfert des cookies du navigateur vers le client HTTP. Privilégier la conservation de la même pile lorsque la préservation du comportement est prioritaire ; aucun taux de succès ou d’échec n’a été mesuré pour les alternatives.

## 6. Séquence d’implémentation proposée, en attente de validation

- [x] Examiner l’identité de la dépendance verrouillée, la visibilité du connecteur, les adaptateurs Tower/HTTP simple existants et les usages par les navigateurs.
- [x] Identifier les exigences TLS, forwarding HTTP, identité des pools, contraintes des entrées/sorties et annulation.
- [x] Reformuler la cible pour tous les parcours concernés en mode proxy et préserver explicitement le mode direct lorsque le proxy est désactivé.
- [ ] Établir une matrice de compatibilité de tous les moteurs et sous-requêtes avec les deux modes ; identifier les adaptateurs qui contournent encore le core.
- [ ] Concevoir une API générique minimale d’injection avant le TLS de destination et choisir entre contribution au projet amont et patch maintenu. Ne pas modifier le cache du registre Cargo.
- [ ] Valider par compilation un prototype utilisant la dépendance patchée choisie, le core existant et aucune ouverture de port loopback.
- [ ] Valider localement le forwarding HTTP, les certificats HTTPS et ALPN, SOCKS, les routes chaînées, la corrélation des réponses, la réutilisation des pools, les redirections et l’annulation. Les nouveaux tests automatiques nécessitent une autorisation explicite de l’utilisateur.
- [ ] Remplacer le parcours Rquest intégré par un choix explicite d’implémentation ; aucun repli caché vers un listener ou une connexion directe à l’origine.
- [ ] Adapter les moteurs navigateur/solveur au transport interne sans loopback, ou signaler leur incompatibilité avec ce mode sans repli direct automatique.
- [ ] Supprimer tous les adaptateurs loopback de compatibilité du parcours cible, y compris leur initialisation générale et celle spécifique aux navigateurs.
- [ ] Vérifier le mode direct séparément : absence d’initialisation du proxy, de jeton proxy et de réutilisation d’une session proxifiée.

Aucun prototype n’a été compilé et aucun nouveau test n’a été créé pendant cette analyse. Les conclusions reposent sur les signatures exactes des sources locales, et non sur un remplacement de transport fonctionnel. L’inspection des sources brutes du projet amont est complémentaire et ne constitue pas une recommandation de migration de version.

## 7. Périmètre complémentaire et critères de clôture

La garantie ne peut pas être déduite de la seule adaptation du client HTTP principal. L’inspection complémentaire a identifié des clients Rquest construits directement dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/services/player_resolver.rs` et `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/services/rtbf_auvio_resolver.rs`, ainsi qu’un client DoH utilisant `.no_proxy()` dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-dns/src/core/transport.rs`. Cela ne signifie pas que tous ces appels doivent devenir proxifiés : vérifier pour chacun le mode applicable, son héritage et l’injection requise, sans affirmer que l’inventaire est exhaustif.

Les fournisseurs de proxies, probes, résolution DNS et accès nécessaires pour établir les connexions au proxy demandent un traitement de bootstrap explicite. Faire dépendre la découverte ou le DNS servant à joindre le proxy de cette même route peut créer une boucle. Le core et le résolveur doivent partager un contrat injectable sans introduire de dépendance cyclique entre crates. Les accès de bootstrap autorisés relèvent d’une politique documentée, pas d’un contournement caché des moteurs HTTP.

Critères de clôture proposés :

- En mode proxy, chaque moteur annoncé compatible et ses sous-requêtes passent par le transport du core, sans port loopback de compatibilité ni configuration autonome du proxy distant.
- En mode direct, les échanges restent directs et n’initialisent pas `arachnea-proxy`.
- Un moteur incompatible en mode proxy produit un diagnostic explicite ; aucun repli automatique vers loopback ou accès direct n’est ajouté.
- Les routes directes et replis intentionnels du core, lorsqu’ils existent, sont identifiés et distingués du mode désactivé.
- Les garanties DNS, TLS, HTTP forward, pools, sessions et corrélation sont vérifiées dans les deux modes ; la suppression du loopback Rust ne suffit pas à déclarer les navigateurs conformes.
- La vérification porte sur les listeners de compatibilité supprimés, pas sur tous les ports locaux de l’application : API entrante, contrôle de navigateur et autres IPC ont des rôles distincts à inventorier. Un port de contrôle n’est pas un relais proxy de trafic, et son éventuelle suppression ne doit pas être supposée demandée.

Cette révision reste documentaire. Aucun changement des modes, transports, solveurs ou politiques de routage n’a été implémenté.