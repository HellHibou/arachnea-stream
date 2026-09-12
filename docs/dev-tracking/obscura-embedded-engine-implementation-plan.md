# Plan d’implémentation : moteur Obscura embarqué dans Arachnea

> Créé le 2026-09-11.  
> Décision : intégrer **Obscura standard** comme bibliothèque Rust embarquée dans `arachnea-http`, sans Chrome/Chromium et sans processus Obscura/CDP obligatoire.  
> Périmètre : nouveau moteur `HttpEngine`, sessions `BrowserPageSession`, Cloudflare/Turnstile, cache de cookies, proxy Arachnea in-process et sélection de configuration.  
> Hors périmètre : analyse des moteurs alternatifs, fork Obscura existant comme dépendance principale, réécriture du proxy core ou migration immédiate de tous les utilisateurs de `chaser-cf`.

## Objectif et critères bloquants

Le moteur retenu doit remplacer les responsabilités exposées par
`server/crates/arachnea-http/src/engine/chaser_cf.rs`, tout en supprimant la
dépendance à Chrome/Chromium.

Les critères de validation sont ordonnés et éliminatoires :

1. **Cloudflare / Turnstile** : produire un `cf_clearance` utilisable lorsqu’une
   origine autorisée l’émet et lire un token de callback Turnstile lorsqu’un
   workflow Arachnea le demande.
2. **Page persistante** : conserver le DOM, le runtime JavaScript et les cookies
   entre `navigate`, `click_and_wait` et `fetch`, avec attente CSS et
   `window.fetch` dans la même page.
3. **Multiplateforme** : obtenir le même contrat fonctionnel sur macOS, Linux et
   Windows.
4. **Proxy Arachnea in-process** : pour `HttpProxyConfig::Arachnea`, router les
   requêtes au travers de `ArachneaProxyCore` et du `ClientContext` de la
   requête, sans démarrer de listener loopback.
5. **Coût et maintenance** : mesurer RSS, CPU, latence, stabilité et taille de
   distribution face au chemin Chaser/Chromium ; épingler une révision Obscura
   reproductible et préserver la licence Apache-2.0.

Le succès HTTP seul, un code `200`, la présence d’un widget dans le DOM ou un
profil `navigator` ressemblant à Chrome ne sont pas des réussites suffisantes.

## État Arachnea à préserver

### Contrats publics existants

Le nouveau moteur doit implémenter `HttpEngine` :

- `name()` ;
- `send(EngineRequest)` ;
- `refresh_cloudflare(EngineRequest)` ;
- `refresh_cloudflare_fresh(EngineRequest)` ;
- `open_browser_page_session()`.

La session de page doit implémenter `BrowserPageSession` :

- `navigate(PageNavigationRequest)` ;
- `fetch(PageFetchRequest)` ;
- `click_and_wait(PageClickRequest)` ;
- `metadata()` ;
- `read_turnstile_token()` ;
- `clear_turnstile_token()` ;
- `close()`.

Le comportement à conserver est le suivant :

- `send`, `refresh_cloudflare` et `refresh_cloudflare_fresh` n’acceptent que
  `GET` et `HEAD` lorsque le moteur est utilisé comme solveur navigateur ;
- les cookies et le User-Agent observés sont rendus au client via des en-têtes
  `Set-Cookie` synthétisés et `x-arachnea-solver-user-agent` ;
- `CachedChaserSession` conserve les cookies structurés, l’origine, le
  User-Agent et l’expiration de `cf_clearance` dans le `TypedEntityStore` ;
- la cache de session est réutilisée pour `refresh_cloudflare`, mais jamais pour
  `refresh_cloudflare_fresh` ;
- les sessions de page restent isolées par origine, profil navigateur et route
  proxy grâce au `BrowserSessionManager` existant ;
- les valeurs de cookies et tokens ne sont ni journalisées ni placées dans les
  erreurs.

### Écarts documentaires à corriger pendant l’implémentation

Le README de `arachnea-http` décrit actuellement `chaser-cf` comme ne prenant
pas en charge les sessions persistantes, alors que le fichier actuel les
implémente. La migration devra remplacer cette description par les capacités du
moteur Obscura sélectionné et documenter la disponibilité de chaque feature.

## Dépendance Obscura retenue

### Source et verrouillage

Le nom `obscura` sur crates.io ne correspond pas au navigateur choisi. Le
navigateur fournit son propre package `obscura` dans le workspace Git
`h4ckf0r0day/obscura` ; il doit donc être référencé par Git avec une révision
complète immuable, par exemple :

```toml
obscura = {
    git = "https://github.com/h4ckf0r0day/obscura",
    rev = "<full-reviewed-commit>",
    default-features = false,
    features = ["api", "stealth", "render"],
    optional = true,
}
```

La révision exacte ne doit être choisie qu’après le PoC. Ne pas dépendre de la
branche `main`, ni d’un tag seul sans SHA de révision vérifié.

> Mise à jour (2026-09-12) : `server/Cargo.lock` est volontairement ignoré par
> git (`server/.gitignore`) et ne doit **pas** être commité dans ce dépôt. La
> reproductibilité repose uniquement sur l’épinglage `rev` de la dépendance
> Git ; le lockfile reste un artefact local.

### Features nécessaires

| Feature Obscura | Décision | Raison |
|---|---|---|
| `api` | Requise | Fournit la façade Rust `Browser` / `Page`. |
| `stealth` | Requise pour le solveur | Aligne le profil UA/platform et active l’impersonation TLS/HTTP utilisée par Obscura. |
| `render` | Requise pour le premier PoC | Fournit la géométrie et le rendu utiles aux clics réels, boîtes d’éléments et widgets interactifs. Une réduction ultérieure n’est possible qu’après validation des challenges sans cette feature. |

Le build `stealth` tire notamment la pile TLS spécialisée d’Obscura. La CI et
les builds développeur doivent documenter les prérequis de compilation associés
(V8, CMake, compilateur C/C++ et outils de binding selon les plateformes).

> Mise à jour (2026-09-12, Phase 1) : `stealth` et `render` sont exclues de la
> déclaration Cargo actuelle (conflit BoringSSL avec `newwreq` pour `stealth`,
> patches vendor non appliqués aux dépendances Git pour `render`). Elles
> devront être réintégrées avant les gates Cloudflare ; voir la Phase 1 et la
> question ouverte 6.
>
> Mise à jour (2026-09-12, décision A1) : le blocage `stealth` est tranché —
> migration de la pile HTTP du workspace `newwreq` → `wreq` (une seule pile
> BoringSSL), planifiée en Phase 1b ; `render` reste bloqué séparément (patches
> vendor). Détails :
> `docs/dev-tracking/obscura-stealth-tls-conflict-analysis.md`.

### Façade Rust à utiliser

Le crate `obscura` est explicitement présenté comme une API Rust de navigateur.
Sa façade publique exporte au minimum :

- `Browser` et `BrowserConfig` ;
- `Page` ;
- `Cookie` et `CookieStore` ;
- `Response`, `RequestInfo`, `RequestCallback`, `ResponseCallback` ;
- types d’interception `InterceptedRequest` et `InterceptResolution`.

L’adaptateur Arachnea doit dépendre de cette façade publique dans la mesure du
possible. Les crates internes `obscura-browser` et `obscura-net` ne doivent être
importées directement que si la façade ne donne pas une API nécessaire ; toute
utilisation de type interne doit être isolée dans le module moteur afin de
réduire le coût d’une mise à jour Obscura.

## Architecture cible dans Arachnea

### Modules et features Cargo

Ajouter dans `server/crates/arachnea-http/Cargo.toml` :

```toml
[features]
obscura = ["dep:obscura"]
```

Ajouter ensuite :

```text
server/crates/arachnea-http/src/engine/obscura.rs
```

et déclarer le module sous :

```rust
#[cfg(feature = "obscura")]
pub mod obscura;
```

La sélection explicite doit recevoir un nouveau variant :

```rust
CloudflareBrowserSolverKind::Obscura
```

Le variant `ChaserCf` doit conserver sa signification pendant la migration. Le
choix de basculer `CloudflareBrowserSolverKind::Auto` vers Obscura n’est permis
qu’après validation complète des gates. Avant cela, l’usage d’Obscura doit être
explicite par configuration ou par injection de moteur.

### Types internes proposés

```rust
pub const ENGINE_NAME: &str = "obscura";

pub struct ObscuraEngine {
    config: ObscuraEngineConfig,
    session_cache: Option<ObscuraSessionCache>,
    transport: ObscuraTransportMode,
}

struct ObscuraPageSession {
    page: obscura::Page,
    browser: obscura::Browser,
    context: ObscuraPageContext,
}
```

`ObscuraEngineConfig` est un type local qui doit contenir uniquement les
paramètres stables Arachnea : timeouts, activation stealth/rendu, profil UA,
configuration proxy et options de persistance. Il ne doit pas exposer les types
Obscura dans la configuration publique Arachnea.

`ObscuraSessionCache` doit réutiliser le stockage et la forme
`CachedChaserSession` existants au premier jalon. Le renommage vers un nom
neutre (`CachedCloudflareSession`) est un refactor/migration de données séparé,
non nécessaire à l’intégration initiale.

### Cycle de vie du navigateur embarqué

- Créer un `obscura::Browser` embarqué, configuré en stealth et rendu.
- Mutualiser le runtime navigateur lorsque l’API Obscura est `Send + Sync` et
  que le comportement de contexte le permet ; sinon, créer un navigateur par
  session Arachnea avec des limites explicites par `BrowserSessionManager`.
- Créer une page et un contexte isolé pour chaque solve frais ou session de
  page ; ne pas partager les cookies entre origines distinctes.
- Fermer explicitement la page lors de `BrowserPageSession::close`; détruire le
  contexte/page après `send` et après un refresh ponctuel.
- Ne pas activer de stockage disque Obscura par défaut : la persistance
  Arachnea doit rester le `TypedEntityStore` existant. Un storage directory
  Obscura ne peut être considéré qu’en option future, avec un cycle de purge et
  une politique explicite de confidentialité.

## Adaptation des méthodes `HttpEngine`

### `send`

1. Rejeter les méthodes autres que `GET` / `HEAD` avec
   `UnsupportedEngineOperation`.
2. Créer un contexte/page neuf avec le profil, proxy et interception adaptés à
   la requête.
3. Naviguer vers l’URL et attendre la stabilisation configurée sans supposer que
   `networkidle` est atteignable sur une page Cloudflare.
4. Exécuter le protocole de clearance décrit ci-dessous.
5. Extraire cookies et User-Agent ; construire les en-têtes Arachnea.
6. Pour `GET`, retourner le HTML stable avec `Content-Type: text/html;
   charset=utf-8`; pour `HEAD`, retourner un corps vide.
7. Mettre la session dans le cache persistant si `cf_clearance` est présent.
8. Toujours fermer page/contexte dans un bloc de cleanup, même en cas d’erreur.

### `refresh_cloudflare` et `refresh_cloudflare_fresh`

`refresh_cloudflare` :

1. accepter uniquement `GET` / `HEAD` ;
2. lire `CachedChaserSession` pour l’origine ;
3. retourner immédiatement cookies + User-Agent lorsque la clearance est encore
   utilisable au regard de `cookie_refresh_margin` ;
4. sinon, lancer le solve frais et stocker le résultat.

`refresh_cloudflare_fresh` suit le même protocole mais ignore la cache à la
lecture. Les deux méthodes retournent un corps vide et ne doivent pas effectuer
une seconde navigation destinée à récupérer le HTML.

### Protocole Cloudflare / Turnstile

Le protocole doit être générique, borné et observable :

1. après navigation, inspecter le cookie jar et l’état DOM ;
2. si `cf_clearance` existe, attendre une brève stabilisation puis continuer ;
3. détecter les interstitiels et widgets par signaux génériques, par exemple
   URL/titre de challenge, iframe `challenges.cloudflare.com`, champs
   `cf-turnstile-response` / `cf-response` et marqueurs DOM connus ;
4. laisser d’abord le JavaScript de page s’exécuter sans polling agressif ;
5. utiliser l’API d’interaction Obscura pour cliquer uniquement lorsque le
   widget/cible est présent et que le PoC confirme ce comportement ;
6. attendre soit l’apparition de `cf_clearance`, soit un token de callback, soit
   une transition stable hors interstitiel ;
7. échouer avec une erreur contextualisée et sans secret si le timeout est
   atteint.

Le mécanisme ne doit pas prétendre résoudre tous les challenges. Toute
interaction automatisée doit être limitée à des domaines contrôlés ou pour
lesquels l’opérateur est autorisé.

## Adaptation `BrowserPageSession`

### Navigation et HTML

`navigate` doit appeler la navigation Obscura sur la page conservée, appliquer
les en-têtes de navigation lorsque l’API Obscura le permet, puis retourner :

- l’URL finale depuis la page ;
- `Some(html)` lorsque `collect_body` est activé ;
- `None` autrement.

La collecte HTML doit passer par l’API Obscura de contenu/DOM, et non par une
nouvelle requête HTTP, afin de conserver les mutations JavaScript et l’état de
la page.

### `fetch`

Construire un script injecté à partir d’un payload JSON sérialisé :

```javascript
async function () {
  const input = /* JSON */;
  const response = await fetch(input.url, {
    method: input.method,
    headers: input.headers,
    body: input.body,
    credentials: 'same-origin',
  });
  const headers = {};
  response.headers.forEach((value, name) => { headers[name] = value; });
  return {
    url: response.url,
    status: response.status,
    headers,
    body: await response.text(),
  };
}
```

L’adaptateur doit :

- conserver le comportement actuel de corps UTF-8 seulement, sauf décision
  explicite de faire évoluer `PageFetchResponse` vers des bytes ;
- convertir les headers JS en `HeaderMap` validé ;
- propager les erreurs d’évaluation sous `PageFetchFailed` ;
- laisser le client Arachnea gérer le placeholder et la cache de token
  Turnstile, comme aujourd’hui.

### `click_and_wait`

1. trouver la cible CSS avec l’API Obscura ou une évaluation JavaScript sûre ;
2. déclencher un clic navigateur réel lorsqu’Obscura le permet ;
3. attendre le sélecteur de résultat avec le mécanisme d’attente Obscura,
   jusqu’au timeout Arachnea ;
4. si un challenge est injecté après le clic, lancer le protocole Cloudflare
   borné sur la même page, sans détruire son contexte ;
5. retourner le HTML courant quand le sélecteur apparaît.

L’adaptateur ne doit pas remplacer le clic par un simple `element.click()` tant
que le PoC n’a pas établi son équivalence pour les widgets ciblés. Le mode de
clic retenu doit être documenté dans les tests de compatibilité.

### Métadonnées et Turnstile

`metadata` :

- extraire l’ensemble des cookies du contexte Obscura ;
- les convertir en `Set-Cookie` avec le helper Arachnea existant ;
- ajouter `SOLVER_USER_AGENT_HEADER` avec le User-Agent réellement configuré
  dans le contexte ;
- retourner l’URL finale de page.

`read_turnstile_token` : évaluer, de manière bornée, un script qui tente :

1. `window.turnstile.getResponse()` ;
2. `[name="cf-response"]` ;
3. `[name="cf-turnstile-response"]`.

`clear_turnstile_token` : appeler `window.turnstile.reset()` si disponible,
vider les champs de réponse et retourner une erreur seulement si l’évaluation
elle-même échoue.

## Cookies, cache et transfert vers HTTP

Réutiliser les helpers de `chaser_cf.rs` dans un module neutre plutôt que les
dupliquer :

- conversion cookie Obscura → `StructuredCookie` ;
- génération de `Set-Cookie` ;
- calcul d’expiration `cf_clearance` ;
- clé d’origine normalisée ;
- lecture/écriture de `CachedChaserSession` ;
- logique `is_usable(refresh_margin)`.

Le refactor doit être local : déplacer ces utilitaires vers
`chaser_session.rs` ou un nouveau module `cloudflare_session.rs` seulement si
les deux moteurs les appellent. Ne pas changer le schéma persistant dans le
même jalon.

Le cache n’est valide qu’avec une cohérence route/profil : une clearance issue
d’un proxy ou User-Agent Obscura ne doit jamais être transférée vers une autre
route proxy ou un autre profil de fingerprint. Si la clé persistante actuelle
ne contient pas ces dimensions, le risque doit être analysé avant d’activer la
réutilisation Obscura au-delà d’une instance homogène.

## Proxy Arachnea sans loopback

### Contrainte

`HttpProxyConfig::Arachnea` ne doit pas être transformé en URL proxy locale pour
Obscura. Chaque requête doit recevoir son `ClientContext`, y compris les
paramètres de routage comme le pays, sans les exposer à l’origine cible.

### Étape 1 — intercepteur applicatif Obscura

Obscura expose un `RequestInterceptor` qui peut notamment retourner
`Fulfill(Response)`. Le premier adaptateur doit vérifier si cet intercepteur est
appliqué à :

- navigation principale ;
- redirections ;
- iframes ;
- scripts, styles, images et autres sous-ressources ;
- appels JavaScript `fetch` / XHR ;
- formulaires et navigations déclenchées après clic.

Pour chaque requête interceptée, l’adaptateur :

1. traduit l’URL, méthode, en-têtes et body vers une requête Arachnea ;
2. construit un `ConnectRequest` avec le `ClientContext` associé à la page ;
3. exécute la requête à travers `ArachneaProxyCore` / `SimpleHttpClient` ;
4. convertit la réponse en `obscura::Response` et retourne `Fulfill` ;
5. laisse les cookies de réponse être intégrés au cookie jar Obscura ou les y
   injecte explicitement si cette intégration n’est pas automatique.

Cette étape est acceptée uniquement si les trois gates prioritaires restent
validés. Elle peut modifier l’empreinte TLS/HTTP observée par la cible et doit
donc être activée derrière une option interne de PoC avant de remplacer le
transport normal.

### Étape 2 — backend de transport Obscura si nécessaire

Si `Fulfill(Response)` ne couvre pas toutes les requêtes ou casse un challenge,
forker Obscura à une révision figée et introduire une abstraction de transport
dans `obscura-net` :

```rust
#[async_trait]
pub trait ObscuraTransport: Send + Sync {
    async fn execute(
        &self,
        request: ObscuraRequest,
        context: ObscuraRequestContext,
    ) -> Result<obscura::Response, ObscuraTransportError>;
}
```

L’implémentation Arachnea traduit `ObscuraRequestContext` en `ClientContext` et
appelle le core. Cette extension doit couvrir les navigations, sous-ressources,
XHR/fetch et redirections. Elle ne doit pas être introduite avant que le PoC
montre clairement la lacune de l’interception applicative.

## Erreurs, délais et observabilité

Ajouter des variants d’erreur explicites seulement lorsqu’ils apportent une
classification utile, par exemple `ObscuraFailure(String)`. Garder les erreurs
de session/page existantes (`PageFetchFailed`, `PageInteractionFailed`,
`TokenAbsent`, `TokenRejected`) pour préserver le contrat client.

Journaliser :

- nom du moteur ;
- origine normalisée ;
- durée de navigation, de challenge et de clic ;
- présence/absence des noms de cookies importants ;
- mode de transport sélectionné (`network-proxy`, `interceptor-fulfill`,
  `embedded-transport`) ;
- plate-forme et version Obscura épinglée.

Ne jamais journaliser : valeurs de cookies, token Turnstile, contenu de
`Cookie`, `Set-Cookie`, `Authorization`, URL signées ou bodies contenant des
secrets.

Les timeouts doivent rester configurables et séparés : navigation normale,
clearance initiale, challenge après clic, lecture de token et timeout total de
requête. Les valeurs actuelles de Chaser-CF servent de borne de départ, mais
doivent être ajustées avec les mesures Obscura plutôt que copiées sans test.

## Plan d’implémentation structuré

### Phase 1 — intégration Cargo et squelette isolé

> Statut (2026-09-12) : intégration réalisée et validée localement sur Windows.
> Révision Obscura épinglée : `eec047a188cc75b7a1a257397ad84493ee59c091`
> (HEAD `main` au moment du pin, à revalider après le PoC). Deux blocages
> découverts pendant l'intégration : la feature `stealth` d'Obscura
> (`wreq`/`btls-sys`) lie BoringSSL (`links = "boringssl"`) en conflit avec
> `newwreq`/`boring-sys2` déjà lié par le workspace, et la feature `render` ne
> compile pas car les `[patch]` vendor (`taffy`, `cosmic-text`) du workspace
> Obscura ne s'appliquent pas aux dépendances Git. La dépendance est donc
> déclarée avec `features = ["api"]` uniquement (voir question ouverte 6).
>
> Décision (2026-09-12) : le blocage `stealth` est tranché par l'option A1
> (migration `newwreq` → `wreq`, Phase 1b ci-dessous) ; l'exclusion de
> `render` reste en vigueur. Voir
> `docs/dev-tracking/obscura-stealth-tls-conflict-analysis.md`.

- [x] Ajouter la dépendance Git Obscura épinglée et la feature `obscura` dans
  `arachnea-http` (révision épinglée ; `stealth` et `render` exclues, voir
  blocages ci-dessus).
- [x] Ajouter `engine/obscura.rs`, `ENGINE_NAME` et les erreurs de construction
  (nouveau variant `ArachneaHttpError::ObscuraFailure`).
- [x] Ajouter `CloudflareBrowserSolverKind::Obscura` et les builders dans
  `engine/mod.rs` sans modifier `Auto` (validation ajoutée : le variant exige
  la feature `obscura`).
- [x] Ajouter les options internes Obscura dérivées de `ArachneaHttpConfig` :
  stealth, rendu, UA, timeouts et mode de transport (`ObscuraEngineConfig`,
  `ObscuraTransportMode`, `ObscuraSessionCache` réutilisant
  `CachedChaserSession`).
- [ ] Compiler les features concernées sur macOS, Linux et Windows dans la CI ou
  les jobs de validation disponibles. *(Windows validé : `cargo check` et tests
  OK sans la feature, avec `obscura`, et avec `obscura,arachnea-proxy`.)*

**Sortie attendue :** le crate compile avec et sans `obscura`; aucune route de
production ne bascule automatiquement. *(Atteint avec `obscura` = `api`.)*

### Phase 1b — migration de la pile HTTP du workspace (`newwreq` → `wreq`)

> Décision A1 du 2026-09-12 : supprimer le conflit `links = "boringssl"` à la
> racine en migrant le workspace du crate gelé `newwreq` 5.1.7 vers sa
> continuation active `wreq` (épinglé comme `obscura-net`), afin de
> réintégrer `obscura/stealth`. Analyse complète :
> `docs/dev-tracking/obscura-stealth-tls-conflict-analysis.md`.

- [x] Dans `server/Cargo.toml`, remplacer
  `rquest = { package = "newwreq", version = "5.1.7", ... }` par
  `rquest = { package = "wreq", version = "=6.0.0-rc.29", ... }` en alignant
  les features sur les besoins réels (`json`, `cookies`, `gzip`, et à évaluer
  `socks`/`stream`/`zstd`). Conserver la clé `rquest` (déjà un alias, comme
  `newwreq` l'était) pour ne pas changer les contrats existants (feature
  `arachnea-proxy/rquest`, exemple `rquest_loopback`, `use rquest::…`).
  *(Fait sur Windows : `server/Cargo.toml` = `rquest = { package = "wreq",
  version = "=6.0.0-rc.29", features = ["json", "cookies", "gzip"] }`. Les
  features supplémentaires `socks`/`stream`/`zstd` ne sont pas requises par le
  workspace hors `obscura` : elles sont portées par la déclaration interne
  d'`obscura-net/stealth` quand la feature `obscura` est activée.)*
- [x] Migrer les points d'appel `rquest::` : `arachnea-http`
  (`engine/rquest.rs`, client loopback dans `client.rs`), `arachnea-proxy`
  (`connectors/rquest.rs`), `arachnea-dns` (`core/transport.rs` DoH),
  `arachnea-stream` (résolveurs FranceTV, player, RTBF, RTL Play).
  *(Deltas rc réels rencontrés et corrigés : `Response::url()` → `uri()` dans
  `engine/rquest.rs`, et `rquest::Url` supprimé de la racine `wreq` — les
  résolveurs FranceTV/RTL Play importent désormais `url::Url`.*
- [x] Corriger les deltas d'API rc documentés en amont (renommages type
  `cert_store` → `tls_cert_store`, `CertStore` déplacé hors de `tls`, builder
  `Emulation` restructuré) et revérifier `Proxy::all`,
  `redirect::Policy::none`, `custom_http_headers` sur la version épinglée.
  *(Revérifiés sur `wreq` rc.29 par compilation : `Proxy::all`,
  `Proxy::custom_http_headers`, `redirect::Policy::none`, le builder
  `ClientBuilder` et les re-exports `header::…` existent ; `Url` est absent
  (voir item précédent). Les renommages `tls_cert_store`/`Emulation` sont
  portés par la couche stealth d'Obscura, validée à la compilation.)*
- [ ] Vérifier les prérequis de build de `btls-sys` (BoringSSL : CMake,
  compilateur C/C++, NASM et toolchain Go) sur chaque plateforme de build.
  *(Windows validé sur la machine de test : CMake 3.31 + NASM 3.01 + MSVC
  suffisent ; `btls-sys` 0.5.6 compile sans la toolchain Go. macOS/Linux à
  confirmer en CI, cf. validation multiplateforme Phase 5.)*
- [x] Repasser la dépendance Obscura en `features = ["api", "stealth"]` et
  vérifier qu'un seul paquet du graphe porte `links = "boringssl"`.
  *(Fait : `arachnea-http/Cargo.toml` = `features = ["api", "stealth"]`.
  `cargo tree --invert btls-sys` montre un unique `btls-sys v0.5.6`, consommé
  par le `wreq` workspace/proxy ET par `obscura-net` ; `newwreq`/`boring-sys2`
  absents du lock.)*
- [x] Validation : `cargo check` full workspace, puis avec `obscura` et
  `obscura,arachnea-proxy` ; tests `arachnea-http` et `arachnea-proxy` ;
  smoke des résolveurs `arachnea-stream` ; contrôle d'empreinte TLS
  (JA3/JA4) si un harnais de mesure est disponible.
  *(Windows (2026-09-12) : `cargo check --workspace --all-targets` OK ;
  `cargo check -p arachnea-http --features obscura` et
  `--features obscura,arachnea-proxy` OK ; tests verts `arachnea-http`
  (44 unit + 4 doc), `arachnea-proxy` (66 + 4 + 1), `arachnea-stream` (13),
  `arachnea-dns` (9), `arachnea-core` lib (22). Les tests `resolve_url` de
  `arachnea-scrapyfy` ne compilaient plus (signature `apply` à 7 arguments) :
  réalignés (7e argument `None`), 69 tests passent. Échecs d'exécution
  restants préexistants et hors périmètre : panique d'état global
  `application root already resolved` (3 tests `execute_query_async_*` de
  `scraper_agregator`), assertion de séparateur de chemin Windows dans
  `resolve_manifest_sources_expands_recursive_imports_in_depth_first_order`,
  et doc-test `application::get_application_data_path` (identifiant non
  configuré). Le smoke des résolveurs avec accès réseau et le contrôle
  d'empreinte JA3/JA4 ne sont pas exécutables ici (réseau / harnais
  indisponibles).)*

**Gate :** workspace compilant avec une seule pile BoringSSL (`btls`),
`obscura/stealth` activée, sans régression de tests. `render` reste exclue :
le blocage des patches vendor (`taffy`, `cosmic-text`) est un follow-up
séparé (duplication des patches en path deps ou fork, à trancher).

### Phase 2 — solveur HTTP et cache de session

> Statut (2026-09-12) : implémentée et validée localement sur Windows
> (compilation et tests unitaires ; le PoC Cloudflare réel sur cible autorisée
> reste à exécuter, voir gate ci-dessous).
>
> Points d’implémentation notables :
>
> - `obscura::Page` n’est pas `Send` (runtime V8 affinitaire au thread) : le
>   solve complet s’exécute sur un thread dédié via
>   `tokio::task::spawn_blocking` + runtime Tokio mono-thread local
>   (`run_blocking_solve` / `block_on_local` dans `engine/obscura.rs`) ; seuls
>   des entrées `Send` entrent et seul le résultat `Send`-sûr en sort.
> - Le protocole de clearance est borné et observable : fenêtre passive de 2 s
>   après navigation, boucle de poll via `page.settle` (le runtime embarqué ne
>   avance pas pendant un simple `sleep`), clics Turnstile same-origin limités
>   à une tentative toutes les 2 s, timeout dédié de 180 s, et échec
>   contextualisé ne portant que des labels de signaux non secrets
>   (`ChallengeSignals`) ; aucune valeur de cookie/token n’est journalisée.
> - Les cookies et l’UA observés sont restitués via des en-têtes `Set-Cookie`
>   synthétisés et `x-arachnea-solver-user-agent` ; `GET` renvoie le HTML stable
>   (`Content-Type: text/html; charset=utf-8`), `HEAD` un corps vide ; `send`
>   rejette les méthodes autres que GET/HEAD (`UnsupportedEngineOperation`).
> - La cache de session réutilise `CachedChaserSession` tel quel (schéma
>   inchangé) : `refresh_cloudflare` lit la cache et évite un solve Obscura
>   quand la clearance reste utilisable au regard de `cookie_refresh_margin`,
>   `refresh_cloudflare_fresh` ignore la cache à la lecture ; seules les
>   sessions portant un `cf_clearance` sont persistées.
> - Les helpers partagés (`cache_origin_key`, `cached_session_headers`,
>   `clearance_expires_at`, `set_cookie_header`, `unix_timestamp`,
>   `expires_http_date`, `CACHE_TTL_NO_EXPIRY`, `is_usable`) sont extraits dans
>   `chaser_session.rs` derrière `any(feature = "chaser-cf", feature =
>   "obscura")` ; `chaser_cf.rs` les consomme désormais aussi.
> - `HttpProxyConfig::Arachnea` (mode `InterceptorFulfill`, Phase 4) est refusé
>   explicitement plutôt que de bypasser la route ; seuls les proxys réseau
>   `http`/`https` sont acceptés par la pile stealth.
>
> Validation locale (Windows, 2026-09-12) : `cargo check --all-targets` OK sans
> feature, avec `obscura`, avec `chaser-cf`, avec `obscura,arachnea-proxy` et
> avec `chaser-cf,obscura` ; tests verts `obscura` (52 unit + 4 doc), `chaser-cf`
> (46 + 4) et `chaser-cf,obscura` (54 + 4), dont 8 tests unitaires ciblés
> `engine::obscura` (transport, conversion cookie, signaux de challenge).
>
> Mise à jour (2026-09-12, correction résiduelle) : la conversion cookie
> `chaser-cf` a été réalignée sur les helpers partagés (`set_cookie_header`
> prend désormais `&StructuredCookie`), et le warning mort
> `EntityQuery::predicates` dans `arachnea-core` est couvert par
> `#[allow(dead_code)]` (le helper sert au stockage SQLite des sessions).

1. [x] Construire une page Obscura éphémère en stealth/rendu.
2. [x] Implémenter `send`, `refresh_cloudflare` et `refresh_cloudflare_fresh`.
3. [x] Extraire cookies, UA, HTML et URL finale ; raccorder le cache
   `CachedChaserSession` existant.
4. [x] Implémenter la détection de challenge et le protocole d’attente borné.
5. [x] Réutiliser ou extraire les helpers de sérialisation de cookies sans
   modifier le schéma de persistance.

**Gate :** sur une cible de test autorisée, le solveur remet un `cf_clearance`
valide lorsque la cible en émet, puis le chemin HTTP rapide réutilise les
cookies et l’UA sans relancer Obscura. *(Partie locale validée ; l’exécution
PoC sur cible autorisée reste à faire et confirmera ou non la gate.)*

### Phase 3 — session de page persistante

1. Implémenter `ObscuraPageSession` sur une `obscura::Page` retenue.
2. Implémenter `navigate`, récupération HTML, URL finale et `metadata`.
3. Implémenter `fetch` via évaluation JavaScript et conversion rigoureuse des
   headers/statut.
4. Implémenter `click_and_wait` avec clic navigateur, attente CSS et protocole
   de challenge injecté dans la même page.
5. Implémenter lecture/remise à zéro du token Turnstile.
6. Vérifier fermeture, invalidation et éviction par le
   `BrowserSessionManager` existant.

**Gate :** la séquence navigation → clic → attente CSS → token si requis →
`window.fetch` se déroule dans la même page, sans Chrome, sur la cible de test
autorisée.

### Phase 4 — proxy in-process

1. Introduire un adaptateur de PoC `RequestInterceptor::Fulfill` branché au
   core, sans listener local.
2. Vérifier couverture réelle des types de requêtes ; instrumenter les requêtes
   non interceptées sans exposer de secrets.
3. Vérifier cookies, redirections, CORS, iframes et XHR/fetch après réponse
   fournie par Arachnea.
4. Comparer le comportement Cloudflare/Turnstile avec le transport Obscura
   normal et sélectionner le mode qui satisfait les gates.
5. Si la couverture ou le fingerprint échoue, préparer un fork Obscura minimal
   avec backend de transport ; ne pas construire ce fork avant ce constat.

**Gate :** `HttpProxyConfig::Arachnea` ne démarre aucun listener loopback et
conserve les résultats des phases 2 et 3.

### Phase 5 — validation multiplateforme et charge

1. Répéter les gates Cloudflare/page persistante sur macOS, Linux et Windows.
2. Mesurer RSS au repos et pic, CPU, démarrage, navigation, solve et fermeture.
3. Vérifier les contraintes de build Obscura/V8/stealth sur chaque plateforme.
4. Vérifier que le nombre de runtimes/pages et la fermeture restent bornés sous
   charge, y compris après timeout ou erreur de challenge.
5. Fixer les limites de sessions/concurrence par configuration plutôt que les
   laisser implicites.

**Gate :** contrat fonctionnel identique sur les trois OS et ressources dans
les seuils fixés avant mesure.

### Phase 6 — bascule et retrait progressif

1. Mettre à jour README `arachnea-http`, `docs/TODO.md`, `CHANGELOG.md` et les
   commentaires publics affectés.
2. Rendre Obscura sélectionnable explicitement en production.
3. Ne basculer `Auto` qu’après une période de validation ; conserver
   `ChaserCf` comme rollback explicite pendant cette période.
4. Retirer `chaser-cf` et `chaser-oxide` seulement lorsque les usages actifs,
   les builds et les plateformes ont tous migré, puis nettoyer les APIs
   intrinsèquement couplées à `ChaserConfig`.

## Validation et tests

Ne pas ajouter une nouvelle infrastructure de test. Réutiliser les tests
existants de `arachnea-http` pour :

- cache persistant de sessions ;
- client et refresh Cloudflare ;
- cycle de vie de `BrowserSessionManager` ;
- token cache et rejet de token ;
- erreurs de page/mock engine.

Ajouter des tests unitaires ciblés uniquement pour les nouveaux helpers purs
(conversion cookie Obscura, détection non secrète de challenge, sérialisation
des résultats JS et choix de transport). Les tests Cloudflare/Turnstile réels
doivent être des validations de PoC contrôlées, pas des tests CI publics ou
fragiles sur des sites tiers.

## Livrables attendus

1. feature Cargo `obscura` compilable et version Obscura épinglée ;
2. `ObscuraEngine` derrière `HttpEngine` ;
3. `ObscuraPageSession` derrière `BrowserPageSession` ;
4. cache de clearance/UA compatible avec le stockage actuel ;
5. stratégie de proxy in-process validée ou lacune documentée justifiant le
   fork de transport ;
6. résultats de PoC Cloudflare/Turnstile, clic/fetch persistant et
   macOS/Linux/Windows ;
7. documentation et changelog à jour au moment de la bascule effective ;
8. workspace sur une seule pile BoringSSL (`wreq`) avec `obscura/stealth`
   réintégrée (Phase 1b).

## Questions ouvertes à trancher pendant la phase 1 / PoC

1. Quelle révision Git Obscura validée doit être épinglée après le premier build
   reproductible ? *(Phase 1 : épinglage initial
   `eec047a188cc75b7a1a257397ad84493ee59c091`, à confirmer après le PoC.)*
2. L’interception `Fulfill(Response)` couvre-t-elle réellement toutes les
   requêtes nécessaires tout en préservant les gates Cloudflare/Turnstile ?
3. Si non, l’équipe accepte-t-elle de maintenir un fork minimal du transport
   Obscura, ou faut-il conserver le proxy réseau uniquement pour Obscura ?
4. Quels seuils mesurés rendent Obscura acceptable en RSS, CPU, démarrage et
   délai de solve par rapport à Chaser/Chromium ?
5. Quelles origines contrôlées/autorisées seront la référence de validation des
   trois gates sur macOS, Linux et Windows ?
6. Comment réconcilier la pile stealth d'Obscura (`wreq`/`btls-sys`,
   `links = "boringssl"`) avec `newwreq`/`boring-sys2` déjà lié par le
   workspace, et compenser les patches vendor (`taffy`, `cosmic-text`) requis
   par la feature `render` ?
   *(Tranchée le 2026-09-12 pour la partie stealth : option A1 — migration
   `newwreq` → `wreq` en Phase 1b, voir
   `docs/dev-tracking/obscura-stealth-tls-conflict-analysis.md`. La partie
   `render` reste ouverte : duplication des patches vendor dans le workspace
   Arachnea ou fork Obscura.)*