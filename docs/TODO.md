# TODO

Ce fichier suit les tâches DNS, HTTP et proxy prévues qui restent pertinentes pour l'espace de travail serveur. Il doit être mis à jour lorsque des tâches sont ajoutées, terminées, déplacées vers un outil de suivi ou rendues obsolètes par des décisions de conception ultérieures.
## Streaming

- Dans la description des services, ajouter des tags cle/valeur pour pouvoir distinguer le pays et la langue des vidéos.
- Identités YAML des rails : clés standardisées `rail:<provider-id>` ajoutées pour Arte, TF1, France TV et RMC BFM Play ; clé fixe du rail LN24. Trouver une identité persistante pour les rails featured TeleMB et recommandations Canal Zoom/Télésambre sans descripteur ; les UIDs de slider changent entre requêtes et sont exclus. Ne pas utiliser de position ni de titre normalisé comme fausse clé stable.
- Tests `arachnea-scrapyfy` : les tests `resolve_url` (appel de `apply` sans le paramètre `ProxyFollowRedirects`) ne compilent plus suite au changement de signature du proxy ; préexistant et hors périmètre, à réaligner comme les tests `arachnea-proxy`.
- Utiliser obscura pour contourner Cloudflare (navigateur rust avec résolution cloudflare interne). 

## Front

- Si on affiche le dernier épisode chargé de la liste et que 'Charger plus' est dispo, il faut charger plus de données. Si le signet est actif, il faut charger plus si l'épisode courant n'a pas été chargé dans la liste.
- Ajouter le support de diffusion ChromeCast et AirPlay.
- Facultatif (frontend) : cesser d'envoyer `arachneaEtag` / `enableEtag` dans `front/src/services/rustify.ts` — l'en-tête `If-None-Match` suffit depuis la migration du contexte contrôleur (`docs/dev-tracking/request-context-query-parameters-analysis.md`).





## Server/DNS

- Affiner les modes DNSSEC afin que `report_only`, `opportunistic` et `strict` aient des comportements distincts, au lieu d'envoyer tous les modes non `off` dans le même chemin de validation Hickory.
- Implémenter un transport ODoH effectif derrière la fonctionnalité `odoh`.
- Améliorer le résolveur récursif interne avec des reprises par autorité, un DNSSEC strict de bout en bout, la minimisation QNAME et des contrôles de politique bailiwick plus complets.
- Implémenter `SmartDnsAction::Route` afin que Smart DNS puisse router une requête vers un serveur amont spécifique.
- Ajouter le chargement TOML complet pour `smart_dns.rules` et `proxy_targets`.
- Ajouter des sources externes de listes de blocage et leur rechargement, y compris les listes simples de domaines, le format hosts, les domaines wildcard et de futurs formats compatibles AdGuard/uBlock.
- Ajouter Prometheus ou un autre chemin d'export de métriques en plus des statistiques internes actuelles et des logs JSON.
- Étendre le cache négatif DNSSEC agressif pour synthétiser depuis NSEC3 uniquement après avoir géré correctement la validation et le hachage canonique.
- Évaluer les DNS Cookies pour les déploiements de serveurs publics lorsque les bibliothèques DNS choisies les rendent pratiques.
- Évaluer les listeners DNS chiffrés comme DoT, DoH ou DoQ uniquement si un besoin concret est confirmé.
- Ajouter le support de racines DNS alternatives comme OpenNIC et leurs TLD associés.
- Ajouter une intégration optionnelle permettant à `arachnea-dns` d'utiliser `arachnea-proxy` pour les connexions DNS sortantes sans créer de boucles de résolution DNS/proxy.
- Ajouter des vérifications de blacklist IP après résolution pour la détection de censure : essayer le résolveur suivant lorsqu'une IP résolue est bloquée, exposer les métadonnées de censure dans l'API et utiliser EDNS Extended DNS Error 16 dans les réponses du serveur DNS lorsque c'est approprié.
- Ajouter une escalade configurable vers le mode confidentialité du proxy lorsque le comportement DNS suggère une censure, y compris lorsque le repli de résilience passe à un résolveur ultérieur.
- Garder les zones DNS autoritaires hors de la v1, tout en laissant de la place dans l'architecture pour un futur module `authoritative/` avec SOA, NS, TSIG et support des transferts de zone.
- Valider le comportement du noyau sur Android et garder la logique spécifique à la plateforme, comme le DNS système, les sockets, les permissions et les chemins par défaut, derrière des abstractions.
- Étendre les tests DNS pour les paquets malformés, la compression de noms invalide, les boucles CNAME, les comportements d'inondation, la récursion non autorisée, les changements de protocole DoH/DoT, le comportement du cache et les logs respectueux de la confidentialité.


## Server/HTTP

- [x] Corriger la preuve d’envoi Rquest après les traces FranceTV/6play : `is_connect()` avant tout redirect établit `NotSent` et autorise un autre proxy pour un POST jamais envoyé. CONNECT refusé, TLS non fiable, POST effectivement transmis et erreur sur redirect vérifiés dans une fixture existante ; TLS reste vérifié. Diagnostic et limites : section 26 de l’analyse des proxies localisés.

- Finish any source-specific or caller-reported invalidation hooks for reusable origin-scoped browser sessions. The HTTP primitive, Scrapyfy page-fetch sub-query, bounded retry policy, domain-scoped in-memory callback-token cache, Cloudflare cookie handoff, explicit invalidation, frontend recoverable error, and mock-engine tests are implemented. Chaser-CF is now limited to Cloudflare session solving; persistent page workflows require another browser engine. See `docs/dev-tracking/papadustream-browser-getxfield-analysis.md` and `docs/dev-tracking/chaser-cf-session-to-rquest-analysis.md`.
- Décider si `arachnea-http` doit exposer une API `tower::Service` en plus du constructeur de requêtes fluide.
- Ajouter des garde-fous optionnels d'usage responsable, comme de la limitation de débit, des délais entre requêtes, des reprises bornées, des vérifications de masquage des cookies et un support optionnel de `robots.txt` si le crate évolue vers du crawling.
- Étendre les tests pour le parsing, l'expiration, la suppression, le filtrage domaine/path/secure des cookies, la détection Cloudflare, les reprises bornées, le masquage des cookies et les réponses locales simulant des challenges.


## Server/Proxy

- [x] Isoler la pile des sondes partagées TCP/TLS après le stack overflow FranceTV de 13:20 : tâche enfant possédée et annulable, mutualisation et créneaux conservés ; fixture existante étendue. Section 33 du suivi.
- [ ] Distinguer les délais du proxy préféré déjà accepté par le domaine et ceux de la découverte : le handshake SOCKS reste plafonné à 5 s pendant la réutilisation. Proposition et confirmation requise pour les nouvelles valeurs en section 33 du suivi.
- [x] Réconcilier les exclusions `Other` avec les succès postérieurs de la même association proxy/destination, à la réception et lors de la sélection préférentielle : cache existant réparé et persisté ; blocages explicites, échecs plus récents ou de date identique et autres destinations préservés. Section 32 du suivi.
- [x] Isoler la pile du rafraîchissement fournisseur après le second stack overflow FranceTV : le futur partagé possède une tâche enfant annulée par `JoinSet` quand le dernier abonné disparaît ; mutualisation, sémaphores et échéances conservés. Fixture existante étendue pour la séparation du polling, le contexte technique et la libération des créneaux. Section 31 du suivi ; qualification de lecture média parallèle toujours ouverte.

- [x] Appliquer les délais confirmés après l’échec 6play front-auth : TCP localisé 3 s, handshake proxy 5 s, connexion complète Rquest 15 s, sans augmenter les délais plus courts configurés, avec preuves d’envoi et recherche 120 s conservées. La fixture POST existante vérifie le CONNECT muet interrompu à 15 s puis `NotSent` ; validation réelle authentifiée encore ouverte. Section 30 du suivi.

- [x] Corriger la profondeur linéaire des routes REST mise en évidence par les rapports de stack overflow macOS pendant une sonde TLS : arbre Warp équilibré, ordre d’enregistrement et ACL conservés. Diagnostic et validation : section 29 du suivi des proxies localisés ; la compatibilité réelle des proxies reste à qualifier.

- [ ] Traiter les lenteurs 6play/Gigya auditées le 16 octobre : harmoniser les délais des phases de connexion avec l’enveloppe HTTP et remonter une preuve d’envoi typée, conserver les causes des sondes permissives, éviter les sondes annulées/recommencées et la suspension complète de découverte pendant un essai métier ; revoir séparément rafraîchissement des listes et cooldown de 24 h des erreurs ambiguës. Mesures et priorités : section 28 de l’analyse des proxies localisés. Aucun correctif moteur inclus dans cet audit.

- [x] Supprimer les plafonds de nombre de préchecks, validations et essais métier : recherche jusqu’au succès, épuisement des candidats ou délai partagé de 120 secondes. Concurrence bornée conservée ; réservations annulées libérées et retries des pools dynamiques alignés. Suivi : section 27 de l’analyse des proxies localisés.

- [x] Corriger le blocage de sélection observé sur FranceTV avec persistance : deux validations attendaient un verrou détenu par un précheck que le pipeline ne faisait plus avancer. Reproduction déterministe avec écritures suspendues, puis correction de la progression des deux phases sans augmenter les plafonds ni détacher les commits. Suivi : section 25 de l’analyse des proxies localisés.

- [x] Distinguer dans les logs les listes chargées, préchecks TCP, validations et essais métier : compteurs de recherche corrélés, capacités réellement prouvées et échecs/reprises d’origine. Le scénario existant vérifie trois validations successives avec deux créneaux ; plafonds inchangés. Suivi : section 24 de l’analyse des proxies localisés.

- [x] Isoler la détection du pays courant du contexte d’acceptation de la requête métier : régression reproduite localement puis corrigée après l’échec FranceTV K7 du 16 octobre 2026. La réponse HTTP de géolocalisation ne clôt plus la recherche K7 ; timeout du resolver et annulation de l’appel conservés. Détails : section 23 du suivi des proxies localisés.

- [x] Proxies localisés : lots 0 à 6 intégrés et recette locale du lot 7 réalisée le 16 octobre 2026 ; 254 tests distincts réussis, compilations et mesures locales consignées. Suivi : `docs/dev-tracking/analyse-optimisation-proxies-localises.md`, sections 11 et 21.
- [ ] Clore la qualification des proxies localisés : parcours HTTPS de `ProxyProbe` et secours, concurrence réseau/géolocalisation en charge, recherche à froid après redémarrage réel, POST à effet serveur et média long, horizon réel 120/135 secondes et timeouts de déploiement, coupures processus/disque. Checklist consolidée : section 22 du suivi. Les loopbacks contextuels reconnectent entre appels.
- [x] Implémenter l'approche 2 retenue : gestion distincte du store SQLite et des vues typées par table, regroupement des tables `proxy-inventory`, `proxy-failures` et `proxy-preferences` dans `proxy-inventory.sqlite3`, métadonnées par table et nommage générique `<STORENAME>.sqlite3*` sans sous-dossier. Aucune reprise des données existantes ; ancien reset/marqueur du store de services retiré. Bilan : [analyse-regroupement-persistance-proxies.md](dev-tracking/analyse-regroupement-persistance-proxies.md), section 9 ; 234 tests existants réussis et compilation Stream validée. Atomicité métier multi-tables hors périmètre.
- Lot 2 : commits des repositories proxy sérialisés entre wrappers du même store `Arc`, sans worker. Limite documentée : objets stores indépendants visant le même fichier et écritures multi-processus non coordonnées ; ne pas déclarer corrigée l’abstraction générique `FileEntityStore`.
- Surveiller la croissance de la base SQLite `proxy-inventory` ; les proxys dont la dernière validation dépasse 24 heures ne sont supprimés qu'après un échec observé, donc un très grand nombre de proxys jamais réutilisés justifiera un suivi de volume et, au besoin, une politique de nettoyage ou un vacuum planifié.
- Évaluer MASQUE CONNECT-UDP après la stabilisation du socle UDP et d'une pile Rust HTTP/3 compatible.
- Ajouter l'orchestration `ExternalTunnel` pour les processus locaux comme obfs4proxy, WebTunnel, les plugins Shadowsocks ou un daemon Tor local.
- Ajouter les modes d'intégration Tor :
  - processus Tor local via `ExternalTunnel` ;
  - support Arti optionnel interne derrière la fonctionnalité `tor-arti-client` ;
  - routage `.onion` en mode `system_relay` ;
  - routage `.onion` et repli Tor optionnel en mode `single_forwarder` ;
  - repli Tor et routage `.onion` en mode `resilience` ;
  - Tor comme route par défaut pour `privacy` lorsqu'il est configuré ou compilé.
- Ajouter une intégration concrète `hyper-util` une fois que les traits de connecteur sont suffisamment stables pour l'API du crate.
- Durcir l'authentification optionnelle HTTP proxy et SOCKS5 avec des tests d'échec, la rotation des secrets et un stockage externe plus robuste des secrets.
- Ajouter une vraie limitation de débit par IP client ; l'implémentation actuelle applique des limites de connexions, tandis que la limitation de débit reste déclarative.
- Ajouter des benchmarks pour le débit TCP, la latence CONNECT, le comportement du relais UDP et le surcoût du chainage multi-hop.
- Concevoir le passage interne par `arachnea-proxy` sans loopback pour tous les échanges concernés lorsque le proxy est activé, en préservant le mode direct explicite lorsque le proxy est désactivé. `connector_layer` seul ne permet pas de construire les types privés de `newwreq 5.1.7` ; adapter Rquest, les solveurs et navigateurs sans exception loopback ni accès autonome au proxy distant. Auditer sous-requêtes, modes, bootstrap DNS/fournisseurs, sessions et appels HTTP construits hors client partagé ; signaler les moteurs incompatibles sans repli direct implicite. Analyse : `/Users/jdecker/Downloads/arachnea/docs/dev-tracking/analyse-remplacement-loopback-rquest.md`. Aucun remplacement implémenté.
- Ajouter ou terminer un profil `censorship_resistance` s'il reste prévu dans l'API publique.
- Garder l'interception TLS hors du comportement proxy normal ; ajouter seulement un futur mode de debug explicite si le modèle de sécurité est documenté.
- Continuer à valider les composants anti-censure comme fonctionnalités modulaires : serveurs amont Tor, transports enfichables, tunnels externes, expérimentations de padding/jitter, recherche ECH et documentation claire du modèle de menace.




## Divers

- [x] Vérifier et corriger la suite complète Scrapyfy : double initialisation du répertoire applicatif corrigée au lot 7 ; 80 tests réussis en série et en parallèle avec `arachnea-proxy,sqlite-persistence` (16 octobre 2026).
- [ ] Corriger séparément les écarts de formatage préexistants du workspace relevés par `cargo fmt --all -- --check` ; les fichiers du chantier proxy ont fait l’objet d’un contrôle ciblé.
