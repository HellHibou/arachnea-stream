# TODO

Ce fichier suit les tâches DNS, HTTP et proxy prévues qui restent pertinentes pour l'espace de travail serveur. Il doit être mis à jour lorsque des tâches sont ajoutées, terminées, déplacées vers un outil de suivi ou rendues obsolètes par des décisions de conception ultérieures.
## Streaming

- Vérifier visuellement le flou fort du dialogue de gestion et les actions compactes flèches/corbeille de l’éditeur en navigateur/Tauri (clavier, limites de déplacement et annulation sans persistance).

- Valider en navigateur/Tauri l’accueil par défaut (démarrage sur `/`, clic marque, liens directs conservés, définition supprimée/stockage indisponible), ainsi que les tableaux de l’éditeur élargi (1000px), leur défilement horizontal et les actions Enregistrer/Annuler sans prévisualisation.

- Étape 1 — Lots 1A/1B implémentés : filtre `load_home`, découverte des capacités et homes par source. Terminer la validation manuelle navigateur/Tauri, REST/304, navigation rapide et pagination sur sources réelles. Analyse et contrôles : `docs/dev-tracking/source-filtered-custom-homes-analysis.md`.
- Étape 2 — Lots 2A/2B/2C/2D implémentés : stockage versionné additif et références source, composition multisource avec fusion par titre optionnelle/entrelacement, pagination indépendante des contributions, erreurs partielles/retry et route `/homes/:homeId`. Valider en navigateur/Tauri : persistance, upgrades/quota/concurrence, navigation rapide, sous-sections, bannières et pagination réelle des fournisseurs. Lot 2C livré : éditeur/réparation/preview, sauvegarde atomique, suppression confirmée et réorganisation ; ajouts aux bannières services (pas custom), catégories dont custom et sections sources uniquement en édition via fenêtre destination/création, Raccourci/Section et Annuler/Ajouter sans éditeur ; ordre configurable uniquement dans les homes personnalisées (éditeur et haut/bas sur les rails), retraits confirmés des sections/catégories et choix du bloc pour les fusions ; épingles compactes et icônes Home/édition uniquement au titre latéral pour le groupe. Lot 2D livré : navigation latérale homes → Lives → sources, tiroir mobile modal et focus, masquage plein écran, groupe sources persisté et intégration toolbar/arrière-plans. Valider les dialogues, clavier/focus, plein écran Safari/Tauri, paramètres/notifications, onglets natifs indépendants et conflits réels en navigateur/Tauri. Analyse : `docs/dev-tracking/source-filtered-custom-homes-analysis.md`.
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

- Finish any source-specific or caller-reported invalidation hooks for reusable origin-scoped browser sessions. The HTTP primitive, Scrapyfy page-fetch sub-query, bounded retry policy, domain-scoped in-memory callback-token cache, Cloudflare cookie handoff, explicit invalidation, frontend recoverable error, and mock-engine tests are implemented. Chaser-CF is now limited to Cloudflare session solving; persistent page workflows require another browser engine. See `docs/dev-tracking/papadustream-browser-getxfield-analysis.md` and `docs/dev-tracking/chaser-cf-session-to-rquest-analysis.md`.
- Décider si `arachnea-http` doit exposer une API `tower::Service` en plus du constructeur de requêtes fluide.
- Ajouter des garde-fous optionnels d'usage responsable, comme de la limitation de débit, des délais entre requêtes, des reprises bornées, des vérifications de masquage des cookies et un support optionnel de `robots.txt` si le crate évolue vers du crawling.
- Étendre les tests pour le parsing, l'expiration, la suppression, le filtrage domaine/path/secure des cookies, la détection Cloudflare, les reprises bornées, le masquage des cookies et les réponses locales simulant des challenges.


## Server/Proxy

- Terminer la recette exhaustive des lots 1/2 et du handler commun livré au lot 3, puis implémenter les lots 5 à 7 après validation de leur conception. Le lot 4 est intégré : pays canoniques sans priorité, cache persistant avant exploration et classement commun HTTP/HTTPS. Restent le parcours HTTPS propre à `ProxyProbe` (racines fixes), les coupures processus/disque et la recette exhaustive des erreurs/timeouts de handlers, flux médias et moteurs non corrélés. Le lot 3 ajoute politiques YAML/URL, injection code et inspection non destructive ; les budgets/retries communs restent au lot 5. Suivi : `/Users/jdecker/Downloads/arachnea/docs/dev-tracking/analyse-optimisation-proxies-localises.md`, sections 17 et 18.
- Corriger la compilation isolée de Scrapyfy avec `arachnea-proxy,sqlite-persistence` : le helper importé par `resolve_url.rs` dépend actuellement de la feature proxy `controller-service`, non activée dans ce cas ; la compilation de l’application complète masque ce défaut par unification des features.
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

- Vérifier la suite complète Scrapyfy : trois tests d’agrégateur échouent après résolution préalable du répertoire de données applicatif (Once empoisonné), mais passent en isolation ; observé lors du lot 1.
