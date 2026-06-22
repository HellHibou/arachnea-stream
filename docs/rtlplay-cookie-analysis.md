# Analyse : Problème de cookies dans `rtlplay_resolver.rs`

## Problème observé

Les logs démontrent que le cache de cookies est vide lors des appels vers les APIs RTL Play :

```
2026-06-21 13:05:13  INFO arachnea_http::client: sending request engine="rquest" method=GET url=https://lfvp-api.dpgmedia.net/RTL_PLAY/detail3/...
2026-06-21 13:05:13 DEBUG arachnea_http::client: headers:{"user-agent": "RTL_PLAY/25.260415 (com.tapptic.rtl.tvi; build:30644; Android 30)"}
2026-06-21 13:05:13 DEBUG arachnea_http::client: cookies:RwLock { data: SharedCookieCache { cookies: {} } }
```

**Aucun cookie n'est envoyé**, même après login.

## Architecture des domaines RTL Play

D'après `server/services/rtlplay-be.yaml` :

| Rôle | Domaine | Utilisation |
|------|---------|-------------|
| Site web | `www.rtlplay.be` | Page player, warm-up, SSO |
| API scraping | `lfvp-api.dpgmedia.net` | Toutes les requêtes YAML (search, home, detail, etc.) |
| API player | `videoplayer-service.dpgmedia.net` | Récupération de la configuration de lecture (play-config) |

## Flux d'exécution

### 1. Scraper YAML (indépendant du résolveur)

Le scraper effectue des requêtes vers `lfvp-api.dpgmedia.net` **avant même** que le résolveur ne soit appelé.

Exemples :
- `GET https://lfvp-api.dpgmedia.net/RTL_PLAY/search?query=...`
- `GET https://lfvp-api.dpgmedia.net/RTL_PLAY/storefronts/accueil`

Ces requêtes utilisent le `HttpClient` créé par le scraper, **pas** celui du résolveur. Le cache de cookies doit donc déjà contenir les cookies de session pour `lfvp-api.dpgmedia.net`.

### 2. Flux dans le résolveur (`rtlplay_resolver.rs`)

```
resolve_replay_stream / resolve_live_stream
  └─ create_http_client(ScraperHttpConfig { mode: Direct, ... })
      └─ get_or_login_session(http_client, credentials_store)
          ├─ [1] GET https://www.rtlplay.be/rtlplay (warm-up)
          │      → cookies collectés : lfvp_device_id, gig_bootstrap_3_...
          ├─ [2] GET https://www.rtlplay.be/rtlplay/connexion
          │      → cookies collectés : lfvp_auth.redirect_uri, possibly others
          ├─ [3] POST https://sso.rtl.be/api/account/login
          │      → retourne encryptedToken (pas de cookie)
          ├─ [4] GET https://sso.rtl.be/oidc/account/authenticate?token=...
          │      → Set-Cookie: lfvp_rtlplay_auth=... (domaine: rtl.be ?)
          └─ STORE cookies pour https://www.rtlplay.be/rtlplay (ligne 249)
              → stocké pour domaine: www.rtlplay.be, path: /rtlplay
  └─ resolve_final_video_url(http_client, session, video_url, ...)
      ├─ STORE cookies pour https://www.rtlplay.be/rtlplay (ligne 367)
      ├─ GET video_url (www.rtlplay.be/rtlplay/player/...)
      │   → cookies envoyés: OUI (domaine match www.rtlplay.be)
      └─ POST config_url (videoplayer-service.dpgmedia.net)
          → cookies envoyés: NON (domaine différent)
```

## Cause racine

Le `SharedCookieCache` (`server/crates/arachnea-http/src/cookies.rs`) effectue un matching strict par domaine :

```rust
// cookies.rs ligne 358-370
fn cookie_matches_url(cookie: &CookieEntry, url: &Url, now: SystemTime) -> bool {
    // ...
    let domain = cookie.domain.trim_start_matches('.').to_ascii_lowercase();
    let domain_match = host == domain || host.ends_with(&format!(".{domain}"));
    domain_match && url.path().starts_with(&cookie.path)
}
```

**Un cookie stocké pour `www.rtlplay.be` n'est jamais envoyé vers :**
- `lfvp-api.dpgmedia.net` (utilisé par le scraper YAML)
- `videoplayer-service.dpgmedia.net` (utilisé pour le play-config)

Résultat :
- Le scraper YAML fait des appels **sans cookies** vers `lfvp-api.dpgmedia.net` → réponses erronées ou vides
- Le résolveur fait un appel **sans cookies** vers `videoplayer-service.dpgmedia.net` → échec de récupération du manifest DASH

## Solution implémentée

### Problème supplémentaire identifié

Outre le matching strict par domaine, le scraper YAML s'exécute **avant** que le résolveur ne soit appelé. Les requêtes vers `lfvp-api.dpgmedia.net` partent donc avec un cache vide, même si le résolveur fait le login plus tard.

### Approche

1. **Propagation des cookies vers tous les domaines cibles** dans `get_or_login_session` :
   - `www.rtlplay.be` (déjà fait)
   - `lfvp-api.dpgmedia.net` (ajouté - requis pour le scraper YAML)
   - `videoplayer-service.dpgmedia.net` (ajouté dans `resolve_final_video_url` - requis pour le play-config)

2. **Pré-authentification depuis le scraper** : la fonction `get_or_login_session` a été rendue `pub(crate)` et est appelée préventivement avant chaque requête YAML RTL Play via `ensure_sessions_for_sources` dans `StreamScraper`.

### Fichiers modifiés

- `server/crates/arachnea-stream/src/services/rtlplay_resolver.rs`
  - `get_or_login_session` devient `pub(crate)` pour être accessible depuis `StreamScraper`.
  - Ajout de `HttpClient::store_cookies_for_url("https://lfvp-api.dpgmedia.net", &seen_cookies)` après login.
  - Ajout de `HttpClient::store_cookies_for_url("https://videoplayer-service.dpgmedia.net", ...)` dans `resolve_final_video_url`.
  - `RtlPlaySession` passe à `pub(crate)`.

- `server/crates/arachnea-stream/src/stream_scraper.rs`
  - Ajout de `ensure_sessions_for_sources` appelé avant `search`, `get_entry`, `get_season`, `get_live`, `list_lives`, `load_home`, `get_service`, `get_category`.
  - Import de `anyhow::Context` pour la gestion d'erreur.
  - Import qualifié de `rtlplay_resolver::{self, RtlPlayResolver}`.

### Résultat

- Le cache contient désormais les cookies pour `lfvp-api.dpgmedia.net` avant que le scraper YAML n'exécute ses requêtes.
- Les requêtes du résolveur vers `videoplayer-service.dpgmedia.net` incluent également les cookies.

## Fichiers concernés

- `server/crates/arachnea-stream/src/services/rtlplay_resolver.rs`
- `server/services/rtlplay-be.yaml`
- `server/crates/arachnea-http/src/cookies.rs` (mécanisme de matching)

## Impact attendu

- Les requêtes du scraper YAML vers `lfvp-api.dpgmedia.net` incluront les cookies de session
- La récupération du play-config depuis `videoplayer-service.dpgmedia.net` inclura les cookies
- Le complétion du flux de résolution devrait fonctionner avec authentification

## Vérification

Après modification, les logs doivent montrer :

```
2026-06-21 13:05:13 DEBUG arachnea_http::client: cookies:RwLock { data: SharedCookieCache { cookies: {("www.rtlplay.be", "/rtlplay", "lfvp_device_id"): CookieEntry { ... }, ...} } }
```

Et pour `lfvp-api.dpgmedia.net` :

```
2026-06-21 13:05:13 DEBUG arachnea_http::client: cookies:RwLock { data: SharedCookieCache { cookies: {("lfvp-api.dpgmedia.net", "/", "lfvp_device_id"): CookieEntry { ... }, ...} } }