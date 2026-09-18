# Analyse : indication normalisée du prix et de l’accès aux vidéos

> Créé le 2026-09-18  
> Périmètre : `server/services/arachnea-stream/legal-stream/{antennereunion-fr,tf1-fr,rtbf-auvio-be}.yaml` et `front/public-app/`  
> Statut : implémenté le 2026-09-18.

---

## 1. Constat actuel

Les services **Antenne Réunion**, **TF1+** et **RTBF Auvio** peuvent retourner des programmes qui ne sont pas accessibles avec un simple compte gratuit : accès inclus dans une offre premium, achat/location à l’unité, ou droits dépendant du compte.

Les réponses YAML publient désormais un champ homogène optionnel `price` pour les accès premium explicitement exposés par les catalogues. Le frontend prévient donc l’utilisateur avant l’ouverture du lecteur, tout en conservant le contrôle d’autorisation final chez le fournisseur.

Le besoin est de faire remonter une information **facultative** nommée `price`, visible :

- dans les vignettes de l’accueil et des catégories ;
- dans les résultats de recherche ;
- dans `get_entry`, au niveau de l’entrée globale ;
- dans `get_entry` et `get_season`, au niveau de chaque épisode.

Les valeurs fonctionnelles demandées sont :

- `free` : contenu accessible sans paiement supplémentaire ;
- `premium` : contenu nécessitant un abonnement/offre premium ;
- un montant avec devise : par exemple `5.99 EUR` ou `6.99 USD`, pour un achat/location unitaire.

---

## 2. État de l’architecture

### 2.1 Backend et YAML

Les résultats des requêtes de scraping sont des objets JSON façonnés par les entrées YAML. Il n’existe pas de modèle Rust fermé pour les champs de carte ou de détail qui imposerait l’ajout préalable d’un attribut côté backend. Un champ YAML scalaire `price` peut donc traverser l’agrégateur tel quel.

Les trois sources ciblées possèdent déjà les requêtes nécessaires :

| Source | Accueil / catégorie | Recherche | Détail | Épisodes |
|---|---|---|---|---|
| `antennereunion-fr` | `load_home`, `get_category`, `get_section` | `search` | `get_entry` | ancre `entries_episode`, `get_season` |
| `tf1-fr` | `load_home`, `get_category`, `get_section` | `search` | `get_entry` | `get_season` |
| `rtbf-auvio-be` | `load_home`, `get_category`, `get_section` | `search` | `get_entry` | ancre `rtbf_episode_entries`, `get_season` |

Les extracteurs implémentés restent déclaratifs dans les YAML : Antenne Réunion utilise le package premium `packageIds[*] = 116`, TF1+ utilise `offers[*].isPremium`, et RTBF Auvio utilise `resourceType: MEDIA_PREMIUM`. Les réponses inspectées n’exposaient aucun montant transactionnel unitaire fiable : aucun YAML n’émet donc de montant pour le moment.

### 2.2 Frontend

Le frontend filtre et normalise les propriétés brutes dans `front/public-app/src/services/rustify.ts` :

- `normalizeMediaItem()` construit les cartes utilisées par l’accueil, les catégories et la recherche ;
- `normalizeEntryDetails()` construit l’objet de `get_entry` ;
- `normalizeEntryEpisode()` construit les épisodes ;
- `normalizeEntryEpisodes()` et `normalizeEntrySeasons()` réutilisent cette normalisation pour les listes embarquées ou chargées avec `get_season`.

Les types frontend à étendre sont :

- `MediaItem` dans `front/public-app/src/types/media.ts` ;
- `EntryDetails` et `EntryPlayableItem` dans `front/public-app/src/types/entry.ts`.

Les vignettes sont centralisées par `MediaCard` et ses sous-composants. Ce même parcours couvre les collections de l’accueil, catégories, recherche et épisodes (`EntryDetailsCatalogSection` transforme les épisodes en `MediaItem` via `toEntryEpisodeMediaItem()`). Cela permet un affichage cohérent avec une modification locale, sans cas particulier par écran.

Le détail d’une entrée possède un panneau de métadonnées distinct (`EntryDetailsMetadata.vue`), alimenté par `entryDetailsMetadataPresentation.ts`. C’est l’emplacement adapté pour afficher l’accès global sous la forme d’un fait ou d’un badge.

---

## 3. Contrat recommandé pour `price`

### 3.1 Présence et absence

`price` doit être un champ optionnel de type `string | null` à tous les niveaux. Son absence signifie que le contenu est accessible gratuitement et qu’aucune offre payante pertinente n’est proposée par le service pour cette vidéo. Dans ce cas, le frontend n’a pas besoin d’afficher une information d’accès.

`free` reste une valeur autorisée lorsqu’un fournisseur l’expose explicitement ou lorsqu’elle est utile pour distinguer une offre gratuite d’autres offres du même service. Son affichage demeure toutefois facultatif : l’interface peut ne rien afficher pour `free`, comme pour une valeur absente.

### 3.2 Valeurs canoniques

La valeur normalisée publiée par les YAML devrait appartenir au domaine suivant :

```text
free | premium | <montant> <devise>
```

Exemples :

```text
free
premium
5.99 EUR
6.99 USD
4.50 CHF
```

#### Recommandation sur le format monétaire

Le format initialement proposé `5.99eur`/`6.99$` est acceptable pour un affichage brut, mais présente deux défauts :

1. `$` est ambigu (USD, CAD, AUD, etc.) ;
2. les minuscules et la ponctuation libre rendent la validation, le tri et la localisation moins fiables.

La recommandation est donc de **conserver le nom public `price`**, mais de standardiser les valeurs monétaires en montant décimal, suivi d’une espace puis du code ISO 4217 à trois lettres et en majuscules : `5.99 EUR`, `6.99 USD`.

Le frontend peut ensuite afficher `Achat/location : 5,99 €` en français, ou une forme équivalente localisée selon la langue active, tandis que les valeurs `free` et `premium` sont traduites via les fichiers i18n. Si la compatibilité avec le format initial est impérative, le parseur frontend peut temporairement accepter `eur`, `€`, `$` et les convertir lorsque la devise est non ambiguë ; les YAML ne devraient toutefois émettre qu’une seule forme canonique.

### 3.3 Sémantique métier

| Valeur | Sens UI | Conséquence sur le lecteur |
|---|---|---|
| `null` / absente | aucune indication ; contenu gratuit sans offre payante à signaler | lecture proposée normalement |
| `free` | aucune indication par défaut, ou « Gratuit » si une distinction explicite est utile | lecture proposée normalement |
| `premium` | « Abonnement requis » | ne bloque pas automatiquement la lecture ; informe avant l’erreur éventuelle |
| `<montant> <ISO>` | « Achat/location : 5,99 € » | ne bloque pas automatiquement la lecture ; informe avant la résolution |

`price` est une **information d’accès**, pas une preuve d’autorisation. Le resolver doit conserver son comportement : le droit final dépend toujours de la session, de l’offre réellement détenue et des contrôles du fournisseur.

Si une API distingue location, achat et abonnement, le seul champ `price` ne préserve pas totalement cette nuance. Dans le périmètre demandé, le montant sera affiché comme un accès payant unitaire. Une évolution ultérieure pourrait ajouter un champ distinct, par exemple `price-kind: rent | buy | subscription`, sans modifier la signification de `price`.

---

## 4. Extraction et normalisation dans les YAML

### 4.1 Principe général

Chaque source doit publier `price` au plus près de l’objet d’origine, à partir des métadonnées d’offre/droit réellement exposées par son API. La conversion des valeurs propres au fournisseur doit se faire dans les YAML par des actions génériques (`map`, formatage, extraction de texte), conformément à l’architecture : aucune logique spécifique Antenne Réunion/TF1/RTBF ne doit être ajoutée dans `stream_scraper.rs` ni dans une façade Rust.

Pour éviter les divergences entre les écrans, une ancre YAML partagée par forme de contenu doit contenir l’extracteur :

- carte programme ;
- carte vidéo/épisode ;
- entrée `get_entry` ;
- épisode de saison.

L’ordre de priorité recommandé est :

1. montant transactionnel explicite ;
2. statut premium explicite ;
3. statut gratuit explicite ;
4. absence de champ pour un contenu gratuit sans offre payante à signaler.

Un simple `credentials.required: true` au niveau du service ne doit **jamais** être traduit en `premium` : il signifie seulement qu’un compte est requis pour le service, pas que chaque programme exige un abonnement payant.

### 4.2 Antenne Réunion — implémenté

Le YAML possède déjà l’ancre `entries_episode` (actuellement utilisée par `get_season`) et plusieurs listes construites depuis les payloads Tucano. `get_entry` lit `/result/items/0` et les épisodes reposent principalement sur les métadonnées `directMetadata` et l’objet asset.

Les assets Tucano inspectés distinguent les droits par `packageIds`. Le package `116`, présent sur l’asset premium vérifié « Ma vie de Courgette » (`25291`) et absent de l’asset gratuit de comparaison inspecté, est normalisé en `premium`. L’extracteur parcourt l’ensemble des packages avant son filtrage : il ne dépend donc pas de leur ordre. Il est présent dans l’ancre de carte d’asset, l’ancre `entries_episode`, les lignes `homecontent`, la recherche et `get_entry`.

### 4.3 TF1+ — implémenté

TF1+ construit les listes avec les ancres `android_home_item_entries`, `android_home_video_item_entries` et une forme de programme GraphQL partagée. La recherche rassemble séparément `sliderOfSearchPrograms` et `sliderOfSearchVideos`; le détail enrichit ensuite les données par les requêtes Smart TV/GraphQL.

Les réponses GraphQL inspectées exposent `offers[*].isPremium` et `offers[*].type: SVOD` pour TF1+ Premium. Les cartes programme/vidéo, la recherche, les enrichissements vidéo de `get_entry` et les épisodes de `get_season` normalisent `isPremium: true` en `premium`. Les `prices[].formattedAmount` observés correspondent aux formules d’abonnement SVOD et ne sont donc pas publiés comme un montant « Achat/location ». Le paramètre GraphQL `rightType=SHORT` n’est pas utilisé comme signal de droit.

### 4.4 RTBF Auvio — implémenté

RTBF Auvio réutilise l’ancre `rtbf_episode_entries` pour les épisodes embarqués et chargés par saison. Ses listes proviennent principalement de `bff-service.rtbf.be`, tandis que le player passe ensuite par l’entitlement RedBee.

Les réponses BFF inspectées exposent `resourceType: MEDIA_PREMIUM` et des produits partenaires, sans montant unitaire exploitable. Les cartes de section et de recherche, l’entrée `get_entry` et l’ancre `rtbf_episode_entries` normalisent `MEDIA_PREMIUM` en `premium`. Les sous-requêtes de programmes conservent les épisodes `MEDIA` existants et ajoutent en parallèle les épisodes `MEDIA_PREMIUM`.

---

## 5. Évolution frontend proposée

### 5.1 Types et normalisation

Ajouter un attribut optionnel `price: string | null` à :

- `MediaItem` ;
- `EntryDetails` ;
- `EntryPlayableItem`.

Dans `rustify.ts`, centraliser la lecture et la validation dans une petite fonction commune, par exemple `normalizePrice(record.price)`. Elle devra :

- retourner `null` pour une valeur absente ou vide ;
- accepter exactement `free` et `premium` sans différence de casse ;
- accepter le format canonique `<montant> <ISO>` ;
- durant une éventuelle phase transitoire, convertir les variantes confirmées émises par les YAML ;
- ne jamais inventer un statut à partir d’un player, de `credentials.required` ou d’un message d’erreur.

Cette fonction est appelée par `normalizeMediaItem`, `normalizeEntryDetails` et `normalizeEntryEpisode`.

`toEntryEpisodeMediaItem()` devra transférer `episode.price` vers la carte générée ; sans cela, les épisodes du détail ne l’afficheront pas bien que leur payload le contienne.

### 5.2 Libellés et rendu

Prévoir une fonction de présentation commune, par exemple `formatPriceAccess(price, locale)`, retournant au minimum :

- aucune étiquette par défaut pour `free` et pour une valeur absente ;
- `Abonnement requis` pour `premium` ;
- `Achat/location : 5,99 €` pour une valeur monétaire en français, avec un montant et une devise localisés.

Les clés de traduction devront être ajoutées aux ressources i18n existantes. Les chaînes fournisseur ne doivent pas être réutilisées directement pour les deux valeurs sémantiques.

Affichages recommandés :

| Écran | Emplacement recommandé |
|---|---|
| Accueil, catégories, recherche | badge compact superposé à la vignette ou adjacent aux métadonnées ; même composant `MediaCard` pour tous les modes grid/ligne/liste |
| Aperçu d’une carte | fait lisible dans `MediaCardDetailsContent`, afin que l’information reste visible sans dépendre uniquement du poster |
| Détail global | fait « Accès » dans `EntryDetailsMetadata.vue`, ou badge dans la rangée existante de métadonnées |
| Épisodes | badge/fait porté par la même carte réutilisée dans `EntryDetailsCatalogSection` |

Le rendu devrait différencier visuellement les trois états sans rendre le badge dépendant uniquement de la couleur (texte explicite et contraste accessible). L’absence de `price` ne crée aucun espace vide ni badge par défaut.

### 5.3 Interaction avec la lecture

Le premier jalon ne doit pas empêcher le clic ni modifier le resolver. Un utilisateur premium déjà authentifié doit pouvoir lire le contenu. Un utilisateur non abonné reçoit une information avant la lecture, mais l’autorisation demeure déterminée côté fournisseur.

Une amélioration ultérieure, hors périmètre initial, pourrait utiliser `price` pour afficher un message contextualisé si la résolution renvoie une erreur d’entitlement connue. Cela exige une taxonomie d’erreurs distincte et ne doit pas être mélangé à la simple extraction de catalogue.

---

## 6. Plan d’implémentation proposé

1. **Échantillonnage fournisseur.** Obtenir, pour chaque source ciblée, des réponses JSON sauvegardables/inspectables pour les états gratuit, premium et transactionnel lorsqu’il existe.
2. **Décision de contrat.** Valider le format canonique `free | premium | <montant> <ISO>` et la signification de l’absence de valeur.
3. **YAML.** Ajouter les extracteurs `price` aux ancres et requêtes concernées ; privilégier les valeurs directement offertes par le catalogue.
4. **Types frontend.** Étendre `MediaItem`, `EntryDetails` et `EntryPlayableItem`.
5. **Normalisation frontend.** Implémenter la lecture/validation/formatage partagés et propager la donnée aux cartes d’épisodes.
6. **UI.** Ajouter l’indicateur dans le composant de carte commun et dans les métadonnées globales de détail.
7. **Traductions.** Ajouter les libellés français/anglais ou toute locale déjà supportée par l’application.
8. **Validation.** Vérifier les cas détaillés ci-dessous avec des payloads réels et les contrôles existants.
9. **Documentation de livraison.** Ajouter l’entrée correspondante à `CHANGELOG.md`. `docs/TODO.md` n’a pas à être modifié : il est réservé aux suivis DNS, HTTP et proxy et aucune tâche de cette catégorie n’est créée ici.

---

## 7. Validation attendue

### Données

- Chaque YAML se charge et reste valide.
- Une carte gratuite émet `price: free` si l’API le confirme.
- Une carte premium émet `price: premium` si l’API le confirme.
- Un achat/location émet un montant normalisé avec devise ISO, si l’API expose ce prix.
- Une vidéo gratuite sans offre payante laisse `price` absent ou nul ; l’interface n’affiche alors aucun indicateur.
- `get_entry`, les épisodes embarqués et les épisodes récupérés avec `get_season` conservent la valeur.

### Interface

- Les collections accueil, catégorie et recherche affichent le même indicateur.
- Les modes grille, rangée unique et liste restent lisibles sur petit écran.
- Le détail global affiche son niveau d’accès.
- Un épisode premium ou unitaire affiche son propre niveau, même si celui de la série diffère.
- Le contenu reste sélectionnable et la lecture des comptes ayant le droit reste inchangée.
- Un contenu sans `price` conserve exactement l’apparence actuelle.

### Contrôles techniques

- exécuter la vérification TypeScript et le build frontend déjà définis par `front/public-app/package.json` ;
- exécuter les contrôles YAML/scraper ciblés disponibles dans le workspace ;
- tester manuellement au moins une entrée réellement gratuite et une entrée restreinte par fournisseur, avec les contextes de compte appropriés quand ils sont disponibles.

---

## 8. Risques et limites

- **Droits dynamiques :** le statut peut varier selon le pays, l’heure, le profil ou l’abonnement. `price` doit être présenté comme une indication issue du catalogue, non une garantie de lecture.
- **Données partielles :** les endpoints d’accueil et de recherche peuvent ne pas inclure les mêmes droits que le détail. Lorsqu’aucune offre payante n’est identifiable, le contrat prévoit l’absence de `price` et aucun affichage ; il reste indispensable de ne pas inventer une valeur premium ou transactionnelle.
- **Prix multiples :** une API peut exposer plusieurs formules (location HD/UHD, achat, promotion, abonnement). Le contrat minimal ne peut en représenter qu’une ; la règle de sélection devra être documentée par source si ce cas est rencontré.
- **Régressions de YAML :** la mutualisation via ancres réduit le risque, mais une source peut employer des formes différentes entre programme et vidéo. Les extractions doivent être vérifiées sur chacune des branches concernées.
- **Messages player :** ce travail améliore la prévention ; il ne remplace pas à lui seul une normalisation des erreurs d’entitlement au moment de la lecture.

---

## 9. Décisions appliquées

1. le format de sortie monétaire accepté par le frontend est `5.99 EUR`, plutôt que `5.99eur` ou `6.99$` ;
2. le libellé UI français d’un montant est « Achat/location : 5,99 € » ;
3. l’absence de `price` signifie un contenu gratuit sans offre payante à signaler et n’affiche aucun badge ;
4. aucun montant unitaire n’a été observé dans les réponses fournisseurs inspectées, donc les YAML actuels n’émettent que `premium` lorsque le droit est explicite.