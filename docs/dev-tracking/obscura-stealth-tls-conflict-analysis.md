# Analyse : conflit TLS stealth d'Obscura et décision A1 (`newwreq` → `wreq`)

> Créée le 2026-09-12.  
> Décision : option **A1** — migrer la pile HTTP du workspace du crate gelé
> `newwreq` 5.1.7 vers sa continuation active `wreq` (épinglé sur la même
> version que `obscura-net`), afin de ne garder qu'une seule pile BoringSSL et
> de réintégrer la feature `obscura/stealth`.  
> Liée à : `obscura-embedded-engine-implementation-plan.md` (Phase 1b,
> question ouverte 6).

## 1. État constaté (Phase 1)

L'intégration Obscura est réalisée avec `features = ["api"]` uniquement. Les
premiers essais avec `features = ["api", "stealth", "render"]` puis
`["api", "render"]` ont échoué à la **résolution** Cargo, avant toute
compilation :

```text
error: failed to select a version for `btls-sys`.
    ... required by package `wreq v6.0.0-rc.29`
    ... which satisfies dependency `wreq = "=6.0.0-rc.29"` of package `obscura-net`
    ... which satisfies git dependency `obscura` of package `arachnea-http`
package `btls-sys` links to the native library `boringssl`, but it conflicts
with a previous package which links to `boringssl` as well:
package `boring-sys2 v5.0.0-alpha.10`
    ... which satisfies dependency `boring2 = "^5.0.0-alpha.10"` of package
    `newwreq v5.1.7`
```

Faits vérifiés :

- `btls-sys` 0.5.6 (crates.io, `lib_links = "boringssl"`, auteur 0x676e67) ;
- le workspace lie déjà BoringSSL via `newwreq` 5.1.7 → `boring2` /
  `boring-sys2` 5.0.0-alpha.10 (constaté dans `server/Cargo.lock`) ;
- le conflit se déclenche dès que la déclaration de dépendance Git demande
  `stealth`, même sans activer la feature `obscura` de `arachnea-http` ;
- avec `features = ["api"]`, `wreq` et `btls-sys` disparaissent du graphe
  (absents du lock) : l'état Phase 1 est propre.

## 2. Mécanisme du blocage

- La règle `links` de Cargo interdit que deux paquets du graphe déclarent le
  même `links = "boringssl"` : contrôle effectué au niveau des métadonnées,
  donc inconditionnel.
- Patcher la clé `links` de `btls-sys` via `[patch.crates-io]` ferait passer la
  résolution mais produirait **deux BoringSSL statiques** exportant les mêmes
  symboles C (`SSL_new`, `SSL_CTX_new`, …) dans le même binaire : échec à
  l'édition de liens, deux versions de la bibliothèque dans un process.
- Le symbol prefixing (renommage des exports) existe chez `wreq`
  (`prefix-symbols`) mais Obscura ne l'active que sur Linux/Android ; le
  commentaire de `obscura-net/Cargo.toml` cite l'issue #39 : renommage
  incorrect et symboles non résolus sur les autres plateformes. Le gate 3 du
  plan (macOS + Linux + Windows) interdit d'en faire la solution générale.

## 3. Les deux piles, et leur parenté

| | Pile workspace actuelle | Pile stealth Obscura |
|---|---|---|
| Client HTTP | `newwreq` 5.1.7 (clé Cargo `rquest`) | `wreq` `=6.0.0-rc.29` + `wreq-util` 3.0.0-rc.12 |
| Binding BoringSSL | `boring2` / `boring-sys2` 5.0.0-alpha.10 | `btls` / `btls-sys` 0.5.6 |
| État | gelé depuis 2025-06 (6 versions, ~3,8k téléchargements) | actif (rc.31 en date de l'analyse) |
| Rôle | empreinte TLS/JA3-JA4 Chrome pour `RquestEngine` et le proxy loopback | empreinte TLS/JA3-JA4 Chrome du navigateur embarqué |

Découverte clé (métadonnées crates.io) : `newwreq` est une **republication de
la ligne rquest 5.x** (description et `documentation` pointant vers
`docs.rs/wreq`, `repository` vers `0x676e67/wreq`). L'auteur d'origine a
renommé `rquest` en `wreq`, et `btls`/`btls-sys` (créés en 2026-02, même
auteur) sont la continuation de `boring2`/`boring-sys2`. La migration A1 est
donc un **retour vers l'upstream actif de la même famille**, pas un changement
de technologie.

## 4. Ce que la feature `stealth` apporte

- `obscura-net/stealth` remplace le client par défaut (`reqwest` 0.12 + rustls)
  par `wreq` : ClientHello Chrome exact (ciphers, extensions, GREASE, ALPN) et
  SETTINGS HTTP/2 Chrome — la couche score par Cloudflare (JA3/JA4/Akamai).
- `obscura-js/stealth` route les `fetch()`/XHR déclenchés depuis la page
  (Turnstile, widgets) à travers le même client `wreq`.
- Le flag `BrowserConfig::stealth = true` (déjà positionné par le squelette)
  pilote le spoofing DOM/JS seul ; sans la feature Cargo, la couche TLS reste
  `reqwest`/rustls. Signature mixte (`navigator` Chrome + TLS rustls) que des
  bot managers flaggent spécifiquement.

## 5. Options évaluées

| Option | Contenu | Verdict |
|---|---|---|
| **A1** | Migrer le workspace `newwreq` → `wreq` (une seule pile `btls`) | **Retenue** : supprime le conflit sur les 3 OS, repasse sur l'upstream actif, supprime une dépendance gelée |
| B | Fork Obscura avec transport branchable (prévu Phase 4 si besoin) | Coûteux pour ce seul problème ; reste pertinent pour le proxy in-process Phase 4 |
| C | Contribution upstream (client stealth pluggable) | Zéro fork, mais délai hors de notre contrôle |
| D | Statu quo (stealth DOM-only, TLS rustls) pour le PoC | État intermédiaire actuel ; risque signature mixte si les cibles refusent |

## 6. Décision

**A1 (2026-09-12).** La migration `newwreq` → `wreq` est planifiée en Phase 1b
du plan d'intégration. L'état Phase 1 (`features = ["api"]`) reste la référence
jusqu'à l'atterrissage de la migration.

## 7. Périmètre de migration dans ce dépôt

- `server/Cargo.toml` : `rquest = { package = "newwreq", version = "5.1.7",
  features = ["json", "cookies", "gzip"] }` →
  `rquest = { package = "wreq", version = "=6.0.0-rc.29", ... }`. Conserver la
  clé `rquest` : la clé est déjà un alias (package `newwreq`) et préserve les
  contrats existants (`use rquest::…`, feature `arachnea-proxy/rquest`,
  exemple `rquest_loopback`). Un renommage complet en `wreq` peut être un
  nettoyage séparé, pas un prérequis.
- Points d'appel relevés (`rquest::`) :
  - `arachnea-http` : `engine/rquest.rs` (`RquestEngine`), `client.rs`
    (client loopback `rquest::Client`/`Proxy`) ;
  - `arachnea-proxy` : `connectors/rquest.rs` (`Proxy::all`,
    `redirect::Policy::none`, `custom_http_headers`), feature `rquest` ;
  - `arachnea-dns` : `core/transport.rs` (client DoH) ;
  - `arachnea-stream` : `francetv_resolver.rs`, `player_resolver.rs`,
    `rtbf_auvio_resolver.rs`, `rtlplay_resolver.rs`.
- Deltas d'API à vérifier (rc churn documenté par Obscura) : renommages type
  `cert_store` → `tls_cert_store`, `CertStore` déplacé hors de `tls`, builder
  `Emulation` restructuré ; les signatures utilisées ici (`Proxy`,
  `redirect`, `header`, builder client) restent à revérifier sur la version
  épinglée au moment de la migration.
- Prérequis de build de `btls-sys` : BoringSSL upstream exige CMake, un
  compilateur C/C++, NASM (Windows) et **Go** ; l'environnement actuel compile
  déjà `boring-sys2` (CMake/NASM présents), la toolchain Go reste à vérifier.

## 8. Validation et critères de sortie

1. `cargo check` full workspace, puis avec `obscura` et
   `obscura,arachnea-proxy` ;
2. tests `arachnea-http` et `arachnea-proxy` sans régression ;
3. smoke des résolveurs `arachnea-stream` concernés ;
4. repasse de la dépendance Obscura en `features = ["api", "stealth"]` :
   résolution OK avec un seul paquet `links = "boringssl"` dans le graphe ;
5. contrôle d'empreinte TLS (JA3/JA4) si un harnais de mesure est disponible.

Le lockfile reste un artefact local (règle dépôt, non commité).

### Résultats de validation (2026-09-12, atterrissage Phase 1b sur Windows)

- `cargo check --workspace --all-targets` : OK.
- `cargo check -p arachnea-http --features obscura` et
  `--features obscura,arachnea-proxy` : OK. `obscura/stealth` → `wreq`
  `=6.0.0-rc.29` + `wreq-util` `=3.0.0-rc.12` compilent, `btls-sys` 0.5.6
  unique dans le graphe (`cargo tree --invert btls-sys`), plus de
  `newwreq`/`boring-sys2` dans `Cargo.lock`.
- Tests : verts sur `arachnea-http` (44 + 4 doc), `arachnea-proxy`
  (66 + 4 + 1), `arachnea-stream` (13 lib), `arachnea-dns` (9 lib),
  `arachnea-core` (22 lib). Les tests `resolve_url` de `arachnea-scrapyfy`
  ne compilaient plus (signature `apply` à 7 arguments depuis l'ajout de
  `ProxyFollowRedirects`) : réalignés, 69 tests passent.
- Prérequis build Windows : CMake 3.31 + NASM 3.01 + MSVC suffisent pour
  `btls-sys` 0.5.6 ; la toolchain Go n'est pas requise sur la machine de test.
- À couvrir ensuite (hors scope local, réseau/harnais indisponibles) :
  smoke des résolveurs `arachnea-stream` avec accès réseau ; contrôle
  d'empreinte JA3/JA4 si un harnais est disponible ; validation macOS/Linux
  en CI (cf. plan Phase 5) ; échecs d'exécution préexistants de
  `arachnea-scrapyfy` (état global `application root already resolved` dans
  les tests `execute_query_async_*`, séparateur de chemin Windows dans
  `resolve_manifest_sources_...`) et du doc-test
  `application::get_application_data_path`.

## 9. Ce que A1 ne règle pas

La feature `render` d'Obscura ne compile pas indépendamment de la pile TLS :
les `[patch.crates-io]` vendor d'Obscura (`taffy`, `cosmic-text`) ne
s'appliquent pas aux dépendances Git. Options restantes : dupliquer les
patches vendor en dépendances `path` dans le workspace Arachnea, ou fork
Obscura (le même fork porterait alors le transport de la Phase 4). À trancher
après le PoC de la Phase 2.