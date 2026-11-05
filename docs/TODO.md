# TODO

Ce fichier suit les tâches DNS, HTTP et proxy prévues qui restent pertinentes pour l'espace de travail serveur. Il doit être mis à jour lorsque des tâches sont ajoutées, terminées, déplacées vers un outil de suivi ou rendues obsolètes par des décisions de conception ultérieures.

## Streaming

- Ajouter des vues dédiées aux contenus `audio/*` et `images/*` ; leur routage vers `get_entry` reste temporaire : [migration media-type](dev-tracking/media-type-routing-migration-analysis.md).
- Identités YAML des rails : clés standardisées `rail:<provider-id>` ajoutées pour Arte, TF1, France TV et RMC BFM Play ; clé fixe du rail LN24. Trouver une identité persistante pour les rails featured TeleMB et recommandations Canal Zoom/Télésambre sans descripteur ; les UIDs de slider changent entre requêtes et sont exclus. Ne pas utiliser de position ni de titre normalisé comme fausse clé stable.
- Utiliser obscura pour contourner Cloudflare (navigateur rust avec résolution cloudflare interne).

## Front

- Découper les 8 fichiers sources ≥ 1000 lignes : lots 0–7 terminés (`dash.ts` → `dash/`, CSS `VideoJsMediaRenderer.vue`/`HomeHeroBanner.vue`, `storage.ts` → `storage/`, `rustify.ts` → `rustify/`, `useVideoJsMediaRenderer.ts` vagues 1 et 2 puis consolidation par contrôleurs propriétaires de leur état, `useVideoPlayer.ts` découpé en slices et `ProgramEntryDetails.vue` en composables `entry-details` selon [l’analyse](dev-tracking/front-large-source-files-split-analysis.md)). Restent en surveillance : `entryVideoPlayer.ts` et `VideoPlayer.vue` (lot 9).
- Valider manuellement le lot 5.3 et les corrections de la [revue de `ccfaeab0`](dev-tracking/review-ccfaeab0.md), implémentées : storyboard VTT/JSON, changements rapides de source et démontage pendant le chargement, restauration des préférences, qualité DASH et sauvegarde à la fermeture. Build, type-check et lint CSS validés.
- Valider manuellement le lot 6 : transitions média/bande-annonce, changements de langue/lecteur avec restauration de lecture, préférences persistées, standalone et iframe en modes `safe`/`confirmation`/`unsafe`. Build, type-check et lint CSS validés ; aucun test ajouté.
- Valider manuellement le lot 7 : restauration et progression des signets, navigation paginée entre saisons, autoplay et annulation lors des changements de fiche/langue/lecteur, recommandations différées. Build, type-check et lint CSS validés ; aucun test ajouté.
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

- Lots 1 à 4 de l’[extraction des responsabilités Stream](dev-tracking/stream-scrapyfy-remaining-responsibilities-analysis.md) implémentés : activations, orchestration IP et récupération HTML dans Scrapyfy, primitives pays dans Core et adaptateurs conservés. Fabriques des tables de sources et lecteurs génériques de données également extraits ; assemblage applicatif et conversion vidéo conservés dans Stream. Le lot HTML conserve les budgets HTTP/JavaScript distincts, les cycles sur URL demandées et le contrôle de taille après matérialisation ; les éventuels durcissements restent séparés.
- Préserver l'ordre de déclaration des paramètres YAML lors de la fusion des surcharges `services.json` dans Scrapyfy : la reconstruction actuelle via une table de hachage peut modifier l'ordre des pipelines de paramètres dépendants. Limite et contournement documentés dans les spécifications YAML ; vérifier le chargement via les manifestes concernés avant de modifier ce comportement.
- Décider si `arachnea-http` doit exposer une API `tower::Service` en plus du constructeur de requêtes fluide.
- Ajouter des garde-fous optionnels d'usage responsable, comme de la limitation de débit, des délais entre requêtes, des reprises bornées, des vérifications de masquage des cookies et un support optionnel de `robots.txt` si le crate évolue vers du crawling.

## Server/Proxy

- Traiter ultérieurement les bugs éventuels du rafraîchissement CLI IP→pays : la commande crée un inventaire neuf vide, sans charger SQLite ni découvrir de proxys, puis remplace le fichier relatif `data/ip-countries.json` par `[]` si l’initialisation et l’écriture réussissent. Les résultats partiels du helper remplacent également tout le fichier. Décision utilisateur : conserver ces comportements pendant le transfert vers Scrapyfy ; ce suivi ne bloque pas l’extraction. Voir l’[analyse des responsabilités](dev-tracking/stream-scrapyfy-remaining-responsibilities-analysis.md).
- Qualifier l’interface proxy dans les navigateurs ciblés (login vide et alternative `default`) et sur Windows/Linux ; vérifier les modes réseau depuis un client distant. Implémentation admin, clés API de 64 octets et application à chaud livrées ; voir l’[analyse et validation](dev-tracking/proxy-admin-configuration-analysis.md).
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
- Ajouter ou terminer un profil `censorship_resistance` s'il reste prévu dans l'API publique.
- Garder l'interception TLS hors du comportement proxy normal ; ajouter seulement un futur mode de debug explicite si le modèle de sécurité est documenté.
- Continuer à valider les composants anti-censure comme fonctionnalités modulaires : serveurs amont Tor, transports enfichables, tunnels externes, expérimentations de padding/jitter, recherche ECH et documentation claire du modèle de menace.

## Divers

