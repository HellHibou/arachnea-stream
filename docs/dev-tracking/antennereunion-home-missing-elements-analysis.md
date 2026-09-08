# Analyse — Antenne Réunion : éléments manquants dans certaines sections

**Fichier concerné** : `server/services/arachnea-stream/work-in-progress/antennereunion-fr.yaml`
**Étape** : refonte du `load_home` (post-processor générique `group_items_by_header` ajouté dans `arachnea-scrapyfy`)
**Date de l'analyse** : à partir du payload live de `POST /proxy/homeContent`

---

## 1. Rappel du contexte

L'endpoint `homeContent` renvoie une grille plate ordonnée :

- `idType=6` → **en-têtes de section** (titre dans `content/description`) ;
- `idType=8` → items catalogue (`idCatalog`) ;
- `idType=4` → **assets bruts** (VOD, `idAsset`, **sans `idCatalog`**) ;
- `idType=1` → chaînes live (`idChannel`) ;
- `idType=7` → tuiles lien vers des routes applicatives (`/page/*`, `/offers/`…).

Le nouveau `load_home` extrait toutes les lignes dans un champ temporaire `home_rows`, puis
`group_items_by_header` reconstruit les sections éditoriales à partir des en-têtes.

**Résultat vérifié côté backend** (test d'intégration live + payload) : ~14-15 sections
reconstituées, par ex. « Les chaînes en direct » (4-5), « Séries » (15), « Films à l'affiche » (8),
« Divertissement » (13-14), « Magazines » (37), « Les derniers bons plans » (50-51),
« 100 % jeunesse avec Benshi ! » (12-13), « Toute l'info » (7), « Encore + de sport ! » (7), etc.
Le contenu du payload est **dynamique** : les comptages varient légèrement entre deux appels.

---

## 2. Symptôme 1 : « Films à l'affiche » n'affiche que 2 éléments au lieu de 8

### Observation

Le payload contient bien **8 items** dans « Films à l'affiche », tous de type asset brut
(`idType=4`, designMode `poster`). Données réelles (capture live) :

| Item | `category[]` | `link` produit par le YAML (`category/0`) |
|---|---|---|
| LE MYSTERE DAVAL | [85044, 19063] | `85044` |
| BREAKWATER | [85044, 19063] | `85044` |
| BLIND WATERS | [85044, 19063] | `85044` |
| OLD HENRY | [85044] | `85044` |
| ESCAPE THE FIELD | [85044] | `85044` |
| LETTRES A UN TUEUR | [85044] | `85044` |
| SEX THERAPY | [85043, 85044] | `85043` |
| LA CARRIERE | [85044] | `85044` |

Histogramme des `link` : **85044 → 7 items, 85043 → 1 item**.

### Cause racine

1. Les assets `idType=4` n'ont **pas d'id résolvable via `listContent`** :
   `listContent?id=32614` (idAsset), `?id=27477`, `?id=2226430` (pageid) renvoient tous
   `{"status":true,"result":{"items":[]}}`. Aucun endpoint de détail asset n'a été identifié
   (`getAsset`/`assetInfo`/`asset`/`getVod`/`vodInfo`/`assetContent`/`getMedia` → tous invalides).
   Le YAML utilise donc en `link` le premier id de `category[]` (`/content/0/content/category/0`)
   comme **lien de secours cliquable** vers la catégorie englobante.

2. **Le frontend déduplique les items d'une section par leur URL d'entrée** :
   - `rustify.ts` → `mergeHomeSections` → `dedupeMediaItems` →
     `buildMediaDeduplicationKey` :
     ```ts
     if (normalizedSource && normalizedEntryUrl)
       return `entry:${normalizedSource}:${normalizedEntryUrl}`
     ```
   - `entryUrl` = `resolveEntryUrl(link, source)` = **le `link` tel quel** (un simple id numérique
     n'est pas préfixé par une base).

3. Conséquence : les 7 items partageant le lien `85044` sont **fusionnés en 1** → la section
   n'affiche que `{85044}` + `{85043}` = **2 cartes** au lieu de 8.

4. Conséquence accessoire : cliquer sur une de ces cartes appelle `get_entry` avec `85044`
   (resp. `85043`) → ouvre la catégorie englobante (« Tous les films » vérifié pour 85044),
   pas l'asset précis.

### Vérification dans le code front

- `front/public-app/src/services/rustify.ts` :
  - `normalizeMediaItem` → `entryUrl = resolveEntryUrl(link, source)` ;
  - `buildMediaDeduplicationKey` (l. 2814) : clé `entry:<source>:<link>` dès que link non vide ;
  - `mergeHomeSections` (l. 1290) : `items: dedupeMediaItems(section.items)`.
- Simulation Node du pipeline (payload réel) : `uniqueLinks = 2` → **confirme le 8 → 2**.

---

## 3. Symptôme 2 : « Les sections ne sont plus visibles »

### Ce que dit le code front

- `HomeCatalog.vue` :
  - l. 218-263 : rend `pinnedSections` quand la liste est non vide ;
  - l. 273 : `<section v-if="currentCatalog.sections.length > 0">` rend `otherSections`
    (mode home) ou toutes les sections (mode catégorie) ;
  - aucune condition ne masque une section « normale » : seules les sections épinglées
    (`pinnedSectionKeySet`) sont retirées de `otherSections` mais continuent d'être rendues
    au-dessus (bloc épinglé).
- `useHomeCatalog.ts` : `isSectionPinnable = mode home && Boolean(section.label)` ;
  par défaut `pinnedSectionOrder` = liste vide (IndexedDB) → toutes les sections tombent dans
  `otherSections` → **elles doivent toutes être rendues**.
- `normalizeHomeCatalog` (rustify.ts) ne garde une section que si
  `items.length > 0 || sources.length > 0` — nos 14 sections ont bien des items.

### État réel côté backend

Le test d'intégration et le payload montrent **14-15 sections avec leurs entrées** :
rien ne semble perdu côté extraction.

### Hypothèses les plus vraisemblables (à trancher avec l'utilisateur)

1. **Confusion avec la barre de catégories** : l'ancien `load_home` exposait un bloc `categories`
   (les tuiles `idType=7`), affiché par `HomeCategoryStrip`
   (`v-if="currentCatalog.categories.length > 0"`). Ce bloc a été **retiré** car non fonctionnel
   (routes `/page/*` non résolvables par `listContent`). La disparition de cette « bande » de
   navigation peut être perçue comme la disparition des « sections ».
2. **Cache / build** : `load_home` est servi via `FullCache` côté serveur ; un ancien payload ou un
   binaire front non redéployé peut montrer une version antérieure de la home.
3. **La section « Nos catégories »** ne contient que des tuiles `idType=7`, toutes filtrées par
   `is_skipped` → `group_items_by_header` n'émet pas cette section vide : c'est voulu, mais cela
   retire un repère visuel entre « 100 % jeunesse » et « Les recettes réunionnaises ».
4. Bug réel à confirmer : si l'utilisateur voit la home sans **aucune** section alors que le backend
   en renvoie 14, il faut un screenshot + l'onglet réseau pour identifier ce qui casse à la
   normalisation (une exception JS ferait tomber tout le chargement et afficherait l'écran d'erreur).

---

## 4. Autres constats utiles

- **UTF-8** : le mojibake observé en console PowerShell (`Ã©`, `Ã`…) est un **artefact d'affichage
  de la console** ; l'API et le payload bruts sont en UTF-8 propre (vérifié via Node).
- **« Destination Seychelles ! » en double** : l'API expose deux en-têtes identiques consécutifs
  (2 sections). Le front les fusionne (même `preferenceKey` = `label:<label>`), items concaténés →
  pas de perte.
- **Bannières** : 6 bannières fullscreen ; les 2 sans `idCatalog` (1 asset `idType=4`, 1 tuile
  `idType=7`) sont écartées par le frontend (condition « a un `link` + image/titre »).
- **Site web** : le pattern détail observé côté officiel est `/series/<slug>-<id>/details`
  (ex. `/series/les-routes-du-gout-by-terlaba-1000551/details`) ; aucun équivalent film identifié
  pour l'instant.
- Le bloc `categories` est retiré du YAML ; la résolution des routes `/page/*` est suivie dans
  `docs/TODO.md`.

---

## 5. Options de correction pour « Films à l'affiche »

### Option A — lien unique par asset, parsable par `get_entry` (premier choix)

Construire un lien **unique par asset** de la forme `{idCatégorie}|{idAsset}` (ex. `85044|32614`)
via `build_url`, et faire en sorte que `get_entry` ignore la partie après `|` :

- `build_url` peut concaténer deux champs du row (`category/0` + `idAsset`) ;
- côté `get_entry`, appliquer à `{entry_id}` une action `regex_replace_all`
  (`^([^|]+).*$` → `$1`) (ou `split`) pour ne conserver que l'id de catégorie appelé par
  `listContent`.

Bénéfices :
- la **dédup front voit 8 liens distincts → 8 cartes affichées** ;
- l'ouverture ouvre la catégorie englobante (même comportement qu'aujourd'hui), sans perte
  d'affichage.

À valider : la chaîne `link` → `entry` (front) → `query_url` / `{entry_id}` (backend) pour
choisir un séparateur impossible dans les ids (les ids sont numériques, `|` est sûr).

### Option B — supprimer le fallback `category/0` (simple, dégradé)

Ne plus fournir de `link` pour les assets `idType=4` → `entryUrl = null` → la clé de dédup
front tombe sur `id:<id>` (unique) → 8 cartes visibles, mais **aucune carte n'est cliquable**.

### Option C — trouver le vrai endpoint de détail asset (propre, long terme)

Le site officiel utilise des URLs de détail du type `/series/<slug>-<id>/details`
(observé en dur dans le HTML de `https://www.antennereunion.fr/` pour
« Les routes du goût by Terlaba » : `/series/les-routes-du-gout-by-terlaba-1000551/details`).
Chercher l'équivalent **films** côté API/site pour produire de vrais liens détail uniques et
résolubles. À investiguer dans un suivi séparé (voir `docs/TODO.md`).

---

## 6. Investigation complémentaire — `get_entry` et identifiant des assets

### Ce qui a été confirmé (payload live + bundles Nuxt du site officiel)

1. **L'identifiant unique des assets est le `idItem` racine de la row** (= `idAsset`).
   Structure réelle d'une row `homeContent` :
   `{ idType (dans content/0), idItem (dans content/0), tags, content: [ { content: { ... } } ] }` :
   - `idType=8` (catalogue) : `content/0/idItem == content/0/content/idCatalog` (ex. 1000547) ;
   - `idType=4` (asset brut) : `content/0/idItem` = id asset (ex. **32614** pour LE MYSTERE DAVAL),
     et `content/0/content/category` est présent **uniquement** sur ces rows ;
   - `idType=6/7/1` : `idItem` existe aussi mais ne désigne pas du contenu catalogue.
   Aucune row n'a à la fois `idCatalog` et `category` → pas de collision entre les
   entrées `link` du YAML.

2. **`get_entry` n'est pas cassé pour les catalogues** : `listContent?id=1000547` renvoie bien
   le détail. En revanche `listContent?id=32614` renvoie `{"items":[]}` : l'endpoint
   `listContent` ne résout **pas** les assets.

3. **Le vrai endpoint de détail asset existe** : `POST /proxy/assets` avec le corps
   `assetIds=[32614]&languageId=fra` (découvert dans les bundles Nuxt de
   `https://www.antennereunion.fr`, utilise le même proxy et les mêmes en-têtes que
   `listContent`). Il renvoie le détail complet de l'asset (titre, synopsis, casting,
   réalisation, genres, durée, moralité, catégories, validité).

### Conséquence pour les options de la section 5

- L'option A reste la première étape : lien **unique par asset** (`{category/0}|{idItem}`,
  ex. `85044|32614`) + `get_entry` qui retire la partie après `|`. Le moteur Scrapyfy
  suffît : `build_url` accepte une base non-URL et est évaluable dans les actions d'une
  entrée (le row est passé aux actions), et `request_body_actions` s'applique
  séquentiellement (le `regex_replace_all` opère sur le corps produit par `format_text`).
- L'option C est maintenant **concrétisée** : il ne s'agit plus de « trouver » l'endpoint
  mais de l'intégrer. Le `get_entry` idéal consommerait `POST /proxy/assets` quand le
  `entry_id` est un id asset. Scrapyfy n'ayant pas de branchement conditionnel natif sur
  le format du `entry_id` (vérifié : `request_url_actions`, `fetch_actions_to_field`, etc.),
  cela demanderait soit un post-process générique de type `fallback_query` dans
  `arachnea-scrapyfy` (tenter `listContent`, puis basculer sur `assets` si 0 ligne),
  soit l'ajout d'un format de lien préfixé interprété par le backend. Suivi dans `docs/TODO.md`.

## 7. Décision et implémentation (option A)

Retenue : **option A**.

1. `load_home` : l'entrée `link` de secours des assets devient
   `build_url` base `"{category}|{idItem}"` sur les champs `content/0/content/category/0`
   et `content/0/idItem`. Comme le pointeur `content/0/content/category/0` ne matche que
   les rows assets, les autres types (catalogue, live, tuiles) conservent leur `link`
   d'origine.
2. `get_entry` : ajout d'un `regex_replace_all` après le `format_text` du corps
   (`^(id=\d+)\|\d+` → `${1}`) : le corps `id=85044|32614&includeRoot=…` devient
   `id=85044&includeRoot=…`. Les ids simples (catalogues) ne contiennent pas de `|`
   et ne sont pas modifiés. Vérifié par micro-test Rust sur le crate `regex` :
   `id=85044|32614&…` → `id=85044&…`, `id=85043|32800&…` → `id=85043&…`,
   `id=1000547&…` inchangé.

Résultat attendu : 8 liens distincts dans « Films à l'affiche » → 8 cartes après
déduplication front ; le clic ouvre la catégorie englobante (comportement inchangé,
en attendant l'intégration de `POST /proxy/assets`).

### Limites connues

- Le clic sur un asset ouvre toujours la catégorie englobante (pas le détail du film) ;
  voir `docs/TODO.md` pour le suivi « get_entry assets ».
- La partie après `|` est ignorée par `get_entry` : le séparateur `|` est sûr car les ids
  sont numériques.

### Correctif complémentaire — placeholders jamais alimentés (`get_entry`, `get_season`)

Symptôme : `load_home` fonctionnait mais `get_entry` ne renvoyait aucun résultat.

Cause racine : le backend (`StreamScraper::get_entry` / `get_season` dans
`server/crates/arachnea-stream/src/stream_scraper.rs`) n'injecte qu'un param utile,
**`query_url`**, alimenté avec le lien de la carte tel qu'envoyé par le front
(ex. `1000547` ou le composite `85044|32614`). Les placeholders `{entry_id}` et
`{season_id}` du YAML n'étaient **jamais définis** : le corps POST partait avec le
placeholder non résolu et `listContent` renvoyait `items: []`.

Correctif : `get_entry` et `get_season` construisent leur corps avec
`id={query_url}&includeRoot={include_root}&languageId={language_id}`
(`get_section` utilise déjà `{link}` en param par source, `get_category` utilise
`{category_id}` écho par le front — canaux différents, inchangés).

Validation live (API réelle) :
- `id=1000547&includeRoot=true&languageId=fra` → payload complet
  (`name`, `synopsis`, `serie`, `idCatalog`, `categories` avec les saisons) ;
- `id=2000719&includeRoot=true&languageId=fra` (saison) → payload avec 12 `assets`
  (consommés par `entries_episode`) ;
- `id=85044&includeRoot=true&languageId=fra` → « Tous les films » (cible du lien
  composite `85044|32614` après strip).

---

## 8. Section « sections non visibles » — actions de vérification
- Demander un screenshot de la home et la console réseau (onglet `load_home` + erreurs JS).
- Vérifier que le binaire/la build front et le cache serveur ont bien pris la nouvelle version
  du YAML (le hash YAML alimente la clé de cache serveur → normalement cache miss).
- Vérifier si le problème ne concerne que la **barre de catégories** (retirée volontairement) :
  si oui, la traiter dans le follow-up des routes `/page/*` et non comme une régression des sections.