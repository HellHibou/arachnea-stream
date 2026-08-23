# Analyse du hoster Byse (lukefirst.lol)

**URL analysée :** https://lukefirst.lol/e/ztwuh0ypemxk

## Plateforme

- **Nom** : Byse Frontend
- **Type** : Site d'hébergement vidéo
- **Base URL** : https://lukefirst.lol
- **Type de page** : Embed (intégration)
- **Description** : SPA React (React 18.3.1) qui charge le contenu via API, en mode intégration vidéo

## Route

- **Pattern** : `/e/:code`
- **Code** : `ztwuh0ypemxk`

## Vidéo

| Champ | Valeur |
|-------|--------|
| ID | 59194374 |
| Code | ztwuh0ypemxk |
| Titre | Star Trek Strange New Worlds S04E05 MULTi 1080p WEB H264-SUPPLY-1 |
| Poster | https://img-place.com/ztwuh0ypemxk.jpg |
| Description | uploaded with File Uploader (z-o-o-m.eu) |
| Date de création | 2026-08-20T08:13:52Z |
| Propriétaire privé | false |
| URL frame embed | https://dismz4n3wp6xnr3.org/{prefix}/{code} (préfixe variable) |
| Cache | hit |

## Paramètres d'accès

- **Premium uniquement** : non
- **Téléchargement autorisé** : oui
- **Pays autorisés** : Tous
- **Pays bannis** : Aucun
- **Domaine embed autorisé** : (vide)
- **Sous-titres** : autoplay désactivé, langue "Select"
- **Audio autoplay** : non
- **Bouton de téléchargement** : non
- **Accès VPN désactivé** : non
- **Accès proxy résidentiel désactivé** : non
- **Mode publicités** : "0" (lecture et téléchargement)
- **Captcha requis** : oui
- **Cache** : hit

## Endpoints API

| Endpoint | Description |
|----------|-------------|
| `/api/videos/{code}/embed/details` | Métadonnées de la vidéo (public, sans auth) |
| `/api/videos/{code}/embed/settings` | Paramètres de lecture (public, sans auth) |
| `/api/videos/{code}/embed/playback` | Configuration de lecture (chiffrée) |
| `/api/videos/{code}/embed/captcha` | Démarrage captcha (proof-of-work) |
| `/api/videos/{code}/embed/captcha/verify` | Vérification du captcha |
| `/api/videos/{code}/embed/captcha/image` | Captcha par tuiles d'images |
| `/api/videos/access/challenge` | Obtention du nonce à signer (attestation) |
| `/api/videos/access/attest` | Attestation d'empreinte (signature ECDSA P-256) |
| `/api/videos/{code}/embed/view` | Compteur de vues (chiffré) |
| `/api/videos/{code}/embed/heartbeat` | Heartbeat de lecture |
| `/api/videos/{code}/embed/timeslider` | Miniatures de la timeline |
| `/api/videos/{code}/embed/downloads` | Options de téléchargement |
| `/api/videos/stream/{token}` | Flux vidéo direct (token du flux de téléchargement) |

## Notes de sécurité

- La lecture nécessite un captcha (preuve de travail) **et** une attestation d'empreinte
  navigateur avant d'obtenir la configuration de lecture.
- Les en-têtes `X-Embed-Origin`, `X-Embed-Referer` et `X-Embed-Parent` sont envoyés pour
  l'anti-leech (non obligatoires pour un simple client).
- Les événements de vue sont chiffrés en AES-GCM avec un jeton d'attestation d'empreinte.
- La reCAPTCHA est activée pour les téléchargements (clé de site fournie par le serveur).
- Le téléchargement passe par un countdown + token : `di(code, { quality, countdownToken })`
  → `{ token }` → `window.open('/api/videos/stream/' + token)` (nécessite le contexte
  navigateur : attestation + captcha).

## Architecture SPA

La page embed est un shell React vide (`<div id="root">`, bootstrap `video-embed-mode`) qui
charge ensuite ses bundles :

| Asset | Rôle |
|-------|------|
| `/assets/index-DocunfmE.js` | Bundle principal (shell, routing `/e/:code`) |
| `assets/videoPagesBundle-Bgi0QmPo.js` | Page vidéo / player (`aa`, `us`) |
| `pow-DEJGtdh2.js` | Solveur PoW (hash personnalisé) — script **externe**, pas en inline |
| `/player/jw8_26/jwplayer.js` | Lecteur JW Player 8 (chargé à la demande) |

- **Player** : le composant `aa` reçoit `sources[]` en props React puis appelle
  `B.setup({ playlist: [{ image, sources, tracks }], hlshtml: true, androidhls: true })`.
- **Déchiffrement** : AES-GCM via WebCrypto (`crypto.subtle`) / noble-ciphers, directement
  dans le bundle (`/*! noble-ciphers - MIT */`).
- **Cache clearance** : `localStorage` sous `byse:captcha-clearance:*` ; viewer_id sous
  `byse_viewer_id`.
- **Frame CDN** : `embed_frame_url` pointe vers `https://dismz4n3wp6xnr3.org/{prefix}/{code}`
  qui sert la **même** application « Byse Frontend » avec le même niveau de protection
  (pas de raccourci exploitable).

## Attestation d'empreinte (détails)

L'attestation est un schéma de signature **ECDSA P-256** dont la validité est **vérifiée
côté serveur** (tests effectués) :

| Étape | Méthode / URL | Corps / sortie |
|-------|---------------|----------------|
| 1 | `POST /api/videos/access/challenge` | → `{ nonce }` |
| 2 | `POST /api/videos/access/attest` | `{ viewer_id, device_id, challenge_id, nonce, signature, public_key, client, storage, attributes }` |

- `signature` = `r‖s` (64 octets) en base64url, `r` et `s` extraits de la signature DER ;
  `public_key` = JWK ECDSA P-256.
- Réponse : `{ token, viewer_id, device_id, confidence, expires_at }` (confidence ≈ 0,82).
- Résultats des tests de rejet :
  - empreinte non signée → `400 signature required` ;
  - clé publique absente → `400 public key required` ;
  - challenge inconnu/absent → `400 challenge not found` ;
  - captcha **et** playback exigent un fingerprint attesté, sinon `400 invalid request body`.
- Réutilisé ensuite dans les corps d'appels : `fingerprint: { token, viewer_id, device_id, confidence }`.

## Récupération du flux vidéo (liste des appels et données échangées)

Le flux est obtenu en mode embed via `/api/videos/{code}/embed/...`. Tous les appels
envoient `credentials: include` (cookies de session). En mode embed, le frontend ajoute
les en-têtes `X-Embed-Origin`, `X-Embed-Referer`, `X-Embed-Parent` (origine de la page
parente, pour l'anti-leech) — non obligatoires pour un simple client mais reflètent le
comportement du site.

### Appel 1 — Métadonnées de la vidéo

| Élément | Valeur |
|---------|--------|
| Méthode | `GET` |
| URL | `https://lukefirst.lol/api/videos/ztwuh0ypemxk/embed/details` |

**Champs récupérés :** `id`, `code`, `title`, `poster_url`, `description`, `created_at`,
`owner_private`, `embed_frame_url`. À garder pour l'affichage (poster, titre).

### Appel 2 — Paramètres de lecture

| Élément | Valeur |
|---------|--------|
| Méthode | `GET` |
| URL | `https://lukefirst.lol/api/videos/ztwuh0ypemxk/embed/settings` |

**Champs récupérés :** `code`, `premium_only`, `download_allowed`, `allowed_countries`,
`banned_countries`, `embed_domain_allowed`, `subtitle`, `audio_autoplay`,
`download_button`, `vpn_access_disabled`, `residential_proxy_access_disabled`,
`ads_mode`, `captcha_required` (ici `true`).

**Décision :** si `captcha_required` est `true`, il faut passer par l'étape 3 avant
l'appel de lecture.

### Appel 3 — Attestation d'empreinte + résolution du captcha (proof-of-work)

**3a. Attestation d'empreinte (fingerprint) :**

| Élément | Valeur |
|---------|--------|
| Méthode | `POST` |
| URL | `https://lukefirst.lol/api/videos/access/challenge` puis `.../access/attest` |
| Corps | `{ viewer_id, device_id, challenge_id, nonce, signature, public_key, client, storage, attributes }` |

**Champs récupérés :** `viewer_id`, `device_id`, `token`, `confidence`, `expires_at`.
Le `token` et les IDs sont réutilisés dans les corps des appels suivants (champ
`fingerprint: { token, viewer_id, device_id, confidence }`).

**3b. Démarrer le défi PoW :**

| Élément | Valeur |
|---------|--------|
| Méthode | `POST` |
| URL | `https://lukefirst.lol/api/videos/ztwuh0ypemxk/embed/captcha` |
| Corps | `{ "fingerprint": { token, viewer_id, device_id } }` |

**Champs récupérés :** `pow_nonce`, `pow_difficulty`, `pow_token`, `expires_in`.
Conserver `pow_token` et `expires_in` (le PoW doit être résolu dans ce délai).

**3c. Résoudre le PoW :** le client calcule une chaîne `solution` telle que le hachage
de `pow_nonce + ":" + solution` possède `pow_difficulty` bits de tête à zéro (hash
personnalisé SHA-256-like implémenté dans `pow-DEJGtdh2.js`). Délai max de calcul :
`min(20000, max(4000, (expires_in - 3) * 1000))` ms.

**3d. Vérifier le PoW :**

| Élément | Valeur |
|---------|--------|
| Méthode | `POST` |
| URL | `https://lukefirst.lol/api/videos/ztwuh0ypemxk/embed/captcha/verify` |
| Corps | `{ "pow_token": "<pow_token>", "solution": "<solution>", "fingerprint": { token, viewer_id, device_id } }` |

**Champs récupérés :** `status` (`"ok"` si succès), `token`, `expires_in`.
Si `status === "ok"`, récupérer `token` : c'est le jeton de clearance à passer à
l'appel de lecture. (Le site le met en cache dans `localStorage` sous
`byse:captcha-clearance:embed` pour éviter de le refaire pendant `expires_in` secondes.)

### Appel 4 — Configuration de lecture (obtention des sources)

| Élément | Valeur |
|---------|--------|
| Méthode | `POST` (si fingerprint fourni) ou `GET` |
| URL | `https://lukefirst.lol/api/videos/ztwuh0ypemxk/embed/playback` |
| En-tête | `X-Captcha-Token: <token du captcha>` (si captcha requis) |
| Corps (si POST) | `{ "fingerprint": { token, viewer_id, device_id, confidence } }` |

**Réponse brute (champ `playback`) :** un objet chiffré AES-GCM contenant :
- `version` : numéro de version utilisé pour sélectionner les fragments de clé
- `iv` : IV encodé en base64url
- `key_parts` : liste de fragments de clé (base64url)
- `payload` : le payload chiffré (base64url)
- `skip_intro` : objet éventuel avec `{ start, end }`

### Appel 5 — Déchiffrement de la configuration

Algorithme (relevé dans le bundle) :
1. Sélectionner les fragments de clé : selon `version` (plage 1-20), retenir les
   indices de `key_parts` calculés par une table fixe `[n, 31-n]` (indices 1-based).
   Si l'indice est hors bornes, on garde tous les fragments.
2. Décoder chaque fragment en base64url et les concaténer → clé AES brute.
3. Déchiffrer `payload` en AES-GCM avec l'`iv` et cette clé.
4. Le JSON déchiffré contient :
   - `sources` : tableau de flux, chaque élément ayant `{ quality, label, mime_type,
     url, bitrate_kbps, height, size_bytes }`. **C'est `sources[].url` qui est le flux
     vidéo** (HLS/DASH/MP4 selon `mime_type`).
   - `tracks` : sous-titres `{ language, title, url, kind }`.
   - `poster_url` : poster éventuel.

### Chemin résumé (flux réel du site)

```
GET  /api/videos/{code}/embed/details          → id, code, title, poster_url
GET  /api/videos/{code}/embed/settings         → captcha_required
POST /api/videos/access/challenge              → { nonce }
POST /api/videos/access/attest                 → fingerprint { token, viewer_id, device_id }
POST /api/videos/{code}/embed/captcha          → { pow_nonce, pow_difficulty, pow_token, expires_in }
     (calcul local du PoW)
POST /api/videos/{code}/embed/captcha/verify   → { status, token, expires_in }  ← clearance token
POST /api/videos/{code}/embed/playback         (header X-Captcha-Token: <token>)
     → playback chiffré → décryptage AES-GCM → sources[].url = flux vidéo
```

### Chemin alternatif — captcha par tuiles d'images

Si le PoW échoue (3 essais max), le site bascule sur un captcha d'images :
- `POST /api/videos/{code}/embed/captcha/image` (corps `{ fingerprint }`) →
  `{ challenge_id, target, tiles, columns, expires_in, remaining, image_base }`.
- `POST /api/videos/{code}/embed/captcha/image/{challenge_id}/verify` (corps
  `{ selection, fingerprint }`) → `{ status, token, expires_in, remaining }`.
- Si `status === "ok"`, récupérer `token` et l'utiliser comme clearance token.

### Notes

- L'endpoint `/api/videos/stream/{token}` existe (flux de téléchargement) mais le flux
  réel de lecture passe par `sources[].url` obtenu après déchiffrement de la réponse de
  `playback`.
- Le compteur de vues (`/embed/view`) et le heartbeat (`/embed/heartbeat`) sont envoyés
  pendant la lecture ; ils ne sont pas nécessaires pour obtenir le flux.

## Le problème de la récupération du m3u8

### Pourquoi le m3u8 n'est jamais disponible en clair

1. **L'API ne renvoie jamais l'URL en clair.** `POST /embed/playback` retourne un payload
   chiffré AES-GCM (`key_parts` + `iv` + `payload`). Le m3u8 n'existe côté client qu'après
   déchiffrement — contrairement à DoodStream qui renvoie directement une URL CDN.
2. **Le déchiffrement se fait dans le JS de la page, en mémoire.** Le résultat
   (`sources[]`) est passé en props React au composant player `aa`, qui le donne à
   JW Player 8 via `B.setup({ playlist: [{ image, sources, tracks }] })`.
3. **Chrome + hls.js → `video.src = blob:`.** Avec `hlshtml: true` et `androidhls: true`,
   JW8 utilise hls.js dans Chrome et pose une URL `blob:` sur l'élément `<video>`. Le m3u8
   **n'apparaît donc jamais dans le DOM** (ni attribut, ni `<video src>`, ni lien de
   téléchargement sur la page embed — `download_button: false`).
4. **L'état JW reste en mémoire JS.** L'instance du player est tenue dans un ref React
   (`Z.current`) ; le global `window.__jw8` n'est posé que conditionnellement
   (`Ce && (window.__jw8 = B)`), pas garanti.

### Contraintes du format YAML (`exec_js` / `boa_engine`)

`exec_js` exécute du JS dans **`boa_engine`** (moteur ECMAScript pur, en Rust, sandboxé) :

- **Pas d'environnement hôte** : pas de `window`/`document`, pas de `fetch`, pas de
  WebCrypto (`crypto.subtle`), pas de Node `crypto`.
- **Sortie limitée aux variables numériques globales** : le JS ne « rend » que des lignes
  `Name=Valeur` (lues via `extract_variables`). Impossible de faire ressortir une chaîne
  (le JSON déchiffré / l'URL m3u8).
- **Budget d'instructions** : `timeout_ms` (défaut 500) borne via `loop_iteration_limit` ;
  une implémentation JS pure d'AES-GCM (ou un PoW long) serait coupée.
- **Attestation** : aucune action du format ne fait de crypto ; la signature ECDSA P-256
  est **validée par le serveur** → non exprimable en YAML.
- **PoW** : l'algo vit dans `pow-DEJGtdh2.js` (script externe), pas en inline → non
  injectable via `inject_html_scripts`.

Conséquence : **AES-GCM est inaccessible** — pas d'API pour chiffrer/déchiffrer, et même
en le réimplémentant en JS pur, la sortie string serait perdue et le budget dépassé.

### Modes navigateur du moteur (`page_navigate` / `page_click` / `page_fetch`)

Le mode navigateur exécute réellement le JS de la page (attestation + PoW + captcha +
playback + déchiffrement), mais :

- il ne renvoie que le **HTML rendu** ;
- le m3u8 n'y est pas (point 3 ci-dessus : URL `blob:` + état en mémoire JS).

**Conclusion :** le résolveur YAML (`byse.yaml`) ne peut pas émettre de `stream_url`
native. Le moteur retombe donc sur `{ "embed-link": "<url>" }` et la lecture se fait en
iframe — le navigateur de l'utilisateur déroule toute la chaîne de protection.

## Solutions pour récupérer le m3u8 (voie navigateur)

Exigence minimale : **évaluer du JS dans la page vivante**, **intercepter le réseau**,
ou **hooker WebCrypto**. Librairies Rust candidates :

| Librairie | Mécanisme | Usage pour Byse |
|-----------|-----------|-----------------|
| `chromiumoxide` | CDP (Chromium headless) | `Runtime.evaluate` (lire l'état JW), `Fetch.enable` (intercepter `/embed/playback`), `Page.addScriptToEvaluateOnNewDocument` (hook `crypto.subtle.decrypt` avant chargement) |
| `playwright` (bindings Rust) | Pilotage Playwright | `page.evaluate(js)`, `page.on("request")`, `page.add_init_script()` — le plus simple |
| `thirtyfour` / `fantoccini` | WebDriver (Selenium) | `execute()` de JS dans la page ; plus lourds |

Déchiffrement hors-ligne du payload intercepté : crate **`aes-gcm`** en Rust (clé =
concaténation des `key_parts` sélectionnés par la table `[n, 31-n]` 1-based, iv 12 octets,
pas d'AAD) — même logique que `fetch_stream.py`.

Stratégies d'extraction :
1. **Lire l'état du player** : attendre `.jwplayer video` ou `window.__jw8`, puis
   `window.__jw8.getPlaylist()[0].sources[0].file` (fragile : `__jw8` conditionnel).
2. **Intercepter `/embed/playback`** : capturer la réponse (payload chiffré), la
   décrypter en Rust avec `aes-gcm`.
3. **Hooker `crypto.subtle.decrypt`** : injecter un wrapper au tout début de la page pour
   enregistrer le texte clair (équivalent d'un devtools Snippet).

`boa_engine`, `rquickjs`, `deno_core` ou `rusty_v8` ne suffisent **pas** : ce sont des
moteurs JS sans le contexte page (il faudrait réimplémenter fetch, fingerprint, tokens,
etc.).


## Implémentation de référence (`fetch_stream.py`)

Script Python complet qui déroule le flux de bout en bout (attestation ECDSA P-256,
résolution du PoW — C compilé à la volée avec fallback Python pur —, captcha verify,
playback et déchiffrement AES-GCM). Dépends de `requests` et de la librairie
`cryptography`. Il constitue la référence comportementale que le résolveur YAML et
une future implémentation Rust devraient reproduire.

```python
#!/usr/bin/env python3
"""Récupère et affiche les informations du flux vidéo depuis lukefirst.lol (mode embed).

Flux des appels :
  GET  /api/videos/{code}/embed/details     -> métadonnées vidéo
  GET  /api/videos/{code}/embed/settings    -> paramètres d'accès (captcha_required)
  POST /api/videos/access/challenge         -> nonce à signer (attestation)
  POST /api/videos/access/attest            -> fingerprint {token, viewer_id, device_id}
  POST /api/videos/{code}/embed/captcha     -> défi PoW {pow_nonce, pow_difficulty, pow_token}
       (résolution locale du PoW : hash à `difficulty` bits de tête à zéro)
  POST /api/videos/{code}/embed/captcha/verify -> clearance token (header X-Captcha-Token)
  POST /api/videos/{code}/embed/playback    -> payload chiffré AES-GCM
       (déchiffrement AES-GCM -> sources[], tracks[], poster_url)
"""

import base64
import json
import os
import subprocess
import sys
import tempfile

import requests
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.asymmetric.utils import decode_dss_signature
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

# ---------------------------------------------------------------------------
# Constantes
# ---------------------------------------------------------------------------

VIDEO_CODE = "ztwuh0ypemxk"
BASE_URL = "https://lukefirst.lol"

# ---------------------------------------------------------------------------
# Solveur PoW
# ---------------------------------------------------------------------------

_POW_C_SOURCE = r"""
#include <stdint.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

static inline uint32_t rotl32(uint32_t t, uint32_t e) {
    return (t << e) | (t >> (32 - e));
}

static inline void ye(uint32_t *t) {
    t[0] = t[0] + t[1];
    t[3] = rotl32(t[3] ^ t[0], 16);
    t[2] = t[2] + t[3];
    t[1] = rotl32(t[1] ^ t[2], 12);
    t[0] = t[0] + t[1];
    t[3] = rotl32(t[3] ^ t[0], 8);
    t[2] = t[2] + t[3];
    t[1] = rotl32(t[1] ^ t[2], 7);
}

static void gr(const uint8_t *seq, size_t len, uint32_t *out) {
    uint32_t e[4] = {1779033703u, 3144134277u, 1013904242u, 2773480762u};
    for (size_t i = 0; i < len; i++) {
        e[0] = e[0] + seq[i];
        e[0] = rotl32(e[0], 7);
        ye(e);
    }
    for (int i = 0; i < 8; i++) ye(e);
    enum { BE = 512, DR = 2 };
    const uint32_t LT = BE - 1;
    const uint32_t LR = 2654435761u;
    const uint32_t HR = 2246822519u;
    uint32_t r[BE];
    for (int i = 0; i < BE; i++) {
        ye(e);
        r[i] = e[0] ^ e[2];
    }
    for (int i = 0; i < DR; i++)
        for (int s = 0; s < BE; s++) {
            uint32_t a = r[s] & LT;
            uint32_t c = r[s] + r[a];
            c = rotl32(c, 13);
            c = c ^ (r[(s + 1) & LT] * LR);
            r[s] = c;
            e[0] = e[0] ^ c;
            ye(e);
        }
    for (int i = 0; i < 8; i++) {
        ye(e);
        uint32_t s = e[0];
        size_t a = (size_t)i * (BE / 8);
        for (int c = 0; c < BE / 8; c++) {
            uint32_t d = r[a + c];
            s = s + d;
            s = rotl32(s, 5);
            s = s ^ (d * HR);
        }
        out[i] = s ^ e[2];
    }
}

static int leading_zero_bits(const uint32_t *words) {
    int total = 0;
    for (int i = 0; i < 8; i++) {
        uint32_t w = words[i];
        if (w == 0) { total += 32; continue; }
        total += __builtin_clz(w);
        break;
    }
    return total;
}

int main(int argc, char **argv) {
    if (argc < 3) return 2;
    const char *nonce = argv[1];
    int difficulty = atoi(argv[2]);
    if (difficulty <= 0) { printf("0\n"); return 0; }
    size_t nonce_len = strlen(nonce);
    char buf[128];
    memcpy(buf, nonce, nonce_len);
    buf[nonce_len] = ':';
    uint64_t s = 0;
    for (;;) {
        int n = snprintf(buf + nonce_len + 1, sizeof(buf) - nonce_len - 1, "%llu", (unsigned long long)s);
        if (n < 0) return 3;
        uint32_t out[8];
        gr((uint8_t *)buf, nonce_len + 1 + (size_t)n, out);
        if (leading_zero_bits(out) >= difficulty) {
            printf("%llu\n", (unsigned long long)s);
            return 0;
        }
        s++;
    }
}
"""

_MASK32 = 0xFFFFFFFF


def _rotl32(t, e):
    return ((t << e) | (t >> (32 - e))) & _MASK32


def _ye(t):
    t[0] = (t[0] + t[1]) & _MASK32
    t[3] = _rotl32(t[3] ^ t[0], 16)
    t[2] = (t[2] + t[3]) & _MASK32
    t[1] = _rotl32(t[1] ^ t[2], 12)
    t[0] = (t[0] + t[1]) & _MASK32
    t[3] = _rotl32(t[3] ^ t[0], 8)
    t[2] = (t[2] + t[3]) & _MASK32
    t[1] = _rotl32(t[1] ^ t[2], 7)


def _gr(data):
    e = [1779033703, 3144134277, 1013904242, 2773480762]
    for b in data:
        e[0] = (e[0] + b) & _MASK32
        e[0] = _rotl32(e[0], 7)
        _ye(e)
    for _ in range(8):
        _ye(e)
    be, lt, dr = 512, 511, 2
    lr, hr = 2654435761, 2246822519
    r = [0] * be
    for i in range(be):
        _ye(e)
        r[i] = (e[0] ^ e[2]) & _MASK32
    for _ in range(dr):
        for s in range(be):
            a = r[s] & lt
            c = (r[s] + r[a]) & _MASK32
            c = _rotl32(c, 13)
            c = (c ^ ((r[(s + 1) & lt] * lr) & _MASK32)) & _MASK32
            r[s] = c
            e[0] = (e[0] ^ c) & _MASK32
            _ye(e)
    out = []
    for i in range(8):
        _ye(e)
        s = e[0]
        for c in range(64):
            d = r[i * 64 + c]
            s = (s + d) & _MASK32
            s = _rotl32(s, 5)
            s = (s ^ ((d * hr) & _MASK32)) & _MASK32
        out.append((s ^ e[2]) & _MASK32)
    return out


def _leading_zero_bits(words):
    total = 0
    for w in words:
        if w == 0:
            total += 32
            continue
        total += 32 - w.bit_length()
        break
    return total


def _solve_pow_python(nonce, difficulty):
    if difficulty <= 0:
        return "0"
    prefix = nonce + ":"
    s = 0
    while True:
        h = _gr([ord(c) & 255 for c in prefix + str(s)])
        if _leading_zero_bits(h) >= difficulty:
            return str(s)
        s += 1


def _build_pow_solver():
    """Compile le solveur C si gcc est disponible, sinon fallback Python pur."""
    try:
        d = tempfile.mkdtemp(prefix="byse_pow_")
        src = os.path.join(d, "pow_solve.c")
        with open(src, "w") as f:
            f.write(_POW_C_SOURCE)
        exe = os.path.join(d, "pow_solve")
        result = subprocess.run(
            ["gcc", "-O2", "-o", exe, src],
            capture_output=True, timeout=30,
        )
        if result.returncode != 0:
            raise RuntimeError("gcc failed")
        return exe
    except Exception:
        return None


_POW_EXE = _build_pow_solver()


def solve_pow(nonce, difficulty):
    """Résout le défi : retourne `s` tel que hash(nonce:s) ait `difficulty` bits de zéro."""
    if _POW_EXE is not None:
        out = subprocess.run(
            [_POW_EXE, nonce, str(difficulty)],
            capture_output=True, timeout=120,
        )
        return out.stdout.decode().strip()
    return _solve_pow_python(nonce, difficulty)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def b64url_encode(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def b64url_decode(data: str) -> bytes:
    pad = "=" * (-len(data) % 4)
    return base64.urlsafe_b64decode(data + pad)


def _client_fingerprint():
    return {
        "user_agent": (
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
            "(KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        ),
        "brand_full_versions": [{"brand": "Chromium", "version": "120.0.0.0"}],
        "platform": "Windows",
        "platform_version": "10.0",
        "architecture": "x86",
        "bitness": "64",
        "pixel_ratio": 1.0,
        "screen_width": 1920,
        "screen_height": 1080,
        "color_depth": 24,
        "languages": ["en-US"],
        "timezone": "Europe/Paris",
        "hardware_concurrency": 8,
        "device_memory": 8,
        "touch_points": 0,
        "webgl_vendor": "Google Inc. (NVIDIA)",
        "webgl_renderer": "ANGLE (NVIDIA, NVIDIA GeForce RTX 3060 Direct3D11 vs_5_0 ps_5_0)",
        "pointer_type": "fine",
        "extra": {"vendor": "Google Inc.", "appVersion": "5.0 (Windows NT 10.0; Win64; x64)"},
    }


def attest_fingerprint(session):
    """Atteste l'empreinte navigateur (ECDSA P-256 sur le nonce) et retourne le fingerprint."""
    challenge = session.post(f"{BASE_URL}/api/videos/access/challenge").json()
    private_key = ec.generate_private_key(ec.SECP256R1())
    public_key = private_key.public_key()
    nums = public_key.public_numbers()
    jwk = {
        "kty": "EC",
        "crv": "P-256",
        "x": b64url_encode(nums.x.to_bytes(32, "big")),
        "y": b64url_encode(nums.y.to_bytes(32, "big")),
    }
    der_sig = private_key.sign(challenge["nonce"].encode(), ec.ECDSA(hashes.SHA256()))
    r_int, s_int = decode_dss_signature(der_sig)
    signature = b64url_encode(r_int.to_bytes(32, "big") + s_int.to_bytes(32, "big"))
    response = session.post(f"{BASE_URL}/api/videos/access/attest", json={
        "viewer_id": "",
        "device_id": "",
        "challenge_id": challenge["challenge_id"],
        "nonce": challenge["nonce"],
        "signature": signature,
        "public_key": jwk,
        "client": _client_fingerprint(),
        "storage": {},
        "attributes": {"entropy": "high"},
    })
    response.raise_for_status()
    data = response.json()
    return {
        "token": data["token"],
        "viewer_id": data["viewer_id"],
        "device_id": data["device_id"],
        "confidence": data["confidence"],
    }


def solve_captcha(session, code, fingerprint):
    """Démarre le défi PoW, le résout et retourne le clearance token."""
    response = session.post(
        f"{BASE_URL}/api/videos/{code}/embed/captcha",
        json={"fingerprint": fingerprint},
    )
    response.raise_for_status()
    challenge = response.json()

    solution = solve_pow(challenge["pow_nonce"], challenge["pow_difficulty"])
    if not solution:
        raise RuntimeError("Échec de la résolution du proof-of-work")

    response = session.post(
        f"{BASE_URL}/api/videos/{code}/embed/captcha/verify",
        json={
            "pow_token": challenge["pow_token"],
            "solution": solution,
            "fingerprint": fingerprint,
        },
    )
    response.raise_for_status()
    verification = response.json()
    if verification.get("status") != "ok":
        raise RuntimeError(f"Vérification du captcha refusée : {verification.get('reason')}")
    return verification["token"]


def fetch_playback(session, code, fingerprint, clearance_token):
    """Récupère la configuration de lecture chiffrée."""
    response = session.post(
        f"{BASE_URL}/api/videos/{code}/embed/playback",
        json={"fingerprint": fingerprint},
        headers={"X-Captcha-Token": clearance_token},
    )
    response.raise_for_status()
    return response.json()


def select_key_parts(playback):
    """Sélectionne les fragments de clé selon la version (table [n, 31-n], indices 1-based)."""
    version = int(playback.get("version", ""))
    parts = playback.get("key_parts", [])
    indices = []
    if 1 <= version <= 20:
        a, b = version, 31 - version
        if 1 <= a <= len(parts):
            indices.append(a - 1)
        if 1 <= b <= len(parts):
            indices.append(b - 1)
    if not indices:
        indices = list(range(len(parts)))
    return [parts[i] for i in indices]


def decrypt_playback(playback):
    """Déchiffre le payload AES-GCM et retourne le JSON des sources."""
    key = b"".join(b64url_decode(p) for p in select_key_parts(playback))
    iv = b64url_decode(playback["iv"])
    payload = b64url_decode(playback["payload"])
    plaintext = AESGCM(key).decrypt(iv, payload, None)
    return json.loads(plaintext.decode("utf-8"))


def main():
    session = requests.Session()

    print(f"=== Vidéo {VIDEO_CODE} sur {BASE_URL} ===\n")

    details = session.get(f"{BASE_URL}/api/videos/{VIDEO_CODE}/embed/details").json()
    print("Titre      :", details["title"])
    print("Poster     :", details.get("poster_url"))
    if details.get("description"):
        print("Description:", details["description"])
    print("Créée le   :", details.get("created_at"))
    print("Cache      :", details.get("cache_status"))
    print()

    settings = session.get(f"{BASE_URL}/api/videos/{VIDEO_CODE}/embed/settings").json()
    print(f"Captcha requis        : {'oui' if settings.get('captcha_required') else 'non'}")
    print(f"Premium uniquement    : {'oui' if settings.get('premium_only') else 'non'}")
    print(f"Téléchargement autorisé: {'oui' if settings.get('download_allowed') else 'non'}")
    print()

    print("Attestation d'empreinte (ECDSA P-256)...")
    fingerprint = attest_fingerprint(session)
    print(f"  viewer_id = {fingerprint['viewer_id']}")
    print(f"  device_id = {fingerprint['device_id']}")
    print(f"  confidence = {fingerprint['confidence']:.3f}")
    print()

    print("Résolution du captcha (proof-of-work)...")
    clearance_token = solve_captcha(session, VIDEO_CODE, fingerprint)
    print("  Clearance token obtenu.")
    print()

    print("Récupération de la configuration de lecture...")
    raw = fetch_playback(session, VIDEO_CODE, fingerprint, clearance_token)
    playback = raw["playback"]
    print(f"  Version de chiffrement : {playback.get('version')}")
    print(f"  Fragments de clé       : {len(playback.get('key_parts', []))}")
    print()

    config = decrypt_playback(playback)
    print("=== Flux vidéo (sources) ===")
    sources = config.get("sources", [])
    if not sources:
        print("Aucune source retournée.")
    for i, src in enumerate(sources):
        print(f"\nSource #{i + 1}")
        print(f"  Qualité  : {src.get('label')} (hauteur {src.get('height')})")
        print(f"  MIME     : {src.get('mime_type')}")
        print(f"  Débit    : {src.get('bitrate_kbps')} kbps")
        print(f"  Taille   : {src.get('size_bytes')} octets ({src.get('size_bytes', 0) / 1024 / 1024:.1f} Mo)")
        print(f"  URL      : {src.get('url')}")
    if config.get("tracks"):
        print("\n=== Sous-titres ===")
        for track in config["tracks"]:
            print(f"  {track.get('language')} - {track.get('title')}: {track.get('url')}")
    if config.get("poster_url"):
        print("\nPoster :", config["poster_url"])
    print("\nExpire le :", config.get("expires_at"))


if __name__ == "__main__":
    try:
        main()
    except requests.HTTPError as e:
        print(f"Erreur HTTP : {e}", file=sys.stderr)
        try:
            print(e.response.text[:500], file=sys.stderr)
        except Exception:
            pass
        sys.exit(1)
    except Exception as e:
        print(f"Erreur : {e}", file=sys.stderr)
        sys.exit(1)
```

## Résolveur YAML (`byse.yaml`)

Structure calquée sur `doodstream.yaml` (racine `id/title/description/http/queries`).
Limité à la détection + métadonnées publiques ; la résolution native du flux y est
documentée mais non exprimable (voir sections précédentes).

```yaml
id: byse
title: Byse
description:
  en: "Resolver for Byse embed pages (lukefirst.lol and mirror hosts)"
http:
  mode: auto

# --------------------------------------------------------------------------
# Attention : résolution native du flux NON exprimable en YAML pur.
#
# Le lecteur SPA de Byse (route /e/:code, bundles /assets/index-*.js et
# videoPagesBundle-*.js) protège la lecture derrière une chaîne de crypto qui
# s'exécute exclusivement dans le navigateur (voir fetch_stream.py pour le
# flux complet implémenté en Python) :
#
#   1. GET  {base}/api/videos/{code}/embed/details    -> métadonnées vidéo
#   2. GET  {base}/api/videos/{code}/embed/settings   -> captcha_required, ...
#   3. POST {base}/api/videos/access/challenge        -> nonce à signer
#   4. POST {base}/api/videos/access/attest           -> fingerprint ECDSA P-256
#        (signature r||s en base64url + JWK ; validée côté serveur)
#   5. POST {base}/api/videos/{code}/embed/captcha    -> défi PoW
#        {pow_nonce, pow_difficulty, pow_token} ; hash à `difficulty` bits
#        de tête à zéro, résolu en local (script C/JS compilé à la volée)
#   6. POST {base}/api/videos/{code}/embed/captcha/verify -> clearance token
#        (envoyé ensuite dans le header X-Captcha-Token)
#   7. POST {base}/api/videos/{code}/embed/playback   -> payload AES-GCM
#        (clé = concat des key_parts sélectionnés par la version via la table
#        [n, 31-n] 1-based, iv 12 octets, pas d'AAD) -> sources[] {url,label,
#        height,mime_type,bitrate_kbps,size_bytes} + tracks[] (sous-titres)
#
# Contraintes d'expression en YAML :
#   - attestation : ECDSA P-256 sur nonce -> aucun action crypto dans le
#     format (exec_js s'exécute dans boa_engine, sans WebCrypto ; il ne
#     retourne que des variables numériques). Le serveur VALIDE réellement la
#     signature (testé : empreinte non signée / clé publique absente ->
#     400 "signature required" / "public key required").
#   - PoW : implémentation JS personnalisée (pow-DEJGtdh2.js) non présente
#     en inline dans le HTML, injectable uniquement via scripts[] hardcodés.
#   - déchiffrement : AES-GCM indisponible dans le moteur d'actions.
#
# => La résolution native du flux doit passer par le navigateur
#    (http.execution: page_navigate / page_click) où le JS de la page
#    exécute attestation + PoW + playback + déchiffrement. Le m3u8 résolu
#    reste en mémoire JS (player JW8 + hls.js -> <video>.src = blob), il
#    n'apparaît PAS dans le DOM ; une extraction sélecteur native est donc
#    impossible. Comportement attendu du moteur : aucun stream_url n'étant
#    émis, get_stream retombe sur { "embed-link": "<url>" } et la lecture se
#    fait en iframe (le navigateur de l'utilisateur déroule toute la chaîne
#    de protection). Les métadonnées (titre, poster, description) restent
#    récupérables via l'API publique /embed/details, sans authentification.
# --------------------------------------------------------------------------

queries:
  # ------------------------------------------------------------------------
  # can_resolve_html — reconnaissance de la page embed Byse
  #
  # La page brute (SPA React) ne contient que le shell : <div id="root">,
  # le bootstrap video-embed-mode et le <title>Byse Frontend</title>. On
  # détecte ces marqueurs sur le HTML pré-fetched ({html}) ainsi que sur
  # l'origine ({origine}) pour accepter lukefirst.lol et ses miroirs
  # (ex. le frame CDN dismz4n3wp6xnr3.org qui sert la même application).
  # ------------------------------------------------------------------------
  - name: can_resolve_html
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{html}"
        actions:
          - type: regex_find_all
            pattern: '(?i)(<title>Byse Frontend</title>|video-embed-mode|dismz4n3wp6xnr3\.org|/assets/index-[A-Za-z0-9_-]+\.js)'
            format: "{service_id}"

  # ------------------------------------------------------------------------
  # resolve_stream — extraction des métadonnées via l'API publique.
  #
  # La valeur de l'entrée porteuse (details_url) est construite à partir du
  # code vidéo extrait de l'URL d'entrée, puis servie comme URL de requête
  # de la sous-requête JSON. Les champs extraits (title, poster, description)
  # sont fusionnés à la racine. Aucun stream_url n'est émis volontairement :
  # voir le commentaire de tête pour la justification (crypto navigateur).
  # ------------------------------------------------------------------------
  - name: resolve_stream
    scraper_type: html
    base_url: "{url}"
    query_url: "{url}"
    input_html: "{html}"
    row_selector: "html"
    entries:
      # Code vidéo extrait de l'URL : /e/{code}, /api/videos/{code}/..., ...
      - name: code
        type: string
        value: "{url}"
        actions:
          - type: regex_find_all
            pattern: '(?:/e/|/api/videos/)([A-Za-z0-9]+)'
            format: "{1}"
      # Porteuse : transforme le code en URL d'API, puis sous-requête JSON.
      - name: details_url
        type: string
        value: "{url}"
        actions:
          - type: regex_find_all
            pattern: '(?:/e/|/api/videos/)([A-Za-z0-9]+)'
            format: "{origine}/api/videos/{1}/embed/details"
        sub_queries:
          - scraper_type: json
            row_pointer: "/"
            entries:
              - name: title
                type: string
                pointer: "/title"
              - name: image/title > link
                type: string
                pointer: "/poster_url"
              - name: description
                type: string
                pointer: "/description"
```
