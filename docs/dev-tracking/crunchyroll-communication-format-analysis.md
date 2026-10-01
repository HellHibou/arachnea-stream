# Analyse préliminaire du format de communication Crunchyroll

> Créée le 2026-09-25.  
> Source analysée : `docs/private/crunchyroll-downloader-1.5.1/`, version 1.5.1.  
> Périmètre initial : analyse statique des fichiers Go locaux. Les compléments sont ajoutés au fur
> et à mesure, avec leur provenance et leur niveau de validation.

## 1. Objectif

Cette analyse prépare la création d’un nouveau service `arachnea-stream` pour Crunchyroll. Elle
documente le protocole observé dans le client Go de référence : authentification, catalogue,
sélection des versions audio, ouverture d’une session de lecture, manifeste DASH, sous-titres,
licence Widevine, accès CDN et fermeture de session.

Le code étudié est un **téléchargeur**, pas un lecteur Arachnea. Certaines opérations locales
(sélection d’une représentation, téléchargement des segments, déchiffrement et assemblage MKV)
ne devront donc pas être reproduites telles quelles. Pour Arachnea, la cible est plutôt de fournir
au lecteur un MPD, un proxy de licence et les pistes de sous-titres compatibles.

Les endpoints décrits sont non documentés publiquement dans cette source et peuvent changer. Une
capture réseau récente devra confirmer le contrat avant l’implémentation.

Le projet retient comme contrainte fonctionnelle fournie que **Crunchyroll n’applique aucune
géorestriction dans le périmètre visé par ce service**. Le service n’a donc pas à sélectionner un
pays de sortie, à annoncer un territoire requis ou à gérer un fallback géographique.

## 2. Vue d’ensemble

La communication observée suit cette chaîne :

1. récupérer un cookie de session utilisateur `etp_rt` depuis une session Crunchyroll connectée ;
2. échanger ce cookie contre un jeton d’accès Bearer ;
3. interroger l’API CMS pour obtenir séries, saisons, épisodes et variantes audio ;
4. ouvrir une session de lecture pour le GUID de l’épisode et de la langue audio voulue ;
5. récupérer dans la réponse l’URL du manifeste DASH, les sous-titres et un jeton vidéo ;
6. charger et analyser le MPD ;
7. transmettre le challenge Widevine avec l’identifiant de contenu et le jeton vidéo ;
8. laisser le lecteur récupérer les médias depuis le CDN avec les en-têtes attendus ;
9. supprimer explicitement la session de lecture à la fin.

```text
etp_rt
  │
  ▼
POST /auth/v1/token ───────────────► access_token
  │
  ├─► GET /content/v2/cms/... ─────► catalogue + GUID par langue audio
  │
  └─► GET /playback/v3/{guid}/... ─► MPD + sous-titres + video token
                                         │
                                         ├─► GET MPD / segments CDN
                                         ├─► POST /license/v1/license/widevine
                                         └─► DELETE /playback/v1/token/{guid}/{video_token}
```

## 3. Conventions HTTP communes

### 3.1 Hôte API

Les appels d’authentification, de catalogue, de lecture et de licence utilisent tous :

```text
https://www.crunchyroll.com
```

Les URL de manifeste, de segments et de sous-titres sont fournies dynamiquement par les réponses
de lecture et le MPD ; le code ne fixe pas leur domaine.

### 3.2 User-Agent

Le client Go force sur presque toutes les requêtes un User-Agent Firefox Linux. Le numéro de
version exact est probablement accidentel et ne doit pas devenir une constante fonctionnelle du
service : Arachnea devrait utiliser son profil Firefox configuré.

### 3.3 Bearer token

Après authentification, les appels CMS, playback, manifeste et licence envoient :

```http
Authorization: Bearer <access_token>
```

Le client de référence renouvelle le jeton et rejoue automatiquement la requête lorsqu’il reçoit
un statut HTTP `401 Unauthorized`. Pour les requêtes avec corps, le corps doit être recréé avant
le nouvel envoi ; cela concerne notamment le challenge Widevine binaire.

### 3.4 En-têtes CDN et licence

Les segments, fichiers « on demand », sous-titres et licences utilisent le contexte suivant :

```http
Origin: https://static.crunchyroll.com
Referer: https://static.crunchyroll.com/
User-Agent: <profil Firefox>
```

Le téléchargement des segments n’ajoute pas de Bearer token dans le client observé. Le manifeste
est en revanche demandé avec le Bearer token.

## 4. Authentification

### 4.1 Entrée nécessaire

Le client exige la valeur du cookie de session :

```text
etp_rt=<secret utilisateur>
```

Cette valeur donne accès au compte et doit être traitée comme un secret. Elle ne doit jamais être
écrite dans le YAML, les logs, le changelog ou les erreurs publiques.

### 4.2 Échange du cookie contre un access token

**Requête**

```http
POST https://www.crunchyroll.com/auth/v1/token
Authorization: Basic <identifiant client web observé dans la source privée>
Content-Type: application/x-www-form-urlencoded
User-Agent: <profil Firefox>
Cookie: device_id=<uuid>; etp_rt=<secret utilisateur>
```

Corps encodé en formulaire :

```text
device_id=<uuid>&device_type=Firefox+on+Linux&grant_type=etp_rt_cookie
```

| Champ | Format observé | Rôle |
|---|---|---|
| `device_id` | UUID généré au démarrage | Identifie l’appareil logique ; envoyé aussi comme cookie |
| `device_type` | `Firefox on Linux` | Description déclarative du client |
| `grant_type` | `etp_rt_cookie` | Demande un jeton à partir du cookie `etp_rt` |

Le code génère un seul `device_id` par exécution et le réutilise pendant toute cette exécution.
Pour un service persistant, il est préférable de conserver un identifiant stable par installation
ou par compte tant qu’une capture réseau n’a pas prouvé qu’une rotation est attendue.

**Réponse JSON minimale consommée**

```json
{
  "access_token": "<bearer token>"
}
```

La source ne modélise ni `expires_in`, ni `refresh_token`, ni le type du jeton. Elle se contente de
renouveler l’accès après un `401`. L’implémentation Arachnea devra conserver la réponse complète
lors des essais afin de vérifier si une expiration explicite peut éviter les requêtes échouées.

### 4.3 Conséquence pour Arachnea

Le modèle actuel de service devra pouvoir stocker un secret de type cookie/session, et pas
seulement supposer un couple identifiant/mot de passe. Une option de transition serait d’utiliser
le champ secret existant pour `etp_rt`, mais le libellé de l’interface et le contrat du résolveur
devraient être explicites.

## 5. API de catalogue CMS

Toutes les réponses observées sont en JSON et utilisent un tableau racine `data`.

### 5.1 Métadonnées d’un épisode

```http
GET /content/v2/cms/objects/{content_id}?ratings=true&preferred_audio_language=ja-JP&locale=en-US
Authorization: Bearer <access_token>
```

Structure consommée :

```json
{
  "data": [
    {
      "title": "Episode title",
      "episode_metadata": {
        "audio_locale": "ja-JP",
        "episode_number": 1,
        "season_number": 1,
        "series_title": "Series title",
        "availability_starts": "<date>",
        "versions": [
          {
            "audio_locale": "ja-JP",
            "guid": "<playback content GUID>"
          }
        ]
      }
    }
  ]
}
```

Le premier élément de `data` est utilisé sans contrôle supplémentaire dans le client Go.

### 5.2 Liste des saisons d’une série

```http
GET /content/v2/cms/series/{series_id}/seasons?force_locale=&preferred_audio_language={audio_locale}&locale={ui_locale}
Authorization: Bearer <access_token>
```

Valeurs par défaut du client :

- `preferred_audio_language=ja-JP` ;
- `locale=en-US`.

Structure minimale consommée :

```json
{
  "data": [
    {
      "id": "<season_id>",
      "season_number": 1
    }
  ]
}
```

### 5.3 Liste des épisodes d’une saison

```http
GET /content/v2/cms/seasons/{season_id}/episodes?preferred_audio_language={audio_locale}&locale={ui_locale}
Authorization: Bearer <access_token>
```

Structure minimale consommée :

```json
{
  "data": [
    {
      "id": "<episode_id>",
      "versions": [
        {
          "audio_locale": "fr-FR",
          "guid": "<playback content GUID>"
        }
      ],
      "season_number": 1,
      "episode_number": 1,
      "series_title": "Series title",
      "audio_locale": "ja-JP",
      "title": "Episode title",
      "availability_starts": "<date>"
    }
  ]
}
```

### 5.4 Sémantique des langues audio

Le champ `episode_metadata.versions` est la source autoritative pour associer une langue audio à
un GUID de lecture. Chaque doublage est traité comme un contenu de lecture séparé, avec :

- son propre GUID ;
- sa propre réponse playback ;
- son propre jeton vidéo ;
- son propre manifeste ;
- sa propre licence Widevine.

Le `content_id` de l’épisode ne doit servir de repli que lorsque `versions` est vide. Le champ
`audio_locale` seul ne suffit pas : selon le commentaire du client, l’API peut annoncer la langue
préférée tout en conservant un ID pointant vers la version originale.

Pour un lecteur web, il faudra décider entre :

1. exposer chaque doublage comme un player distinct ;
2. changer de GUID et rouvrir une session playback quand l’utilisateur change de langue ;
3. agréger plusieurs MPD uniquement si une future réponse Crunchyroll fournit réellement toutes
   les pistes dans un même manifeste.

L’analyse du client favorise l’option 1 pour une première implémentation fiable.

### 5.5 Liste du catalogue — endpoint `browse`

Cette partie n’est pas présente dans les fichiers Go 1.5.1. Elle a été recoupée le 2026-09-25 avec
deux clients indépendants :

- `crunchy-labs/crunchyroll-rs` version `0.19.1`, commit
  `2ae7fca908b5da2c513b53ebea4f3e438e0d3390` du 2026-09-23 ;
- `PD-Codes/crunchyroll-api`, commit
  `966d595c56f356d42f440a73fdb53e798f8ddebf` du 2026-07-03.

Les deux implémentations utilisent le même endpoint et les mêmes paramètres principaux. Une
validation directe a été tentée depuis l’environnement d’analyse, mais la page web, le endpoint de
token et l’API ont tous répondu par un challenge Cloudflare `403`. Le contrat ci-dessous est donc
**fortement recoupé dans du code récent**, mais pas encore confirmé par une réponse live capturée
dans cet environnement.

#### 5.5.1 Requête

```http
GET https://www.crunchyroll.com/content/v2/discover/browse
Authorization: Bearer <access_token>
User-Agent: <profil Firefox>
```

Exemple recommandé pour la première page du catalogue A–Z des séries :

```text
/content/v2/discover/browse
  ?type=series
  &sort_by=alphabetical
  &n=100
  &start=0
  &ratings=true
  &locale=fr-FR
  &preferred_audio_language=ja-JP
```

Les retours à la ligne sont uniquement documentaires ; la requête réelle utilise une query string
standard.

| Paramètre | Valeurs observées | Rôle | Décision initiale Arachnea |
|---|---|---|---|
| `type` | `series`, `movie_listing`, `episode` | Type de média retourné | `series` pour la liste principale |
| `sort_by` | `alphabetical`, `popularity`, `newly_added` | Ordre du catalogue | `alphabetical` pour le catalogue complet |
| `n` | entier positif | Taille de page | `100`, à réduire si l’API ou le temps de réponse l’impose |
| `start` | entier à partir de `0` | Offset absolu | `(page - 1) * page_size` |
| `categories` | slugs séparés par des virgules | Filtre par genres | optionnel |
| `is_dubbed` | booléen | Ne retourner que les contenus doublés | optionnel |
| `is_subbed` | booléen | Ne retourner que les contenus sous-titrés | optionnel |
| `seasonal_tag` | identifiant tel que `winter-2026` | Filtre par saison de simulcast | hors catalogue A–Z initial |
| `ratings` | `true` | Inclut les informations de classification | toujours demandé |
| `locale` | locale Crunchyroll, par exemple `fr-FR` | Localise titres et descriptions | paramètre de service |
| `preferred_audio_language` | locale audio | Influence la présentation/localisation du contenu | paramètre de service |

Les catégories sont sérialisées en une seule valeur séparée par des virgules, par exemple :

```text
categories=action,fantasy
```

L’endpoint accepte l’absence de `type` et peut alors retourner plusieurs familles de médias. Pour
la première implémentation Arachnea, il faut **toujours envoyer `type=series`** afin de conserver un
catalogue homogène et de ne pas mélanger séries, épisodes, films unitaires et contenus musicaux.
Les films pourront être ajoutés ensuite dans une section distincte avec `type=movie_listing`.

#### 5.5.2 Pagination

La réponse est paginée par offset et expose un nombre total :

```json
{
  "data": [
    {
      "id": "<series_id>",
      "type": "series",
      "title": "Series title"
    }
  ],
  "total": 1234,
  "meta": {
    "prev_page": "<optional URL>",
    "next_page": "<optional URL>"
  }
}
```

Le champ `meta` n’est pas garanti sur toutes les réponses ou toutes les versions de l’API. La
bibliothèque Rust récente applique la stratégie suivante :

1. utiliser `meta.next_page` lorsqu’il est présent ;
2. sinon comparer le nombre d’éléments déjà lus à `total`.

La seconde implémentation incrémente `start` par le nombre réellement reçu, pas par la taille de
page demandée. Cette règle est plus robuste si la dernière page est partielle ou si l’API renvoie
moins d’éléments que prévu :

```text
next_start = current_start + data.length
have_more = data.length > 0 && next_start < total
```

Pour le contrat Arachnea où `page` est basé sur `1`, une requête indépendante peut calculer :

```text
start = (page - 1) * page_size
current_page = page
total_pages = ceil(total / page_size)
have_more = start + data.length < total
```

La taille `100` est utilisée par le second client pour parcourir le catalogue complet. Elle doit
rester configurable et être validée en direct ; la couche Arachnea ne doit pas supposer qu’une
taille supérieure sera acceptée.

#### 5.5.3 Forme d’une entrée série

Les modèles récents indiquent qu’une entrée `series` peut fournir directement :

| Champ Crunchyroll | Utilisation Arachnea proposée |
|---|---|
| `id` | `link`, sous forme d’un identifiant ou d’une URL interne vers `get_entry` |
| `title` | `title` |
| `slug` ou `slug_title` | construction de `web-link` si nécessaire |
| `description` | `description` |
| `series_launch_year` | `year` |
| `episode_count` | information de catalogue optionnelle |
| `season_count` | information de catalogue optionnelle |
| `is_subbed`, `is_dubbed` | labels ou informations de langue |
| `audio_locales` | `audio` ou `lang` |
| `subtitle_locales` | information de sous-titres disponible |
| `images.poster_tall` | `img/portrait > link` |
| `images.poster_wide` | `img/landscape > link` |
| `tenant_categories` | `theme`, lorsque présent |
| `content_descriptors` | avertissements de contenu éventuels |
| `maturity_ratings` | `content-advisor` ou classification équivalente |
| `availability_status` | filtrage des entrées indisponibles |

Les groupes d’images peuvent être soit une liste simple, soit une liste de listes. Chaque image
contient au minimum :

```json
{
  "source": "https://<image CDN>/...",
  "type": "...",
  "height": 1350,
  "width": 900
}
```

La normalisation recommandée est d’aplatir le groupe puis de sélectionner l’image de plus grande
largeur. Une stratégie plus précise pourra ensuite choisir une taille proche du besoin du frontend
pour limiter le poids réseau.

Selon les versions de réponse, certains champs peuvent être placés directement sur l’objet ou dans
`series_metadata`. Le parseur du second client accepte déjà cette variation pour `episode_count`,
`season_count`, `maturity_ratings`, `is_dubbed`, `is_subbed`, `audio_locales` et
`subtitle_locales`. Le YAML devra être validé sur une réponse réelle avant de fixer les pointeurs.

#### 5.5.4 Catégories utilisables comme filtres

La liste principale des catégories est disponible par :

```http
GET https://www.crunchyroll.com/content/v2/discover/categories?locale=fr-FR
Authorization: Bearer <access_token>
```

Une catégorie expose notamment `id`, `slug`, `images` et `localization`. Les catégories principales
recensées par le client Rust sont :

- `action` ;
- `adventure` ;
- `comedy` ;
- `drama` ;
- `fantasy` ;
- `music` ;
- `romance` ;
- `sci-fi` ;
- `seinen` ;
- `shojo` ;
- `shonen` ;
- `slice-of-life` ;
- `sports` ;
- `supernatural` ;
- `thriller`.

Des sous-catégories existent, dont `harem`, `historical`, `idols`, `isekai`, `mecha`, `mystery` et
`post-apocalyptic`. Elles peuvent être récupérées par :

```text
/content/v2/discover/categories/{category_id}/sub_categories?locale=fr-FR
```

Pour la première liste de catalogue, les catégories ne sont pas obligatoires. Elles pourront
alimenter ensuite `load_home.categories` ou un filtre de `get_category` sans modifier le contrat de
pagination de `browse`.

#### 5.5.5 Mapping proposé vers `arachnea-stream`

Le catalogue complet doit être chargé à la demande plutôt que préchargé dans `load_home` :

1. `load_home` émet une section « Catalogue A–Z » avec un `link` ou un descripteur de requête ;
2. `get_section` appelle `discover/browse` avec `type=series` et `sort_by=alphabetical` ;
3. `page` est converti en `start` ;
4. `data[]` devient `entries[]` ;
5. `total`, `page_size` et `current_page` alimentent `derive_pagination` ou un calcul équivalent.

Résultat conceptuel :

```json
{
  "current_page": 1,
  "have_more": true,
  "entries": [
    {
      "source": "crunchyroll",
      "title": "Series title",
      "link": "<series_id>",
      "web-link": "https://www.crunchyroll.com/series/<series_id>/<slug>",
      "description": "Synopsis",
      "media-type": "video/show/anime",
      "year": 2026,
      "audio": ["ja-JP", "fr-FR"],
      "img/portrait": { "link": "https://<image CDN>/poster-tall" },
      "img/landscape": { "link": "https://<image CDN>/poster-wide" }
    }
  ]
}
```

Le `web-link` est une commodité et ne doit pas être utilisé comme identifiant interne : `id` reste
la clé stable pour `get_entry`.

#### 5.5.6 Points encore à valider pour le catalogue

- forme JSON live exacte : champs directs contre objet `series_metadata` ;
- présence réelle et stabilité de `meta.next_page` ;
- taille maximale acceptée pour `n` ;
- comportement lorsque `start` dépasse `total` ;
- présence de doublons entre pages pendant une mise à jour du catalogue ;
- effet exact de `preferred_audio_language` sur les titres, images et résultats ;
- réponse anonyme contre réponse authentifiée par `etp_rt` ;
- traitement des contenus `availability_status = not available` ;
- construction actuelle des URLs publiques localisées (`/series/...` ou `/{locale}/series/...`) ;
- catégories et classifications réellement localisées en `fr-FR`.

## 6. Ouverture d’une session de lecture

### 6.1 Requête playback

```http
GET https://www.crunchyroll.com/playback/v3/{content_guid}/web/firefox/play
Authorization: Bearer <access_token>
User-Agent: <profil Firefox>
```

Le chemin fixe le contexte client à `web/firefox`.

### 6.2 Réponse consommée

```json
{
  "url": "https://<cdn>/<manifest>.mpd",
  "subtitles": {
    "fr-FR": {
      "language": "fr-FR",
      "format": "ass",
      "url": "https://<cdn>/<subtitle>.ass"
    }
  },
  "captions": {
    "en-US": {
      "language": "en-US",
      "format": "vtt",
      "url": "https://<cdn>/<caption>.vtt"
    }
  },
  "token": "<video token>",
  "error": false,
  "reason": ""
}
```

| Champ | Rôle |
|---|---|
| `url` | URL du manifeste DASH MPD |
| `subtitles` | Dictionnaire de sous-titres, indexé par locale |
| `captions` | Dictionnaire de sous-titres pour sourds et malentendants / transcription du doublage |
| `token` | Jeton de session vidéo exigé par la licence et par la fermeture de session |
| `error` | Champ polymorphe : chaîne en erreur, mais aussi `false`, `null`, `0` ou nombre |
| `reason` | Précision textuelle facultative sur l’erreur |

Le parseur doit accepter plusieurs types JSON pour `error`. Une chaîne non vide représente une
erreur. Le client traite particulièrement les erreurs dont le texte commence par `429` comme une
limitation de débit du compte.

### 6.3 Durée de vie et concurrence

Le code indique qu’une session playback ouverte compte comme un stream actif. Chaque GUID audio
ouvre une session différente. Une fuite de jetons peut empêcher d’ouvrir d’autres épisodes ou
atteindre une limite de lectures simultanées.

Le futur résolveur devra donc associer au résultat de lecture au minimum :

- `content_guid` ;
- `video_token` ;
- `access_token` ou un moyen de le renouveler ;
- une expiration interne ;
- une stratégie de fermeture en fin de lecture, à l’expiration ou lors d’une erreur.

## 7. Fermeture de la session playback

```http
DELETE https://www.crunchyroll.com/playback/v1/token/{content_guid}/{video_token}
Authorization: Bearer <access_token>
User-Agent: <profil Firefox>
```

Le succès attendu est :

```text
204 No Content
```

Cette opération est appelée après chaque téléchargement et dans un nettoyage de secours si une
étape échoue. Dans Arachnea, le simple retour de `get_stream` ne correspond pas à la fin réelle de
la lecture. Il faudra donc prévoir un mécanisme adapté : endpoint de libération appelé par le
frontend, expiration serveur avec nettoyage différé, ou renouvellement limité à la durée utile.
Ce point est un écart architectural important par rapport aux résolveurs purement stateless.

## 8. Manifeste MPEG-DASH

### 8.1 Requête

```http
GET <url reçue dans playback.url>
Authorization: Bearer <access_token>
User-Agent: <profil Firefox>
```

Le client attend un document XML MPD. Il ne vérifie pas explicitement le code HTTP avant le
décodage ; l’implémentation Arachnea devra le faire.

### 8.2 Deux formes de manifeste observées

#### A. DASH segmenté avec `SegmentTemplate`

Caractéristiques exploitées :

- premier `AdaptationSet` considéré comme vidéo ;
- second `AdaptationSet` considéré comme audio ;
- `Representation/BaseURL` ;
- `Representation@id` ;
- `Representation@height` pour choisir la vidéo ;
- `Representation@bandwidth` ou ID contenant `audio/` pour choisir l’audio ;
- `SegmentTemplate@initialization` ;
- `SegmentTemplate@media` ;
- `SegmentTimeline/S@r` ;
- placeholders `$RepresentationID$`, `$Number$` et `$Number%05d$`.

Le téléchargement observé suppose un `startNumber` de `1` et remplace les numéros avec cinq
chiffres. Ces détails sont utiles pour comprendre le format mais doivent être laissés au moteur
DASH du lecteur plutôt que réimplémentés dans le service.

#### B. DASH « on demand » avec `SegmentBase`

Cette variante place le média dans un fichier MP4 contigu par représentation :

```xml
<Representation id="..." bandwidth="..." height="...">
  <BaseURL>https://...</BaseURL>
  <SegmentBase indexRange="...">
    <Initialization range="..." />
  </SegmentBase>
</Representation>
```

Le client détecte cette forme lorsque le premier `AdaptationSet` n’a pas de `SegmentTemplate`. Il
utilise alors des requêtes HTTP Range :

```http
Range: bytes=<start>-<end>
```

Les réponses acceptées sont `200 OK` et `206 Partial Content`. Le champ `Content-Range` est utilisé
pour connaître la taille totale.

Un lecteur DASH standard devrait gérer `SegmentBase`, mais cette compatibilité doit être testée
avec le moteur DASH réellement embarqué par le frontend Arachnea.

### 8.3 ContentProtection et PSSH

Le PSSH peut se trouver :

- sur l’`AdaptationSet` ;
- sur chaque `Representation`.

Le client cherche les deux emplacements et privilégie le `ContentProtection` dont le
`schemeIdUri` contient l’identifiant système Widevine :

```text
edef8ba9-79d6-4ace-a3c8-27dcd51d21ed
```

La source signale aussi des MPD où les données PSSH utilisables sont présentes dans une boîte
étiquetée avec un autre système, notamment PlayReady/common. Le téléchargeur remplace alors
localement le SystemID de la boîte avant de construire le challenge. Un navigateur utilisant EME
et dash.js peut réagir différemment : ce cas devra être testé sur un épisode concerné avant de
conclure qu’une réécriture du MPD est nécessaire.

## 9. Licence Widevine

### 9.1 Requête

```http
POST https://www.crunchyroll.com/license/v1/license/widevine
Authorization: Bearer <access_token>
Content-Type: application/octet-stream
X-Cr-Content-Id: <content_guid>
X-Cr-Video-Token: <video_token>
Origin: https://static.crunchyroll.com
Referer: https://static.crunchyroll.com/
User-Agent: <profil Firefox>

<challenge Widevine binaire>
```

Le corps entrant est le challenge CDM brut, sans enveloppe JSON.

### 9.2 Réponse

La réponse observée est une enveloppe JSON :

```json
{
  "license": "<licence Widevine encodée en base64>"
}
```

Le client décode `license` en base64 avant de la transmettre au parseur Widevine. Le navigateur
attend généralement une licence binaire. Le proxy DRM Arachnea devra donc :

1. recevoir le challenge binaire du lecteur ;
2. retrouver la session Crunchyroll associée au token proxy ;
3. ajouter les en-têtes Bearer, contenu et vidéo ;
4. envoyer le challenge inchangé à Crunchyroll ;
5. parser le JSON ;
6. décoder le champ `license` en base64 ;
7. retourner les octets de licence au lecteur avec un type de contenu binaire approprié.

Un simple `license_url` pointant directement vers Crunchyroll ne suffit probablement pas : il
exposerait le Bearer token et le jeton vidéo, ne transformerait pas la réponse JSON en binaire et
ne permettrait pas le renouvellement automatique après un `401`.

### 9.3 Renouvellement après 401

Si Crunchyroll répond `401`, le client :

1. ferme la réponse ;
2. redemande un access token avec `etp_rt` ;
3. remplace l’en-tête `Authorization` ;
4. recrée le corps du challenge ;
5. rejoue la requête.

Le proxy Arachnea devra limiter le nombre de rejouements pour éviter une récursion infinie si le
cookie n’est plus valide.

## 10. Accès aux médias et aux sous-titres

### 10.1 Segments et fichiers MP4

Les requêtes média utilisent :

```http
GET <url CDN>
Origin: https://static.crunchyroll.com
Referer: https://static.crunchyroll.com/
User-Agent: <profil Firefox>
```

Pour les fichiers `SegmentBase`, l’en-tête `Range` est ajouté. Le client accepte `200` et `206`.

Dans Arachnea, ces en-têtes devront être intégrés aux URL du proxy média ou rendus disponibles au
lecteur selon le mécanisme déjà prévu pour `stream_headers`. Les URLs signées doivent être
considérées comme temporaires.

### 10.2 Sous-titres et captions

Les URL sont téléchargées avec les mêmes `Origin`, `Referer` et User-Agent que les médias.

Formats observés :

- `ass` pour des sous-titres stylés ;
- `vtt` pour certaines captions.

Le contrat public Arachnea expose actuellement des pistes externes WebVTT. Les pistes VTT peuvent
être renvoyées après proxy. Les pistes ASS demandent en revanche une conversion serveur vers VTT,
une nouvelle prise en charge frontend, ou leur omission contrôlée dans une première version.

Les dictionnaires `subtitles` et `captions` doivent rester distincts afin de pouvoir libeller les
captions comme pistes CC et ne pas les activer par défaut.

## 11. Gestion des erreurs observée

| Situation | Comportement du client de référence | Recommandation Arachnea |
|---|---|---|
| HTTP `401` sur une requête authentifiée | renouvelle le Bearer puis rejoue | un seul renouvellement contrôlé, avec erreur contextualisée ensuite |
| `playback.error` non vide | interrompt la lecture | conserver `error` et `reason` dans l’erreur backend |
| erreur commençant par `429` | signale une limitation du compte | ne pas boucler ; propager un message de rate limit |
| qualité absente | prend la première représentation | laisser le moteur DASH choisir en mode adaptatif |
| langue audio absente | ignore la langue ; abandonne si aucune ne reste | ne pas créer de player pour le GUID absent |
| sous-titre absent | ignore la piste | omettre seulement la piste concernée |
| fermeture playback différente de `204` | avertissement | journaliser et programmer un nettoyage de secours |
| média CDN différent de `200` | cinq tentatives avec attente croissante | le proxy peut appliquer sa politique standard de retry aux erreurs transitoires |

Le client Go ne valide pas systématiquement les statuts ni les `Content-Type` des APIs JSON. Le
service Arachnea devra être plus strict pour éviter de parser une page HTML d’erreur comme du JSON.

## 12. Correspondance proposée avec `arachnea-stream`

### 12.1 Partie catalogue probablement exprimable en YAML

Sous réserve que les primitives Scrapyfy puissent injecter un Bearer obtenu dynamiquement, les
opérations suivantes sont structurellement compatibles avec un service légal YAML :

- `service_stream_metadata` ;
- `get_entry` pour les métadonnées série/épisode ;
- `get_season` pour les épisodes ;
- génération de players distincts par `versions[*].audio_locale` et `versions[*].guid` ;
- extraction des titres, numéros, dates et langues depuis les réponses CMS.

Les fichiers Go analysés ne contiennent aucun endpoint de page d’accueil, catégorie, section ou
recherche. Ces requêtes devront être découvertes séparément ; la source permet seulement un MVP
centré sur les URLs `/series/{id}` et `/watch/{id}` ou sur des IDs déjà connus.

### 12.2 Partie nécessitant probablement du Rust

Un résolveur Crunchyroll dédié paraît nécessaire pour :

- échanger `etp_rt` contre un access token ;
- mettre en cache le jeton et le renouveler après `401` ;
- ouvrir une session playback par GUID ;
- conserver côté serveur `content_guid` et `video_token` ;
- construire une URL de proxy de licence opaque ;
- transformer la réponse JSON/base64 de licence en réponse binaire ;
- transmettre les en-têtes média nécessaires ;
- fermer ou expirer la session playback ;
- éventuellement convertir les sous-titres ASS en WebVTT.

Le comportement est stateful et ne doit pas être ajouté à la façade générique
`stream_scraper.rs`. Il devrait rester dans un résolveur source dédié sous
`server/crates/arachnea-stream/src/services/`, sauf si des primitives génériques réellement
réutilisables sont identifiées.

### 12.3 Résultat `get_stream` visé

Exemple conceptuel, sans fixer encore les noms de resolver ni le schéma final :

```json
{
  "stream_url": ["<URL proxy du MPD>"],
  "manifest_type": "mpd",
  "license_url": "/api/get_drm_license/crunchyroll/<opaque-token>",
  "license_headers": {},
  "subtitles": [
    {
      "lang": "fr-FR",
      "label": "Français",
      "link": "<URL proxy WebVTT>"
    }
  ]
}
```

Le token public doit être opaque. Il ne doit contenir directement ni `access_token`, ni `etp_rt`,
ni `video_token`.

## 13. État serveur recommandé

Une entrée de session interne pourrait contenir :

```text
opaque_token -> {
  service_id,
  account/session key,
  content_guid,
  video_token,
  manifest_url,
  access_token reference,
  created_at,
  expires_at,
  released
}
```

Recommandations :

- durée de vie courte et bornée ;
- suppression après expiration ;
- aucune sérialisation des secrets dans les URLs ou logs ;
- fermeture playback idempotente ;
- isolation des sessions par compte ;
- verrou ou déduplication lors du renouvellement concurrent du Bearer ;
- limitation du nombre de sessions simultanées ouvertes par résolution.

## 14. Points à confirmer avant implémentation

1. **Réponse complète de `/auth/v1/token`** : expiration, type de jeton, éventuel refresh token et
   erreurs lorsque `etp_rt` est invalide.
2. **Stabilité du client Basic** : vérifier qu’il correspond toujours au client web et décider s’il
   peut être configuré plutôt que compilé.
3. **Accueil et recherche** : découvrir les endpoints home, rails éditoriaux, bannières et recherche.
4. **Catalogue live** : valider les points listés en 5.5.6 sur une session qui passe Cloudflare.
5. **Série détaillée** : relever les champs et images de la page série nécessaires à `get_entry`.
6. **Premium** : relever les codes/statuts d’un contenu inaccessible au compte.
7. **Durée réelle du Bearer, du `video_token` et des URLs CDN**.
8. **Limite de streams simultanés** et effet exact d’une session non supprimée.
9. **Lecture navigateur des MPD `SegmentBase`** avec la version de dash.js utilisée par Arachnea.
10. **PSSH atypiques** : vérifier si EME accepte les manifests actuels sans réécriture.
11. **CORS** : confirmer quels éléments doivent obligatoirement passer par le proxy Arachnea.
12. **Sous-titres ASS** : choisir entre conversion, support natif ou exclusion initiale.
13. **Changement de langue audio** : confirmer si chaque doublage doit rester un player distinct.
14. **Fermeture de session** : définir le signal frontend ou la politique d’expiration serveur.
15. **Logout/révocation** : déterminer si une action spécifique existe ou si la suppression locale du
    cookie suffit.

## 15. Proposition de séquence d’implémentation

1. Capturer et documenter des réponses réelles anonymisées pour token, catalogue `browse`, objet,
   saisons, épisodes, playback, MPD et licence.
2. Ajouter un mode de credential adapté à `etp_rt` ou définir explicitement son encodage dans le
   stockage de credentials existant.
3. Implémenter un gestionnaire de session Crunchyroll côté Rust : device ID, token cache et retry
   unique sur `401`.
4. Implémenter le résolveur playback et le proxy de licence JSON/base64.
5. Tester un épisode avec `SegmentTemplate`, puis un épisode avec `SegmentBase`.
6. Ajouter les sous-titres VTT ; traiter ensuite la conversion ASS.
7. Ajouter le catalogue A–Z des séries avec `discover/browse`, puis les détails série/saison/épisode
   dans un YAML légal.
8. Découvrir et ajouter home, catégories et recherche dans une seconde étape.
9. Ajouter le nettoyage explicite ou différé des sessions playback.
10. Activer le service seulement après validation d’un contenu gratuit, d’un contenu Premium, de
    plusieurs doublages et d’un cas rate-limité.

## 16. Fichiers source principaux

| Fichier | Informations extraites |
|---|---|
| `token.go` | échange `etp_rt` → Bearer, device ID, formulaire et cookies |
| `http_request.go` | renouvellement automatique après `401` et rejeu du corps |
| `episode.go` | endpoint playback, réponse, métadonnées épisode et suppression de session |
| `season.go` | endpoints saisons et épisodes CMS |
| `mpd.go` | chargement du MPD, représentations et timeline DASH |
| `ondemand.go` | variante `SegmentBase`, byte ranges et réponses `200`/`206` |
| `drm.go` | PSSH, challenge Widevine, en-têtes de licence et réponse JSON/base64 |
| `download.go` | en-têtes CDN, sous-titres, association locale/GUID et cycle multi-audio |
| `main.go` | paramètres de langue, URL `/watch` ou `/series` et dépendance à `etp_rt` |
| `README.md` | prérequis fonctionnels et options utilisateur |
| `crunchy-labs/crunchyroll-rs` 0.19.1 | endpoint `browse`, filtres, catégories, modèles média, images et pagination |
| `PD-Codes/crunchyroll-api` | parcours complet du catalogue, pagination par offset et normalisation des séries |

## 17. Conclusion

Le protocole Crunchyroll observé n’est pas un simple catalogue JSON suivi d’une URL DASH. Il
combine une authentification par cookie, un Bearer renouvelable, des GUID distincts par doublage,
des sessions playback comptabilisées, un jeton vidéo, une licence Widevine encapsulée en JSON et
des contraintes d’en-têtes CDN.

La partie catalogue peut raisonnablement rester déclarative dans un service YAML. La résolution
de lecture et la licence nécessitent vraisemblablement un composant Rust stateful dédié, avec un
stockage opaque des sessions et une fermeture explicite ou différée des streams. L’implémentation
ne doit commencer qu’après confirmation des réponses réelles et de la compatibilité du lecteur
avec les deux formes de MPD identifiées.