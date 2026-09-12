# Analyse : remplacement de `chaser_cf` par un moteur embarqué plus léger

> Créée le 2026-09-11.  
> Périmètre : `server/crates/arachnea-http/src/engine/chaser_cf.rs`, ses contrats publics dans `arachnea-http` et l’intégration in-process de `arachnea-proxy`.  
> État : analyse de décision uniquement ; aucune dépendance ni implémentation n’est modifiée par cette note.

## Objectif

Réduire le coût opérationnel du chemin Cloudflare actuellement fondé sur
`chaser-cf` / `chaser-oxide`, lequel lance et pilote une instance Chromium par
CDP. Le remplacement recherché doit, autant que possible, fournir l’équivalent
des méthodes publiques de `ChaserCfEngine` et ne pas rompre les contrats
consommés par `ArachneaHttpClient` et `arachnea-scrapyfy`.

Le terme « embarqué » doit être précisé : il peut signifier soit une dépendance
Rust liée au processus, soit un binaire Rust plus léger que Chromium géré comme
sous-processus. Ces deux modèles n’ont ni les mêmes garanties, ni le même coût
d’intégration.

Le remplacement doit aussi supprimer la dépendance au **proxy loopback local**
lorsque `HttpProxyConfig::Arachnea` est sélectionné. Un simple changement de
binaire navigateur qui conserverait une URL `http://127.0.0.1:<port>` ne
répondrait donc pas entièrement à l’objectif.

## État actuel dans Arachnea

La fonctionnalité Cargo `chaser-cf` active aujourd’hui `chaser-cf 0.2.1` et
`chaser-oxide 0.2.4`. Ce dernier dépend de composants `chromiumoxide` et pilote
Chrome/Chromium. Le crate `arachnea-stream` active indirectement cette
fonctionnalité via `arachnea-scrapyfy`.

`ChaserCfEngine` fait deux travaux distincts :

1. **Résolution et transfert de session HTTP Cloudflare** : navigation vers une
   origine, attente d’un cookie `cf_clearance`, extraction des cookies et du
   `User-Agent`, persistance par origine, puis remise de ces métadonnées au
   chemin HTTP rapide `rquest`.
2. **Automatisation persistante de page** : conservation d’une page avec état
   JavaScript et cookies ; navigation, `fetch()` exécuté dans la page, clic DOM
   suivi d’une attente CSS, lecture/remise à zéro d’un jeton Turnstile.

Le second point est déterminant. `BrowserPageSession` exige explicitement de
conserver l’état de page, les cookies et l’exécution JavaScript entre
`navigate`, `click_and_wait` et `fetch`. Un simple client HTTP ne peut pas être
équivalent à cette interface.

### État actuel du proxy interne

`ArachneaProxyCore` possède déjà le socle utile à une intégration sans socket
locale :

- `connect_request(ConnectRequest)` applique le routage, les chaînes, les pools
  et le `ClientContext` de la requête avant de retourner un `ProxyStream` ;
- `ArachneaTowerService` l’adapte en `tower::Service<http::Uri>` ;
- `SimpleHttpClient` peut exécuter une requête HTTP(S) directement via le core
  et porte un `ClientContext` par requête.

La boucle locale vient uniquement de limitations d’intégration des clients
actuels, pas du noyau proxy :

- `RquestEngine` accepte une URL proxy ou un `rquest::Client` déjà construit.
  `ArachneaRquestLoopback` construit ce client avec `rquest::Proxy::all()`.
- `PreparedProxyRuntime` démarre un second listener loopback, avec paramètres
  liés, pour Chaser-CF parce que Chrome ne peut pas envoyer les en-têtes de
  paramètres Arachnea lors de `CONNECT`.
- Ghostwire et Chaser-CF reçoivent eux aussi une URL proxy. Ils ne peuvent donc
  pas consommer directement `ArachneaProxyCore`.

Un moteur cible ne satisfait le critère sans loopback que s’il peut recevoir un
adaptateur de transport Rust auquel Arachnea transmet **à chaque requête** le
`ClientContext` dérivé de `proxy_parameters`. Passer seulement une URL de proxy
externe reste accepté pour `HttpProxyConfig::Network`, mais ne doit pas être
utilisé pour `HttpProxyConfig::Arachnea`.

### Divergence documentaire à traiter lors de la migration

Le code courant implémente `open_browser_page_session`, tandis que
`server/crates/arachnea-http/README.md` indique actuellement que `chaser-cf` ne
supporte pas les sessions de page persistantes. Cette incohérence préexiste à
la présente analyse. Le choix d’architecture retenu devra mettre à jour le
README afin qu’il reflète le comportement effectivement exposé.

## Inventaire d’équivalence requis

| Élément actuel | Exigence observable | Moteur HTTP Rust sans navigateur | Navigateur Rust / WebView |
|---|---|---:|---:|
| `ChaserCfEngine::new` | Configuration, proxy, cache de sessions | Oui | Oui |
| `from_chaser_config` | Construction à partir du type natif Chaser | Non, API à remplacer ou déprécier | Oui, avec nouveau type de configuration |
| `with_session_cache`, `with_session_cache_refresh_margin`, `without_session_cache` | Cache persistant de cookies/UA | Oui | Oui |
| `HttpEngine::send` (`GET`/`HEAD`) | Réponse HTML ou en-têtes normalisés | Oui pour les sites ne demandant pas de JS/challenge | Oui, selon compatibilité Web |
| `refresh_cloudflare` | Réutiliser une clearance encore valide | Oui, si une clearance a déjà été obtenue | Oui |
| `refresh_cloudflare_fresh` | Forcer une nouvelle résolution | Non pour un défi JS/Turnstile | Potentiellement, à valider par cible |
| `open_browser_page_session` | Conserver DOM, cookies et runtime JS | Non | Oui, seulement avec DOM/JS/fetch suffisamment complets |
| `BrowserPageSession::navigate` | Navigation, redirections, HTML stabilisé | Partiel : HTTP seulement | Oui, selon implémentation |
| `BrowserPageSession::fetch` | Vrai `window.fetch`, origine et cookies de page | Non : une requête HTTP n’est pas équivalente au contexte navigateur | Oui, selon implémentation |
| `click_and_wait` | Sélecteurs CSS, événements et mutations DOM | Non | Oui, selon DOM, événements et timers |
| `read_turnstile_token` / `clear_turnstile_token` | API Turnstile et champs générés par JS | Non | Incertain ; PoC obligatoire |
| Route proxy Arachnea | `ClientContext` interne, sans listener local | Oui avec `ArachneaProxyCore` / `SimpleHttpClient` | Oui seulement si le moteur accepte un transport injecté |

Les fonctions privées de parcours CDP, de clic dans les shadow roots et de
simulation du curseur ne font pas partie du contrat public. Elles ne doivent
pas être portées à l’identique : elles sont un détail d’implémentation propre à
Chromium. En revanche, les résultats qu’elles cherchent à produire
(`cf_clearance`, token et page rendue) doivent être évalués dans le PoC.

## Évaluation des pistes

### Priorités bloquantes de sélection

Les capacités suivantes sont prioritaires et **éliminatoires**, dans cet ordre :

1. **Cloudflare / Turnstile** : obtenir de manière reproductible les cookies de
   clearance et, lorsque le flux Arachnea le requiert, un jeton Turnstile sur
   des origines contrôlées ou explicitement autorisées.
2. **Page persistante** : réaliser `navigate`, `click_and_wait` avec attente CSS
   et `window.fetch` dans le même contexte de page, avec les cookies, le DOM et
   le JavaScript conservés entre les opérations.
3. **Multiplateforme** : fonctionner nativement sur macOS, Linux et Windows,
   avec le même contrat fonctionnel et une distribution maintenable.

Un candidat qui échoue à l’une de ces trois priorités ne doit pas devenir le
remplaçant de `chaser-cf`, même s’il est plus léger ou permet un meilleur
branchement au proxy interne. La quatrième exigence est alors le transport
`ArachneaProxyCore` sans loopback, suivi seulement ensuite par le coût en
ressources, la licence et la maintenance.

### Tableau comparatif des capacités

Légende : **Oui** = capacité disponible dans le modèle du moteur ; **Non** =
hors périmètre ; **Partiel** = capacité limitée ou dépendante de la plateforme ;
**PoC** = aucune équivalence Arachnea/Cloudflare ne doit être supposée avant un
test reproductible sur une origine autorisée. « Transport interne » signifie
consommer `ArachneaProxyCore` et un `ClientContext` sans URL proxy ni listener
loopback lorsque `HttpProxyConfig::Arachnea` est utilisé. L’exigence
**multiplateforme** signifie macOS, Linux et Windows, avec un moteur maintenable
sur ces trois cibles ; une WebView système dont le moteur varie selon l’OS ne la
satisfait que partiellement.

| Moteur / approche | **Priorité 1 : Cloudflare / Turnstile** | **Priorité 2 : clic, attente CSS, `window.fetch`** | **Priorité 3 : multiplateforme** | JS, DOM et page persistante | Cookies et `User-Agent` transférables | Proxy URL HTTP/SOCKS | Transport interne Arachnea sans loopback | Processus / moteur sous-jacent | Licence / statut pour Arachnea |
|---|---|---|---|---|---|---|---|---|---|
| `rquest` + Ghostwire | Partiel : défis HTTP seulement ; pas de garantie JS/Turnstile | Non | Oui pour le chemin Rust HTTP, sous réserve des cibles de dépendances | Non | Oui pour HTTP et cookies de réponse | Oui | Non avec l’adaptateur actuel ; `rquest` passe aujourd’hui par le helper loopback | In-process ; client HTTP | Déjà intégré ; niveau léger uniquement |
| **Obscura** | **PoC** : prérequis présents, mais issue Turnstile ouverte sur `v0.2.2` en stealth ; aucune réussite Arachnea démontrée | Oui via CDP/MCP ou API interne, à adapter | **PoC** : vérifier les artefacts et le fonctionnement natif sur les trois OS avant sélection | Oui, selon son navigateur DOM/JS | Oui, cookie jar et UA existent | Oui | Non dans le binaire/CDP ; **possible seulement via fork** ajoutant un backend de transport | Binaire Rust externe, ou fork de crates internes | Apache-2.0 ; candidat expérimental |
| **Servo** | **PoC** ; pile Web plus large, mais aucun résultat Arachnea démontré | Oui en théorie via embedder ; adaptation Arachnea importante | Oui déclaré : macOS, Linux et Windows 64 bits ; PoC d’embedder requis | Oui, navigateur complet/prototype | Oui, avec adaptation de l’embedder | Configuration réseau interne, à adapter | Possible par fork/intégration de la pile réseau ; aucun hook stable confirmé | In-process Rust | MPL-2.0 ; piste structurante, maintenance élevée |
| **Lightpanda** | **PoC** ; ne pas présumer une compatibilité anti-bot | Oui via CDP | **Non** : binaires officiels Linux/macOS ; pas de binaire Windows natif | Oui, navigateur headless JS | Oui, selon CDP et ses APIs | Oui | Non ; processus externe et proxy réseau requis sans modification profonde | Binaire Zig externe | AGPL-3.0 ; exclu par défaut |
| **Wry / Tauri** | **PoC** ; dépend du moteur système et de son empreinte | Partiel : APIs WebView et boucle d’événements ; dépend de l’OS | Partiel : trois OS visés, mais moteurs WebView et comportements diffèrent | Oui, WebView native | Oui, selon WebView/plateforme | Partiel, dépend de la plateforme | Non confirmé ; pas de connecteur `ArachneaProxyCore` stable | In-process, mais WebView2/Edge Chromium, WKWebView ou WebKitGTK | Apache-2.0/MIT ; utile pour UI locale, pas backend portable léger |
| **Boa seul** | Non | Non | Oui pour le moteur Rust, mais insuffisant comme navigateur | JS ECMAScript seulement ; pas de DOM/navigateur | Non, à reconstruire entièrement | Non | Possible seulement en construisant aussi HTTP/DOM/cookies, hors périmètre | In-process Rust | Déjà présent ; non candidat navigateur |
| Playwright / Puppeteer / Fantoccini / Chromiumoxide | **PoC** ; Chromium conserve généralement la meilleure compatibilité | Oui | Partiel : dépend des navigateurs et de leur distribution par OS | Oui, car ils pilotent un navigateur | Oui | Oui | Non pour le navigateur externe sans proxy réseau | Pilote + Chrome/Firefox/Chromium | Ne réduit pas la dépendance à Chrome |
| `chaser-cf` actuel | Implémentation actuelle, mais dépendante de Chrome et fragile par cible | Oui | Partiel : dépend de Chrome/Chromium installé et compatible sur chaque OS | Oui | Oui | Oui | Non ; utilise le listener loopback paramétré | Chrome/Chromium piloté par CDP | Référence de compatibilité transitoire |

Le tableau compare des capacités techniques, non une autorisation de contourner
des protections. Les tests de Cloudflare et Turnstile doivent rester limités aux
origines contrôlées ou explicitement autorisées.

### 1. Conserver `rquest`/Ghostwire pour HTTP et supprimer le navigateur

**Description.** Utiliser le chemin `rquest` existant, avec Ghostwire pour les
cas de challenge HTTP qu’il sait traiter, et retourner
`UnsupportedEngineOperation` pour les sessions de page.

**Avantages.**

- Changement de dépendances faible ; `rquest` et Ghostwire sont déjà présents.
- Consommation mémoire et temps de démarrage très inférieurs à Chromium.
- Le cache persistant `CachedChaserSession` peut devenir générique sans changer
  son format de cookies ni le transfert du `User-Agent`.
- Bon candidat pour les requêtes ordinaires et les cibles sans JavaScript.

**Limites bloquantes.**

- Ne fournit pas une résolution générale des défis JavaScript modernes.
- Ne peut pas offrir `BrowserPageSession`, les clics, le DOM dynamique ou les
  callbacks Turnstile.
- Remplacer silencieusement le navigateur par ce moteur modifierait le contrat
  de `page_navigate`, `page_click` et `page_fetch` et casserait les services
  YAML qui demandent un `browser_token`.

**Conclusion.** À utiliser comme niveau léger explicite, pas comme substitut
fonctionnel universel de `ChaserCfEngine`.

### 2. Obscura comme navigateur Rust externe

**Constat vérifié le 2026-09-11.** Le projet Obscura de
`h4ckf0r0day/obscura` est un workspace Rust Apache-2.0 récent qui inclut des
crates DOM, réseau, navigateur, CDP, JavaScript, rendu, CLI et MCP. Son dépôt
publie des binaires et expose notamment navigation, évaluation JavaScript,
clic, attente de sélecteur, cookies, proxy HTTP/SOCKS5 et un mode « stealth ».
Le dépôt a publié `v0.2.2` le 2026-09-05.

Il ne faut toutefois pas confondre ce projet avec le crate crates.io nommé
`obscura` : celui-ci est un projet de raytracing sans rapport, version `0.0.2`
publiée en 2019. Ajouter `obscura = "..."` à `Cargo.toml` ne permettrait donc
pas d’intégrer le navigateur visé.

**Compatibilité avec le proxy interne.** La couche `obscura-net` expose un
`ObscuraHttpClient`, mais le workspace utilise `reqwest` (et, pour le mode
stealth, une autre couche cliente dédiée). Aucun trait public de connecteur
équivalent à `tower::Service<Uri>` n’est exposé par le manifeste et l’API
publique inspectés. Le binaire/CDP accepte un proxy HTTP/SOCKS5 par URL, ce qui
nécessite un proxy réseau pour appeler Arachnea depuis un sous-processus.

Il faut distinguer le transport du **handler d’interception** déjà fourni par
Obscura. Son trait public `RequestInterceptor` reçoit un `RequestInfo` et peut
retourner `Continue`, `Block`, `ModifyHeaders` ou `Fulfill(Response)`. Ce dernier
cas permet à un adaptateur de produire une réponse à partir d’un client Arachnea
interne, au lieu de laisser Obscura effectuer cette requête sur `reqwest`/`wreq`.
Les travaux amont récents sur le transport de page confirment que les ressources
de rendu passent par les politiques de page (cookies, proxy, CORS, redirections,
blocage et interception).

Ce handler n’est toutefois **pas** un connecteur TCP/TLS/HTTP injectable : il ne
remplace pas la pile réseau, le handshake TLS, le DNS ni l’empreinte de la
requête effectuée par `reqwest`/`wreq` lorsque l’action est `Continue`. Pour
Cloudflare, répondre à un niveau applicatif avec `Fulfill(Response)` peut aussi
changer les signaux réseau/fingerprint que la cible observe. Il faut donc un PoC
sur la navigation principale, les redirections, iframes, sous-ressources et
`fetch()` JavaScript avant de considérer cette approche équivalente.

À la vérification du 2026-09-11, aucun fork public identifié ne fournit déjà un
backend de transport générique ou un handler Arachnea prêt à relier
`ArachneaProxyCore::connect_request(...)` à Obscura. Les forks inspectés sont
principalement des forks génériques sans description de modification réseau.
Deux voies restent donc possibles :

1. **Adaptateur par `RequestInterceptor::Fulfill`** : construire une
   `Response` Obscura via `ArachneaProxyCore` / `SimpleHttpClient`, avec un
   `ClientContext` par requête. C’est la voie la moins intrusive, mais son
   équivalence Cloudflare/Turnstile est incertaine.
2. **Fork épinglé avec backend de transport** : introduire un vrai backend qui
   crée des `ConnectRequest` pour toutes les connexions de page. C’est la voie
   nécessaire si le PoC établit que l’interception applicative ne conserve pas
   les propriétés réseau requises par les challenges.

Un pilotage du binaire Obscura uniquement par CDP ne peut pas appeler directement
un trait Rust détenu par le processus Arachnea.

#### Pourquoi Cloudflare / Turnstile reste un PoC

Le statut **PoC** ne signifie pas qu’Obscura ne peut jamais exécuter un défi ;
il signifie que ses prérequis ne constituent pas encore une preuve que les
résultats prioritaires d’Arachnea sont atteints : obtention d’un
`cf_clearance`, lecture d’un callback Turnstile et continuité de la session dans
le même contexte de page.

**Capacités vérifiées qui justifient le PoC.**

- Obscura utilise V8 au travers de `deno_core`, et non un interpréteur
  JavaScript minimal ; sa couche JS inclut des primitives WebCrypto et des
  appels `fetch()`/XHR.
- La page maintient un runtime JavaScript, un DOM et des realms de frames. Les
  opérations CDP documentées incluent navigation, évaluation, clic, attente de
  sélecteur et accès à des contextes d’exécution de session.
- La couche réseau conserve ses propres cookies. Le profil `--stealth` aligne un
  `User-Agent` Chrome, `navigator.platform`, les client hints et une
  impersonation TLS/HTTP de profil Chrome ; le même profil peut être appliqué
  aux requêtes `fetch()`/XHR exécutées par JavaScript.
- La release `v0.2.2` indique que les corps binaires XHR/fetch, les en-têtes et
  les corps de requête sont conservés, que les contextes d’exécution survivent
  aux changements d’onglet et que certaines interactions DOM/formulaires ont
  été renforcées.

Ces éléments permettent de construire l’adaptateur `BrowserPageSession`, mais
ils ne garantissent ni une empreinte complète de Chrome ni la résolution d’un
challenge particulier.

**Éléments qui empêchent de conclure à une compatibilité.**

- Aucun solveur Cloudflare/Turnstile, aucune API dédiée à `cf_clearance` et
  aucune méthode dédiée à la réponse Turnstile n’ont été identifiés dans les
  interfaces publiques inspectées. L’adaptateur devrait donc lire les cookies
  et le token par les mécanismes génériques de page/DOM/CDP.
- Les notes de `v0.2.2` précisent que des revendications de rendu WebGL/GPU ont
  été retirées du téléchargement stealth. Or le moteur Chaser-CF actuel active
  explicitement une émulation GPU logicielle parce que certains challenges
  dépendent de WebGL/WebGPU et de leur fingerprint.
- Le ticket Obscura **#878** est ouvert au 2026-09-11. Il rapporte, sous Linux,
  avec `v0.2.2`, build rendu + stealth et `--stealth` actif, une page affichant
  « Just a moment… » puis des erreurs `ShadowRoot.appendChild` et
  `addEventListener` null sur une page signalée comme protégée par Cloudflare
  Turnstile. Le rapport décrit un crash et ne fournit ni `cf_clearance`, ni
  token, ni téléchargement final. Il n’existe alors aucun commentaire de
  correctif ou preuve de résolution sur ce ticket.
- Ce ticket utilisateur est un signal de risque, pas une preuve que tous les
  défis Turnstile échouent. Il confirme toutefois qu’Obscura ne peut pas être
  déclaré compatible par simple présence de V8 ou du mode stealth.

**Conditions de réussite du PoC Obscura.** Une exécution réussie doit démontrer
simultanément, sans intervention humaine et sur une cible explicitement
autorisée :

1. la disparition de l’interstitiel/challenge et l’arrivée sur le contenu cible
   stable ;
2. la présence d’un `cf_clearance` utilisable et transférable vers le cache
   Arachnea lorsque la cible en émet un ;
3. la lecture d’un token Turnstile valide depuis le callback ou le champ de
   réponse lorsque le workflow applicatif l’exige ;
4. un clic, une attente CSS et un `window.fetch` réussis dans la même session
   après le challenge ;
5. le même résultat sur macOS, Linux et Windows.

Un simple statut HTTP `200`, un DOM contenant le widget, ou un `navigator` dont
le profil ressemble à Chrome ne sont pas des critères de succès.

#### Forks et dérivés publics pertinents observés

La majorité des quelque 1 900 forks observables sont des miroirs ou des copies
sans description d’écart fonctionnel. Les dérivés suivants ont un changement
public identifiable au 2026-09-11 :

| Projet | Ajout vérifié | Intérêt pour les trois gates prioritaires | Limites pour Arachnea |
|---|---|---|---|
| **`Rhevin/obscura-solverr`** | Ajoute le crate `obscura-solverr`, le sous-commande `solverr` et un serveur HTTP compatible FlareSolverr (`POST /` ou `/v1`, `GET /health`). Supporte `request.get`, `sessions.create`, `sessions.destroy` et `sessions.list`; conserve une `BrowserContext` et une `Page` par session, avec proxy URL, stealth et UA ; limite par défaut à 25 requêtes de session. | Le seul fork trouvé qui implémente une boucle explicite de détection de challenge : navigation `Load`, lecture du HTML/cookie jar, détection `cf_clearance`, réexécution immédiate des scripts avec budget complet puis nouvelles tentatives périodiques. Il retourne HTML, UA et cookies au format FlareSolverr. Son README revendique un essai protégé réussi lorsque `cf_clearance` est retourné. | Ne remplace pas `reqwest`/`wreq` par un transport injectable : le crate ne dépend que des composants Obscura. Il démarre un listener HTTP sur le port 8191 et utilise seulement une URL proxy HTTP/SOCKS5. `request.post` n’est pas implémenté ; aucune API `click_and_wait`, `window.fetch` générale ou lecture de token Turnstile n’est exposée par son API FlareSolverr. Les revendications de résolution restent à reproduire dans le PoC Arachnea, sur les trois OS. |
| **`Lawlietr/obscura-cjk`** | Ajoute des fontes Noto Sans CJK SC/TC embarquées derrière la feature `cjk`, un répertoire de fontes à l’exécution (`--fonts` / `OBSCURA_FONTS_DIR`), des archives `-cjk` / `-cjk-stealth` et du packaging Docker multi-architecture. Le fork documente des archives Linux, macOS et Windows. | Peut améliorer le rendu de pages/challenges contenant du texte CJK ou des exigences de fontes sans dépendre des fontes hôtes. Il améliore aussi la distribution multiplateforme, pas la logique de challenge. | Le README indique que le moteur, stealth, proxy, CDP et automatisation restent ceux de l’amont. Aucun handler réseau Arachnea, solveur Cloudflare/Turnstile ou API de token supplémentaire n’est ajouté. Les fontes CJK ajoutent environ 30 Mio au binaire de base selon le fork. |
| **`liuwen/obscura`, branche `vision-input-fixes`** | Branche de travail dont le dernier commit observé documente des limitations `Referer`; les commits visibles portent aussi sur CI/V8 et corrections vision/input. | Peut être utile comme source ponctuelle de corrections de runtime/interaction. | Aucun ajout public vérifié concernant Cloudflare/Turnstile, transport/proxy injectable ou API de session Arachnea. Ce n’est pas un candidat de base : ses changements doivent être sélectionnés au commit/PR plutôt qu’adoptés comme distribution. |
| **`Hunter2030ZeRo/obscura-for-weber`** | La description annonce une optimisation pour Project Weber. | Aucun bénéfice vérifiable identifié. | La comparaison avec l’amont observée le 2026-09-11 indique `ahead_by: 0` et `behind_by: 34` : aucun commit propre ni changement de fichier à réutiliser à cet instant. |

`obscura-solverr` est donc une **référence de PoC utile**, car il rend explicite
une stratégie de retry et de remise de `cf_clearance`. Il ne satisfait cependant
pas à lui seul le contrat Arachnea : son API est une façade solveur HTTP et non
une implémentation de `BrowserPageSession`; sa dépendance à un listener et à une
URL proxy ne résout pas non plus l’exigence de transport interne sans loopback.

Avant toute réutilisation de code de fork, Arachnea doit épingler un commit,
conserver l’attribution Apache-2.0, exécuter les tests sur les trois OS et
réappliquer ses règles de non-divulgation des cookies et des tokens dans les
logs/erreurs.

**Modèles d’intégration possibles.**

1. **Sous-processus versionné + CDP local** : Arachnea lance un binaire Obscura
   fourni/contrôlé, crée une session par contexte Arachnea et implémente un
   adaptateur `BrowserPageSession` via CDP.
2. **Fork Git figé** : importer les crates internes du dépôt via une révision
   Git précise ou un fork maintenu dans l’organisation, puis écrire un
   adaptateur Rust direct.
3. **Fork vendored** : copier le workspace nécessaire dans le dépôt. Ce choix
   augmente fortement la maintenance et ne doit être envisagé qu’après un PoC
   concluant et une revue des mises à jour de sécurité.

**Atouts.**

- Peut théoriquement recouvrir les opérations de page que le contrat Arachnea
  requiert, sans exiger l’installation de Chrome.
- La séparation CDP permet de préserver une frontière d’adaptateur et de ne pas
  propager les types Obscura dans les APIs publiques d’Arachnea.
- La licence Apache-2.0 est compatible avec la double licence du workspace.

**Risques et points non démontrés.**

- Obscura reste un navigateur, donc pas un simple client HTTP ; les binaires
  publiés indiquent un coût de distribution significatif. La réduction de
  ressources doit être mesurée, et non supposée.
- Sa compatibilité Web n’est pas celle d’un Chrome stable. Turnstile, les
  iframes cross-origin, les shadow roots fermés, WebGL/WebGPU, Canvas,
  empreintes TLS et APIs anti-bot sont des risques majeurs.
- Le mécanisme actuel dépend explicitement de capacités GPU/WebGL ou de leur
  émulation logicielle. Un navigateur Rust qui ne reproduit pas ces surfaces ne
  peut pas être annoncé comme équivalent face aux défis concernés.
- Le dépôt est très récent et évolue rapidement ; une intégration par branche
  `main` serait non reproductible. Il faut épingler une release/commit, vérifier
  les licences transitives, les plateformes supportées et les CVE.

**Conclusion.** Meilleure piste pour remplacer Chrome *si* le besoin inclut
encore une page JavaScript persistante, mais elle doit être classée
**expérimentale** tant qu’un PoC reproductible ne valide pas les origines
autorisées d’Arachnea et le transport interne. Ce n’est pas actuellement une
dépendance Cargo prête à l’emploi.

### 3. Servo comme moteur navigateur Rust in-process

Servo est un moteur de navigateur prototype écrit en Rust, distribué sous MPL
2.0. Il possède des crates d’embarquement et une pile Web étendue (DOM, réseau,
WebGL/WebGPU, etc.) ; il peut donc s’exécuter sans Chromium. C’est le seul
candidat étudié qui réunit à la fois un langage compatible avec le workspace et
un modèle d’intégration potentiellement in-process.

**Atouts.**

- Le code peut être lié au processus Rust ; aucune URL proxy loopback n’est
  conceptuellement nécessaire.
- La pile réseau Servo est interne et assez profonde pour qu’un fork puisse
  brancher le routage Arachnea au niveau de la création de connexion, avant les
  requêtes HTTP de page et de sous-ressources.
- Il couvre un périmètre Web bien plus large qu’un moteur DOM de scraping.

**Risques bloquants.**

- Servo se décrit lui-même comme un prototype. Son API d’embedder et son
  architecture réseau sont beaucoup plus lourdes qu’un simple adaptateur
  `HttpEngine` ; l’intégration serait une modification architecturale majeure.
- Le code réseau observé construit son propre client HTTP et ses propres états
  de requête. Aucun hook public et stable équivalent au connecteur Arachnea
  n’est confirmé par cette analyse. Le branchement au core implique donc un
  fork ou une contribution amont substantielle.
- MPL-2.0 est compatible avec une dépendance, mais impose de publier les
  modifications des fichiers Servo couverts. Ce point doit être validé par une
  revue de licence avant tout fork.
- La consommation et le délai de compilation/distribution doivent être mesurés :
  Servo n’est pas présenté comme un moteur léger de scraping.

**Conclusion.** Candidat stratégique pour satisfaire simultanément « navigateur
Rust » et « transport Arachnea in-process », mais non adapté à un petit
remplacement local. À considérer seulement si le projet accepte un fork moteur
et une maintenance longue durée.

### 4. Lightpanda comme navigateur léger CDP externe

Lightpanda est un navigateur headless écrit en Zig, non fondé sur Chromium,
pilotable par CDP et annoncé pour l’automatisation. Son projet publie des
benchmarks favorables face à Headless Chrome (123 Mio de pic mémoire contre 2
Gio pour 100 pages dans son propre benchmark) et fournit des binaires macOS et
Linux.

**Atouts.**

- Candidat intéressant pour un PoC de consommation et de navigation sans
  Chromium ; il exécute JavaScript et cible Playwright/Puppeteer via CDP.
- Les opérations CDP correspondent globalement aux besoins de navigation,
  évaluation, clic et attente de sélecteur.

**Exclusions actuelles.**

- C’est un processus Zig externe, donc il ne peut pas appeler
  `ArachneaProxyCore` directement. Sa configuration de proxy est une
  configuration réseau ; il faudrait conserver un listener ou modifier
  profondément Lightpanda.
- Sa licence est **AGPL-3.0**. L’intégration, la distribution et surtout un fork
  dans un serveur accessible par réseau requièrent une revue juridique dédiée.
  Elle est incompatible avec l’objectif de conserver simplement le modèle de
  licence MIT/Apache d’Arachnea sans obligations supplémentaires.
- Il ne doit pas être présumé compatible Turnstile/Cloudflare sans validation
  autorisée par cible.

**Conclusion.** Bon comparateur de performance externe, mais exclu comme
solution par défaut à cause du transport in-process absent et de l’AGPL-3.0.

### 5. Wry/Tauri et les WebViews système

Wry est déjà une dépendance optionnelle du workspace via le solveur Tauri. Il
embarque une WebView native dans le processus, mais le moteur réel dépend de la
plateforme : WebView2/Edge Chromium sous Windows, WKWebView sous macOS et
WebKitGTK sous Linux. Il exige une boucle d’événements et une fenêtre/handle
natif ; le support proxy est également dépendant de la plateforme.

**Conclusion.** Il reste utile pour un flux interactif local, mais ne remplace
pas Chromium de manière uniforme et ne garantit pas un transport
`ArachneaProxyCore` injecté. Il ne répond pas au besoin de moteur léger backend
portable.

### 6. Intégrer seulement un moteur JavaScript tel que Boa

Boa est déjà une dépendance de workspace. Il fournit un moteur ECMAScript,
contexte, compilateur et machine virtuelle. Ce n’est pas un navigateur complet
avec pile DOM/HTML/CSS, navigation, cookie jar, fetch Web, frames, rendu ou
surface d’empreinte navigateur.

**Conclusion.** Boa peut aider pour un traitement JavaScript isolé et maîtrisé,
mais ne peut pas remplacer les méthodes de page ni exécuter de façon fiable les
scripts Cloudflare/Turnstile. Construire un navigateur autour de Boa serait un
projet d’architecture majeur, à exclure de cette migration.

### 7. Navigateurs pilotant Chromium/Firefox ou moteurs de rendu CEF/WebKit

Les bibliothèques de pilotage telles que Playwright, Puppeteer, Fantoccini,
chromiumoxide ou les wrappers CEF ne résolvent pas le problème : elles pilotent
un navigateur existant ou embarquent un moteur Chromium. Elles peuvent améliorer
l’API d’automatisation mais ne réduisent pas fondamentalement la dépendance ou
le coût de Chrome.

Les alternatives WebKit/CEF suivent la même limite proxy : un moteur externe ou
natif qui ne propose pas de hook de connexion Arachnea nécessite une URL proxy
et donc, pour le core interne, un listener de compatibilité.

### 8. Navigateur Chromium conservé, mais isolé comme dernier recours

Cette option ne satisfait pas seule l’objectif de suppression de Chrome, mais
elle est le repli qui préserve les flux existants pendant la migration : garder
un adaptateur Chromium derrière une fonctionnalité optionnelle et sélectionner
un moteur léger avant lui.

**Conclusion.** C’est le chemin de sécurité fonctionnelle recommandé pendant
le déploiement progressif. Il évite de transformer des échecs de compatibilité
en régression silencieuse des services.

## Architecture recommandée

### Décision proposée

Ne pas remplacer le contenu de `chaser_cf.rs` par une unique implémentation
HTTP. Introduire à la place une séparation explicite :

1. **`CloudflareSessionSolver` interne** : contrat limité à obtenir et restituer
   une session transférable (`cookies`, `User-Agent`, expiration, origine).
   Le cache `CachedChaserSession` reste attaché à ce niveau et est renommé de
   manière neutre seulement lors d’une migration de schéma planifiée.
2. **`ArachneaTransport` interne** : façade asynchrone sur
   `ArachneaProxyCore::connect_request`, portant le `ClientContext` demandé.
   Ses premiers adaptateurs sont `tower::Service<Uri>` et le client HTTP direct.
   Le transport ne convertit jamais un core interne en URL loopback.
3. **Moteur léger (`rquest` + Ghostwire)** : choisi pour les requêtes normales
   et les cas qu’il résout réellement. Pour `HttpProxyConfig::Arachnea`, il doit
   évoluer vers le transport interne ou être remplacé par un client compatible,
   au lieu de démarrer `ArachneaRquestLoopback`.
4. **`BrowserPageSession` explicite** : fourni seulement par un adaptateur qui
   peut prouver navigation/DOM/JS/fetch/cookies **et** accepter
   `ArachneaTransport`. Obscura exige aujourd’hui un fork pour cette condition ;
   Servo est le seul candidat évalué dont le modèle in-process la rend
   concevable, au prix d’un fork plus important.
5. **Repli Chromium temporaire et explicite** : maintenu sous `chaser-cf` tant
   que le PoC Obscura n’atteint pas les critères convenus. Il ne doit pas être
   lancé comme fallback implicite d’une requête `direct`.

### Contrat de transport proposé

Le nouveau contrat doit être indépendant d’un client HTTP concret et préserver
les paramètres de route sans les sérialiser dans des en-têtes proxy :

```rust
#[async_trait]
pub trait BrowserTransport: Send + Sync {
    async fn connect(
        &self,
        destination: Destination,
        context: ClientContext,
    ) -> Result<ProxyStream, ArachneaHttpError>;
}
```

Une variante fondée sur `tower::Service<Uri>` est possible pour les clients qui
ne permettent pas de contexte par requête, mais elle ne suffit pas pour les
routes dynamiques par pays. Le navigateur doit associer le `ClientContext` à sa
page/sous-requête avant chaque connexion. Les requêtes directes et les proxies
déclarés avec `HttpProxyConfig::Network` restent des implémentations distinctes
du même contrat.

Ce contrat est une direction de conception, pas une API à ajouter sans PoC : le
support actuel de `rquest` indique explicitement que le remplacement in-process
de son transport n’est pas stable. Sa concrétisation implique donc soit une
intégration Hyper/Tower complète, soit un client HTTP Arachnea dédié pour les
flux sans navigateur.

Cette structure fait de la compatibilité de page une capacité déclarée, au
lieu de l’associer par erreur à tout solveur Cloudflare.

### Équivalents publics proposés

Pour éviter une rupture immédiate, conserver `ChaserCfEngine` durant la phase
de transition. Le nouveau moteur peut exposer :

```rust
pub const ENGINE_NAME: &str = "obscura";

pub struct ObscuraEngine { /* adapter configuration and session cache */ }

impl ObscuraEngine {
    pub fn new(config: &ArachneaHttpConfig) -> Result<Self, ArachneaHttpError>;
    pub fn with_session_cache(
        self,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) -> Self;
    pub fn with_session_cache_refresh_margin(self, margin: Duration) -> Self;
    pub fn without_session_cache(self) -> Self;
}
```

`HttpEngine::{send, refresh_cloudflare, refresh_cloudflare_fresh,
open_browser_page_session}` doit avoir un équivalent. En revanche,
`from_chaser_config(ChaserConfig)` est intrinsèquement couplée à la dépendance
sortante : elle ne peut pas avoir un équivalent de type identique sans
conserver `chaser-cf`. Deux options compatibles sont possibles :

- conserver la méthode uniquement sur `ChaserCfEngine` comme API de migration ;
- proposer `ObscuraEngine::from_config(ObscuraEngineConfig)` avec un nouveau
  type public, et documenter la migration.

La seconde est plus saine. La première évite une rupture mais ne doit pas être
présentée comme une équivalence sémantique.

## Plan de PoC avant toute migration

Le PoC doit être réalisé exclusivement sur des origines contrôlées ou pour
lesquelles l’opérateur possède une autorisation explicite. Il doit tester une
version Obscura épinglée, un proxy identique à celui du client HTTP et des
exécutions reproductibles sur les plateformes Arachnea ciblées.

### Phase 0 — gates prioritaires et observabilité

- Définir un jeu de pages autorisées couvrant d’abord les trois gates : scénario
  Cloudflare/Turnstile contrôlé si disponible, page actionnable avec clic et
  attente CSS, `window.fetch` dans la même page, puis exécution native sur
  macOS, Linux et Windows.
- Mesurer mémoire RSS au repos, pic RSS, CPU, délai de premier démarrage,
  délai de navigation et nombre de processus pour Chaser/Chromium, Obscura,
  Servo et, à titre comparatif seulement, Lightpanda.
- Journaliser uniquement des noms de cookies, statuts, durées et identifiants
  d’origine ; jamais les valeurs de cookies ni les jetons.
- Vérifier l’absence de bind/listen local dans le chemin
  `HttpProxyConfig::Arachnea` avec un test de processus et de sockets ; une URL
  `HttpProxyConfig::Network` reste autorisée lorsqu’elle est explicitement
  configurée par l’opérateur.

### Phase 1 — transport et métadonnées

- Implémenter en prototype `send`, `refresh_cloudflare` et
  `refresh_cloudflare_fresh` derrière une feature non activée par défaut.
- Vérifier proxy avec et sans authentification, redirections, cookies multiples,
  domaine/path/secure/samesite, expiration et `SOLVER_USER_AGENT_HEADER`.
- Vérifier le routage par `ClientContext` (notamment paramètres de pays) sur la
  navigation principale, les redirections, les iframes, les sous-ressources et
  le `fetch()` de page, sans en-tête de paramètres divulgué au site cible.
- Vérifier la réutilisation et l’invalidation de `CachedChaserSession` avec les
  tests existants ; ajouter des tests uniquement si le changement de contrat le
  nécessite explicitement.

### Phase 2 — priorité page persistante et validation multiplateforme

- Prouver `navigate`, `metadata`, `fetch`, `click_and_wait`,
  `read_turnstile_token`, `clear_turnstile_token` et `close` sur les pages de
  test, sans recréer la page entre le clic, l’attente CSS et le `fetch()`.
- Exécuter ce même scénario sur macOS, Linux et Windows ; tout écart de moteur,
  de résultat Cloudflare/Turnstile ou de contrat de page est bloquant.
- Vérifier isolation origine/profil/proxy, fermeture effective des processus et
  éviction par `BrowserSessionManager`.
- Définir précisément les opérations non supportées et retourner
  `UnsupportedEngineOperation` plutôt qu’une simulation incomplète.

### Phase 3 — sélection et déploiement

- Ajouter un nouveau variant explicite de `CloudflareBrowserSolverKind` ; ne
  pas changer rétroactivement la signification de `ChaserCf`.
- Garder Chromium disponible pendant une période de validation et permettre un
  rollback de configuration sans migration de cache.
- Après validation, mettre à jour le README, `docs/TODO.md`, le changelog et
  les spécifications concernées ; supprimer `chaser-cf` seulement quand aucune
  fonctionnalité active ne dépend plus de ses types publics.

## Critères de décision

Un moteur candidat peut devenir le moteur par défaut à la place de Chromium
seulement si, dans cet ordre :

1. Cloudflare/Turnstile est validé de manière reproductible sur les flux
   autorisés, avec cookies de clearance et jeton de callback lorsqu’il est
   requis ;
2. `navigate`, `click_and_wait`, l’attente CSS et `window.fetch` fonctionnent
   dans une même `BrowserPageSession` persistante sans régression ;
3. ces deux résultats sont identiques fonctionnellement sur macOS, Linux et
   Windows ;
4. `HttpProxyConfig::Arachnea` utilise le core et un `ClientContext` par requête
   sans démarrer de listener loopback ni exposer les paramètres de route au site
   ou au navigateur comme en-têtes proxy ;
5. le coût mesuré (RSS, CPU, démarrage et distribution) est inférieur au chemin
   Chromium selon des seuils décidés avant le test ;
6. une version fixe, ses artefacts, licences transitives et plateformes sont
   intégrables dans la politique de livraison Arachnea ;
7. les limitations connues sont configurables et visibles, sans fallback
   navigateur implicite.

En cas d’échec des critères de page/Turnstile, la décision correcte reste :
**Ghostwire/rquest pour les flux légers, Chromium facultatif pour les flux qui
nécessitent un navigateur**, plutôt qu’un faux remplacement total.

## Références vérifiées le 2026-09-11

- Code local : `server/crates/arachnea-http/src/engine/chaser_cf.rs`,
  `src/engine/mod.rs`, `src/browser.rs`, `src/engine/ghostwire.rs`,
  `Cargo.toml` et `Cargo.lock`.
- Dépôt Obscura : README, manifeste du workspace, métadonnées et releases du
  dépôt `h4ckf0r0day/obscura` ; release la plus récente observée : `v0.2.2`,
  2026-09-05.
- crates.io : le crate `obscura` publié est distinct du navigateur Obscura et
  décrit une bibliothèque de raytracing.
- Dépôts et documentations officielles des alternatives : Servo (moteur Rust,
  MPL-2.0), Lightpanda (navigateur Zig/CDP, AGPL-3.0) et Wry (WebViews système,
  Apache-2.0/MIT).
- Code local proxy : `ArachneaProxyCore`, `ArachneaTowerService`,
  `SimpleHttpClient`, `ArachneaRquestLoopback` et `PreparedProxyRuntime`.
- Documentation Boa : `boa_engine` fournit un moteur ECMAScript, non une pile
  navigateur complète.