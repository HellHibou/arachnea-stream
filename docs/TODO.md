# TODO

Ce fichier suit les tâches DNS, HTTP et proxy prévues qui restent pertinentes pour l'espace de travail serveur. Il doit être mis à jour lorsque des tâches sont ajoutées, terminées, déplacées vers un outil de suivi ou rendues obsolètes par des décisions de conception ultérieures.

## Streaming

- Contrôles génériques des sources : lots 1–5 terminés. Tests live ignorés par défaut ; lancement direct du test Rust `test_all_services` ; `ARACHNEA_TEST_QUERY_SOURCE` absent ou `*` sélectionne tous les YAML, sinon un identifiant/chemin sélectionne une source. Voir [la politique du lot 5](dev-tracking/source-checks-lot5-2026-10-30.md).
- Qualification live des douze contrats ajoutés (AnimeUltime, Coflix, FrenchAnimes, Papadustream V2, Wiflix, Antenne Réunion, Bouke, Canal Zoom, RMC BFM Play, TeleMB, Télésambre, TV5MONDE+) : les 21 configurations disposent désormais d’un contrat et la vérification locale des métadonnées passe pour les 21. Un run live complet a été exécuté ; Wiflix passe après correction de son contrat `get_category` (les sections de production n’exposent pas `label`). Restent à qualifier/corriger : AnimeUltime (catégorie dont les entrées sélectionnées sont nulles), Coflix (payload JSON invalide sur l’URL de catégorie), FrenchAnimes (timeout de recherche) et RMC BFM Play (catégories d’accueil nulles). Voir le [guide de création des YAML de test](specifications/arachnea-stream-source-checks-en.md).
- Qualification LN24 : rejouer la fiche avec un routage belge utilisable. Les deux passages du lot 4 conservent une erreur d’enveloppe après absence de proxy BE et repli direct ; l’extraction de la fiche n’est pas qualifiée.
- Qualification RTL Play : décider comment traiter la description facultative `null` de la recherche, incompatible avec la contrainte v1 `string` lorsqu’elle est présente (deux objets sur deux). Analyser la sortie publique et les attentes avant de changer leur contrat ; conserver l’échec en attendant.

- Dans la description des services, ajouter des tags cle/valeur pour pouvoir distinguer le pays et la langue des vidéos.
- Identités YAML des rails : clés standardisées `rail:<provider-id>` ajoutées pour Arte, TF1, France TV et RMC BFM Play ; clé fixe du rail LN24. Trouver une identité persistante pour les rails featured TeleMB et recommandations Canal Zoom/Télésambre sans descripteur ; les UIDs de slider changent entre requêtes et sont exclus. Ne pas utiliser de position ni de titre normalisé comme fausse clé stable.
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

- Préserver l'ordre de déclaration des paramètres YAML lors de la fusion des surcharges `services.json` dans Scrapyfy : la reconstruction actuelle via une table de hachage peut modifier l'ordre des pipelines de paramètres dépendants. Limite et contournement documentés dans les spécifications YAML ; vérifier le chargement via les manifestes concernés avant de modifier ce comportement.

- Décider si `arachnea-http` doit exposer une API `tower::Service` en plus du constructeur de requêtes fluide.
- Ajouter des garde-fous optionnels d'usage responsable, comme de la limitation de débit, des délais entre requêtes, des reprises bornées, des vérifications de masquage des cookies et un support optionnel de `robots.txt` si le crate évolue vers du crawling.
- Étendre les tests pour le parsing, l'expiration, la suppression, le filtrage domaine/path/secure des cookies, la détection Cloudflare, les reprises bornées, le masquage des cookies et les réponses locales simulant des challenges.

## Server/Proxy

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

- Terminé : nettoyage des 491 diagnostics Clippy distincts du workspace Rust ; Clippy strict passe avec tous les targets et toutes les fonctionnalités. La [migration approuvée des dernières enums](dev-tracking/rust-validation-large-enums-analysis-2026-10-29.md) supprime les trois derniers diagnostics, avec une allocation par flux réussi et lors de la construction/du clonage de la variante proxy intégrée. Les contrats YAML/JSON sont conservés ; la migration de l’API Rust publique est documentée. Bilan et inventaire vide : [validation Rust du 28 octobre 2026](dev-tracking/rust-validation-2026-10-28.md).
