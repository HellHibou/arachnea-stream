# Analyse : transfert de session `chaser-cf` vers `rquest`

Date : 2026-08-13

## Scope retenu

`chaser-cf` a désormais deux usages complémentaires dans `arachnea-http` :

### 1. Résolution de session Cloudflare (transfert vers `rquest`)

L'API publique `ChaserCF::solve_waf_session` résout le challenge Cloudflare et
retourne :

- les cookies de la session, dont `cf_clearance` ;
- le `User-Agent` observé dans le navigateur.

`arachnea-http` enregistre ensuite ces données dans ses caches partagés. `rquest`
effectue la requête HTTP réelle, suit les redirections via la façade existante et
récupère le HTML. Un échec du transfert après les reprises bornées reste un
`CloudflareBlocked`; aucun repli HTML navigateur implicite n'est réintroduit sur
ce chemin.

```text
chaser-cf solve_waf_session
  -> Set-Cookie + User-Agent
  -> caches partagés Arachnea
  -> rquest (Cookie + User-Agent)
  -> HTML ou CloudflareBlocked
```

### 2. Session de page persistante (`BrowserPageSession`)

`open_browser_page_session` est de nouveau supporté par le moteur `chaser-cf`. Il
retourne une `ChaserCfPageSession` conservant un navigateur Chrome dédié pour les
workflows qui ont besoin d'une vraie page : navigation, `fetch` same-page,
clic puis attente d'un sélecteur, et lecture du jeton Turnstile.

La page persistante **ne résout jamais elle-même** un challenge Cloudflare :
chaque navigation résout d'abord une session fraîche via la façade publique
`ChaserCF::solve_waf_session`, puis rejoue le `User-Agent` observé et les cookies
de clearance sur cette page retenue avant d'exécuter la navigation. Aucune
logique de défi (`wait_for_clearance`, clic CDP, stabilité du DOM) n'est
réimplémentée dans `arachnea-http`.

## État de l'implémentation

### Moteur `ChaserCfEngine`

Le moteur utilise la façade publique `ChaserCF` et transforme le `WafSession` en
en-têtes `Set-Cookie` et en métadonnée interne
`x-arachnea-solver-user-agent`. Le chemin `send` assemble une réponse HTML
directe pour `GET`/`HEAD` via `get_source` (transport direct optionnel).

### Session de page persistante

Un navigateur lazy dédié (`page_browser`) est créé pour les sessions de page,
distinct du navigateur de la façade. Chaque `open_browser_page_session` :

1. obtient le `BrowserManager` dédié ;
2. crée un contexte de navigation avec le proxy de l'engine
   (`create_context(self.proxy.as_ref())`) pour partager la route proxy avec le
   handoff `rquest` ;
3. ouvre une page `about:blank` (`new_page`) ;
4. installe le hook Turnstile sur les nouveaux documents.

Le plan de `ChaserCfPageSession::navigate` est :

1. résoudre une session fraîche via `ChaserCF::solve_waf_session` ;
2. vérifier que la session contient bien un cookie `cf_clearance` ;
3. appliquer le `User-Agent` observé via CDP (`EmulationSetUserAgentOverride`) ;
4. injecter les cookies de la session via CDP (`SetCookiesParams` /
   `CookieParam`) ;
5. appliquer les en-têtes de requête personnalisés (le `Referer` passe par le
   champ natif de navigation de Chrome) ;
6. naviguer et retourner l'URL finale et, si demandé, le corps HTML.

`fetch` exécute un `fetch` natif same-origin dans la page via l'évaluation JS.
`click_and_wait` sonde le DOM jusqu'à l'apparition du sélecteur cible, borné par
le timeout du moteur. `metadata` rend les cookies observés et le `User-Agent`
au cache partagé. La lecture du jeton Turnstile utilise la variable globale
capturée par le hook installé à l'ouverture.

La session retenue garde son `BrowserManager` vivant tant qu'elle existe, de
sorte qu'un client configuré temporairement ne peut pas fermer Chrome pendant la
validité de la session.

## Symptômes observés pendant les essais

Le CAPTCHA se résout correctement, mais deux erreurs peuvent encore apparaître :

1. `CONNECT brunhild.challenges.cloudflare.com:443` échoue avec l'erreur DNS
   Windows `11004` ;
2. le CAPTCHA est résolu, mais la requête HTML suivante reçoit toujours une page
   Cloudflare et se termine par `CloudflareBlocked`.

Les fermetures Windows `10054` sont généralement une conséquence du navigateur
qui abandonne les connexions de ressources après l'échec d'un `CONNECT`. Elles
ne constituent pas la cause initiale.

## Analyse du proxy loopback

### Cause

Le coeur `arachnea-proxy` choisit un proxy dynamique avec le contexte client,
notamment le paramètre `country`. `rquest` envoie ce paramètre au proxy loopback
par l'en-tête `Arachnea-Proxy-Country`. Chrome, lancé par `chaser-cf`, ne connaît
qu'une URL de proxy et ne peut pas ajouter cet en-tête à ses requêtes `CONNECT`.

Partager une simple URL `http://127.0.0.1:<port>` n'assure donc pas que Chrome
et `rquest` emploient la même route. Chrome atteignait le listener sans contexte
de pays et pouvait prendre une route directe ou une sélection dynamique
différente. L'échec DNS vers le sous-domaine de challenge est ainsi un symptôme
de route incorrecte, pas un échec de la résolution du CAPTCHA.

### Correctif implémenté

Deux listeners loopback sont nécessaires :

```text
rquest  -> listener normal + paramètres dans les en-têtes de chaque requête
chaser  -> listener dédié + ClientContext fixé au démarrage
```

`ArachneaRquestLoopback::start_with_parameters` construit un `ClientContext`
avec les définitions de paramètres du coeur proxy. Le nouveau handler
`handle_with_client_context` transmet ce contexte aux requêtes `CONNECT` et aux
requêtes HTTP absolues. Les en-têtes éventuels de la requête gardent priorité
sur le contexte fixé. Chrome peut ainsi utiliser la route de pays demandée sans
connaître le protocole d'en-têtes interne.

Cette correction doit être compilée et le service doit être entièrement
redémarré. Des traces montrant encore un `CONNECT` sans route pays peuvent venir
d'un exécutable non reconstruit ou d'une requête sans `proxy_country` configuré.

## Limite fondamentale du transfert Chrome vers `rquest`

Transmettre `cf_clearance` et le `User-Agent` est nécessaire, mais n'est pas une
garantie que Cloudflare acceptera `rquest`. La version publique 0.2.1 de
`chaser-cf` ne fournit que les cookies et `navigator.userAgent`; elle ne fournit
ni un contexte Chrome réutilisable, ni l'ensemble des en-têtes de navigation.

Cloudflare peut également lier une clearance à d'autres propriétés : IP de
sortie, empreinte TLS/JA3, réglages HTTP/2, ordre des en-têtes, Client Hints ou
comportement navigateur. `rquest` conserve le cookie et le `User-Agent`, mais
ne devient pas Chrome. Un `CloudflareBlocked` après une résolution réussie —
notamment sur PapaDuStream — peut donc signaler une incompatibilité d'empreinte
et non pas l'absence du cookie dans le cache.

Le point déterminant est d'abord l'IP : le solveur et `rquest` doivent employer
la même route proxy effective. Avec un pool dynamique, cette identité doit aussi
rester stable entre la résolution et la requête HTML. La session de page
persistante hérite de la même exigence : son contexte de navigation est créé
avec le proxy de l'engine pour rester sur la même route que le solveur.

## Cache de sessions et proxy dynamique

Le cache persistant actuel est indexé uniquement par origine. C'est risqué avec
un pool de proxys dynamique : une clearance obtenue depuis une IP peut être
réutilisée plus tard depuis une autre IP. Cette réutilisation peut provoquer un
blocage qui ressemble à une défaillance de `rquest`.

Le cache doit donc être isolé par identité de route effective, ou désactivé pour
le routage dynamique tant qu'il n'existe pas de clé de route stable. La simple
URL du listener loopback n'est pas suffisante : elle change par client et ne
représente pas le proxy dynamique réellement sélectionné.

## Diagnostics requis

Les journaux ne doivent jamais contenir les valeurs de cookie. Ils doivent
permettre de vérifier les faits suivants :

- origine demandée ;
- type de proxy et noms des paramètres liés au listener `chaser-cf` ;
- présence (sans valeur) de `cf_clearance` et du `User-Agent` mémorisé ;
- route/selection proxy utilisée pour le `CONNECT` navigateur et pour `rquest` ;
- raison compacte de détection Cloudflare après la reprise.

## Étapes de validation

1. Recompiler puis redémarrer complètement le service avec le listener
   paramétré.
2. Effectuer une requête protégée avec `proxy_country: BE` et vérifier que les
   `CONNECT` de Chrome et les requêtes `rquest` utilisent la même route BE.
3. Vérifier que `cf_clearance` et le `User-Agent` sont présents dans la requête
   `rquest` sans en exposer les valeurs dans les logs.
4. Faire un second appel sur la même origine : il ne doit pas réutiliser une
   clearance appartenant à une autre route dynamique.
5. Ouvrir une session de page persistante (`page_fetch` / `page_click`) et
   vérifier que la navigation rejoue la session fraîche, injecte cookies et
   `User-Agent` via CDP, et conserve la route proxy de l'engine.
6. Si route, cookie et `User-Agent` sont corrects mais que la cible renvoie
   toujours Cloudflare, qualifier l'échec comme incompatibilité d'empreinte
   transport. Les options sont alors une émulation TLS/HTTP2 Chrome dans
   `rquest` ou un moteur navigateur explicitement choisi pour le HTML ; cette
   dernière option est hors du scope validé pour `chaser-cf`.
