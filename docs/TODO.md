# TODO

Ce fichier suit les tâches DNS, HTTP et proxy prévues qui restent pertinentes pour l'espace de travail serveur. Il doit être mis à jour lorsque des tâches sont ajoutées, terminées, déplacées vers un outil de suivi ou rendues obsolètes par des décisions de conception ultérieures.


## Divers

- Utiliser obscura pour contourner Cloudflare (navigateur rust avec résolution cloudflare interne).
- Pour le scrapper: Persistance de cookies + récupérer tout les cookies pour mieu contourner les protections (ex: crunchyroll)
- Ajouter la gestion réutilisable du mode serveur et du mode application de bureau :
    - N'afficher la console que si l'application est exécutée depuis la console. ✅
    - En mode serveur, si le mode graphique est disponible, afficher une icône de notification pour :
        - Afficher la console (fenêtre de logs dédiée `Show log`, via le log cache en mémoire). ✅
        - Démarrer le navigateur avec l'URL du serveur (`Open <url>`). ✅
        - Fermer l'application (`Shutdown server`, arrêt HTTP gracieux). ✅
        - Masquer la console par défaut si l'application n'est pas lancée depuis la ligne de commande. ✅
        - Redémarrer le serveur en rechargeant la configuration (à compléter).


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


## Front

- Si on affiche le dernier épisode chargé de la liste et que 'Charger plus' est dispo, il faut charger plus de données. Si le signet est actif, il faut charger plus si l'épisode courant n'a pas été chargé dans la liste.
- Dans l'accueil et les catégories, ajouter des boutons Section suivante / précédente en bas à droite.
- Ajouter le support de diffusion ChromeCast et AirPlay.
