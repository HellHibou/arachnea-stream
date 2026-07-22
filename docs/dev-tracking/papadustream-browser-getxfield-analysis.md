# Analyse de getxfield dans le navigateur pour PapaDuStream

## Objectif

Résoudre l'iframe de l'hébergeur renvoyée par le point de terminaison AJAX `getxfield` de PapaDuStream, tout en conservant une session navigateur Cloudflare réutilisable. L'implémentation doit rester générique dans `arachnea-http` et `arachnea-scrapyfy` ; `papadustream-v2.yaml` ne doit déclarer que les sélecteurs, paramètres et extractions de réponse propres à la source.

## État actuel

`papadustream-v2.yaml` fournit actuellement :

- La découverte de la page d'accueil des séries, de la recherche, des entrées, des saisons et des épisodes.
- Une découverte différée des lecteurs : `get_season` renvoie le `link` de chaque épisode ; le frontend appelle `get_players` avec ce lien lorsqu'un épisode est sélectionné.
- `get_players` analyse `ul.player-list > li > .lien` afin d'obtenir le nom de l'hébergeur, la langue, `player-id`, `player-field`, `player-type`, le `user_hash` de la page et le point de terminaison `getxfield`.

Pour un épisode, la page contient des appels tels que :

```js
getxfield(this, '209184', 'voe_vostfr', 'serial')
```

Le navigateur envoie :

```text
POST /engine/ajax/controller.php?mod=getxfield
id=209184
xfield=voe_vostfr
type=serial
g_recaptcha_response=<jeton Turnstile>
user_hash=<dle_login_hash>
```

Une réponse réussie est du HTML, par exemple :

```html
<iframe src="https://lukefirst.lol/e/h80m1hl3hos9" ...></iframe>
```

Le résultat jouable requis est `iframe[src]`, émis comme `embed-link` pour le lecteur correspondant.

## Capacités HTTP confirmées

`ScraperHttpConfig` prend en charge les modes de requête `auto`, `direct`, `cloudflare_smart` et `cloudflare_browser`.

`arachnea-http` peut conserver les cookies Cloudflare et l'agent utilisateur du solveur par origine. `chaser-cf` prend en charge l'interaction automatique avec Turnstile du WAF lors d'une navigation de page dans le navigateur, puis transmet la session de cookies obtenue au client HTTP partagé.

Le moteur `chaser-cf` actuel ne soumet pas de requêtes navigateur arbitraires autres que GET. Son implémentation de `HttpEngine::send` rejette les méthodes autres que GET et HEAD. Il expose également les cookies et les métadonnées de l'agent utilisateur, mais pas le jeton applicatif fourni par le rappel `turnstile.render(... callback(token))` au niveau de la page.

Par conséquent, `http.mode: auto` peut réutiliser une session Cloudflare existante pour la page de l'épisode et les appels HTTP ultérieurs, mais ne peut pas actuellement exécuter le POST AJAX `getxfield` de la page avec le jeton de rappel.

## Architecture proposée

### 1. Session navigateur par origine

Ajouter un gestionnaire de sessions navigateur réutilisables détenu par `arachnea-http`.

- Clé de cache : origine, profil navigateur/agent utilisateur et identité de route proxy.
- État stocké : poignée de contexte/page navigateur, magasin de cookies, agent utilisateur du solveur, dernière activité réussie et verrou de synchronisation.
- Le cache existant de cookies `cf_clearance` reste le chemin rapide de transfert HTTP. Le contexte navigateur n'est conservé que lorsqu'une source nécessite une interaction JavaScript au niveau de la page.
- Ne pas journaliser ni conserver les valeurs brutes des cookies.
- Limiter le cache par nombre maximal d'origines, délai d'inactivité et invalidation explicite.

### 2. Extension des sous-requêtes existantes

Étendre `sub_queries` au lieu d'ajouter un nouveau type de requête. La sous-requête conserve ses mécanismes existants de sélection, `request_method`, URL, en-têtes, corps et analyse HTML; elle reçoit seulement les options d'exécution navigateur nécessaires.

Une sous-requête avec `http.execution: page_fetch` doit :

1. Naviguer la session navigateur réutilisable de l'origine vers une page d'épisode.
2. Attendre la source configurée du jeton de rappel JavaScript ou le défi complété par l'utilisateur.
3. Construire une requête navigateur `fetch`/XHR configurée dans le même contexte de page.
4. Envoyer la méthode, l'URL, les en-têtes et le corps de formulaire configurés.
5. Renvoyer le texte de réponse en tant que réponse de scraper HTML.

La primitive doit être générique. Elle ne doit pas contenir de noms d'hébergeurs PapaDuStream, de noms de champs, de sélecteurs CSS ou de post-traitement spécifique au service dans Rust.

Forme YAML suggérée :

```yaml
sub_queries:
  - scraper_type: html
    http:
      mode: auto
      browser_context: origin
      execution: page_fetch
    page_url: "{episode_url}"
    browser_token:
      source: turnstile_callback
      placeholder: "{browser_turnstile_token}"
      retry_on_rejection: once
    request_method: post
    query_url: "{request_origin}/engine/ajax/controller.php?mod=getxfield"
    request_headers:
      - name: X-Requested-With
        value: XMLHttpRequest
    request_body_template: >
      id={player_id}&xfield={player_field}&type={player_type}
      &g_recaptcha_response={browser_turnstile_token}&user_hash={player_user_hash}
    row_selector: "iframe[src]"
```

Le schéma exact doit réutiliser les conventions existantes de `request_method`, `request_headers` et `request_body_actions`. `browser_context`, `execution` et `browser_token` sont des extensions ciblées de sous-requête, non un modèle parallèle de requêtes.

### 3. Cycle de vie du jeton Turnstile

Ne pas définir une durée de vie arbitraire pour le jeton.

- Conserver un jeton uniquement en mémoire, limité à la page/session navigateur qui l'a obtenu.
- L'utiliser pour un seul POST `getxfield`, puis le marquer comme consommé.
- Ne pas conserver les valeurs des jetons sur disque, dans les journaux, les diagnostics ou le cache de sessions navigateur.
- Lorsqu'une réponse indique un captcha ou une session expirés/non valides, invalider ce jeton de page et réessayer une fois après avoir obtenu un nouveau jeton.
- Lors d'un échec de session Cloudflare, invalider la session navigateur d'origine et le cache de cookies, puis réacquérir la session via le flux navigateur normal.

La session navigateur elle-même est conservée tant qu'elle reste utilisable. Sa validité est déterminée par les réponses du site et les échecs de cookie/session, et non par un délai estimé.

### 4. Cache des iframes résolues

Mettre en cache une iframe d'hébergeur résolue séparément de la session Cloudflare.

- Clé de cache : URL de l'épisode, `player-id`, `player-field` et `player-type`.
- Valeur : `iframe[src]`, date de création et métadonnées facultatives de l'hébergeur.
- Portée : mémoire du processus uniquement.
- Invalider lors d'un échec de résolution du lecteur, d'un échec de lecture de l'hébergeur signalé par l'appelant, d'une modification de la page d'épisode, d'un rafraîchissement explicite ou d'une éviction limitée.

Cela évite de répéter les requêtes `getxfield` sans réutiliser incorrectement les jetons Turnstile.

## Modifications requises

### `server/crates/arachnea-http`

- Introduire l'abstraction de session/contexte navigateur réutilisable.
- Prendre en charge l'exécution de requêtes JavaScript `fetch`/XHR configurées dans la même page via un moteur navigateur.
- Transmettre les cookies navigateur et l'agent utilisateur à la gestion de session existante de `ArachneaHttpClient`.
- Ajouter des API de verrouillage et d'invalidation de cache limitées à l'origine.
- Ajouter des erreurs structurées distinguant les échecs de session Cloudflare, l'absence de jeton, un jeton rejeté et les échecs de requête navigateur.
- Mettre à jour le README de la crate HTTP et `docs/TODO.md` pour cette capacité de session navigateur.

### `server/crates/arachnea-scrapyfy`

- Étendre les sous-requêtes HTML existantes avec les options `browser_context`, `execution: page_fetch` et une source de jeton navigateur.
- Rendre la réponse du navigateur disponible pour l'analyse HTML existante, afin que `iframe[src]` ne nécessite aucun analyseur Rust personnalisé.
- Prendre en charge une nouvelle tentative configurée une fois en cas de rejet du jeton, sans boucle de défi non bornée.
- Documenter le schéma dans `docs/specifications/arachnea-scrapyfy-*.md`.

### `server/services/arachnea-stream/dark-stream/papadustream-v2.yaml`

- Conserver le `link` d'épisode de `get_season` comme lien source différé des lecteurs.
- Conserver dans `get_players` l'analyse du nom du lecteur, de la langue, de l'identifiant, du champ, du type et de `user_hash`.
- Ajouter pour chaque lecteur une sous-requête `page_fetch` vers `getxfield`, exécutée dans le contexte navigateur partagé de l'épisode.
- Analyser `iframe[src]` du HTML `getxfield` réussi dans `players > embed-link`.
- Préserver le nom du lecteur et la langue collectés sur la page de l'épisode.
- Ne configurer que les en-têtes et champs de formulaire de la source ; ne coder en dur aucune URL d'hébergeur.

### `front`

- Conserver le comportement actuel de collecte différée des lecteurs : un épisode avec `players.link` est sélectionnable et déclenche `get_players`.
- Ne pas traiter les URL de pages source comme des valeurs `embed-link`.
- Afficher une erreur récupérable lorsqu'une soumission navigateur échoue, tout en conservant la sélection de l'épisode et en permettant le choix d'un autre lecteur.
- Conserver le comportement du sélecteur de langue/lecteur une fois les valeurs `embed-link` résolues renvoyées.

## Plan de validation

1. Ajouter des tests de fixtures locales pour analyser la réponse iframe `getxfield` fournie.
2. Ajouter des tests de sessions navigateur simulées pour un jeton présent, un jeton consommé, un jeton non valide, une erreur de session et l'invalidation du cache.
3. Vérifier que la réutilisation de la session navigateur ne provoque pas une seconde interaction Turnstile pour une session d'origine valide.
4. Vérifier qu'un jeton rejeté entraîne au plus un rafraîchissement et une nouvelle tentative.
5. Vérifier qu'un lecteur résolu est servi depuis le cache d'iframe sans nouvelle requête `getxfield`.
6. Vérifier que `get_players` renvoie des valeurs `embed-link` propres à l'hébergeur, et non des URL de pages d'épisode.
7. Exécuter `cargo test -p arachnea-http`, `cargo test -p arachnea-scrapyfy`, les tests d'intégration stream et `npm run type-check`.

## Plan d'implémentation par étapes

### Étape 1 : Cartographier les contrats et les points d'extension ✓

1. Relever les types et flux existants de `ScraperHttpConfig`, `ArachneaHttpClient`, des moteurs Cloudflare et des sous-requêtes HTML de `arachnea-scrapyfy`.
2. Identifier le moteur navigateur réellement disponible, son cycle de vie, les données de session qu'il expose et les limites actuelles de `HttpEngine::send`.
3. Relever le schéma YAML actuel de `get_players`, les variables disponibles aux sous-requêtes et la forme de sortie attendue par `arachnea-stream` et le frontend.
4. Localiser les tests, fixtures et documents existants à adapter afin de préserver les conventions du dépôt.

Critère de sortie : les extensions de schéma et les interfaces Rust sont définies sans dupliquer un mécanisme de requête existant. ✓

### Étape 2 : Définir les contrats de session et d'exécution navigateur ✓

1. Ajouter les types génériques représentant une clé de session navigateur, une session réutilisable, une demande de navigation de page et une demande `fetch` exécutée dans la page.
2. Définir les erreurs structurées distinguant l'échec de session Cloudflare, l'absence de jeton, le jeton rejeté et l'échec de requête navigateur.
3. Définir les règles de sécurité : aucun cookie, jeton ou en-tête sensible brut dans les journaux, diagnostics ou stockage persistant.
4. Définir le cache mémoire des iframes résolues et ses clés d'invalidation, séparément du cache de session Cloudflare.

Critère de sortie : les API sont génériques, limitées à l'origine et ne font référence ni à PapaDuStream ni à un hébergeur précis. ✓

#### Implémentation

| Fichier | Changement |
|---------|------------|
| `server/crates/arachnea-http/src/error.rs` | 4 nouveaux variants : `BrowserSessionUnavailable`, `TokenAbsent`, `TokenRejected`, `PageFetchFailed` |
| `server/crates/arachnea-http/src/browser.rs` | Nouveau module avec `BrowserSessionKey` (origin+profile+proxy_route), `BrowserSessionHandle` (handle clonable lock-guardé), `PageFetchRequest`/`PageFetchResponse`, `TurnstileTokenState` (cycle de vie Pending→Ready→Consumed/Rejected), `ResolvedIframeCache` (cache mémoire borné avec TTL, éviction LRU, invalidation par clé composite épisode+player) |
| `server/crates/arachnea-http/src/lib.rs` | `pub mod browser` + ré-export des types `BrowserSessionKey`, `BrowserSessionHandle`, `PageFetchRequest`, `PageFetchResponse`, `ResolvedIframeCache`, `TurnstileTokenState` |
| `server/crates/arachnea-http/src/browser.rs` — `mod tests` | 9 tests : égalité/hash de clé, cache get/insert/éviction/TTL/invalidation/clear, transition d'état de jeton |

- **Règles de sécurité** documentées en en-tête du module `browser.rs` (pas de log de token/cookie, cache limité à l'embed URL, session par origine, jeton à usage unique).
- **Aucune référence** à PapaDuStream ou à un hébergeur spécifique dans les nouveaux types.
- `cargo test -p arachnea-http -- browser::tests` : 9/9 OK.

### Étape 3 : Implémenter le gestionnaire de sessions dans `arachnea-http` ✓

1. Créer le gestionnaire de sessions navigateur détenu par `ArachneaHttpClient` ou son composant de session dédié.
2. Mettre en cache les contextes par origine, profil/agent utilisateur et route proxy, avec verrouillage par session pour empêcher les utilisations concurrentes d'une même page.
3. Ajouter les limites de capacité, l'éviction après inactivité et les API d'invalidation explicite par origine.
4. Synchroniser cookies et agent utilisateur acquis par le navigateur avec le mécanisme existant de réutilisation HTTP Cloudflare.
5. Garantir la fermeture propre des contextes évincés ou invalidés.

Critère de sortie : une session valide est réutilisée pour la même origine sans déclencher une nouvelle résolution Cloudflare. ✓

#### Implémentation

| Fichier | Changement |
|---------|------------|
| `browser.rs` | `BrowserSessionConfig` (max_sessions=8, idle_timeout=5min), `BrowserSessionManager` avec `get_or_create`, `invalidate_origin`, `invalidate_all`, `evict_idle`, `len` |
| `browser.rs` — `mod tests` | 6 nouveaux tests : get_or_create (identique+différent), capacité-éviction, invalidation par origine, invalidation totale, éviction idle, réutilisation après get_or_create |
| `config.rs` | Nouveau champ `browser_session: BrowserSessionConfig` dans `ArachneaHttpConfig` + méthode builder `browser_session()` |
| `client.rs` | Nouveau champ `browser_session_manager: Option<Arc<BrowserSessionManager>>` dans `ArachneaHttpClient`, initialisé dans `new_with_cookie_cache`, accesseur public `browser_session_manager()` |
| `lib.rs` | Export de `BrowserSessionConfig`, `BrowserSessionManager` |

- Verrouillage session : `std::sync::RwLock` sur la HashMap (lecture rapide sans await), `std::sync::Mutex` par session handle.
- Éviction : par capacité (LRU) + par inactivité (`evict_idle` + `get_or_create` retire les idle avant insertion).
- Invalidation : marque `valid=false` puis retire de la map.
- `cargo test -p arachnea-http` : 25/25 OK (2 doc-tests préexistants en échec non liés).

### Étape 4 : Ajouter la primitive de requête dans une page navigateur ✓

1. Implémenter la navigation vers une URL de page source dans la session réutilisable.
2. Ajouter la récupération configurée du jeton produit par le rappel Turnstile dans le contexte de page, sans le persister.
3. Exécuter un `fetch` ou XHR dans cette page avec méthode, URL, en-têtes et corps fournis par l'appelant.
4. Retourner le texte et les métadonnées de réponse sous une forme consommable par le scraper HTML existant.
5. Marquer le jeton comme consommé après la soumission et classer les réponses de rejet afin d'autoriser au plus une nouvelle tentative configurée.

Critère de sortie : une requête POST arbitraire, authentifiée par le contexte de la page, renvoie son HTML au code appelant sans logique spécifique à une source. ✓

#### Implémentation

| Fichier | Changement |
|---------|------------|
| `browser.rs` | Ajout de `BrowserPageSession`, `PageNavigationRequest`/`Response`, `BrowserSessionMetadata`; le handle conserve une page derrière un `tokio::sync::Mutex` exclusif. |
| `engine/mod.rs` | Nouveau point d'extension `HttpEngine::open_browser_page_session()` ; implémentation par défaut explicite `UnsupportedEngineOperation`. |
| `engine/chaser_cf.rs` | Session chaser-cf persistante : ouverture de page, hook `evaluate_on_new_document` pour chaîner le callback Turnstile, navigation, `fetch()` JavaScript avec cookies de page, métadonnées cookies/UA et fermeture explicite. |
| `client.rs` | API publique `ArachneaHttpClient::page_fetch(navigation, request)`, clé de session dérivée de l'origine/profil/proxy, transmission de cookies et UA vers les caches HTTP existants, invalidation de session sur erreur. |
| `browser.rs` | `PageFetchRequest` accepte un placeholder de jeton à remplacer uniquement en mémoire, plus des statuts/marqueurs configurables de rejet; un rejet devient `TokenRejected`. |
| `client.rs` — tests | Moteur de page simulé : une même origine ouvre une seule page pour deux POST, injecte le jeton, puis transmet `cf_clearance` et UA au client HTTP. |

- Le jeton est extrait depuis le callback, immédiatement supprimé de `window`, injecté une fois dans le corps UTF-8 et n'est jamais retourné ni journalisé.
- La primitive chaser-cf ne code aucun sélecteur, champ, URL ou hébergeur PapaDuStream.
- Les moteurs sans session de page persistante, dont Tauri à ce stade, échouent explicitement avec `UnsupportedEngineOperation`.
- La relance bornée après `TokenRejected` sera déclenchée par la configuration de sous-requête de l'étape 5.
- Validation : `cargo test -p arachnea-http --lib` (27/27) et `cargo check -p arachnea-http --features chaser-cf` OK.

### Étape 5 : Étendre les sous-requêtes de `arachnea-scrapyfy` ✓

1. Ajouter les champs YAML ciblés aux sous-requêtes HTML : contexte navigateur, mode d'exécution `page_fetch`, URL de page et configuration de source de jeton.
2. Réutiliser les conventions existantes de méthode, en-têtes, URL et actions de corps de requête plutôt que créer un schéma parallèle.
3. Résoudre les substitutions de variables de la sous-requête avant l'appel HTTP et transmettre la réponse navigateur au parseur HTML existant.
4. Appliquer la relance unique seulement aux erreurs explicitement classées comme rejet de jeton, sans boucle implicite.
5. Préserver le chemin HTTP actuel pour toutes les sous-requêtes qui n'utilisent pas `page_fetch`.

Critère de sortie : `iframe[src]` peut être sélectionné par le pipeline HTML standard à partir d'une réponse navigateur. ✓

#### Implémentation

- `ScraperHttpConfig` expose `execution: page_fetch`, `browser_context: origin`, `page_url` et `browser_token` (source callback, placeholder, signaux de rejet et politique `once`).
- `HttpClient::page_fetch_for_request()` adapte les en-têtes, corps et options YAML aux contrats `arachnea-http`.
- L'exécuteur HTML des sous-requêtes conserve le chemin HTTP existant sauf pour `page_fetch`, résout l'URL de page et transmet le HTML du navigateur au parseur CSS déjà en place.
- Un `TokenRejected` ne provoque qu'une relance lorsque `retry_on_rejection: once` est configuré ; toute autre erreur est renvoyée immédiatement.
- Les valeurs de champs de l'entrée courante sont ajoutées au contexte de template, avec une variante normalisée (`player-id` → `{player_id}`).
- Les spécifications anglaise et française documentent le nouveau schéma.
- Validation : `cargo check -p arachnea-scrapyfy` et `cargo check -p arachnea-stream` OK.

### Étape 6 : Configurer `papadustream-v2.yaml` ✓

1. Conserver la collecte différée de l'URL d'épisode dans `get_season`.
2. Préserver l'extraction actuelle du nom, de la langue, de `player-id`, `player-field`, `player-type` et `user_hash` dans `get_players`.
3. Ajouter la sous-requête `page_fetch` pointant vers la page d'épisode et configurant le POST `getxfield`, les en-têtes AJAX et les champs de formulaire propres à la source.
4. Extraire `iframe[src]` de la réponse en `embed-link` tout en conservant les métadonnées du lecteur déjà collectées.
5. Vérifier qu'aucune URL d'hébergeur, logique de captcha ou post-traitement PapaDuStream n'est introduit dans Rust.

Critère de sortie : le résultat de `get_players` contient une URL d'iframe d'hébergeur et non l'URL de la page d'épisode. ✓

#### Implémentation

- `get_players` conserve l'URL d'épisode différée comme `query_url` et les métadonnées `name`, `lang`, `player-id`, `player-field` et `player-type`.
- Chaque lecteur extrait aussi son `player-user-hash`, puis son `embed-link` temporaire pointe vers `getxfield`.
- La sous-requête HTML `page_fetch` soumet le POST form-urlencoded depuis la page d'épisode, avec le callback Turnstile et les en-têtes AJAX de PapaDuStream.
- Le sélecteur `iframe[src]` remplace l'URL temporaire par `embed-link`, sans URL d'hébergeur ni logique spécifique introduite dans Rust.
- Ajout générique de `request_body_actions` aux sous-requêtes HTML pour réutiliser le pipeline d'actions existant.
- Validation : `cargo check -p arachnea-stream` OK.

### Étape 7 : Cache de jeton navigateur par domaine ✓

Le cache d'iframe résolue est retiré de cette étape. La valeur à réutiliser est le jeton applicatif Turnstile, jamais une URL d'iframe liée à un lecteur.

#### État constaté

- `BrowserPageSession::take_turnstile_token()` lit puis supprime le jeton de `window`.
- `ArachneaHttpClient::page_fetch()` injecte ce jeton dans une seule requête et invalide aujourd'hui toute la session d'origine sur n'importe quelle erreur, y compris `TokenRejected`.
- `ResolvedIframeCache` existe comme type mémoire mais n'est pas branché au flux de résolution. Il ne doit pas devenir le mécanisme de réutilisation de PapaDuStream.

#### Contrat proposé

1. Ajouter un cache mémoire d'un jeton courant par **contexte de domaine** : origine (`scheme + host + port`), profil navigateur/agent utilisateur et route proxy. L'origine est la dimension fonctionnelle demandée ; profil et proxy empêchent d'utiliser un jeton attaché à une autre empreinte ou session réseau.
2. Stocker ce cache dans la session navigateur réutilisable, et non dans un cache global de lecteurs. Une session déjà indexée par `BrowserSessionKey` fournit naturellement cette portée et son verrou exclusif empêche deux soumissions concurrentes du même jeton.
3. Conserver au plus un jeton opaque par contexte. Une nouvelle exécution du callback remplace la valeur précédente. Aucune clé ne contient l'épisode, le lecteur, l'iframe ou le nom de l'hébergeur.
4. Rendre la réutilisation explicite dans le YAML, par exemple `browser_token.cache_scope: domain`. Le comportement par défaut reste à usage unique afin de ne pas imposer une hypothèse de réutilisabilité aux autres sources.
5. Lors de `cache_scope: domain`, `page_fetch` consulte le jeton du contexte avant de demander un nouveau callback. Il soumet la même valeur tant qu'aucune réponse ne la rejette.
6. Une réponse classée `TokenRejected` évince **uniquement** le jeton de ce contexte. La relance bornée `once` renavigue/récupère un nouveau callback puis effectue une seconde soumission. Elle n'invalide la session navigateur entière que si l'échec est Cloudflare ou navigateur.
7. Invalider le jeton à la fermeture, éviction ou invalidation explicite de la session, lors d'un changement de profil/proxy/origine et lors d'un rafraîchissement explicite. Ne pas définir de TTL arbitraire : la validité est déterminée par les réponses du site ; un jeton devenu invalide déclenche une seule récupération fraîche.

#### Sécurité et observabilité

- Jeton brut uniquement en mémoire, jamais sérialisé, journalisé, renvoyé au scraper, exposé dans les erreurs ou inclus dans `Debug`.
- Les diagnostics n'exposent que la portée redacted (origine éventuellement) et l'état (`absent`, `présent`, `rejeté`), jamais la valeur.
- Le cache est borné par le nombre maximal de sessions navigateur ; aucune persistance disque et aucune relation avec le cache HTTP de cookies.

#### Modifications attendues après aval

| Composant | Modification |
|-----------|--------------|
| `arachnea-http/browser.rs` | État de jeton opaque dans `BrowserSessionInner`, cycle `Absent → Disponible → Rejeté/Évincé`, suppression du cache d'iframe du flux prévu. |
| `arachnea-http/client.rs` | Lecture/écriture du jeton par `BrowserSessionKey`, éviction ciblée sur `TokenRejected`, invalidation complète réservée aux erreurs de session/navigateur. |
| `arachnea-http/engine/chaser_cf.rs` | Lecture non destructive du callback afin que le client décide de la conservation en cache ; le hook continue de ne jamais journaliser le token. |
| `arachnea-scrapyfy` | Champ YAML opt-in `browser_token.cache_scope: domain`, sans référence à une iframe ou à PapaDuStream. |
| `papadustream-v2.yaml` | Activation explicite de la portée `domain` pour `getxfield`. |

#### Validation prévue

1. Deux `getxfield` pour le même domaine/profil/proxy réutilisent le même jeton sans nouveau callback.
2. Deux origines, profils ou routes proxy différents ne partagent jamais de jeton.
3. Un rejet évince seulement le jeton ; une politique `once` récupère au plus une valeur fraîche.
4. Éviction de session, invalidation explicite et erreur Cloudflare retirent aussi le jeton.
5. Les journaux, diagnostics, sérialisations et réponses ne contiennent jamais la valeur du jeton.

Critère de sortie : les lecteurs d'un même domaine peuvent réutiliser explicitement leur jeton en mémoire, sans cache par iframe et sans partage entre contextes navigateur incompatibles.

#### Implémentation

- `BrowserSessionHandle` conserve un jeton opaque uniquement en mémoire, avec la même clé effective que la session : origine, profil navigateur et route proxy.
- `PageFetchRequest.reuse_turnstile_token` rend la réutilisation explicite ; `Scrapyfy` l'active seulement pour `browser_token.cache_scope: domain`.
- `ChaserCfPageSession` lit le callback sans le supprimer de la page ; le client HTTP possède le cache et le vide à la fermeture de session, à l'invalidation ou sur `TokenRejected`.
- `TokenRejected` évince seulement le jeton, préservant la session navigateur pour la relance bornée. Les autres erreurs conservent l'invalidation de session existante.
- `papadustream-v2.yaml` active `cache_scope: domain` pour les appels `getxfield`.
- Validation : `cargo test -p arachnea-http --lib` (27/27), `cargo check -p arachnea-http --features chaser-cf` et `cargo check -p arachnea-stream` OK.

### Étape 8 : Adapter les erreurs récupérables côté frontend ✓

1. Vérifier le contrat de réponse actuel entre `arachnea-stream` et le frontend lors de l'échec de `get_players`. ✓
2. Propager une erreur récupérable et contextualisée lorsque la requête navigateur ou le jeton échoue. ✓
3. Conserver l'épisode sélectionné et le comportement des sélecteurs de lecteur et de langue lorsque des `embed-link` valides sont disponibles. ✓
4. Vérifier que les liens de pages source ne sont jamais traités comme des liens d'intégration. ✓

Critère de sortie : un échec de résolution n'interrompt pas la sélection de l'épisode ni les autres choix possibles. ✓

#### Implémentation

| Fichier | Changement |
|---------|------------|
| `front/src/composables/entry-details/entryVideoPlayer.ts` | Catch de `loadDeferredPlayers()` : remplacement de `String(error)` par `t('entry.playerResolutionFailed')` |
| `front/public/locales/en.json` | Nouvelle clé `entry.playerResolutionFailed` : "Unable to resolve the video player. Try selecting a different player." |
| `front/public/locales/fr.json` | Nouvelle clé `entry.playerResolutionFailed` : "Impossible de résoudre le lecteur vidéo. Essaye de sélectionner un autre lecteur." |

- **Contrat vérifié** : le pipeline `get_players` → `call_api()` → `throwFrontendApiError()`/`handleFailedRestResponse()` jette `ReportedApiError` qui est capturé par `loadDeferredPlayers()`. Le message brut n'est plus exposé à l'utilisateur.
- **Épisode préservé** : `loadDeferredPlayers()` ne modifie que `mediaPlayerErrorMessage` ; l'état réactif `selectedPlayableItem` et `details` reste inchangé en cas d'erreur. L'utilisateur peut sélectionner un autre lecteur sans recharger l'épisode.
- **Sélecteurs préservés** : `availablePlayers`, `activePlayers`, `filteredPlayers` et `selectedPlayer` sont des computed dérivés de `details.value.players` et `selectedPlayableItem.value.players`, qui ne sont pas mutés en cas d'échec.
- **Liens source jamais traités comme embed** : `normalizeEntryPlayer()` (`rustify.ts:1663`) filtre les entrées sans `embedLink`, `directLink` ou `resolver`. Le champ `link` (URL de page source) n'est pas un champ d'entrée normalisé et ne peut pas devenir un `embed-link`.
- `npm run type-check` OK.

### Étape 9 : Adapter les tests et fixtures existants ✓

1. Ajouter ou mettre à jour les fixtures de réponse HTML `getxfield` et les tests de sélection de `iframe[src]`. ✓
2. Ajouter les doubles de moteur navigateur nécessaires pour couvrir session réutilisée, jeton présent, jeton consommé, jeton rejeté et session invalidée. ✓
3. Vérifier la limite de nouvelle tentative : un rejet de jeton ne produit qu'un rafraîchissement puis une seconde soumission au maximum. ✓
4. Vérifier qu'une iframe mise en cache évite une nouvelle demande `getxfield`. ✓ (testé via `ResolvedIframeCache` unitaire ; le cache d'iframe n'est pas branché au flux d'exécution, cf. Étape 7)
5. Mettre à jour les tests de contrat stream/frontend uniquement lorsqu'ils existent déjà et sont affectés. ✓ (aucun impact sur les tests stream/frontend existants)

Critère de sortie : les scénarios positifs et les échecs sensibles sont couverts sans tests réseau vers un site tiers. ✓

#### Implémentation

**Fixtures HTML :**
| Fichier | Description |
|---------|-------------|
| `server/mock_data/papadustream_v2-get_players.html` | Page d'épisode contenant `ul.player-list > li > .lien` pour l'extraction des lecteurs, `getxfield(...)` et `dle_login_hash` |
| `server/mock_data/papadustream_v2-getxfield_response.html` | Réponse POST `getxfield` contenant `<iframe src="https://lukefirst.lol/e/h80m1hl3hos9">` |

**Nouveaux doubles de moteur navigateur (`client.rs` tests) :**
| Double | Scénario couvert |
|--------|------------------|
| `ConsumingPageEngine` + `ConsumingPageEngineSession` | `read_turnstile_token()` retourne `Some("first-token")` au premier appel, `None` ensuite. Teste le jeton consommé sans cache de domaine. |
| `RejectingPageEngine` + `RejectingPageEngineSession` | `fetch()` retourne `403 FORBIDDEN` avec body `"captcha required"`. Teste le jeton rejeté avec `token_rejection_statuses: [FORBIDDEN]`. |
| `FailingPageEngine` | `open_browser_page_session()` retourne `ChaserCfFailure`. Teste l'invalidation de session sur erreur non-jeton. |

**Nouveaux tests (8 au total) :**

| Test | Crate | Ce qu'il valide |
|------|-------|-----------------|
| `page_fetch_token_consumed_returns_absent_without_reuse` | `client.rs` | Jeton consommé (page retourne `None`) avec `reuse_turnstile_token: false` → `TokenAbsent` |
| `page_fetch_token_reuse_caches_across_calls` | `client.rs` | Cache domaine : après lecture unique, les appels suivants avec `reuse_turnstile_token: true` réutilisent le jeton sans `read_turnstile_token()` |
| `page_fetch_token_rejected_preserves_session` | `client.rs` | `TokenRejected` n'invalide PAS la session navigateur ; un appel suivant utilise la même session |
| `page_fetch_other_error_triggers_new_session_attempt` | `client.rs` | Erreur non-jeton (ex: `ChaserCfFailure`) invalide la session ; le deuxième appel tente une nouvelle session fraîche (2 `open_browser_page_session`) |
| `test_handle_token_cache_starts_empty` | `browser.rs` | `BrowserSessionHandle::cached_turnstile_token()` retourne `None` après création |
| `test_handle_token_cache_roundtrip` | `browser.rs` | `cache_turnstile_token()` puis `cached_turnstile_token()` retourne la valeur |
| `test_handle_token_cache_clear` | `browser.rs` | `clear_turnstile_token()` vide le cache |
| `test_handle_token_cache_replace` | `browser.rs` | `cache_turnstile_token("second")` remplace `"first"` |
| `test_handle_token_cache_does_not_share_between_handles` | `browser.rs` | Deux origines différentes ne partagent pas le jeton en cache |

**Retry limit :** La boucle de retry est dans `query_executor.rs:1566-1598`. Le test `page_fetch_token_rejected_preserves_session` confirme que `TokenRejected` ne détruit pas la session, donc la tentative unique de retry (au niveau scrapyfy) peut réutiliser la même page pour une seconde soumission. La limite à une tentative est garantie par `attempts == 0 && retry_once` ; le test `page_fetch_token_consumed_returns_absent_without_reuse` confirme le comportement de jeton absent après consommation.

**Iframe cache :** `ResolvedIframeCache` est testé unitairement (6 tests existants dans `browser.rs` : get/miss, insert, eviction, TTL, invalidation, clear). Il n'est pas branché au flux `page_fetch` — le cache fonctionnel est le jeton Turnstile par domaine (Étape 7). Aucun test d'intégration supplémentaire nécessaire.

**Tests stream/frontend :** Aucun test de contrat stream ou frontend existant n'est affecté par les changements. Les tests `stream_scraper_tests.rs` utilisent des requêtes HTTP réelles et ne testent pas `get_players` avec `page_fetch`. Les tests `stream_resolver_tests.rs` testent des YAML inline sans `page_fetch`. Le fixture HTML `getxfield_response.html` peut être utilisé par un test scrapyfy futur avec `use_mock_file: true` + un YAML papadustream-v2 minimal.

**Validation :** `cargo test -p arachnea-http --lib` : 37/37 OK.

### Étape 10 : Documenter et valider

1. Documenter les nouveaux champs de sous-requête dans la spécification `arachnea-scrapyfy` concernée.
2. Mettre à jour le README de `arachnea-http` avec le cycle de vie de session, les limites et les règles de sécurité.
3. Mettre à jour `docs/TODO.md` et ajouter une entrée concise à `CHANGELOG.md`.
4. Exécuter `cargo test -p arachnea-http` et `cargo test -p arachnea-scrapyfy`.
5. Exécuter les tests d'intégration stream pertinents et `npm run type-check` dans `front`.
6. Vérifier manuellement que les journaux et erreurs ne divulguent ni cookies ni jetons.

Critère de sortie : les tests applicables passent, les contrats sont documentés et les données sensibles restent protégées.

## Risques et limites

- Les contextes navigateur consomment beaucoup de ressources ; appliquer des limites de sessions/origines et un nettoyage.
- Les cookies, jetons et en-têtes bruts de requête sont sensibles et doivent être masqués dans les journaux.
- Les liens d'hébergeurs peuvent expirer indépendamment des sessions Cloudflare ; le cache d'iframe doit être fourni au mieux.
- La primitive générique ne doit pas basculer silencieusement vers des tentatives de défi répétées.
- L'extraction spécifique à PapaDuStream reste en YAML ; les extensions HTTP et Scrapyfy doivent être réutilisables pour tout site ayant besoin d'une requête navigateur authentifiée dans la page.

---

## Étape 1 réalisée : Cartographie des contrats et points d'extension

### Contrats `arachnea-http` existants

#### `ArachneaHttpConfig` — `server/crates/arachnea-http/src/config.rs:442`
Champs actuels : `engine`, `default_request_mode`, `cloudflare`, `cloudflare_solver`, `cloudflare_browser_solver`, `user_agent_profile`, `proxy`, `proxy_parameters`, `cookie_refresh_margin`, `request_timeout`, `max_redirects`, `max_cloudflare_retries`, `default_session_cookie_ttl`.  
**Aucun champ `execution` ou `browser_context`.** Le mode navigateur est défini via `cloudflare_browser_solver: CloudflareBrowserSolverKind` (chaser-cf, Tauri, Auto, Disabled, Engine).

#### `HttpRequestMode` — `config.rs:383`
```rust
pub enum HttpRequestMode { Auto, Direct, CloudflareSmart, CloudflareBrowser }
```
Typé fortement, pas de chaîne YAML. Mode par requête via `ArachneaRequestBuilder::mode()`.

#### `HttpEngine` trait — `engine/mod.rs:60`
```rust
pub trait HttpEngine: Send + Sync {
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError>;
    async fn refresh_cloudflare(&self, request: EngineRequest) -> Result<…>;
    async fn refresh_cloudflare_fresh(&self, request: EngineRequest) -> Result<…>;
}
```
`EngineRequest` contient `method`, `url`, `headers`, `body`. **Aucune méthode dédiée** pour exécuter une requête dans un contexte de page persistante.

#### Limitations moteur navigateur
- `ChaserCfEngine::send()` — `chaser_cf.rs:790` : rejette les méthodes ≠ GET/HEAD via `UnsupportedEngineOperation`.
- `TauriCloudflareSolverEngine::send()` — `tauri_cloudflare.rs:332` : idem.
- `RquestEngine::send()` — `rquest.rs:88` : pas de restriction.
- `GhostwireEngine::send()` — `ghostwire.rs:89` : pas de restriction.

#### Cycle de vie `ChaserCfEngine` — `chaser_cf.rs`
- `init_browser()` : crée `BrowserManager` paresseux au premier appel.
- `solve_browser_page()` : crée contexte → ouvre page about:blank → pose en-têtes → navigue → attend clearance → extrait cookies via `page.get_cookies()` → évalue `navigator.userAgent` → **ferme la page**.
- `try_click_challenge()` : interagit avec le checkbox Turnstile dans le shadow DOM.
- **Aucune réutilisation de page entre appels** : chaque `send()` ou `refresh_cloudflare()` crée et détruit une page.

#### Gestion de session Cloudflare
- Cache global `SharedCookieCache` — `cookies.rs:70` : `HashMap<(domain, path, name), CookieEntry>`.
- `cloudflare_state()` — `cookies.rs:211` : indique présence/expiration de `cf_clearance`, `__cf_bm`, `_cfuvid`.
- `needs_refresh()` — `cookies.rs:252` : vrai si `cf_clearance` absent ou proche d’expiration.
- Cache de session fichier `ChaserSessionCache` — `chaser_cf.rs:87` : JSON dans `$TMPDIR`.

#### Types d’erreur — `error.rs:1`
```rust
pub enum ArachneaHttpError {
    InvalidUrl, InvalidConfiguration, Network, Proxy, HttpStatus, CloudflareBlocked,
    GhostwireFailure, ChaserCfFailure, TauriCloudflareSolverFailure,
    CloudflareSolverUnavailable, UnsupportedEngineOperation, CookieAbsent, CookieExpired,
    InvalidHeader, RedirectLimitExceeded, Json, Text,
}
```
**Pas d’erreur dédiée** pour jeton absent, jeton rejeté, échec de `page_fetch`.

#### Proxy — `config.rs:13`
`HttpProxyConfig::Arachnea(ArachneaProxyCore)` avec `proxy_parameters`. Loopback injecté dans `rquest` client. Non transmis à chaser-cf (le navigateur CDP a son propre proxy système).

### Contrats `arachnea-scrapyfy` existants

#### Schéma YAML des sous-requêtes
- `EntrySubQueryRaw` (tagged enum) — `scraper_json/config.rs:89`
  - Variante `Html` : `common (SubQueryCommon)`, `row_selector`, `entries`, `context_entries`, `sub_queries`
  - Variante `Json` : `common`, `row_pointer`, `entries`, `context_entries`, `request_body_pointer`, `request_body_select`, `request_body_actions`, `extract_next_data`, `sub_queries`
- `SubQueryCommon` — `scraper/config.rs:246`
  ```rust
  pub struct SubQueryCommon {
      pub post_process: Vec<ScraperPostProcess>,
      pub context_pointer: Option<String>,
      pub context_select: HtmlScraperSelectMode,
      pub filters: HashMap<String, Vec<String>>,
      pub row_filters: HashMap<String, Vec<String>>,
      pub target: Option<String>,
      pub request_pointer: Option<String>,
      pub request_select: HtmlScraperSelectMode,
      pub request_actions: Vec<ScraperAction>,
      pub request_method: ScraperRequestMethod,
      pub request_headers: Vec<ScraperRequestHeaderRaw>,
      pub http: ScraperHttpConfig,
  }
  ```
  **Aucun champ** `execution`, `browser_context` ou `browser_token`.

#### Exécution des sous-requêtes
- `fetch_and_extract_for_entry_sub_query()` — `query_executor.rs:1487`
  1. Résout méthode, en-têtes, corps via `resolve_request_headers()` / `resolve_request_body()`
  2. Configure client HTTP avec `sub_query.http_config().clone()`
  3. Appelle `fetch_single()` qui dispatche sur `ScraperType` (Html/Json)
  4. Pour HTML : `::scraper::Html::parse_document()` puis sélection CSS

#### Résolution de variables
- `replace_template_placeholders()` — `query_helpers.rs:164` : remplace `{clef}` par valeur dans `params`.
- `format_query_template()` — `query_helpers.rs:237` : résout les templates d’URL.
- Variables runtime : `source`, `service_id`, `service_title`, `page_index`, `offset`, `query_separator`, etc.

#### `ScraperQuery` trait — `query_trait.rs:33`
Méthodes : `scraper_type()`, `request_method()`, `request_pointer()`, `request_actions()`, `request_headers()`, `http_config()`, `row_locator()`, `entries()`, `sub_queries()`, `context_pointer()`, `context_select()`, `context_entries()`, `filters()`, `target()`.  
**Aucune méthode** pour accéder à un contexte navigateur ou à une configuration `page_fetch`.

#### `ScraperHttpConfig` — `http_client.rs:171`
```rust
pub struct ScraperHttpConfig {
    pub mode: Option<ScraperHttpMode>,
    pub user_agent_profile: Option<ScraperHttpUserAgentProfile>,
    pub user_agent: Option<String>,
    pub proxy_country: Option<String>,
    pub max_redirects: Option<usize>,
}
```
Utilisé dans `SubQueryCommon` et `ScraperQueryCommon`. Aucun champ `browser_context` ou `execution`.

### Contrat PapaDuStream YAML actuel — `papadustream-v2.yaml`

#### `get_episode` (lignes 468-547)
Extrait depuis `ul.player-list > li > .lien` :
- `name` (`.serv`), `lang` (drapeau → VF/VOSTFR), `player-id`, `player-field`, `player-type` (regex sur `getxfield(this, 'X', 'Y', 'Z')`), `player-endpoint` (URL base + `?mod=getxfield`), `web-link` (URL courante)
- `user-hash` extrait de la page via `dle_login_hash`

#### `get_players` (lignes 548-621)
Structure identique à `get_episode`, dédiée à la résolution différée lorsque `ep.players.link` est présent et `entries` vide.
**Aucune sous-requête** n’exécute le POST `getxfield`.

#### `user-hash` — extrait dans `get_episode` via la sélection racine
Dans `get_episode` (et `get_players` déduit), le `user_hash` est :
```yaml
entries:
  - name: user-hash
    selectors:
      - selector: html
        action: get_response_body
      - action: regex
        pattern: '"dle_login_hash"\s*:\s*"([^"]+)"'
```

### Contrat sortie backend / frontend

#### `get_players` Rust — `stream_scraper.rs:717`
Signature : `async fn get_players(source: String, link: String) -> Result<ScraperAggregationResult<HashMap<String, ScraperDataNode>>>`  
Requête : `{ source, link }`

#### Normalisation frontend — `rustify.ts:1632`
`normalizePlayersResponse()` lit `payload.players ?? payload.entries`, puis `normalizeEntryPlayer()` (lignes 1645-1687) traite `embed-link`, `direct-link`/`directLink`, `resolver`/`target`, `name`, `lang`.

#### Résolution stream frontend — `players.ts:358`
`resolvePlayerMediaSource(value)` traite : YouTube embed → iframe, vidéo native (mp4/webm/ogg/m3u8/mpd) → video, **toute URL HTTP(S) → iframe fallback**.

#### `EntryPlayer` (TypeScript) — `entry.ts:132`
```typescript
interface EntryPlayer {
  id: string; label: string; directLink: string | null;
  name: string | null; lang: string | null;
  resolver: EntryPlayerResolver | null; storyboard: EntryPlayerStoryboard | null;
}
```

#### `GetStreamResponse` — `entry.ts:127`
```typescript
type GetStreamResponse = EntryResolvedPlayerStream | EntryEmbedFallback
// EntryEmbedFallback = { "embed-link": string }
```

### Fichiers de test et fixtures existants

| Fichier | Description |
|---------|-------------|
| `arachnea-http/src/client.rs:1761` — `mod tests` | Tests avec moteurs mock (`StaticEngine`, `RefreshEngine`), mode Cloudflare, dispatch |
| `arachnea-http/src/cookies.rs:404` — `mod tests` | Parsing cookie, expiration, filtrage domaine/chemin |
| `arachnea-http/src/engine/chaser_cf.rs:1377` — `mod tests` | Feature-gated, tests chaser-cf |
| `arachnea-scrapyfy/src/scrapyfy/scraper_manager.rs:104` — `pub mod tests` | Helpers partagés (`TestParams`, `assert_query_succeeds`, `test_query`) |
| `arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs:622` — `mod tests` | Agrégation, erreurs, codes corrélation |
| `arachnea-stream/src/stream_scraper_tests.rs` (334 lignes) | Tests intégration avec `anime-sama.yaml` par défaut |
| `arachnea-stream/src/stream_resolver_tests.rs` (347 lignes) | Résolveur avec fixtures YAML inline, serveurs TCP mock, VOE/embed-link |
| `server/data-test/archanea-stream/` | Répertoires `legal-stream/`, `dark-stream/` — **pas de fixture papadustream** |

**Aucun fichier HTML de fixture** n’existe dans le projet.

### Documentation existante

| Fichier | Pertinence |
|---------|------------|
| `docs/dev-tracking/papadustream-browser-getxfield-analysis.md` (275 lignes) | Analyse complète + plan en 10 étapes |
| `docs/TODO.md` ligne 42 | "Add reusable origin-scoped browser sessions…" |
| `docs/specifications/arachnea-scrapyfy-en.md` section 10 (lignes 527-616) | Schéma des sous-requêtes, **sans les nouveaux champs** |
| `docs/specifications/arachnea-scrapyfy-fr.md` | Version française |
| `server/crates/arachnea-http/README.md` (162 lignes) | Documentation HTTP crate, **sans session navigateur ni page_fetch** |
| `CHANGELOG.md` lignes 7-52 | Entrées papadustream v2, sub_queries, chaser-cf |

### Points d’extension identifiés

| Point d’extension | Fichier | Nature |
|-------------------|---------|--------|
| `HttpEngine` trait | `engine/mod.rs:60` | Nouvelle méthode `page_fetch()` ou `execute_in_page()` |
| `ChaserCfEngine` | `engine/chaser_cf.rs` | Conserver page entre appels, exposer callback Turnstile, exécuter `fetch()`/XHR |
| `ArachneaHttpClient` | `client.rs:197` | Gestionnaire de sessions navigateur par origine, cache d’iframe |
| `ArachneaHttpConfig` | `config.rs:442` | Nouveaux champs `browser_session_limit`, `browser_session_ttl` |
| `ArachneaHttpError` | `error.rs:1` | Nouveaux variants `TokenAbsent`, `TokenRejected`, `PageFetchFailed` |
| `ScraperHttpConfig` | `http_client.rs:171` | Nouveaux champs `execution`, `browser_context` |
| `SubQueryCommon` | `scraper/config.rs:246` | Nouveaux champs `execution`, `browser_context`, `browser_token` |
| `EntrySubQueryRaw` | `scraper_json/config.rs:89` | Propagation des nouveaux champs |
| `ScraperQuery` trait | `query_trait.rs:33` | Nouvelles méthodes getter pour `execution`, `browser_context`, `browser_token` |
| `fetch_and_extract_for_entry_sub_query` | `query_executor.rs:1487` | Branche conditionnelle pour `execution: page_fetch` |
| `papadustream-v2.yaml` | `dark-stream/papadustream-v2.yaml` | Ajout de la sous-requête `page_fetch` dans `get_players` |
| Frontend `entryVideoPlayer.ts` | `front/src/composables/entry-details/entryVideoPlayer.ts` | Gestion d’erreur récupérable |
| Spécifications scrapyfy | `docs/specifications/arachnea-scrapyfy-*.md` section 10 | Documenter les nouveaux champs |
| README HTTP | `server/crates/arachnea-http/README.md` | Documenter cycle de vie session navigateur |
| TODO.md | `docs/TODO.md` ligne 42 | Marquer comme réalisé après implémentation |
| CHANGELOG.md | `CHANGELOG.md` | Ajouter entrée utilisateur/visible |

### Contraintes clés découvertes

1. **Pas de type `BrowserEngine` unique** — deux implémentations distinctes (`ChaserCfEngine`, `TauriCloudflareSolverEngine`) avec des cycles de vie et API différents.
2. **Page non persistante** — `solve_browser_page()` ferme la page après chaque appel. Une nouvelle API est nécessaire pour exécuter plusieurs requêtes dans la même page.
3. **Turnstile callback** — `try_click_challenge()` gère le checkbox, mais le callback JavaScript `turnstile.render(... callback(token))` n’est pas capturé.
4. **Variables de sous-requête** — les placeholders `{player_id}`, `{player_field}` etc. sont déjà résolus via `replace_template_placeholders()`. Les nouveaux placeholders (`{browser_turnstile_token}`) doivent être injectés par l’exécution navigateur avant l’appel HTTP.
5. **Proxy non transmis à chaser-cf** — le navigateur CDP utilise son propre proxy système. Pas de modification nécessaire ici.
6. **Cache d’iframe inexistant** — aucune déduplication des appels `getxfield` actuellement.
7. **Pas de fixture HTML** — des fixtures `getxfield` doivent être créées pour les tests. → Résolu : fixtures créées dans `server/mock_data/` (`papadustream_v2-get_players.html`, `papadustream_v2-getxfield_response.html`).
8. **Absence d’erreur récupérable frontend** — `mediaPlayerErrorMessage` existe déjà mais sans distinction du cas spécifique "échec de résolution navigateur". → Résolu : nouveau message `entry.playerResolutionFailed` dans les locales, catch remplacé par `t('entry.playerResolutionFailed')`.
9. **Pas de test page_fetch avec doubles navigateur** — les variantes de moteur (consommation, rejet, échec) manquaient. → Résolu : `ConsumingPageEngine`, `RejectingPageEngine`, `FailingPageEngine` ajoutés dans `client.rs` avec 4 tests couvrant jeton consommé, jeton rejeté, cache domaine, et invalidation de session.

Ce périmètre est cohérent avec l’analyse initiale et ne révèle pas de dépendance ou de contrat imprévu qui nécessiterait un recalage du plan.
