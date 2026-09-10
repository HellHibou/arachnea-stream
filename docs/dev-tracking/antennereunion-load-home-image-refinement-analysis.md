# Analyse — Antenne Réunion : affinage des images et filtrage du `load_home`

**Fichier concerné** : `server/services/arachnea-stream/work-in-progress/antennereunion-fr.yaml`
**Requête concernée** : `load_home` (endpoint `POST /proxy/homeContent`)
**Analyse basée sur** : payload live de `POST /proxy/homeContent` (204 lignes, 15 en-têtes, collecté le 9/9/2026)

---

## 1. Code discriminant trouvé : `tags/designMode/0` et `tags/imageType`

Chaque ligne du payload expose un bloc `tags` racine (`/content/0/tags/...`) avec **trois clés
exploitables** :

| Champ | Valeurs observées | Sémantique |
|---|---|---|
| `tags/imageId` | hash SHA-256 ou **absent/vide** | Id image CDN générique (`type=image`) |
| `tags/imageType` | `asset_vod-poster`, `asset_vod-background`, `category_vod-poster`, `category_vod-background`, `category_taui`, `category_vod-hero`, `channel_taui`, `channel_background`, `link` | **Type d'image CDN natif** (le `type=` du `imgdata`) |
| `tags/designMode/0` | `poster`, `cover`, `square`, `hero-banner`, `fullscreen`, `big-tags` | **Mode de design de la carte** = orientation du visuel |

**Histogramme complet des 204 lignes** (idType × designMode × imageType) :

```
idType=1 dm=cover imgType=channel_background         x2  (chaînes live)
idType=1 dm=cover imgType=channel_taui               x2  (chaînes live)
idType=4 dm=cover imgType=asset_vod-background       x59 (assets, visuel landscape)
idType=4 dm=fullscreen imgType=asset_vod-background  x1  (bannière)
idType=4 dm=poster imgType=asset_taui                x1
idType=4 dm=poster imgType=asset_vod-poster          x18 (assets, visuel portrait)
idType=6 (headers)                                   x15
idType=7 dm=big-tags imgType=link                    x7  (tuiles lien)
idType=7 dm=cover imgType=link                       x1
idType=7 dm=hero-banner imgType=link                 x3
idType=7 dm=poster imgType=link                      x4
idType=7 dm=square imgType=link                      x1
idType=8 dm=cover imgType=category_vod-background    x7  (catalogue, landscape)
idType=8 dm=fullscreen imgType=category_vod-background x4 (bannières)
idType=8 dm=hero-banner imgType=category_taui        x1  (Destination Seychelles)
idType=8 dm=hero-banner imgType=category_vod-hero    x1
idType=8 dm=poster imgType=category_taui             x2
idType=8 dm=poster imgType=category_vod-poster       x71 (catalogue, portrait)
idType=8 dm=square imgType=category_taui             x4  (Top 5)
```

**Règle validée sur les 204 lignes** :
- `designMode = cover` ou `hero-banner` → visuel natif **landscape**
  (`imageType` finit par `-background` / `-hero`, ou taui hero 3480×876 / 2240×672) ;
- `designMode = poster` ou `square` → visuel natif **portrait**
  (`imageType` finit par `-poster`, ou taui portrait 1600×2400) ;
- `designMode = fullscreen` → bannière hero (déjà extraite par le champ `banners`).

---

## 2. Vérifications live CDN (`imgdata`)

Toutes vérifiées en live (téléchargement réel + lecture des dimensions) :

| Test | URL (`{cdn}/proxy/imgdata?...`) | Résultat |
|---|---|---|
| `type=image` sur imageId d'item poster | `objectId=<imageId>&languageId=fra` | **1600×2400** (identique au `category_vod-poster` du même item) |
| `type=image` sur imageId d'item background | | **2240×672** (identique au `category_taui`) |
| `type=asset_vod-background` sur idAsset 32694 | | **1920×1080** |
| `type=asset_vod-background` sur idAsset sans imageId (bons plans) | `objectId=32900` | **1920×1080** ✓ |
| `type=asset_vod-poster` | | **404** sur tout idAsset testé |
| `type=category_vod-background` sur idCatalog | | **3840×2160** |
| `type=category_vod-hero` sur idCatalog 1000579 | | **2240×672** |
| `type=category_taui` sur idCatalog 211693 (Seychelles) | | **3480×876** (landscape hero) |
| `type=channel_taui` sur idChannel 1 | | **1980×1080** |

Constat clé : **`type=image&objectId={imageId}` renvoie exactement la même image que le type
natif** (mêmes octets, mêmes dimensions) pour les lignes qui portent un `imageId`. Mais les
lignes **sans `imageId`** (~50-51 items « bons plans ») ne peuvent être servies **que** via
`type={imageType}&objectId={idItem}`.

---

## 3. Symptômes constatés et causes

### 3.1 « Les chaînes en direct » doit être supprimée

La section regroupe les lignes **`idType=1`** (4 chaînes : Direct TV, Exo TV, Passion
Bollywood, Passion Novelas) + une tuile `idType=7` (lien `player.antennereunionradio.fr`,
déjà filtrée). Ces chaînes sont **déjà exposées par `list_lives`** → doublon dans la home.

**Code applicable** : `content/0/idType == 1`. Le filtrage peut se faire au niveau YAML via
le mécanisme `is_skipped` existant (deuxième entrée `is_skipped` pointant `/content/0/idType`
avec `map {"1": "true"}`), ou via un `filter_items` post-process. La voie `map` + `is_skipped`
est la plus cohérente avec le pattern déjà en place.

---

## 3.2 « Destination Seychelles ! », « Encore + de sport ! » et « Nos tutos… » : visuels landscape exposés dans le slot `poster`

### Anatomie des trois sections (payload live du 9/9/2026)

| Section | En-têtes | Lignes | Composition observée | `designMode` | `imageType` | `imageId` |
|---|---|---|---|---|---|---|
| **Destination Seychelles !** | **×2** (rows 57 et 59 : doublon API, le front fusionne via `preferenceKey`) | 1 + 5 | 1 hero catalogue `idType=8` (row 58, `idItem` = `idCatalog` = 211693) + 5 assets `idType=4` (rows 60-64, `idItem` 32694, 32715, 32786, 32807, 32808) | hero-banner (row 58), cover (rows 60-64) | `category_taui` (row 58), `asset_vod-background` (rows 60-64) | présent partout |
| **Encore + de sport !** | 1 (row 138) | 7 | 7 catalogues `idType=8` (rows 139-145, `idItem` = `idCatalog` : 1000553, 1000550, 135186, 134361, 22681, 22388, 22387) | cover | `category_vod-background` | présent |
| **Nos tutos pour une installation facile d'Antenne Réunion+ sur votre TV** | 1 (row 198, **dernière section** du payload) | 4 + 1 | 4 assets `idType=4` (rows 199-202, `idItem` 24493, 24503, 24504, 24506, `category/0` = 84701) + 1 tuile `idType=7` `hero-banner` (row 203, déjà filtrée par l'`is_skipped` existant) | cover | `asset_vod-background` | présent |

### Cause

Toutes ces lignes sont des visuels **nativement landscape** (règle §1 : `cover` / `hero-banner`) mais le
YAML actuel ne produit que `img/poster > link` (via `actions_image_to_url`, `type=image` sur
`tags/imageId`). Le front lit d'abord `img/landscape` quand l'orientation de la section est `landscape`
(`imageLandscapeUrl ?? imagePosterUrl ?? imageUrl` dans `MediaCardPoster.vue`) : les images *s'affichent*
donc, mais logées dans le slot portrait → cadrage/crop ou letterbox incohérent dès que l'utilisateur
change l'orientation ou le `fit` de la section. Il faut **attribuer chaque image au bon slot** selon son
`designMode`.

Aucun nouveau bug CDN : pour ces rows, `type=image&objectId={imageId}` renvoie exactement l'image native
(vérifié §2 : mêmes octets / dimensions). Sauf row 58 (hero Seychelles) où le type natif exact est
`category_taui` → **3480×876** (hero art du site, plus fidèle que le `category_vod-background` 3840×2160).

---

## 3.3 « Les derniers bons plans » : aucune image (URL cassée faute d'`imageId`)

**51 rows** (147-197), toutes `idType=4`, `designMode=cover`, `imageType=asset_vod-background`,
**sans `tags/imageId`** (ce sont les seules lignes du payload sans `imageId`).

### Cause racine (confirmée dans le moteur)

- `actions_image_to_url` construit `img/poster` avec `build_url { id: /content/0/tags/imageId }`.
- Dans `build_url.rs`, un champ absent du JSON n'entre pas dans `runtime_params`, et
  `replace_template_placeholders` (query_helpers.rs) **conserve le placeholder littéral** quand la clé
  est absente → URL `…&objectId={imageId}…` invalide → carte sans image.

### Correctif possible (vérifié live)

`type=asset_vod-background&objectId={idItem}` (ex. `objectId=32900`) → **1920×1080 ✓**. Pour ces rows
`idItem` == `idAsset` → le type natif du tag sert directement l'image landscape.

---

## 4. Plan d'implémentation (YAML `load_home`, rien à ajouter au moteur)

1. **Chaînes en direct** : deuxième entrée `is_skipped` pointant `/content/0/idType` avec
   `map {"1": "true"}` (même pattern que la tuile `idType=7` existante) — les 4 chaînes `idType=1`
   de la section sont déjà exposées par `list_lives` (doublon). La tuile lien `idType=7` de la même
   section est déjà filtrée.
2. **Champ helper** `_design_mode` (type string, `select: first`, pointer
   `/content/0/tags/designMode/0`) sur `home_rows` pour piloter les suppressions conditionnelles.
3. **`img/landscape > link` pour toutes les lignes** via un `build_url` « natif dynamique » :
   ```yaml
   - type: build_url
     base: "{cdn_base_url}/proxy/imgdata?type={image_type}&objectId={item_id}&languageId={language_id}"
     fields:
       image_type: /content/0/tags/imageType
       item_id: /content/0/idItem
   ```
   - rows **avec** `imageId` : même image (vérifié §2) → aucune régression ;
   - **bons plans** (sans `imageId`) : `asset_vod-background&objectId={idItem}` → 1920×1080 ✓ ;
   - hero Seychelles (row 58) : `category_taui&objectId=211693` → 3480×876 (hero art) ;
   - hero de « Séries » (row 32) : `category_vod-hero&objectId=1000579` → 2240×672.
4. **`img/poster > link` conservé tel quel** (type=image sur `imageId`) pour les lignes portrait
   (`poster` / `square`).
5. **Deux passes `filter_fields`** sur `home_rows` pilotées par `_design_mode`, calquées sur
   `rtbf-auvio-be.yaml` (pattern déjà en place dans le repo) :
   ```yaml
   - type: filter_fields            # lignes portrait : on garde poster, on enlève landscape
     source: home_rows
     field: _design_mode
     pattern: "^(poster|square)$"
     keep_matching: true
     remove:
       - img/landscape > link
       - _design_mode
   - type: filter_fields            # lignes paysage : on garde landscape, on enlève poster
     source: home_rows
     field: _design_mode
     pattern: "^(poster|square)$"
     keep_matching: false
     remove:
       - img/poster > link
       - _design_mode
   ```
   → chaque ligne ne garde qu'une image, dans le slot conforme à son `designMode`.
   (Les tuiles `idType=7` restent sans image ; leurs URLs `type=link&objectId=…` seraient des
   artefacts, à nettoyer si besoin dans les mêmes passes, ou ignorées car déjà `is_skipped`.)
6. **Ordre du `post_process`** : `filter_fields` ×2 **d'abord**, puis `group_items_by_header`
   (source `home_rows`). Ajouter `_design_mode` dans `remove_item_fields` du groupage en garde-fou
   si une branche ne l'a pas déjà retiré.

### Validations effectuées (10/9/2026, serveurs up)

- `cargo test -p arachnea-stream test_query_load_home -- --nocapture` avec
  `ARACHNEA_TEST_QUERY_SOURCE=arachnea-stream/work-in-progress/antennereunion-fr.yaml`
  : **OK**, avec `log_response` activé temporairement :
  - « Les chaînes en direct » absente des `sections` ✓ ;
  - chaque item émet exactement une image : 96 items `img/landscape` (poster
    null) + 68 items `img/poster` (landscape null), aucun item avec les deux ou
    aucun ✓ ;
  - bons plans / tutos : `img/landscape` présent avec
    `asset_vod-background&objectId={idItem}` (ex. `objectId=32900`,
    `objectId=24493`) ✓ ;
  - hero Seychelles : `category_taui&objectId=211693` ✓ (tuile hero
    `hero-banner` conservée en slot landscape).
- `log_response` remis à `false` dans `stream_scraper_tests.rs` après vérification.
