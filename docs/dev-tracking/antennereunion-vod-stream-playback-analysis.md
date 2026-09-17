# Analyse — Récupération du flux vidéo Antenne Réunion (`get_entry`)

**Date** : 16 septembre 2026
**Statut** : analyse terminée ; correctifs locaux appliqués (option C) ;
résolveur Rust VOD implémenté (option A) le 17 septembre 2026 — validation
de bout en bout avec un compte réel restante
**Source analysée** : `https://www.antennereunion.fr/series/Tous-en-douceurs-1000543/details`
**Fichier YAML concerné** : `server/services/arachnea-stream/work-in-progress/antennereunion-fr.yaml`
(service désactivé dans `work-in-progress/services.json`)

---

## 1. Objet

Déterminer comment la plateforme Antenne Réunion fournit le flux vidéo d'un
programme, afin de savoir ce que `get_entry` (et `get_season`) doivent exposer
pour que la lecture fonctionne dans Arachnea.

Toutes les constatations ci-dessous ont été vérifiées live le 2026-09-16 :
reverse des bundles Nuxt 3 du site officiel, appels directs à l'API
`proxies.antennereunion-production.eu-west-3.alphanetworks.tv`, et lecture des
manifestes renvoyés.

---

## 2. État actuel du YAML

| Query | Contenu | Lecture |
|---|---|---|
| `get_entry` | Métadonnées, `seasons`, `web-link`, `trailer` (resolver `scraper-query` → `/proxy/readTrailer`) | Bande-annonce seulement |
| `get_season` | Épisodes via `entries_episode` (titre, durée, dates, visuel, numéro, moralité) | Aucun `players` |
| `get_live` | `players > resolver > kind: scraper-query` → `/proxy/channelStream?idChannel=…` (corrigé, cf. §9.1) | Live anonyme sur les chaînes exposées |
| `resolve_stream` | `POST {url}` + corps reconstruit depuis les paramètres de l'URL cible | Bande-annonce **et** live |

Deux constats :

1. `get_entry` / `get_season` n'exposent **aucun lecteur VOD** ; seuls les blocs
   `trailer` (entry et épisode) permettent une lecture (cf. §5.1).
2. `get_live` référençait le resolver `antennereunion-live`, absent de
   `player_resolver_for_source` / `player_resolver_for_id`
   (`server/crates/arachnea-stream/src/stream_scraper.rs:1412`), ce qui
   produisait `Unsupported player resolver`. Remplacé par `scraper-query`
   (§9.1).

---

## 3. API réelle du lecteur (reverse du site)

Le site est une application Nuxt 3 ; toute la logique de lecture est dans le
bundle `/_nuxt/HXiruTOG.js` (couche `BaseApi`).

### 3.1 Transport

```text
POST https://proxies.{baseHost}/proxy/{endpoint}
Content-Type: application/x-www-form-urlencoded
X-AN-WebService-IdentityKey: HadCogoctandsxEDEsdufOcagidOad
X-AN-WebService-CustomerAuthToken: <token compte>     (optionnel)
X-AN-WebService-ProfileToken:     <token profil>      (optionnel)
X-AN-WebService-DeviceAuthToken:  <token appareil CAS>(optionnel)
```

`baseHost` = `antennereunion-production.eu-west-3.alphanetworks.tv` (payload
`window.__NUXT__.config.public`). Les valeurs d'objet sont sérialisées en JSON
dans le corps form-urlencoded (`a[k] = JSON.stringify(a[k])`).

### 3.2 Endpoints utiles à la lecture

| Endpoint | Paramètres | Rôle |
|---|---|---|
| `POST /proxy/listContent` | `id`, `includeRoot`, `languageId` | Fiche catalogue / saison (assets) |
| `POST /proxy/assets` | `assetIds=["…"]`, `languageId` | Détail d'un asset isolé |
| `POST /proxy/testAsset` | `idAsset` | Éligibilité de lecture (droits) |
| `POST /proxy/readAsset` | `idAsset`, `idAudioLang`, `idSubtitleLang`, `adsMacro` | **Flux VOD signé** (`url`, `seek`) |
| `POST /proxy/getVodLicense` | `idAsset`, `idAudioLang`, `idSubtitleLang` | Charge DRM VOD (`licParam`) |
| `POST /proxy/channelStream` | `idChannel` | **Flux live** (`url`, métadonnées DRM) |
| `POST /proxy/askLicenseWV` | `idChannel` | Charge Widevine live |
| `POST /proxy/askLicenseFP` | `idChannel` | Charge FairPlay live (HLS) |
| `POST /proxy/readTrailer` | `idMedia` | **Bande-annonce** (HLS signé) |
| `GET  /cms/search` | `languageId`, `fts` | Recherche |

L'identité anonyme (`X-AN-WebService-IdentityKey` seule) suffit pour les
endpoints de catalogue (`listContent` et `assets` vérifiés live ; `homeContent`
et `epg` déjà utilisés par le YAML) et pour `readTrailer`. Elle est **refusée**
pour `readAsset`, `testAsset`, `getVodLicense` et les deux `askLicense*`
(vérifié live) ; `channelStream` répond selon la chaîne (cf. §5.3).

---

## 4. Parcours de lecture VOD (site officiel)

Extrait du store `Player` (`/_nuxt/HXiruTOG.js`) :

```js
async openAsset(e, t = false, n) {          // t = isTrailer
  if (!isLogged && !t) { this.loading = false; redirectToLogin(); return; }
  ...
  let audio = n?.audioLanguages[0] ?? "eng";
  let sub   = n?.subtitleLanguages[0] ?? "non";
  const asset = getAssetById(e.id);
  if (t) {                                  // bande-annonce
    if (isAsset(e) && e.trailer) this.streamUrl = (await api.readTrailer(+e.trailer.idMedia)).url;
  } else {
    // AD_LIASMEDI[0].audio / .sub remplacent les valeurs par défaut
    const stream = await api.readAsset(e.id, audio, sub, await buildAdsMacro());
    if ("errorCode" in stream) { alertFromErrorCode(stream.errorCode); return; }
    const license = await api.getVodLicense(e.id, audio, sub);
    this.streamUrl = stream.url;
    this.assetStartTime = Number(stream.seek);
    if (license?.licParam) this.licenseData = buildLicenseData(license);
  }
}
```

Enseignements :

1. **La lecture VOD est conditionnée à une session authentifiée** (`isLogged`).
   Une bande-annonce, à l'inverse, est lue sans compte (`t = true`).
2. `readAsset` renvoie `{ url, seek }` : une URL de manifeste **signée**.
3. Les pistes audio/sous-titres proviennent de `directMetadata.AD_LIASMEDI[0]`
   (`audio`/`sub`, valeurs `"fra"`, `"non"`, …).
4. `getVodLicense` renvoie `{ licParam }` ; `buildLicenseData` en déduit :
   `{ licParam, vendor, url, token|sessionToken }`, le fournisseur étant
   `vualto` si l'URL contient `vudrm.tech`, sinon `irdeto` ; l'URL de licence
   devient `<url>?token=<token>` dans le cas `vualto`.
5. La plateforme est configurée en DRM :
   `drm-protection: widevine`, `drm-provider: castlabs`,
   `api-key: RIpiruj2SidlThIhodruraBrotH6st8h`, `max-inactivity: 3600`.
6. Le live suit le même garde-fou (`openChannel` → `redirectToLogin` si non
   connecté) puis choisit la licence selon l'URL : `.m3u8` → `askLicenseFP`,
   sinon `askLicenseWV`.

### 4.1 Données disponibles dans `get_entry` / `get_season`

`get_entry` (`listContent` sur `1000543`) renvoie la fiche série : `name`,
`serie: true`, `synopsis`, `directMetadata`, et `categories[0]` = la saison
(`idCatalog: 2000712`, `season: true`, `absolute_asset_number: 20`).

`get_season` (`listContent` sur `2000712`) renvoie `payload.assets[]`. Exemple
vérifié (asset `28609`, « Mille-feuille ») :

```json
{
  "idAsset": 28609,
  "directMetadata": {
    "AD_ASSTITLE": "Mille-feuille",
    "AD_SERIESEP": 20,
    "AD_SERIESEA": 2026,
    "AD_SERIECOL": 1000543,
    "AD_LITRAILE": [],
    "AD_NEXTEPI": null,
    "AD_LIASMEDI": [{ "sub": "non", "name": "fra version", "audio": "fra" }],
    "tv.alphanetworks.tucano.medios": [
      { "audioLanguages": ["fra"], "idMedia": 319028, "subtitleLanguages": ["non"], "title": "" },
      { "audioLanguages": ["fra"], "idMedia": 319026, "subtitleLanguages": ["non"], "title": "" }
    ]
  },
  "trailer": null,
  "path": ["4/16636/19065/1000543/2000712"],
  "packageIds": [2, 83, 3, 43],
  "seekTime": null
}
```

Points à retenir pour un futur lecteur :

- `AD_LIASMEDI` fournit les codes audio/sous-titres attendus par `readAsset` ;
  `tv.alphanetworks.tucano.medios`, lui, porte les `idMedia`.
- `AD_LITRAILE` peut être vide alors que l'asset possède une bande-annonce :
  la fiche Nuxt observée pour d'autres assets expose en plus un champ
  `trailer` porteur d'un `idMedia`.
- `TS_DURATION` est en minutes (déjà divisé par 60 dans le YAML).

---

## 5. Vérifications live

### 5.1 Bande-annonce — anonyme, fonctionnelle

```text
POST /proxy/readTrailer  idMedia=76553&languageId=fra
→ HTTP 200 {"status":true,"result":{"url":"https://eu-west-3-antenne-reunion-vod.akamaized.net/bpk-vod/antenne-reunion/default/76208-0ae14d73-b112-4fd8-a783-e25a6bdd8b2f/76208-0ae14d73-b112-4fd8-a783-e25a6bdd8b2f/index.m3u8?hdnts=ip=213.219.179.14~st=1789583700~exp=1789583710"}}
```

Le master récupéré est un HLS clair (Broadpeak) sans `EXT-X-KEY`, avec des
sous-playlists **relatives** (`76208-…-video=797800.m3u8`) et des segments
relatifs (`…-1.ts`). Test complémentaire : les enfants sont servis **sans
token** (HTTP 200), seul le master exige `hdnts`.

### 5.2 Lecture VOD — refusée sans compte

```text
POST /proxy/readAsset      idAsset=26034&idAudioLang=fra&idSubtitleLang=
POST /proxy/readAsset      type=anonymous&languageId=fra&idAsset=26034
POST /proxy/testAsset      type=anonymous&languageId=fra&idAsset=26034
POST /proxy/getVodLicense  type=anonymous&languageId=fra&idAsset=26034&…
→ HTTP 200 {"status":false,"result":null,"error":{"code":-1,"message":"Invalid auth"}}
```

Le refus provient du serveur (HTTP 200 + `status:false`), pas seulement du
client : `readAsset`, `testAsset` et `getVodLicense` exigent un jeton client
(`X-AN-WebService-CustomerAuthToken`) et/ou un jeton de profil
(`X-AN-WebService-ProfileToken`), obtenus via `POST /oauth/token`
(`username`, `password`, `client_id`, `grant_type`, `id_device`) puis
`loginProfile`.

### 5.3 Live — anonyme sur certaines chaînes, 451 sinon

```text
POST /proxy/listChannels
→ idChannel 1 "Direct TV", 2 "Radio Live", 69 "Exo TV", 70 "Passion Bollywood", 71 "Passion Novelas"

POST /proxy/channelStream  idChannel=69
→ HTTP 200 {"status":true,"result":{"url":"https://eu-west-3-antenne-reunion-msl.akamaized.net/hls/live/20000128/main/ch1.m3u8","licenseType":"NonPersistent","firstPlayExpiration":"P1D","digitalVideoProtectionLevel":0,"allowTimeShift":false}}
Manifeste récupéré : HLS clair (aucun EXT-X-KEY), segments .ts relatifs.

POST /proxy/channelStream  idChannel=1  → HTTP 451
POST /proxy/channelStream  idChannel=2  → HTTP 451

POST /proxy/askLicenseWV   idChannel=69 → {"status":false,…,"message":"Invalid auth"}
POST /proxy/askLicenseFP   idChannel=69 → {"status":false,…,"message":"Invalid auth"}
```

Le 451 (Unavailable For Legal Reasons) sur la chaîne principale n'a pas pu être
tranché entre restriction géographique (La Réunion) et restriction d'offre : à
retester avec le routage pays du proxy (`resolver > proxy > country` →
`proxy_country`).

---

## 6. Contraintes techniques à intégrer

1. **URL signée liée à l'IP appelante et très courte** :
   `hdnts=ip=<ip>~st=…~exp=…` avec `exp - st = 10 s`. La résolution doit donc
   avoir lieu au clic (`get_stream`, `CacheType::NoCache`) et le manifeste doit
   être récupéré par le backend dans la fenêtre — ce que fait déjà
   `resolve_scraper_query_stream` via `proxy_resolved_stream`.
2. **Sous-playlists et segments relatifs** : vérifié live, ils sont servis
   **sans token** et la disposition d'URL du proxy
   (`{proxy_path}[/opts_…]/{URL absolue}`) les résout correctement : une URI
   relative ne remplace que le dernier segment du chemin, donc
   `.../opts_X/https://cdn/.../index.m3u8` + `…-video=797800.m3u8` donne
   `.../opts_X/https://cdn/.../…-video=797800.m3u8`, et `…-video=797800-1.ts`
   s'enchaîne de la même façon. Aucune réécriture relative n'est donc
   nécessaire pour ce CDN (contrairement au cas storyboard de `vidzy.yaml` /
   `uqload.yaml`, qui cible des URL d'images dans un VTT).
3. **Réécriture d'URL absolues** : `get_stream` l'applique déjà
   (`proxy_rewrite_manifest_urls`, `stream_resolver.rs:685`), mais uniquement
   pour les URL absolues présentes ligne à ligne.
4. **Cas DRM** : si un contenu est protégé Widevine, la lecture passera par
   `license_url`/`license_headers` du contrat `ResolvedPlayerStream`
   (`player_resolver.rs:88`), donc par un proxy de licence côté backend —
   mécanisme déjà en place pour TF1 (`widevine-license-proxy`).

---

## 7. Conclusion

**Le flux VOD d'Antenne Réunion n'est pas récupérable anonymement.** L'API
`readAsset` (flux signé) et `getVodLicense` (charge DRM) sont réservées à une
session authentifiée, et le client officiel refuse explicitement la lecture
lorsqu'aucun profil n'est connecté. Seules deux lectures restent accessibles
sans compte :

- la **bande-annonce** (`readTrailer`), déjà branchée dans `get_entry` ;
- le **live** de certaines chaînes (`channelStream`), dont le kind de resolver
  déclaré dans le YAML (`antennereunion-live`) n'existe pas encore en Rust.

`get_entry` / `get_season` peuvent donc exposer un descripteur de lecteur, mais
aucun ne peut aboutir sans identifiants ni gestion de licence DRM.

---

## 8. Options d'implémentation

### A. Résolveur Rust dédié `antennereunion-video` (+ `antennereunion-live`)

Motif déjà en place pour `m6play_resolver`, `tf1_resolver`, `rtlplay_resolver` :
lecture des identifiants via `CredentialsStore`, login, appel API, puis
`ResolvedPlayerStream` (licence DRM et proxy de licence inclus).

- Pour : gère l'authentification multi-étapes, la session, le renouvellement, le
  DRM et le proxy de licence ; les identifiants restent chiffrés côté serveur.
- Contre : code Rust spécifique à la plateforme ; duplication partielle de
  l'API AlphaNetworks déjà décrite en YAML.
- Impact : `services/mod.rs`, `services/antennereunion_resolver.rs` (nouveau),
  `stream_scraper.rs` (`player_resolver_for_source` / `player_resolver_for_id`),
  YAML (`credentials`, `players` des épisodes et du live), `CHANGELOG.md`.

### B. Étendre `scraper-query` (identifiants + enchaînement de requêtes)

Ajouter au chemin générique la fourniture des identifiants et l'exécution
d'une séquence (login → lecture → licence).

- Pour : aucune implémentation Rust par plateforme ; réutilisable pour les
  autres services AlphaNetworks.
- Contre : changement de contrat public (`GetStreamRequest`), gestion d'un cache
  de jetons, support de la licence DRM dans la conversion, ergonomie Scrapyfy
  pour des requêtes dépendantes. Chantier d'architecture à cadrer.
- Impact : `arachnea-stream` (contrat + chemin `scraper-query`), Scrapyfy
  (paramètres d'identifiants, requêtes chaînées), frontend (`source`/`target`
  déjà en place).

### C. Socle anonyme seulement, en attendant

- Live : remplacer le kind `antennereunion-live` (inexistant) par un lecteur
  `scraper-query` — vérifié fonctionnel anonymement sur `idChannel=69`.
- Bande-annonce : déjà opérationnelle.
- VOD : reste indisponible ; l'entrée peut l'annoncer (badge « compte requis »)
  via `credentials: required: true` + `signup_url`.
- Pour : débloque le live immédiatement, sans dette d'architecture.
- Contre : ne répond pas à l'objectif « lire une série ».

---

## 9. Plan retenu et état

Décision : **option C puis A** — les correctifs locaux d'abord, le résolveur
Rust ensuite. Le point bloquant (authentification VOD, donc option A) reste
ouvert.

### 9.1 Correctifs locaux — implémentés ✅

1. **Live fonctionnel** : le kind `antennereunion-live` (inexistant) est
   remplacé par un player `scraper-query`
   (`resolver > source: antennereunion-fr`, `resolver > target_id` =
   `{base_url}/proxy/channelStream?idChannel={channel_id}`), et `resolve_stream`
   poste désormais vers l'URL cible fournie par le player tout en
   reconstruisant son corps depuis ses paramètres :

   ```yaml
   query_url: "{url}"
   request_body_actions:
     - type: regex_find_all
       pattern: '[?&](idMedia|idChannel|idAsset)=([^&#]+)'
       format: '{1}={2}'
   ```

2. **Bande-annonce des épisodes** : `entries_episode` expose le même bloc
   `trailer` que `get_entry` ; faute d'`AD_LITRAILE`, aucun `target_id` n'est
   produit et le frontend (`normalizeEntryPlayerResolver`, qui exige `kind` et
   `targetId`) n'affiche pas d'action bande-annonce.

**Validation** : YAML rechargé (`yaml.safe_load`) et ancres résolues ; pipeline
`resolve_stream` rejoué contre l'API réelle avec les motifs exacts du fichier :

| Cible | Corps produit | `manifest_type` | Manifeste |
|---|---|---|---|
| `{base_url}/proxy/readTrailer?idMedia=76553` | `idMedia=76553` | `m3u8` | récupéré en 181 ms (`#EXTM3U`) |
| `{base_url}/proxy/channelStream?idChannel=69` | `idChannel=69` | `m3u8` | récupéré en 518 ms (`#EXTM3U`) |

### 9.2 Reste à faire — option A

Résolveur Rust `antennereunion-video` (et éventuellement un rafraîchissement du
live authentifié) : lecture des identifiants via `CredentialsStore`,
`POST /oauth/token` + `loginProfile`, `readAsset`, puis `getVodLicense` et proxy
de licence Widevine, exposition des `players` VOD dans `get_season`/`get_entry`,
et bandeau « compte requis » (`credentials: required: true` + `signup_url`).

---

## 10. Validations restantes (nécessitent un compte)

- Forme exacte des réponses `readAsset` (`{url, seek}`) et
  `getVodLicense` (`licParam.url`, `vendor`, `token`/`sessionToken`) avec un
  profil connecté, et type de manifeste obtenu (`mpd` Widevine ou `m3u8`
  FairPlay).
- Politique DRM réelle par contenu (tous les assets protégés ? uniquement les
  films ?) et niveau de protection (`digitalVideoProtectionLevel`).
- Comportement du 451 selon l'IP (routage pays du proxy, `proxy_country`).
- Chaînes de refresh des jetons (`X-AN-WebService-CustomerAuthToken` +
  `ProfileToken`) et durée de vie de session (`max-inactivity: 3600`).
- Prise en charge de la licence par le lecteur frontend
  (`license_url` / `widevine-license-proxy`).

---

## 11. Références

- YAML : `server/services/arachnea-stream/work-in-progress/antennereunion-fr.yaml`
- Contrat de résolution de flux : `docs/dev-tracking/yaml-query-stream-resolver-analysis.md`
- Contrat `get_stream` : `docs/specifications/arachnea-stream-fr.md` §5.9
- Réécritures proxy : `server/services/arachnea-stream-hoster/vidzy.yaml:43`,
  `server/crates/arachnea-proxy/src/core/http/actions/replace_all.rs`
- Résolveurs Rust existants : `server/crates/arachnea-stream/src/services/`
- Bundles analysés : `https://www.antennereunion.fr/_nuxt/HXiruTOG.js`
  (couche `BaseApi`, store `Player`), payload
  `window.__NUXT__.config.public`

---

## 12. Note de design — authentification et lecture VOD (base de l'option A)

Reverse complet du flux d'authentification Tucano dans `/_nuxt/HXiruTOG.js`
(vérifié le 2026-09-16). Toutes les valeurs ci-dessous sont confirmées et
constituent la spécification d'implémentation du résolveur Rust
`antennereunion-video`.

### 12.1 Hôtes et clés

| Élément | Valeur |
|---|---|
| API (oauth + crm) | `https://api.antennereunion-production.eu-west-3.alphanetworks.tv` |
| Proxies (endpoints lecture) | `https://proxies.antennereunion-production.eu-west-3.alphanetworks.tv` |
| IdentityKey / client_id (`apiWebKey`) | `HadCogoctandsxEDEsdufOcagidOad` |
| DRM provider déclaré | `castlabs` (`drm-protection: widevine`, `api-key: RIpiruj2SidlThIhodruraBrotH6st8h`) |
| Session | `max-inactivity: 3600` (1 h d'inactivité) |
| languageId | `fra` |

### 12.2 Séquence de login (site officiel)

1. **Login compte** — `POST {api}/oauth/token`,
   `Content-Type: application/x-www-form-urlencoded`, corps :
   `username=<email>&password=<pwd>&client_id=<apiWebKey>&grant_type=password&id_device=<deviceId>`.
   Réponse : `{ access_token, refresh_token, … }`. Le `access_token` devient le
   header `X-AN-WebService-CustomerAuthToken` de toutes les requêtes suivantes.
2. **Rafraîchissement** — `POST {api}/oauth/token` (même URL), cette fois en
   `Content-Type: application/json`, corps :
   `{ "refresh_token": …, "client_id": <apiWebKey>, "grant_type": "refresh_token", "id_device": <deviceId> }`.
   Réponse : nouveaux `access_token`/`refresh_token`.
3. **Liste des profils** — `GET {api}/crm/profile` avec le header
   `X-AN-WebService-CustomerAuthToken`. Réponse : liste de profils (`idProfile`, …).
4. **Activation d'un profil** — `POST {api}/crm/profile/active` corps
   `{ "idProfile": … }`. Réponse : `result.profileToken`, à porter dans le
   header `X-AN-WebService-ProfileToken`.
5. **Lecture** — `POST {proxies}/proxy/readAsset` (form-urlencoded, JSON pour
   les valeurs d'objets) avec `X-AN-WebService-IdentityKey` +
   `X-AN-WebService-CustomerAuthToken` + `X-AN-WebService-ProfileToken` :
   `idAsset=<id>&idAudioLang=<fra|eng>&idSubtitleLang=<non|…>&adsMacro=<json>`.
   `adsMacro` est l'objet `{ "page_url": …, "device_ua": … }` sérialisé en JSON.
   Réponse : `{ "url": <manifeste signé>, "seek": <secondes> }`.
6. **Licence VOD** — `POST {proxies}/proxy/getVodLicense`, mêmes headers :
   `idAsset=<id>&idAudioLang=…&idSubtitleLang=…`. Réponse : `{ "licParam": … }`.

`deviceId` : identifiant d'appareil généré localement par le client (format
UUID-like) et stable par installation ; côté résolveur, un UUID généré une
fois et persisté (ou dérivé de façon stable) suffit.

### 12.3 Traitement de la licence (`buildLicenseData` + player dash.js)

Depuis `licParam` (qui peut être une string query (`a=1&b=2`) ou un objet) :

- `url` = `licParam.url` avec préfixe `skd://` remplacé par `https://` ;
- `vendor` = `licParam.vendor` si présent, sinon `irdeto` ; si `url` contient
  `vudrm.tech`, vendor forcé à `vualto` ;
- `drmToken` = `licParam.sessionToken || licParam.token` ;
- URL de licence : vendor `vualto` → `<url>?token=<urlencoded(drmToken)>`,
  sinon `<url>` tel quel.

Transmission du token au serveur de licence selon le vendor (wrapper dash.js) :

| Vendor | Transmission |
|---|---|
| `irdeto` | header `Authorization: Bearer <drmToken>` |
| `castlabs` / `drmtoday` | header `x-dt-auth-token: <drmToken>` (+ `Content-Type: application/octet-stream`) |
| `vualto` | query param `?token=<drmToken>` (déjà injecté dans l'URL) |

`licParam.serverCertificateURL`, s'il est présent, est chargé comme
certificat DRM côté player.

### 12.4 Choix audio/sous-titres

Le site prend `AD_LIASMEDI[0].audio` / `.sub` (valeurs `fra`, `eng`, `non`,
…) avec repli `eng` / `non`. Le résolveur fera de même à partir des
métadonnées de l'asset, avec `fra`/`non` par défaut.

### 12.5 Implémentation (option A) — réalisée le 17 septembre 2026

1. `server/crates/arachnea-stream/src/services/antennereunion_resolver.rs`
   (nouveau) — implémente le même trait que `m6play_resolver.rs`
   (`PlayerStreamResolver` dans `services/player_resolver.rs`) :
   - `source_id: "antennereunion-fr"`, `resolver_ids: ["antennereunion-video"]` ;
   - `get_stream` : login (§12.2.1) → profils + `loginProfile` (§12.2.3-4)
     → `readAsset` (§12.2.5) → `getVodLicense` (§12.2.6) →
     `ResolvedPlayerStream` (`stream_url` = manifeste signé via le proxy HTTP
      générique, `manifest_type`, `license_url` selon §12.3). Le contrat
      actuel de `ResolvedPlayerStream` ne porte pas encore `assetStartTime` :
      la valeur `seek` renvoyée par la plateforme n'est donc pas exposée ;
   - cache de session (access/profile tokens) TTL < 1 h, comme le cache M6 ;
   - `get_drm_license` : pass-through du challenge Widevine vers l'URL de
     licence avec les headers du vendor (§12.3).
2. `services/mod.rs` + `stream_scraper.rs` : enregistrement dans
   `player_resolver_for_source` / `player_resolver_for_id`.
3. YAML `antennereunion-fr.yaml` : `credentials: required: true` +
   `signup_url` (https://www.antennereunion.fr/inscription), et bloc
   `players` sur les épisodes (`target_id` = `/idAsset`) et l'entry.
4. `CHANGELOG.md`, `docs/TODO.md` : option A implémentée.

### 12.6 Validation possible sans compte réel

- Étape login : vérifiable uniquement avec des identifiants réels.
- En revanche, la mécanique de licence (mapping vendor → headers) et le
  pass-through `get_drm_license` sont testables unitairement, sur le modèle
  des tests existants des autres résolveurs.


---

## 13. Enquête sur le blocage de la lecture (*playback stall*) — 17/09/2026

### État actuel et reproduction de la défaillance

L'endpoint local authentifié `get_stream` parvient à résoudre l'asset `28609` (l'épisode identifié au §4.1). Son MPD Broadpeak proxifié contient cinq périodes : quatre publicités durant respectivement 20,56, 30,52, 10,44 et 20,12 secondes, suivies du contenu du programme qui débute à 81,64 secondes. Les URL de base des périodes diffèrent, et les ID de représentation ainsi que les bandes passantes du programme diffèrent de ceux des publicités. Cette session est une reproduction indépendante ; le titre exact et le navigateur issus du rapport utilisateur n'ont pas encore été établis.

L'analyse (*parsing*) de ce MPD proxifié réel avec la version installée du frontend (`mpd-parser` 1.4.0) génère 25 playlists vidéo (cinq qualités par période), au lieu de cinq pistes de qualité continues. Elle produit également cinq playlists audio distinctes au sein du même groupe audio `en (main)`. La playlist 720p du programme commence à 81,64 secondes, tandis que la première playlist audio ne couvre que la première publicité à l'instant zéro.

La cause est visible dans `mpd-parser/src/toM3u8.js`, `mergeDiscontiguousPlaylists` : les playlists sont regroupées par URL de base, puis fusionnées par ID de représentation et par langue. Des URL de périodes différentes empêchent la fusion, même lorsque les ID de représentation des publicités correspondent ; la différence d'ID des programmes constitue un obstacle supplémentaire. L'implémentation mentionne explicitement son hypothèse selon laquelle les ID de représentation restent identiques d'une période à l'autre.

Les requêtes signalées par l'utilisateur montrent la même association caractéristique : l'initialisation de la vidéo du programme (`bpk-vod/...-video=2395600.dash`) côtoie l'initialisation de l'audio de la publicité et son premier segment (`bpkio-jitt/...index-audio_0=96000...`). Cela vient fortement appuyer l'hypothèse d'un désalignement temporel audio/vidéo comme mécanisme de blocage (*stall*). Aucune lecture de bout en bout au niveau du navigateur n'a été validée au cours de cette enquête.

### Vérification du réseau et limitations

Pour l'épisode résolu de manière indépendante, l'initialisation de la vidéo du programme, l'initialisation de l'audio de la première publicité et le premier segment audio renvoient tous un code HTTP 200 via le proxy local (respectivement 818, 695 et 49 393 octets), après avoir suivi les redirections publicitaires. Le MPD utilise `SegmentTemplate` et `SegmentTimeline`, ces requêtes initiales ne dépendent donc pas d'index d'intervalles d'octets (*byte-range*). Aucune URL de licence DRM n'a été renvoyée pour cet échantillon ; cela ne valide pas les assets chiffrés. Le succès de la récupération ne suffit donc pas, à lui seul, à établir que le flux média est lisible.

### Options d'implémentation et impacts

1. **Recommandé : utiliser un moteur DASH prenant en charge les périodes hétérogènes**, tel que dash.js, derrière l'interface de lecteur existante. Conserver l'interface utilisateur Video.js là où cela est possible et utiliser un gestionnaire/adaptateur de source DASH pour les sources MPD. Adapter la gestion des DRM, la sélection de la qualité, les pistes audio/sous-titres, la destruction des sources, le déplacement dans la lecture (*seeking*) et le transfert des événements de lecture vers le nouveau moteur. Le format HLS et les médias progressifs peuvent conserver le flux actuel. Cela ajoute une dépendance et modifie l'architecture de lecture du frontend ; une approbation dans le fichier `AGENTS.md` à la racine est requise avant toute implémentation.
2. **Étudier une version HLS fournie officiellement** pour cette source. Cela pourrait limiter l'impact sur le frontend, mais nécessite de vérifier les paramètres de session authentifiée, les transitions publicitaires et la compatibilité DRM. Le simple remplacement de `.mpd` par `.m3u8` ne constitue pas une correction validée.
3. **Normaliser le MPD pour VHS** en réécrivant les identités de représentation et les URL des périodes. Cela est fragile : les ID interviennent dans la substitution `$RepresentationID$`, et l'audio, les codecs, les DRM ainsi que les transitions temporelles nécessitent également un traitement cohérent. De simples remplacements d'URL de base par expressions régulières ne peuvent pas réparer le modèle de périodes du parseur.

Ne supprimez pas les publicités ou les paramètres de session en guise de solution de contournement pour la lecture. La réécriture existante des URL de base par le proxy reste utile pour maintenir les requêtes de segments à l'intérieur du proxy, mais elle ne résout pas la lecture des périodes hétérogènes.

### Validation requise après une implémentation approuvée

Vérifiez le démarrage de la lecture avec la synchronisation audio/vidéo des publicités, chaque transition de période vers le programme, le déplacement dans la lecture (*seeking*) à travers les limites, le changement de qualité, le changement/la destruction de source et la propagation des erreurs. Validez également un asset chiffré lorsqu'il sera disponible, ainsi que la lecture HLS existante. Aucun nouveau test n'a été ajouté au cours de cette enquête, conformément aux instructions du dépôt.

Références :

* [Fonctionnalités VHS prises en charge et manquantes](https://github.com/videojs/http-streaming/blob/main/docs/supported-features.md)
* [Prise en charge multi-périodes de dash.js](https://dashif.org/dash.js/pages/usage/multiperiod.html)

### Confirmation du MPD fourni par l'utilisateur

Le fichier `/Users/jdecker/Downloads/index.mpd` fourni ultérieurement fait référence au même identifiant de média programme `722949-caa305f7-6fee-4088-9af0-cd549be19493` que les requêtes signalées. Il contient 35 périodes et annonce une durée totale de 1:55:08.341333333. Aucun élément `ContentProtection` n'est présent.

L'analyse de ce fichier avec le `mpd-parser` installé produit 35 playlists vidéo et sept playlists audio. Celles-ci sont regroupées par URL de base et identité de représentation partagées, et non assemblées en cinq pistes de qualité séquentielles complètes. Dans cet échantillon, les URL publicitaires récurrentes se fusionnent sur certaines périodes ; par conséquent, toutes les périodes ne deviennent pas une playlist distincte, contrairement à la reproduction précédente sur cinq périodes. La fragmentation fondamentale demeure.

La playlist programme `video=2395600` commence à 81,64 secondes et fusionne six périodes de programme, tandis que la première playlist `audio_0=96000` commence à zéro et fusionne six occurrences d'une publicité. Cela confirme la structure de playlist incompatible sur le fichier réel de l'utilisateur et renforce l'explication concernant l'appariement observé entre la vidéo du programme et l'audio de la publicité. La sélection réelle par le navigateur et la lecture n'ont toujours pas été tracées. Le fichier conserve des URL de base relatives ; la réécriture proxy existante est nécessaire indépendamment du correctif du lecteur.
