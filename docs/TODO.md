# TODO

Ce fichier suit les tâches DNS, HTTP et proxy prévues qui restent pertinentes pour l'espace de travail serveur. Il doit être mis à jour lorsque des tâches sont ajoutées, terminées, déplacées vers un outil de suivi ou rendues obsolètes par des décisions de conception ultérieures.
## Streaming

- Modifier load_home pour inclure la possibilité d'inclure une liste de sources a prendre en compte + modifier le front pour pouvoir afficher 1 source a la fois. Dans un 2e temp, ajouter la possibilité de créer des homes personalisées basé sur une source + section de load_home ou categorie.
- Dans la description des services, ajouter des tags cle/valeur pour pouvoir distinguer le pays et la langue des vidéos.
- Terminer la migration et la validation visuelle des vidéos associées : suivi dans `docs/dev-tracking/get-entry-recommendations-migration-analysis.md` ; TF1+ utilise uniquement le rail « Si vous aimez… » et RTBF Auvio le widget contextuel « A découvrir aussi » (validations backend effectuées, rendus frontend restants).
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

- Finish any source-specific or caller-reported invalidation hooks for reusable origin-scoped browser sessions. The HTTP primitive, Scrapyfy page-fetch sub-query, bounded retry policy, domain-scoped in-memory callback-token cache, Cloudflare cookie handoff, explicit invalidation, frontend recoverable error, and mock-engine tests are implemented. Chaser-CF is now limited to Cloudflare session solving; persistent page workflows require another browser engine. See `docs/dev-tracking/papadustream-browser-getxfield-analysis.md` and `docs/dev-tracking/chaser-cf-session-to-rquest-analysis.md`.
- Décider si `arachnea-http` doit exposer une API `tower::Service` en plus du constructeur de requêtes fluide.
- Ajouter des garde-fous optionnels d'usage responsable, comme de la limitation de débit, des délais entre requêtes, des reprises bornées, des vérifications de masquage des cookies et un support optionnel de `robots.txt` si le crate évolue vers du crawling.
- Étendre les tests pour le parsing, l'expiration, la suppression, le filtrage domaine/path/secure des cookies, la détection Cloudflare, les reprises bornées, le masquage des cookies et les réponses locales simulant des challenges.


## Server/Proxy
- Surveiller les compteurs `protocol_conflict_count` et `undeclared_protocol_count` des refreshs dynamiques afin d'identifier les fournisseurs aux déclarations incohérentes et d'ajuster, si nécessaire, l'ordre de détection des protocoles.
- Réduire encore la latence du chargement dynamique après la déduplication fournisseur/cache et le sondage limité aux entrées nouvelles ou expirées : sonder par lots progressifs avec arrêt du chemin bloquant au premier candidat compatible, puis décider si le reste se poursuit en arrière-plan.
- Évaluer la priorité de `FR` sur `AD` pour les listes M6 et rendre la concurrence de sondage configurable depuis la configuration applicative. La passe cache-first multi-pays, le cooldown partagé des consultations fournisseur et la préférence par destination pour le dernier proxy validé sont maintenant implémentés dans l'inventaire.
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
- Prototyper si `rquest::ClientBuilder::connector_layer` peut supporter un connecteur interne ; garder la boucle locale via `rquest::Proxy` comme chemin de compatibilité tant que ce n'est pas prouvé.
- Ajouter ou terminer un profil `censorship_resistance` s'il reste prévu dans l'API publique.
- Garder l'interception TLS hors du comportement proxy normal ; ajouter seulement un futur mode de debug explicite si le modèle de sécurité est documenté.
- Continuer à valider les composants anti-censure comme fonctionnalités modulaires : serveurs amont Tor, transports enfichables, tunnels externes, expérimentations de padding/jitter, recherche ECH et documentation claire du modèle de menace.



 
## Divers
