# Analyse : migration des hosters vStream Python vers les résolveurs YAML

> Créée le 2026-07-14.  
> Périmètre : `docs/private/plugin.video.vstream-3.9.2/plugin.video.vstream/resources/hosters/*.py`.  
> Inventaire : 146 fichiers, dont 144 classes de hoster et deux modules de support.

## Objectif

Migrer progressivement les résolveurs historiques vStream vers des fichiers YAML dans
`server/services/arachnea-stream-resolver/`. Chaque résolveur doit proposer `can_resolve_url`,
`resolve_stream` et la réponse publique `get_stream` déjà stabilisée.

Cette analyse est un plan d’implémentation et un tableau de suivi, pas une validation de la
disponibilité actuelle des hosters. Les extracteurs proviennent de vStream 3.9.2 : domaines,
marquage HTML, APIs et protections doivent être revérifiés au moment de chaque migration.

Références à appliquer :

- [spécification de flux française](../specifications/arachnea-stream-fr.md) et
  [anglaise](../specifications/arachnea-stream-en.md) ;
- [spécification Scrapyfy française](../specifications/arachnea-scrapyfy-fr.md) et
  [anglaise](../specifications/arachnea-scrapyfy-en.md), pour `request_headers`,
  `get_response_body`, `resolve_url` et `suffix` ;
- [résolveur de référence Sibnet](../../server/services/arachnea-stream-resolver/sibnet.yaml).

## Fondations existantes à conserver

Les fondations ci-dessous, transférées de l’analyse précédente, sont déjà implémentées. Toute
nouvelle migration de hoster doit les utiliser sans introduire de sélection par domaine dans Rust.

- Le groupe `arachnea-stream-resolver` est chargé via
  `server/services/arachnea-stream-resolver/services.json`. Chaque YAML déclare obligatoirement
  `can_resolve_url` et `resolve_stream`. En cas de plusieurs correspondances, le premier service
  activé dans `services.json` est retenu : son ordre est donc une priorité fonctionnelle.
- `StreamResolver`, dans `server/crates/arachnea-stream/src/stream_resolver.rs`, valide l’URL
  HTTP(S), agrège `can_resolve_url`, exécute uniquement le `resolve_stream` du service retenu et
  convertit le résultat en `ResolvedPlayerStream`. Le résolveur générique est enregistré sous
  l’identifiant global `stream-resolver`.
- L’absence de service compatible retourne le fallback
  `{ "embed-link": "<url-validée>" }`. À l’inverse, l’échec d’extraction d’un service déjà
  sélectionné reste une erreur contextualisée : il ne doit jamais être masqué par un iframe.
- `get_stream` est une union exclusive : soit `stream_url: string[]` avec les métadonnées de
  lecture, soit `embed-link`. Les URLs sont classées par préférence et le front essaie la suivante
  après une erreur média, sans rappeler le backend.
- La réponse de flux peut inclure `manifest_type`, `title`,
  `"image/title": { "link": "…" }`, `license_url`, `license_headers`, `storyboard_vtt_url` et
  `storyboard`. `stream_headers` est volontairement omis du JSON public après avoir été encodé
  dans l’URL proxy. `license_headers` reste public, car il est utilisé pour une licence DRM.
- Le frontend normalise cette réponse dans `front/src/services/rustify.ts`. Le poster
  `image/title.link` est prioritaire sur l’aperçu de l’épisode ; `storyboard_vtt_url` est prioritaire sur le
  storyboard ; un storyboard séquentiel utilise `rows`, le placeholder `{index}` et, au besoin,
  `first_page_index`. Lorsque `interval` est absent, le frontend le calcule à partir de la durée
  vidéo, des lignes et des colonnes.
- Un lecteur YAML utilise le format plat `{ resolver, target }`. Le format legacy
  `resolver > kind` / `resolver > target_id` demeure pris en charge pour les résolveurs légaux,
  mais aucune nouvelle migration de hoster ne doit le produire.
- Les routes DRM sont séparées : `get_stream` retourne le JSON de lecture et
  `get_drm_license` gère le proxy de licence. Les paramètres propres aux résolveurs légaux
  restent internes au backend.
- Les tests de base existent dans
  `server/crates/arachnea-stream/src/stream_resolver_tests.rs` : détection Sibnet, URL inconnue
  avec fallback iframe et rejet des URLs non HTTP(S). La validation d’un YAML ajouté s’appuie sur
  ces contrôles existants et sur une URL réelle autorisée ; ne pas créer de nouveaux tests sans
  demande explicite.
- Le storyboard M6+ est désormais produit par son résolveur spécialisé, et non par les
  `players[]` du YAML de catalogue. Les lecteurs directs ou intégrés qui ne passent pas par
  `get_stream` peuvent conserver leur storyboard de catalogue.

### Contrat structurel des storyboards

Le nœud YAML `storyboard` décrit le découpage de l’image sprite, et non la taille d’affichage du
tooltip du lecteur. Ses champs sont :

| Champ | Rôle | Valeur par défaut / règle |
|---|---|---|
| `url` | URL de l’image sprite, éventuellement avec `{index}` | Obligatoire |
| `width`, `height` | Largeur et hauteur d’une seule imagette dans le sprite | Obligatoires ; utilisés pour le cadrage source |
| `columns`, `rows` | Nombre d’imagettes par ligne et par image sprite | `rows` doit être déclaré, y compris `1` pour une image unique |
| `first_page_index` | Numéro du premier fichier lorsque `url` contient `{index}` | Optionnel, `0` par défaut |
| `interval` | Nombre de secondes entre deux imagettes | Optionnel ; le frontend calcule `durée_vidéo / (rows × columns)` lorsqu’il est absent |

`first_index` est obsolète et ne doit plus être déclaré. Il n’existe pas d’index d’imagette interne :
la première case du premier sprite correspond toujours au début de la vidéo. Le frontend conserve
une taille de tooltip fixe, indépendamment de `width` et `height`.

## Portée et décisions

- `sibnet.py` est déjà migré par `sibnet.yaml`. Il sert de référence de structure, non
  d’extracteur universel.
- `__init__.py` et `hoster.py` ne sont pas des hosters migrables.
- Un fichier Python ne correspond pas forcément à un domaine unique. La regex
  `can_resolve_url` doit être obtenue des URLs actuelles et de la sélection vStream, puis testée.
- Les débrideurs, secrets, authentification, CAPTCHA, JavaScript évalué, génération aléatoire,
  horodatage et boucles de redirections conditionnelles restent hors YAML tant qu’une primitive
  Scrapyfy générique, testable et utilisée par plusieurs résolveurs n’existe pas.
- Aucun cookie ou en-tête sensible ne doit être renvoyé au navigateur. Les `stream_headers` restent
  internes : ils servent à construire les URLs proxy et ne sont pas sérialisés par `get_stream`.
- « ✅ Migré » implique un YAML activé dans `services.json`, une URL de test autorisée, les
  vérifications de détection/résolution/lecture, et les métadonnées validées.

## Contrat de migration

Chaque YAML commence par le même contrat ; seuls les domaines, requêtes et extracteurs changent.

```yaml
id: example-hoster
title: Example hoster
description:
  en: "Resolver for example.invalid"
http:
  mode: auto

# ############################################################################
# Parameters
# ############################################################################ 
parameters:
  - name: url
    value: ""
    description: "URL of the embed page to be resolved"


# ############################################################################
# Queries
# ############################################################################
queries:

  # --------------------------------------------------------------------------
  # can_resolve_url — determines whether this rule can resolve the URL
  # --------------------------------------------------------------------------
  - name: can_resolve_url
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{url}"
        actions:
          - type: regex_find_all
            pattern: '^https?://(?:www\\.)?example\\.invalid/'
            format: "{service_id}"


  # --------------------------------------------------------------------------
  # resolve_stream — extracts the video stream URL from the player page
  # --------------------------------------------------------------------------
  - name: resolve_stream
    scraper_type: html
    base_url: "{url}"
    query_url: "{url}"
    row_selector: "html"
    request_headers:
      - name: Referer
        actions:
          - type: format_text
            argument: "{url}"
    entries:
      - name: stream_url
        type: string[]
        actions:
          - type: get_response_body
          - type: regex_find_all
            pattern: '...'
            format: "{1}"
          - type: resolve_url
      - name: manifest_type
        type: string
        actions:
          - type: format_text
            argument: mp4
```

| Comportement vStream | Portage requis | Validation |
|---|---|---|
| Page + regex `sources`, `file`, `src` ou JSON intégré | `resolve_stream` HTML/texte, `get_response_body`, `regex_find_all`, `resolve_url` | Au moins une `stream_url` absolue et lisible |
| Plusieurs qualités/miroirs | `stream_url: string[]` dans l’ordre de préférence | Le front tente les URLs suivantes après erreur média |
| Referer / User-Agent CDN | Objet `stream_headers` dans le YAML | Présents dans l’URL proxy, absents du JSON public |
| Sous-requête déterministe | Sous-requête YAML, paramètres et en-têtes explicites | URL, Referer et cookies vérifiés de bout en bout |
| Titre | `title`, d’abord `meta[property='og:title']` | Facultatif, mais valeur réelle si déclarée |
| Image du titre | `image/title > link`, d’abord `og:image` ; `resolve_url` et `proxy: true` si nécessaire | JSON exact : `"image/title": { "link": "…" }` ; utilisée comme poster front |
| Vignettes VTT | `storyboard_vtt_url` | Prioritaire sur le storyboard |
| Sprite unique | `storyboard.url`, `width`, `height`, `columns`, `rows: 1`, `interval` optionnel | Dimensions observées ; intervalle fourni ou calculé depuis la durée vidéo |
| Suite de sprites | Même structure avec placeholder littéral `{index}`, `rows` et `first_page_index` éventuel | Vérifier au minimum les deux premières images et leur numérotation |
| JS packé, token temps/aléa, CAPTCHA | Ne pas porter directement ; proposer une primitive commune ou un résolveur dédié | Tests et au moins deux consommateurs envisagés |
| Débrideur/API authentifiée | Hors périmètre du groupe anonyme | Aucun secret/cookie utilisateur dans YAML ou JSON |

## Procédure obligatoire par ligne

1. Relever dans le Python les domaines, réécritures, requêtes, méthodes, redirections, cookies,
   en-têtes, expressions d’extraction et ordre des qualités.
2. Conserver une URL de test autorisée, une date de contrôle et le résultat attendu.
3. Classer : YAML direct, YAML avec sous-requête, primitive Scrapyfy à proposer, résolveur Rust
   spécialisé, ou hors périmètre.
4. Créer le YAML à partir de `sibnet.yaml` sans introduire de branchement par domaine en Rust.
5. Ajouter `title`, `image/title > link`, `storyboard_vtt_url` et `storyboard` seulement lorsqu’ils sont
   réellement présents et testés.
6. Ajouter le YAML dans `services.json`. Les règles les plus spécifiques doivent précéder les
   règles générales afin de maîtriser les collisions.
7. Tester `can_resolve_url`, `get_stream`, l’URL proxy, les URLs alternatives, le poster et les
   miniatures, puis mettre à jour le tableau.

## Extensions Scrapyfy à cadrer avant implémentation

Une primitive ne doit pas être créée pour une seule particularité historique. Toute proposition
doit spécifier ses entrées/sorties, cookies/redirections, consommateurs et tests. Les besoins à
mutualiser avant extension sont :

- lecture de `Location` après une requête sans redirection ;
- passage contrôlé de cookies entre sous-requêtes, jamais vers le client ;
- transformation JavaScript déterministe et documentée ;
- génération de token basée sur temps/aléa, seulement si plusieurs hosters la partagent ;
- protection anti-bot/CAPTCHA, généralement hors du résolveur YAML.

### Action déterministe `unpack_packer`

Le lecteur `https://minochinos.com/embed/trz6gf7j38ej` est une variante VidHide / Earnvids.
Son script contient un unique paquet Dean Edwards Packer :
`eval(function(p,a,c,k,e,d){...}('payload', 36, 486, 'symbols'.split('|')))`. Il ne
nécessite pas l'exécution du lecteur JWPlayer : `filelions.py` dans vStream dépile ce format
avec `cPacker`, puis extrait `sources[0].file` ou `hls2` / `hls4`.

La primitive ajoutée dans `arachnea-scrapyfy` est une action sans argument
`unpack_packer`, et non une action générique `eval` :

- elle accepte seulement les appels Packer dont le payload, le dictionnaire, le radix, le nombre
  de symboles et le séparateur de `split` sont littéraux ; elle ne doit jamais exécuter du
  JavaScript arbitraire ;
- elle décode le dictionnaire par remplacement de mots entiers, avec les bases 2 à 62 nécessaires
  au format Packer ;
- elle retourne une valeur vide lorsque le texte n'est pas un paquet Packer valide, afin que
  l'extraction YAML échoue explicitement par absence de `stream_url` ;
- son implémentation reste dans `scrapyfy/actions/`, est ajoutée à `ScraperAction` et à son
  dispatch, puis est documentée dans les spécifications Scrapyfy française et anglaise.

Le YAML `vidhide.yaml` reste déclaratif :

```yaml
- name: stream_url
  type: string[]
  actions:
    - type: get_response_body
    - type: unpack_packer
    - type: regex_find_all
      pattern: '(?:sources:\\s*:\\s*\\[\\s*\\{\\s*file\\s*:\\s*["'']|["'']hls[234]["'']\\s*:\\s*["''])([^"'']+)'
      format: "{1}"
    - type: resolve_url
```

`manifest_type` sera `hls`. Le YAML ajoutera `Referer: {url}` dans `stream_headers`, le titre
depuis `meta[name='description']`, le poster depuis l'image initiale de `#vplayer`, et `storyboard_vtt_url`
si l'URL de thumbnails VTT est confirmée dans la sortie dépilée. Il ne doit pas déclarer les
cookies `file_id` et `aff` : ils sont écrits pour le navigateur par le script de publicité et ne
sont pas utilisés par le chemin vStream de résolution.

La validation à exécuter après accord est : résolution de l'URL fournie, lecture du manifeste et
d'un segment avec le Referer proxy, contrôle de l'absence de cookies dans le JSON public, puis
mise à jour de la ligne de suivi avec la date et les métadonnées réellement observées. Les tests
automatisés ne sont pas ajoutés sans demande explicite.

Le statut « ⚠️ mécanisme impératif détecté » vient d’un pré-audit statique
(`cPacker`, `eval`, temps/aléa, API/débrideur ou protection analogue). C’est une priorité
d’examen, pas une preuve que le YAML est impossible.

## Constats déjà établis pour la première vague

| Hoster | Constat issu du code ou de l’analyse existante | Décision de migration |
|---|---|---|
| `sibnet.py` | Extraction HTML directe, Referer requis, métadonnées Open Graph et sprites paginés | Référence déjà migrée ; maintenir et valider à chaque évolution du site |
| `uqload.py` | L’analyse existante relève un GET puis une regex sur `sources` pour un MP4 | Candidat YAML direct après vérification de domaine et de page actuelle |
| `streamtape.py` | Réécriture de domaine, cookie, requête sans redirection et lecture de `Location` | Attendre ou proposer une primitive générique de lecture de `Location` et de gestion contrôlée des cookies |
| `dood.py` | Iframe éventuel, requêtes multiples, suffixe aléatoire et horodatage | Hors YAML direct ; ne traiter qu’après une primitive commune ou un résolveur spécialisé |
| `filemoon.py`, `iframe_secure.py`, `iframe_secured.py` | Dépaquetage/évaluation JavaScript dans le code vStream | Ne pas traduire le JavaScript en regex YAML ; qualifier un mécanisme partagé ou écarter |
| Débrideurs (`alldebrid.py`, `realdebrid.py`, `debrid_link.py`) | API et/ou identifiants utilisateur | Hors périmètre du résolveur YAML anonyme |
| `embed4me.com` (hors vStream) | Vite SPA avec Vidstack HLS, API AES-CBC (clé/IV dérivés de `window.location`), Cloudflare, IMA ads | Hors YAML direct sans primitive Scrapyfy AES-CBC ou résolveur Rust spécialisé ; deux endpoints `/api/v1/info` et `/api/v1/download` chiffrés |

## Tableau de suivi

Légende : ✅ migré et vérifié ; ⬜ à auditer ; ⚠️ mécanisme impératif détecté ; — hors champ.
Les colonnes storyboard et image/titre indiquent ce qui doit être constaté sur le hoster courant :
elles ne supposent pas que ces métadonnées existent. La colonne « Sonde `/e`/`/v` » marque les
cinq résolveurs sélectionnables après l’inspection HTML dynamique de vStream ; « — » signifie
qu’aucune de ces signatures n’est gérée par ce fallback.

Le fallback reconnaît également une redirection JavaScript `Redirecting...`, puis relance la
sélection complète sur l’URL obtenue. Ce chemin n’appartient à aucun fichier Python unique ; il
est donc documenté ici plutôt que rattaché artificiellement à `allow_redirects.py`.

| Fichier Python | Migration vers YAML | Storyboard | `image/title` | Sonde `/e`/`/v` |
|---|---|---|---|---|
| `1fichier.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `__init__.py` | — Module d’initialisation ; ne pas migrer | — | — | — |
| `abcvideo.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `alldebrid.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `allow_redirects.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `aparat.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `archive.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `clickopen.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `clipwatching.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `cloudhost.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `cloudvid.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `daclips.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `dailymotion.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `daisukianime.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `darkibox.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `ddlfr.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `debrid_link.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `directmoviedl.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `dood.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | ✅ Signature Dood CDN |
| `downace.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `dustreaming.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `dwfull.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `estream.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `evoload.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `facebook.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `filelions.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | ✅ Signature Vidhide → Earnvids |
| `filemoon.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | ✅ Signature Filemoon |
| `filepup.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `filetrip.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `flashx.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `flix555.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `frenchvid.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `fsvid.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `giga.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `gofile.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `googledrive.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `googlevideo.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `gorillavid.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `gounlimited.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `hd_stream.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `hdvid.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `hoster.py` | — Classe de base vStream ; référence seulement | — | — | — |
| `iframe_secure.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `iframe_secured.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `jawcloud.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `jetload.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `kvid.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `letsupload.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `letwatch.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `lien_direct.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `lulustream.py` | ✅ Migré par `lulustream.yaml` ; page JWPlayer avec Packer, extraction HLS via `unpack_packer`, poster `meta[name='og:image']` proxy, titre `<title>` | ⬜ Aucun storyboard observé sur le hoster courant | ✅ `meta[name='og:image']` → `image/title > link` avec proxy | — |
| `mailru.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `megadrive.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `megaup.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `megawatch.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `mixcloud.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `minochinos.py` (VidHide) | ⬜ YAML `vidhide.yaml` activé ; validation réelle du Packer et du flux HLS en attente | ✅ `get_slides` VTT extrait depuis le script dépilé | ✅ image `#vplayer img` → `image/title > link` | ✅ Signature VidHide / Earnvids |
| `mixdrop.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `mixloads.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `mystream.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `myvi.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `netu.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `ninjastream.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `ok_ru.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `oneupload.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `onevideo.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `onlystream.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `pdj.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `playreplay.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `playtube.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `plynow.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `prostream.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `pstream.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `rapidstream.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `realdebrid.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `resolver.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `rutube.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `sendvid.py` | ✅ Migré par `sendvid.yaml` ; extraction `og:video`, poster `og:image`, storyboard `thumbnailsSprite` | ✅ `rows: 1`, `columns: 21`, `interval: 71` | ✅ OG image → `image/title > link` | — |
| `sibnet.py` | ✅ Migré par `sibnet.yaml` ; maintenir avec une URL réelle | ✅ `rows: 6`, `first_page_index: 1` | ✅ OG image → `image/title > link` | — |
| `smoothpre.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `soundcloud.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `speedvid.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `speedvideo.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `stagevu.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `streamax.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `streamhide.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `streamlare.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `streamtape.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `streamwish.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `streamz.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `supervideo.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `thevideo_me.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `tomacloud.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `tune.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `turbovid.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `up2stream.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `uplea.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `uploaded.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `upstream.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `uptobox.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `uptostream.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `upvid.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `upvideo.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `uqload.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `userload.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `verystream.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vf-manga.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `vidbem.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidbm.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidbom.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `vidbull.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `vidcloud.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `videobin.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `videovard.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidfast.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidguard.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | ✅ Signature Guardstorage |
| `vidia.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `vidload.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidlox.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidmoly.py` | ✅ Migré par `vidmoly.yaml` ; extraction `sources: [{ file: ... }]`, poster `image:`, storyboard VTT `slides`, titre `<title>` | ✅ `storyboard_vtt_url` via `/api/v1/slides` | ✅ `image:` JWPlayer → `image/title > link` avec proxy | — |
| `vido.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidoza.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidplayer.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `vidshar.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidto.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `vidtodo.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `vidup.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidwatch.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidzi.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `vidzstore.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vidzy.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `viewsb.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `viki.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vimeo.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vimple.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vk.py` | ✅ Migré par `vk.yaml` ; validé le 2026-07-16 sur `video_ext.php` : extraction du manifeste DASH `dash_sep` du bloc JSON `files`, segments MPD réécrits vers le proxy avec le Referer, titre JSON et poster `first_frame` | ✅ `timeline_thumbs` : dimensions et intervalle extraits, `rows` calculé après construction (`count_per_image / count_per_row`), séquence `uidx={index}` proxifiée | ✅ `first_frame[0].url` → `image/title > link` | — |
| `voe.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | ✅ Signature Voe |
| `vshare.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `vudeo.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `vupload.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `watchvideo.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `wholecloud.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `wstream.py` | ⚠️ À qualifier — mécanisme impératif détecté | ⬜ À auditer | ⬜ À auditer | — |
| `xdrive.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `xtremestream.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `yourvid.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `youtube.py` | ⬜ À qualifier — extraction/domaines à relever | ⬜ À auditer | ⬜ À auditer | — |
| `embed4me.com` (hors vStream) | ⚠️ Nouvel hoster — AES-CBC client-side, SPA Vite/Vidstack, Cloudflare, IMA ads. Poster et titre extraits du JSON déchiffré. Hors YAML direct. | ✅ Poster PNG confirmé | ✅ Titre extrait du JSON | — |



## Ordre recommandé

1. Conserver Sibnet comme référence de test : poster et sprites paginés inclus.
2. Auditer d’abord les hosters à extraction HTML/JSON directe et sans état.
3. Regrouper les hosters partageant réellement une plateforme ; ne pas fusionner des séquences HTTP
   différentes dans un YAML générique.
4. Traiter ensuite sous-requêtes déterministes et redirections simples.
5. Isoler les primitives communes avant les hosters JavaScript, protégés ou authentifiés.
6. Ne jamais ajouter un domaine à `can_resolve_url` sans `resolve_stream` correspondant et testé.

## Critères de clôture

Une ligne passe à « ✅ » seulement si le YAML, `services.json`, les tests pertinents et les
spécifications sont alignés. La mise à jour de la ligne doit alors conserver le YAML créé, les
domaines couverts, la date de validation, les en-têtes proxy, et la présence ou absence vérifiée de
`title`, `image/title`, `storyboard_vtt_url` et `storyboard`.
