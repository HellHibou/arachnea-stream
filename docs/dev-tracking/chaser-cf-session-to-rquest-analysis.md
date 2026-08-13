5 000
# Analyse : transfert de session `chaser-cf` à `rquest`

Dates : 2026-08-13

## Portée révisée

Le moteur `chaser-cf` ne doit avoir qu'une seule responsabilité :

1. appelez l'API publique `chaser_cf::ChaserCF::solve_waf_session` ;
2. renvoie un ensemble de cookies `cf_clearance` valide et l'agent utilisateur observé par le navigateur ;
3. laissez le client Arachnea existant transmettre ces valeurs au cookie partagé et 
caches d'agent utilisateur.

`rquest` doit alors effectuer la requête HTTP réelle et obtenir le code HTML. Le
le moteur du navigateur ne doit plus renvoyer la page HTML, réessayez une navigation dans le navigateur pour
HTML, ou implémentez Cloudflare/Turnstile en attendant et en cliquant localement.

## État actuel

Le client dispose déjà de la plupart des mécanismes de transfert souhaités :

```texte
actualisation du solveur de navigateur 
-> EngineResponse (Set-Cookie, x-arachnea-solver-user-agent) 
-> store_response_cookies / store_cloudflare_solver_metadata 
-> Cache de cookies partagé + cache d'agent utilisateur limité à l'origine 
-> requête_moteur 
-> requête avec Cookie et User-Agent 
-> Réponse HTML
```

`ArachneaHttpClient::engine_request` obtient les cookies correspondants du partage
cache de cookies et recherche l'agent utilisateur du solveur stocké pour l'origine de la demande.
`base_headers` installe à la fois en tant que `Cookie` et `User-Agent`, et `RquestEngine`
envoie tous les en-têtes normalisés inchangés. La gestion de la redirection est déjà effectuée
par la façade et stocke les cookies de réponse à chaque saut.

Par conséquent, `HttpRequestMode::CloudflareBrowser` est déjà proche du nouveau
comportement cible : il s'actualise en cas de besoin, puis appelle `rquest`.

Deux voies entrent en conflit avec le champ d’application révisé :

- `ChaserCfEngine::send` ouvre sa propre page de navigateur, implémente le défi local 
interroger/cliquer et renvoie un corps collecté par le navigateur ; et
- après l'échec d'un transfert `rquest`, `ArachneaHttpClient` essaie 
`execute_with_browser_cloudflare_engine`, qui appelle `engine.send` pour récupérer 
HTML dans un navigateur.

Le « ChaserCfEngine » actuel possède également « BrowserManager » directement plutôt que
la façade publique « ChaserCF ». Cela contourne la limite de dépendance prévue.

## Architecture cible

```texte
demande en mode Auto ou CloudflareBrowser 
-> demande (tentative initiale, Auto uniquement) 
-> Blocage Cloudflare ou autorisation manquante/périmée 
-> ChaserCF :: solve_waf_session (url, proxy) 
-> EngineResponse (Set-Cookie + user-agent solveur ; corps vide) 
-> Caches partagées Arachnea 
-> requête (Cookie + User-Agent solveur exact) 
-> HTML / redirections / gestion des réponses ordinaires
```

Le résultat d’une opération de poursuite réussie est une session, pas un document. Un
Le corps vide de `EngineResponse` est intentionnel à la fois pour `refresh_cloudflare` et
`refresh_cloudflare_fresh`.

Si la requête « rquest » de post-résolution est toujours bloquée par Cloudflare, le client doit
renvoie « CloudflareBlocked » après sa politique d'actualisation limitée existante. Il faut
ne pas recourir à une requête HTML du navigateur. Cela entraîne un échec de transfert de cookie
visible au lieu de le masquer avec un transport différent.

## Avantages

- `chaser-cf` possède sa propre logique de timing de défi et d'interaction.
- Arachnea ne duplique plus la traversée du Turnstile CDP ou une boucle de clic.
- Un transport (`rquest`) possède le HTML normal, les redirections et la demande d'application 
sémantique.
- Le cache existant et le transfert de l'agent utilisateur à l'origine sont réutilisés à la place de 
introduire un autre format de session.
- L'échec est déterministe : un transfert d'autorisation accepté conduit à une "demande" 
HTML ; un transfert rejeté reste un échec Cloudflare signalé.

## Contraintes et risques

### La parité des empreintes digitales du navigateur n'est pas complète

Passer `cf_clearance` et l'agent utilisateur exact du navigateur est nécessaire mais peut
ne sera pas suffisant. Cloudflare peut lier l'autorisation à d'autres empreintes digitales et
propriétés du réseau. `rquest` doit utiliser la même route proxy/IP sortante que le
résolution du navigateur ; sinon, le transfert devrait échouer. Même avec le même
route, certaines cibles peuvent rejeter une empreinte digitale TLS/indice client non-navigateur.

Il s'agit d'une limitation opérationnelle attendue de la conception révisée, et non d'une raison
pour réintroduire la solution de secours HTML du navigateur. Il devrait être présenté comme
`CloudflareBlocked` sans valeurs de cookie dans les journaux.

### La gestion du proxy nécessite une implémentation explicite

`RquestEngine` applique déjà la route proxy configurée. Le courant
`ChaserCfEngine` crée chaque contexte de navigateur avec `Aucun`, donc il ne passe pas
le proxy du réseau Arachnea vers `chaser-cf` aujourd'hui. La migration doit cartographier un
URL `HttpProxyConfig::Network` prise en charge vers `chaser_cf::ProxyConfig` et utilisez-la
pour `solve_waf_session`, ou rejetez la configuration comme non prise en charge.

`HttpProxyConfig::Arachnea` nécessite également une décision délibérée de compatibilité.
Les deux requêtes ne peuvent partager l'identité requise que si Chaser-cf peut utiliser le
même itinéraire de bouclage géré. Il ne doit pas être résolu directement et récupéré via
un mandataire.


### Modification des en-têtes de navigation du solveur personnalisé

L'API publique `solve_waf_session` accepte uniquement les URL et les proxy. Il ne peut pas porter
les en-têtes de navigation arbitraires actuels ou le référent explicite. Dans ce cadre,
les en-têtes s'appliquent uniquement à la requête `rquest` suivante. Si une cible nécessite
un référent spécifique lors du challenge Cloudflare lui-même, c'est-à-dire un référent en amont
Lacune de l'API `chaser-cf` et devrait produire une limitation claire plutôt qu'une restauration
le solveur du navigateur local.

### Les sessions de pages de navigateur persistantes sortent du nouveau champ d'application de ce moteur

`BrowserPageSession` est utilisé pour les récupérations JavaScript à l'échelle d'une page et
jetons de rappel Turnstile au niveau de l’application. Le public `chaser-cf` 0.2.1 n'a pas
API de page persistante. Par conséquent `ChaserCfEngine::open_browser_page_session`
ne peut pas rester pris en charge après cette migration.

Tous les workflows nécessitant une page persistante doivent utiliser un autre navigateur explicite
moteur (par exemple le moteur Tauri/Wry existant, le cas échéant), ou être
refactorisé séparément. Il s'agit d'un changement de comportement contractuel que les appelants doivent
être informé.

## Approche de mise en œuvre

1. Remplacez la propriété `BrowserManager` dans `ChaserCfEngine` par un paresseusement 
Instance `ChaserCF` initialisée à l'aide du `ChaserConfig` existant.
2. Implémentez une petite méthode d'adaptateur qui appelle public 
`solve_waf_session(url, proxy)`, mappe les erreurs à `ChaserCfFailure`, valide 
`cf_clearance`, et convertit la `WafSession` renvoyée en en-têtes de réponse.
3. Conservez le cache de session persistant : il ne met en cache que les en-têtes de cookies synthétisés 
et le couple utilisateur-agent, qui reste valable pour cette conception.
4. Faites en sorte que `refresh_cloudflare` et `refresh_cloudflare_fresh` utilisent cet adaptateur. 
Les deux renvoient « StatusCode :: OK » et un corps vide.
5. Faites en sorte que `ChaserCfEngine::send` renvoie `UnsupportedEngineOperation` (ou supprimez 
son remplacement si le contrat de trait est ajusté) car ce moteur n'est pas 
plus un transport HTML.
6. Faites en sorte que `open_browser_page_session` renvoie `UnsupportedEngineOperation` pour 
`chaser-cf` ; supprimez `ChaserCfPageSession` et ses assistants réservés au navigateur.
7. Supprimez `execute_with_browser_cloudflare_engine` de `Auto` et 
Chemins d'échec de « CloudflareBrowser », y compris sa réponse finale du navigateur 
repli. Conservez l'actualisation/nouvelle tentative limitée existante via `rquest`.
8. Mettez à jour le README, `CHANGELOG.md` et `docs/TODO.md` pour indiquer que Chaser-cf 
est un solveur de session uniquement et que les sessions de page de navigateur nécessitent un autre 
moteur.

## Plan de validation

Utilisez uniquement une cible autorisée et supprimez toutes les valeurs des cookies.

1. Vérifiez qu'une requête CloudflareBrowser sans autorisation mise en cache appelle 
`solve_waf_session` une fois, stocke `cf_clearance` et l'agent utilisateur du solveur, puis 
obtient le HTML via `rquest`.
2. Vérifiez que la requête `rquest` générée contient le `Cookie` correspondant et 
valeurs exactes du solveur `User-Agent`, sans enregistrer le cookie.
3. Vérifiez que « Auto » commence par « rquest », s'actualise uniquement sur une petite annonce. 
Blocage Cloudflare, puis nouvelle tentative via « rquest ».
4. Vérifiez qu'un transfert rejeté se termine par « CloudflareBlocked » et ne déclenche pas de 
Interaction GET du navigateur ou défi CDP local.
5. Vérifiez que le mode direct ne lance jamais le solveur ; vérifier la réutilisation des sessions mises en cache, 
expiration et rafraîchissement forcé.
6. Vérifiez que l'identité du proxy est la même pour la résolution et la récupération, ou qu'un 
la configuration du proxy est rejetée avant l'envoi du trafic.

## Décision de portée requise avant la mise en œuvre

Cela supprime le repli direct HTML du navigateur et le persistant `chaser-cf`
capacité de la page, ce qui modifie le comportement du public. Une confirmation est requise
avant la mise en œuvre selon les règles du référentiel.