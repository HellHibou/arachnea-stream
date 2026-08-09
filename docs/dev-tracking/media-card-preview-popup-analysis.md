# Analyse : transformation du panneau d'aperçu des cartes média en popup latéral

> Créée le 2026-08-09.
> Périmètre : `front/src/components/media-card/*`, `front/src/composables/media-card/*`,
> `front/src/composables/media-card-collection/*`, `front/src/components/MediaCard*`.

## État actuel

`MediaCardPreviewPanel.vue` est un panneau overlay affiché **au-dessus de la vignette**
lorsque la souris survole la carte ou que la carte reçoit le focus.

- Il occupe tout l'espace de la carte (`position: absolute; inset: 0`).
- Il applique un fond semi-transparent (`--bg-overlay-media-card-preview`) et un
  `backdrop-filter` qui masquent partiellement la vignette.
- Il contient le titre, le contenu de détails (`MediaCardDetailsContent`) et un bouton
  d'action **« Regarder »** (`MediaCardActionButton`).
- Il est assorti d'un léger `scale` et d'une transition d'opacité.

### Chaîne d'événements actuelle

| Événement | Émetteur | Récepteur |
| --- | --- | --- |
| `openPreview` / `closePreview` | `MediaCardCardLayout` (hover, focus, mouseleave) | `MediaCard` → `MediaCardCollection` → `mediaCardCollectionPreviewManager` |
| `isPreviewOpen` | `MediaCardCollection` (via `openPreviewItemId`) | `MediaCard` → `MediaCardCardLayout` → `MediaCardPreviewPanel` |
| `select` | `MediaCardActionButton` (bouton « Regarder ») | `MediaCardCardLayout.handleSelect` → `MediaCard.handleSelect` → `MediaCardCollection` → `HomeCatalog` → `App.vue.handleSelectedItem` → route `entry-details` / `live-details` |

### Ouverture / fermeture du preview

- **Ouverture** : `@mouseenter` et `@focusin` sur l'article de la carte.
- **Fermeture** : `@mouseleave`, `@focusout` (via `handleFocusOut`), clic hors carte et
  touche `Escape` (globaux dans `mediaCardCollectionPreviewManager.ts`).
- Le mode `list` n'utilise pas le preview (`handlePreviewOpen` est ignoré).

## Problème à résoudre

1. Le panneau d'aperçu recouvre la vignette au lieu de s'afficher **à côté** d'elle.
2. Le panneau a une taille fixe (celle de la carte) ; il doit **s'adapter au contenu**
   (s'agrandir au besoin).
3. Le bouton « Regarder » doit être **supprimé**.
4. Le clic sur la **vignette** doit déclencher la **navigation** (changement de page),
   comme le faisait le bouton « Regarder ».

## Comportement cible

- Au survol ou au focus d'une carte, un **popup** apparaît **à côté de la vignette**
  (à droite par défaut, avec repli automatique à gauche, au-dessus, puis en-dessous
  selon l'espace disponible).
- Le popup est muni d'une **petite flèche** pointant vers la vignette (style tooltip).
- La taille du popup est **calculée d'après son contenu**, plafonnée à **640 × 480 px**
  maximum, avec défilement interne au-delà.
- Le popup reste ouvert tant que le pointeur est sur la vignette **ou** sur le popup.
- Le **clic gauche** sur la vignette déclenche la **navigation directe** (sans ouvrir
  le popup) via la route existante (`entry-details` / `live-details`).
- Le **clic droit** sur la vignette ouvre le lien dans un **nouvel onglet** (comportement
  natif du lien `href`, comme le bouton « Regarder »).
- Le hover/focus continue d'ouvrir le popup ; `Escape`, clic hors zone et perte de focus
  le ferment toujours.
- Le mode `list` reste inchangé (pas de popup).

## Fichiers impactés

| Fichier | Impact |
| --- | --- |
| `front/src/components/media-card/MediaCardPreviewPanel.vue` | Refonte en popup : suppression du bouton, positionnement flottant, taille au contenu |
| `front/src/components/media-card/MediaCardCardLayout.vue` | Clic => sélection/navigation ; gestion hover/focus conservée ; conteneur du popup |
| `front/src/components/media-card/MediaCardActionButton.vue` | **Non supprimé** : toujours utilisé par `MediaCardListLayout.vue` ; seul son usage dans le panneau disparaît |
| `front/src/composables/media-card/mediaCardPreview.ts` | Possible ajout d'un délai de fermeture hover pour laisser la souris rejoindre le popup |
| `front/src/composables/media-card-collection/mediaCardCollectionPreviewManager.ts` | Doit inclure le popup dans la zone qui ne ferme pas au `pointerdown` externe |
| `front/src/components/MediaCard.vue` | Légère adaptation du `canSelectItem` / `href` passés au layout (le popup n'a plus besoin du bouton) |
| `front/src/components/media-card/MediaCardListLayout.vue` | Aucun changement attendu |

## Contraintes techniques du positionnement

La carte actuelle applique :

- `overflow: hidden` sur `.media-card` (nécessaire pour le `border-radius` du poster) ;
- un `transform: scale(...)` sur le panneau pendant la transition ;
- aucun conteneur `position: fixed` fiable : un `position: fixed` est rendu relatif à un
  ancêtre dès que celui-ci possède `transform`, `filter`, `perspective` ou
  `will-change` (contenant bloc). Les états `--preview-open` appliquent un `transform`
  sur le poster et le contenu, pas sur l'article, mais un scale sur le panneau serait
  problématique.

Un popup flottant qui s'affiche **hors des limites de la carte** ne peut donc pas
simplement rester dans le flux de `.media-card` avec `position: absolute`, car
`overflow: hidden` le tronquerait.

## Approches envisagées

### Approche A — Teleport + `position: fixed` + mesure DOM

Le popup est rendu via `<Teleport to="body">`. Sa position est calculée à partir de
`getBoundingClientRect()` de la vignette et de la taille mesurée du popup.

- **Avantages** :
  - Échappe à `overflow: hidden`, aux stacking contexts et aux `transform` des ancêtres ;
  - `z-index` élevé garanti au-dessus des cartes adjacentes ;
  - contrôle fin du placement (droite/gauche/haut/bas) et du clamping au viewport.
- **Inconvénients** :
  - Nécessite un positionnement réactif : recalcule sur `scroll` (viewport `single-row`
    scrollable), sur `resize`, et au changement de taille de contenu (`ResizeObserver`) ;
  - le popup quitte le DOM de la carte : `mouseleave` de la carte doit être compensé
    (délai de fermeture) pour permettre à la souris d'atteindre le popup ;
  - la fermeture au `pointerdown` externe doit ignorer les clics dans le popup téléporté
    (la logique actuelle se base sur `rootElement.contains(target)`).

### Approche B — `position: fixed` sans Teleport

Le popup reste enfant de l'article mais passe en `position: fixed` avec une position
calculée en JS.

- **Avantages** : plus simple que le Teleport (pas de changement de arbre DOM) ; le
  `mouseleave` de l'article couvre le popup tant qu'il est à proximité.
- **Inconvénients** : `position: fixed` peut être cassé si un ancêtre acquiert un
  `transform`/`filter` ; la carte n'en a pas actuellement, mais c'est fragile ;
  l'`overflow: hidden` n'affecte pas un élément fixed **seulement** si aucun ancêtre
  n'introduit de containing block ; à surveiller.

### Approche C — `position: absolute` + `overflow: visible` conditionnel

Sur `.media-card--preview-open`, passer `overflow` à `visible` afin que le popup
(enfant absolu) puisse déborder.

- **Avantages** : aucun JS de positionnement (utilisation `left: 100%` etc.) ;
  le `mouseenter`/`mouseleave` de l'article couvre naturellement popup + vignette.
- **Inconvénients** : le `border-radius` du poster ne sera plus clipé pendant l'ouverture ;
  les cartes voisines (notamment en `single-row` et `grid`) risquent d'être recouvertes
  avec un `z-index` incertain ; le popup sera découpé par le viewport de défilement
  horizontal en mode `single-row` (`.media-card-collection__viewport--single-row` a
  `overflow-x: auto`), ce qui est rédhibitoire pour ce mode.

### Approche D — Positionnement en CSS pur avec ancrage (future `anchor positioning`)

Utiliser `anchor-name` / `position: absolute; position-try` (CSS Anchor Positioning).

- **Avantages** : pas de JS de mesure, déclaratif.
- **Inconvénients** : support navigateur encore partiel (Chrome 125+, absent de Firefox
  et Safari stables en 2026 pour plusieurs parties) ; le viewport scrollable en
  `single-row` reste problématique ; l'`overflow: hidden` de la carte reste un obstacle.

## Recommandation

**Approche A (Teleport + `position: fixed` + mesure DOM)** est la plus robuste :

1. `MediaCardPreviewPanel.vue` :
   - supprimer `MediaCardActionButton`, les props `canSelectItem` / `href` et l'émission
     `select` ;
   - rendre le contenu du panneau dans un `<Teleport to="body">` décoré d'un conteneur
     `position: fixed` ;
   - `width: fit-content`, `height: auto`, `max-width: min(640px, calc(100vw - 16px))`,
     `max-height: min(480px, calc(100vh - 16px))` avec `overflow: auto` interne ;
   - ajouter une **flèche** (pseudo-élément ou élément dédié) orientée vers la vignette ;
   - exposer l'élément racine du popup (via `defineExpose` ou une `ref`) pour la mesure.
2. Nouveau composable de positionnement (ex. `useMediaCardPreviewPosition`) :
   - mesure `getBoundingClientRect()` de la carte (racine) et du popup ;
   - place le popup à droite de la vignette, puis à gauche, puis au-dessus, puis
     en-dessous selon l'espace disponible ;
   - recalcule sur `scroll` (capture), `resize`, et quand le contenu change
     (`ResizeObserver` sur le popup) ;
   - applique le placement via `top`/`left`/`transform` (et non `bottom`/`right` pour
     éviter les sauts au scroll).
3. `MediaCardCardLayout.vue` :
   - exposer la vignette comme lien natif (`<a :href="href">`) pour permettre l'ouverture
     dans un **nouvel onglet** au clic droit, comme le bouton « Regarder » ;
   - `@click` gauche : `preventDefault()` + `handleSelect()` (navigation SPA), sans
     ouvrir le popup ;
   - conserver `@mouseenter`/`@focusin` pour ouvrir le popup ;
   - passer à la racine du popup la position calculée.
4. Fermeture au survol :
   - dans `mediaCardPreview.ts`, ajouter un délai de fermeture (≈150 ms) lors du
     `mouseleave` de la carte, annulé si `mouseenter` du popup est détecté ; ou gérer la
     fermeture côté collection avec un minuteur similaire ;
   - le popup téléporté émet `openPreview` / `closePreview` pour rester synchronisé.
5. `mediaCardCollectionPreviewManager.ts` :
   - conserver la fermeture par `pointerdown` externe et `Escape` ;
   - faire remonter l'élément racine du popup (ou l'ajouter à la map des racines) pour que
     les clics **dans le popup** ne ferment pas le preview.
6. `MediaCard.vue` :
   - ne plus passer `canSelectItem` / `href` au poppanel (seulement au layout liste).

## Points d'attention

- **Mode `single-row`** : le viewport est scrollable horizontalement ; le popup doit
  rester ancré à la vignette pendant le défilement (recalcul au `scroll`).
- **Responsive** : sur écrans étroits, le popup risque de couvrir toute la largeur ;
  prévoir un repli (placement au-dessus/en-dessous, ou popup réduit) similaire aux
  breakpoints existants.
- **Accessibilité** : conserver `tabindex`, `@focusin` / `@focusout` et `Escape`.
  Le popup doit être `aria-hidden` quand fermé et ne pas voler le focus à l'ouverture.
- **Clic droit / nouvel onglet** : pour conserver ce comportement, la vignette doit
  garder un lien `href` natif. Le handler `@click` gauche fera `preventDefault()` pour
  la navigation SPA, sans bloquer le menu contextuel ni l'ouverture dans un nouvel onglet.
- **`MediaCardActionButton`** n'est pas supprimé : la clé i18n `entry.watch` reste
  utilisée par le layout liste. Ne pas retirer ces éléments.
- **Stacking** : s'assurer que le popup (`z-index` élevé) reste sous la barre de
  navigation principale si celle-ci a un `z-index` supérieur, et au-dessus des cartes
  adjacentes.
- **Chargement paresseux des images du popup** : le popup n'étant rendu que lorsqu'il est
  ouvert (conditionné par `isPreviewOpen`), mesurer après le prochain `nextTick()` /
  `ResizeObserver` pour obtenir une taille stable.

## Étapes d'implémentation proposées

1. [x] Créer le composable de positionnement du popup.
2. [x] Refondre `MediaCardPreviewPanel.vue` (Teleport, taille au contenu, suppression du
   bouton et des props obsolètes).
3. [x] Adapter `MediaCardCardLayout.vue` : lien natif sur la vignette (clic droit => nouvel
   onglet), clic gauche => navigation SPA sans popup, liaison position.
4. [x] Ajouter le délai de fermeture hover dans `mediaCardPreview.ts` et la synchronisation
    d'entrée/sortie du popup.
5. [x] Étendre la gestion des clics externes dans
    `mediaCardCollectionPreviewManager.ts` pour inclure le popup.
6. [x] Mettre à jour `MediaCard.vue` (props transmises au layout).
7. [ ] Vérifier visuellement en `single-row`, `grid`, responsive et clavier ; mettre à jour
   `CHANGELOG.md` et `docs/TODO.md` si des suivis existent.