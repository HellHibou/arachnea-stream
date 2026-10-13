# Analyse et plan d'implémentation des homes filtrées par source et personnalisées

Date : 12 octobre 2026

Statut : décisions et clarifications validées. Lots 1A et 1B implémentés sur autorisation de l'utilisateur ; contrôles automatisés et limites de validation consignés ci-dessous. Les lots 2 et les actions d'ajout personnalisées restent en attente.

## 1. Objectifs et limites de livraison

Fournir la fonctionnalité demandée en deux étapes utilisables indépendamment :

1. Autoriser `load_home` à sélectionner une liste de sources backend et permettre au frontend public d'afficher une source à la fois.
2. Permettre aux utilisateurs de créer, nommer, modifier, organiser et supprimer des pages d'accueil personnalisées en combinant bannières, catégories et sections provenant de différentes sources. Introduire une navigation latérale gauche inspirée de la mise en page YouTube demandée.

Ce document consigne les observations initiales, les contrats retenus, les alternatives, les lots d'implémentation et les contrôles d'acceptation. La section 2 décrit l'état avant les lots 1A/1B ; leur état livré est consigné en section 7. Les lots 2 ne sont pas autorisés par la demande actuelle.

L'implémentation initiale de l'interface personnalisée doit être locale au navigateur/à l'interface web de bureau. La synchronisation serveur, les comptes utilisateurs, les URL de scraping personnalisées arbitraires, l'infrastructure de glisser-déposer et la nouvelle infrastructure de test ne sont pas incluses.

## 2. Implémentation actuelle

Tous les chemins ci-dessous sont des chemins absolus dans l'espace de travail inspecté.

### 2.1 Backend Découverte et agrégation

- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/stream_scraper.rs` : `LoadHomeRequest` est vide. `StreamScraper::load_home(context)` exécute la requête `load_home` avec `FullCache`, ajoute la provenance de la source via `Some("source")` et ne transmet aucune liste de sélection de source.
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/reloadable_stream_scraper.rs`: le gestionnaire `load_home` enregistré ignore explicitement la requête et délègue à l'instantané actuel du scraper.
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs` : `execute_query_async` accepte déjà `scrapper_list: Option<&Vec<String>>`. Les collections de la requête sont filtrées avant l'exécution. `None` sélectionne toutes les collections configurées ; une liste explicite correspond exactement aux noms des collections.
Le même agrégateur trie et déduplique la liste des services utilisée dans son flux de cache. Les fragments ETag globaux sont extraits des validations de la source. La sélection de la source doit être effectuée via ce chemin existant et non appliquée après l'extraction des données.
Une liste explicite ne correspondant à aucune collection configurée génère actuellement une erreur d'agrégation structurée, et non une requête vide réussie. Les noms inconnus dans une liste mixte peuvent être exclus silencieusement par le filtrage des collections. Il est inutile de qualifier cela de validation stricte sans l'avoir implémentée.
La politique d'activation des services participe à l'enregistrement et au chargement des sources. Un filtre de requêtes ne doit pas réactiver une source désactivée administrativement.
- `get_service` exécute `service_stream_metadata`; sa représentation frontale comprend actuellement `id`, `title`, `logo`, des descriptions localisées et des thèmes, mais aucun champ de capacité de requête prise en charge.

### 2.2 Endpoints de contenu existant et réutilisables

La fonction `get_category` accepte des descripteurs de source définis en YAML, une page et des paramètres spécifiques à chaque source. Elle nécessite un nom de source et renvoie un catalogue structuré autour de la page d'accueil.
La fonction `get_section` accepte une source, un lien, une page et des paramètres spécifiques à chaque source. L'interface prend déjà en charge le chargement différé initial et la pagination ultérieure par source.
- `get_banners` et le chargeur de bannières différé prennent en charge les collections de bannières découvertes via les descripteurs de source/lien.
- Les spécifications du scraper exigent une découverte `load_home` légère et déconseillent la récupération de chaque catégorie/section/bannière via des sous-requêtes de découverte.

Par conséquent, la composition ne nécessite pas initialement de point de terminaison personnalisé côté serveur. L'extraction spécifique à la source doit rester dans les actions YAML ou les processeurs génériques, et non dans la façade de flux.

### 2.3 Pipeline du catalogue frontend

- `/Users/jdecker/Downloads/arachnea/front/public-app/src/services/rustify.ts`: `loadHomeCatalog()` appelle `load_home` avec `{}` et normalise la réponse.
- `normalizeHomeCatalog` hérite de la provenance de chaque ligne du backend. Il fusionne ensuite les catégories par clé sémantique et les sections par `preferenceKey`, en entrelaçant le contenu provenant de plusieurs sources.
La fonction `normalizeHomeSection` génère un identifiant à partir d'un `id` ou d'une `key` explicite, ou à partir de l'étiquette ou de l'index, en fonction de la source. Sa clé de préférence est dérivée séparément de l'étiquette ou de l'identifiant. Ces identifiants ne constituent pas une identité de fournisseur persistante garantie.
Le fichier `HomeSection.sources` est généré uniquement lorsqu'une section expose un lien et une source héritée. Les sections en ligne uniquement peuvent contenir des entrées sans descripteur de pagination : elles doivent rester sélectionnables pour les pages d'accueil personnalisées.
- `/Users/jdecker/Downloads/arachnea/front/public-app/src/types/home.ts` définit déjà des catégories, des collections de bannières, des sources de sections, des variantes de sous-sections et des données de catalogue.
- `/Users/jdecker/Downloads/arachnea/front/public-app/src/composables/home/homeCatalogData.ts` : la clé de requête est actuellement toujours « home » en dehors du mode catégorie. Un ID de requête incrémenté de façon monotone empêche les réponses de catalogue obsolètes et de bannières différées de remplacer un catalogue plus récent.
- `/Users/jdecker/Downloads/arachnea/front/public-app/src/composables/home/useHomeCatalog.ts` : gère la présentation, l’ordre des éléments épinglés, les signets locaux, les éléments d’arrière-plan, le chargement différé des sections et la pagination des sections. De nombreuses recherches et opérations de remplacement sont indexées par `preferenceKey`.
- `/Users/jdecker/Downloads/arachnea/front/public-app/src/components/HomeCatalog.vue` : affiche l’en-tête, la barre de catégories, les barres d’éléments épinglés/désépinglés, les contrôles de sous-section et les actions de section existantes. Réutilisez cette présentation plutôt que de créer un second moteur de rendu de catalogue.

### 2.4 Navigation et persistance

- `/Users/jdecker/Downloads/arachnea/front/public-app/src/router/index.ts` : les routes incluent `/`, `/search`, `/categorie/:categoryToken`, les détails des entrées et les lives. Aucune route source-home ou personalized-home n'existe.
- `/Users/jdecker/Downloads/arachnea/front/public-app/src/views/HomeView.vue` est un wrapper mince autour de `HomeCatalog`.
Le fichier `/Users/jdecker/Downloads/arachnea/front/public-app/src/App.vue` contient l'arrière-plan, la barre d'outils supérieure, le contenu accessible et les notifications d'erreur. Il constitue la surface d'intégration appropriée pour la navigation latérale, et non l'emplacement d'un éditeur de page d'accueil ou d'un résolveur.
- `/Users/jdecker/Downloads/arachnea/front/public-app/src/components/main-bar/MainBarToolbar.vue` contient actuellement les raccourcis accueil et lives ainsi que les commandes de recherche/paramètres.
Le fichier `/Users/jdecker/Downloads/arachnea/front/public-app/src/composables/useServiceMetadata.ts` fournit un registre de métadonnées à portée de session. Réutilisez-le pour les étiquettes/logos sources ; son chargeur ne comporte actuellement aucune promesse partagée en cours ni action de rafraîchissement explicite.
Le fichier `/Users/jdecker/Downloads/arachnea/front/public-app/src/services/homeSections.ts` stocke les préférences d'affichage des sections épinglées dans la base de données IndexedDB `home_sections`, indexée par un nom global. Il ne s'agit pas d'un magasin de définitions d'accueil personnalisées.
Le fichier `/Users/jdecker/Downloads/arachnea/front/public-app/src/services/storage.ts` correspond à l'intégration existante du stockage Pinia. Les favoris sont déjà stockés indépendamment.
Le fichier `/Users/jdecker/Downloads/arachnea/front/public-app/src/services/indexDB.ts` permet de déclarer des magasins d'objets supplémentaires, sérialise l'initialisation via la promesse de connexion précédente et effectue les mises à jour en cas de magasins manquants. La migration de l'index/schéma des magasins existants n'est pas automatique. Les mises à jour de la base de données peuvent être bloquées par un autre onglet.
- L'interface de bureau prend en charge les onglets/vues Web natifs indépendants. Les modifications persistantes ne doivent pas supposer que chaque page partage une seule instance Pinia en mémoire.

### 2.5 Stack confirmé et contraintes de validation

L'application publique utilise l'API de composition Vue 3, TypeScript, Vue Router, Pinia, Vuetify, Vite et l'utilitaire natif IndexedDB existant. Aucune bibliothèque supplémentaire n'est nécessaire pour la configuration de base.

Les scripts frontend existants incluent `type-check`, `build-only` et `lint:css` ; aucun script de test frontend n'est déclaré dans le package analysé. Les tests backend scraper existants incluent l'exécution en direct (`use_mock_file: false`), de sorte qu'une commande de test non restreinte ne constitue pas une vérification hors ligne déterministe. La tâche actuelle (TODO) signale également un problème de compilation de tests Scrapyfy préexistant ; son état réel doit être vérifié lors de l'implémentation et non présumé.

## 3. Principaux risques de conception

1. **Le filtrage après agrégation est insuffisant :** il contacte toujours des fournisseurs indésirables et peut laisser des catégories/sections déjà fusionnées contenant d'autres sources.
2. **Les étiquettes ne constituent pas des identités persistantes :** elles peuvent changer, entrer en conflit au sein d’une même source ou fusionner entre différents fournisseurs. Une sélection persistante ne doit pas dépendre uniquement de son titre visible ou de son index dans un tableau.
3. **Les préférences globales se répercutent actuellement entre les catalogues :** l'épinglage ou le style d'un rail peut affecter une autre home ayant la même clé de préférence.
4. **Les raccourcis de catégorie et le contenu de catégorie sont différents :** la sélection d'une vignette de catégorie permet actuellement d'accéder à un catalogue de catégories ; l'intégration des sections de ce catalogue dans une page d'accueil personnalisée nécessite la propriété de la résolution et de la pagination.
5. **La découverte doit rester légère :** l'ouverture d'un menu source ne doit pas extraire toutes les pages d'accueil des fournisseurs ; l'ouverture d'une page d'accueil personnalisée ne doit pas hydrater les rails/catégories non sélectionnés.
6. **Les pannes partielles sont normales :** un fournisseur défaillant ou une section supprimée ne doit pas invalider une home personnalisée par ailleurs utilisable.
7. **La navigation rapide affecte plus que les chargements initiaux :** les bannières différées, les chargements de section, la pagination, les candidats d'arrière-plan et la lecture du héros ont tous besoin de l'identité du catalogue actif.

## 4. Étape 1 : filtré par source `load_home`

### 4.1 Contrat de requête proposé

Utilisez un tableau JSON optionnel nommé `sources` contenant les identifiants exacts des sources backend, et non les titres d'affichage, les URL sources ou les descripteurs de catégorie.

Exemples d'objets de requête :

```json
{}
```

```json
{"sources":["arte-fr"]}
```

```json
{"sources":["arte-fr","rtlplay-be"]}
```

Sémantique recommandée :

| Entrée | Comportement |
| --- | --- |
| `sources` omis | Conserver le comportement existant de toutes les sources disponibles. |
| `sources: []` | Sélection vide explicite : n’appelez aucun fournisseur ; renvoyez une agrégation réussie vide sans ETag. |
| Liste non vide | Rejeter les identifiants vides ou composés uniquement d'espaces, conserver les identifiants exacts, supprimer les doublons et limiter l'exécution à la liste. Ne pas corriger silencieusement un identifiant entouré d'espaces. |
| Type JSON incorrect ou `null` | Rejeter la désérialisation ; ne pas basculer silencieusement vers toutes les sources. |
| Source inconnue, indisponible ou non prise en charge | Signaler une erreur contextuelle ; ne jamais recourir à toutes les sources. Conserver les résultats valides pour les sélections mixtes si elles sont représentées par l’enveloppe d’erreur d’agrégation existante. |

L'utilisation de `Option<Vec<String>>` avec la sémantique Serde classique confond les entrées omises et les valeurs `null` explicites. Si la règle stricte concernant les valeurs `null` est approuvée, il convient de l'appliquer lors de la désérialisation des champs et de documenter ce point. La gestion des listes vides nécessite également un court-circuit explicite, car l'agrégateur actuel signale une erreur lorsqu'aucune collection n'est sélectionnée.

Autre solution : définir les listes omises et vides comme « toutes les sources » à l’aide d’un `Vec<String>` par défaut. Cette solution est plus simple, mais elle entraîne une recherche globale indésirable pour une sélection personnalisée vide. Le contrat recommandé fait la distinction entre les listes omises et les listes vides.

L'enveloppe de réponse et les champs d'accueil par source restent inchangés. Les objets `sources` de catégorie conservent leur signification actuelle ; le nom `sources` dans `load_home` fait uniquement référence aux identifiants et ne modifie pas `get_category`.

### 4.2 Stratégie d'implémentation backend

1. Étendez `LoadHomeRequest` avec le contrat de liste de sources optionnel approuvé et mettez à jour rustdoc.
2. Transmettez l'entrée analysée du gestionnaire rechargeable à `StreamScraper::load_home`.
3. Normalisez la liste une seule fois et court-circuitez la sélection explicite vide.
4. Transmettez la liste résultante à l'argument d'agrégateur existant utilisé par les appels de catégorie/section ; préservez l'enrichissement d'exécution, la politique de cache, la provenance, les erreurs et la gestion globale des ETag.
5. Vérifiez la disponibilité et la prise en charge des requêtes des API du registre de sources. N’ajoutez qu’une fonction d’assistance à la validation réutilisable et limitée si un reporting strict par source l’exige ; évitez de créer un second registre de sources dans la façade.
6. Conservez le comportement sans filtre actuel, y compris sa politique d'activation. N'étendez pas cette tâche à la modification du comportement de l'agrégateur pour d'autres points de terminaison.
7. Mettez à jour les appelants concernés et les tests existants lorsque la signature de la méthode change.

### 4.3 Navigation par source et propagation frontend

- Ajouter une route source-home nommée, proposée `/source/:sourceId`, avant la route par défaut.
Réutilisez `HomeView` avec un paramètre source optionnel, ou ajoutez une vue source allégée si cela simplifie la compréhension des routes. Utilisez la route comme source active plutôt que de la dupliquer.
- Propager la sélection de la source via `HomeCatalog` -> `useHomeCatalog` -> `homeCatalogData` -> `loadHomeCatalog({ sources })`.
- Étendre la clé de requête de catalogue avec la sélection de source normalisée ; distinguer la requête « toutes sources » héritée d’une liste vide explicite.
Conservez le transport existant : `call_api` sérialise les tableaux au format JSON dans les paramètres de requête REST et transmet les objets à Tauri. Sa clé de cache ETag inclut déjà les paramètres de requête sérialisés ; un ordre stable évite les entrées de cache dupliquées pour des ensembles de sources équivalents.
- Conservez la provenance de la source pour les bannières différées et toutes les demandes de section/catégorie ultérieures. Une catégorie ouverte à partir d'une page d'accueil de source ne doit contenir que le descripteur de cette source.
- Réutilisez `useServiceMetadata` pour présenter un sélecteur de source léger dans l'étape 1. Il est remplacé/intégré par la barre latérale dans l'étape 2, et non par un nouveau système de navigation de niveau supérieur persistant.
- Afficher le titre/logo de la source, l'état de chargement, la source indisponible/inconnue, la page d'accueil vide et l'état de nouvelle tentative. Ne pas précharger toutes les pages d'accueil pour remplir le sélecteur.
- Conserver `/` comme répertoire d'installation agrégé existant pendant l'étape 1.

### 4.4 Capacités découvrables des sources

Les métadonnées ne permettent actuellement pas de prouver si un service implémente `load_home`. Deux approches sont possibles :

- **Étape minimale 1 :** lister les sources de métadonnées et gérer correctement un service sélectionné sans page d’accueil. Cela évite de modifier les métadonnées/YAML, mais peut entraîner des entrées de menu inutilisables.
- **Navigation robuste recommandée :** ajoutez une liste de capacités de requêtes prises en charge, calculée à partir du registre de requêtes du serveur et jointe par identifiant source avec métadonnées. Ne dupliquez pas manuellement les indicateurs de capacité de requête dans chaque fichier YAML. Étendez `ServiceMetadata`/normalization et les spécifications des flux FR/EN si elles sont approuvées.

La navigation robuste et son contrat de découverte sont retenus avec les décisions par défaut validées. Vérifiez que les identifiants des métadonnées correspondent aux noms des collections de requêtes plutôt que de vous fier uniquement aux titres visibles.

## 5. Étape 2 : définitions des homes personnalisées

### 5.1 Propriétés et persistance recommandées

Stockez les définitions utilisateur dans un espace de stockage d'objets IndexedDB dédié, nommé `custom_homes`, indexé par un identifiant stable opaque et selon l'ordre de navigation. Intégrez-le aux conventions de stockage actuelles de Pinia sans utiliser `home_sections` comme table de définitions.

Conservez une définition déclarative versionnée, et non les entrées récupérées, les URL de vidéos signées, les ETags, les pages actuelles ou les identifiants du scraper. Le contenu actuel est résolu à l'ouverture de la page d'accueil. Le stockage du navigateur est local à une origine/un profil ; aucune synchronisation entre appareils n'est garantie. Le comportement du stockage Tauri sur les pages natives doit être vérifié lors de l'exécution de l'application.

Autre solution : définitions côté serveur utilisant les abstractions de persistance existantes. Cette approche permet le partage de définitions, mais nécessite de prendre des décisions concernant la propriété, l’authentification, le déploiement et la portée utilisateur. Elle est volontairement différée, sauf si des répertoires partagés sont demandés.

### 5.2 Modèle de données proposé

Le contrat ci-dessous est implémenté dans `front/public-app/src/types/customHome.ts` depuis le lot 2A :

```ts
interface CustomHomeDefinition {
  schemaVersion: 1
  id: string
  name: string
  navigationOrder: number
  bannerSources: string[]
  categoryShortcuts: CategoryReference[]
  blocks: CustomHomeBlock[]
  sectionMergeMode: 'none' | 'title'
  showBookmarks: boolean
  createdAt: number
  updatedAt: number
}

interface CategoryReference {
  sourceId: string
  categoryKey: string
  descriptor: Record<string, string>
  labelSnapshot: string
}

interface SectionReference {
  sourceId: string
  sectionKey: string
  subsectionKey?: string
  labelSnapshot: string | null
}

type CustomHomeBlock =
  | { id: string; kind: 'home-section'; reference: SectionReference; titleOverride: string | null }
  | { id: string; kind: 'category-content'; reference: CategoryReference; titleOverride: string | null }
```

Le descripteur enregistré est un instantané de la requête de catégorie définie en YAML, et non des paramètres arbitraires saisis par l'utilisateur. Il est préférable de redécouvrir le descripteur de catégorie actuel à l'aide d'une clé stable avant l'exécution. Les secrets d'authentification ne sont jamais conservés. Les sources de bannières sont ordonnées et font référence à la collection complète de bannières de chaque source sélectionnée ; la sélection de bannières temporaires individuelles n'est pas prise en charge initialement.

L'ordre des blocs suit le tableau `blocks`. Chaque bloc possède son propre identifiant, ce qui permet d'utiliser intentionnellement la même source plusieurs fois sans conflit d'état. Les raccourcis de catégorie sont des vignettes cliquables optionnelles. Les blocs de contenu de catégorie intègrent explicitement les éléments du catalogue de la catégorie sélectionnée ; cela couvre les deux sens possibles de « ajouter des catégories » sans perturber la navigation avec le contenu intégré.

### 5.3 Références stables et normalisation

N'introduisez que les métadonnées d'identité nécessaires à la résolution des sélections :

- Privilégiez la clé ou l'identifiant du fournisseur/YAML avec une portée source pour les sections et les catégories.
- Conserver les clés stables séparément des étiquettes de présentation et de la clé de préférence globale `preferenceKey`.
- Conservez les catalogues sources séparés jusqu'à la composition explicite. Un chemin de normalisation configurable sans fusion ou un chargeur par source est nécessaire pour l'éditeur/résolveur ; la vue agrégée héritée conserve son comportement de fusion actuel.
- Conserver les sections en ligne même sans lien de pagination et préserver l'identité des sous-sections.
Si une source ne possède pas de clé stable, il convient, dans la mesure du possible, de fournir une solution de repli documentée à partir de son descripteur/lien de requête persistant. L'utilisation d'une étiquette comme solution de repli ne peut être qu'une solution de dépannage et doit être clairement réparable ; un index de tableau n'est pas une référence fiable à long terme.
Vérifiez les champs YAML des sources sélectionnables et ajoutez les clés stables manquantes uniquement lorsque cela est nécessaire. Limitez ces modifications aux sources locales et mettez à jour la documentation des fixtures/spécifications en conséquence. Ce lot d'identités YAML nécessite une approbation.
Ne remplacez pas silencieusement une section manquante par une autre section au titre similaire. Conservez la définition et proposez une alternative dans l'éditeur.

### 5.4 Résolution et planification des requêtes

1. Chargez la définition et validez son schéma avant d'effectuer des requêtes auprès du fournisseur.
2. Calculer les identifiants de source uniques utilisés par les bannières, les blocs de section et les raccourcis/contenus de catégorie. Une page d'accueil personnalisée vide renvoie un état modifiable vide sans requête sur l'ensemble du code source.
3. Identifiez uniquement les répertoires sources nécessaires à l'aide du point de terminaison filtré. Utilisez une représentation par source pour la résolution des références. Normalisez chaque ligne source renvoyée indépendamment ou demandez des catalogues sources individuels avec déduplication en cours ; conservez le comportement agrégé actuel ailleurs.
4. Sélectionnez uniquement les bannières/catégories/sections demandées avant l'hydratation différée. N'appelez jamais `loadInitialHomeSectionPages` sur tous les rails détectés simplement pour résoudre quelques sélections.
5. Charger les catalogues de contenu de catégories sélectionnés via `get_category` ; conserver leurs descripteurs Rails et de source sous le bloc propriétaire. Charger les sections différées uniquement lorsqu'elles sont rendues/nécessaires, avec une concurrence limitée.
6. Afficher les blocs dans l'ordre défini par l'utilisateur et prendre en charge dès cette étape la fusion des rails par titre. Conserver les références séparées dans la définition ; fusionner après résolution, sans utiliser le titre comme identité persistante. Voir les propositions en section 5.6 et les précisions restantes en section 10.
7. Pour les blocs de contenu de catégorie, conservez la possibilité de paginer indépendamment chaque colonne renvoyée ; aplatissez les colonnes pour le moteur de rendu existant uniquement avec des identifiants sans conflit contenant l’identité du bloc et de la colonne. La progression de la page du catalogue de catégories, si elle est requise par un fournisseur, doit utiliser son propre descripteur `get_category` existant plutôt que de prétendre qu’il s’agit d’un lien `get_section`.
8. Réutiliser les bannières/chargeurs de section et les notifications d'erreur existantes. Représenter localement les sélections manquantes et les blocs ayant échoué tout en conservant les blocs réussis ; proposer des actions de nouvelle tentative/réparation.
9. Protégez chaque mise à jour asynchrone par la révision active de la page d'accueil/définition, y compris la pagination et les mises à jour en arrière-plan. Réutilisez les protections d'ID de requête ; n'ajoutez l'annulation que lorsque la prise en charge du transport est réellement assurée par REST/Tauri.
10. Dédupliquer les requêtes de découverte/catégorie en cours en utilisant une source stable, un descripteur et une page. Ne pas partager l'état de pagination modifiable entre différents blocs d'accueil.

Politique d'affichage des bannières : les bannières de chaque source sélectionnée sont entrelacées selon l'ordre configuré, dédupliquées par identité source/contenu, et les métadonnées du résolveur/géo-proxy sont préservées. Le chargement différé et l'affichage des bannières interactives sont conservés, sans qu'un tri global puisse modifier la politique d'ordre des sources approuvée.

### 5.5 Portée des préférences et migration

**Dernier ajustement validé de l’éditeur :** supprimer la prévisualisation du brouillon. Afficher bannières, raccourcis et blocs en tableaux ; placer le titre personnalisé sur la ligne du bloc avec le service, le contenu et les actions. Largeur d’édition portée à 1000px, ajout/liste conservés à 680px, défilement horizontal sur écran étroit. Enregistrer/Annuler reprennent le style accent de « Voir la fiche » ; l’enregistrement atomique et l’annulation sans écriture restent inchangés. Ces décisions remplacent les propositions antérieures de prévisualisation.

**Ajustement de l’éditeur :** flou fort partagé en arrière-plan du dialogue ; « Fermer » uniquement hors édition, où Enregistrer/Annuler suffisent. Bannières, raccourcis et blocs affichent des flèches haut/bas et une corbeille à côté du service/titre. « Remplacer la référence » est retiré ; la réparation passe par retrait et nouvel ajout dans le brouillon, sans écriture avant enregistrement.

**Ajustement validé :** les options permettent de choisir l’accueil agrégé (défaut initial) ou un accueil personnalisé existant comme accueil par défaut. Le choix s’applique au démarrage sur `/` sans destination explicite et au clic sur la marque ; les liens directs restent inchangés. Une définition supprimée ou illisible entraîne un repli sur l’agrégat. La création passe par le crayon du groupe Accueil ou l’ajout de contenu, sans ligne « Ajouter accueil ». Le dialogue compact reprend les switches et couleurs des paramètres ; les titres de services remplacent les identifiants affichés et les clés techniques sont masquées sans modifier les références.

- Le système de navigation `/` hérité conserve ses préférences globales existantes en matière d'ordre et de présentation.
- Les répertoires sources utilisent un espace de noms de préférences à portée source ; ils lisent initialement les préférences de style hérité compatibles comme solution de repli, puis écrivent les valeurs à portée.
L'ordre personnalisé des répertoires d'accueil est déterminé par sa définition, et non par l'état global des partitions épinglées/désépinglées. La modification des sections existantes ne doit pas remplacer l'ordre explicite des blocs.
- Conserver les styles de remplacement sous l'ID de la page d'accueil + l'ID du bloc + l'identité Rail résolue, avec des valeurs par défaut de vignette globales pour l'utilisateur comme solution de repli.
- Conserver les enregistrements de signets intacts. Comportement validé : les pages sources affichent uniquement les signets appartenant à cette source ; les pages personnalisées incluent un panneau de signets uniquement lorsque `showBookmarks` est activé. La page d'accueil agrégée héritée conserve tous les signets.
Les nouvelles boutiques s'ajoutent aux autres. Ne réinitialisez pas IndexedDB et n'effectuez pas de migration destructive de `home_sections`. Validez les anciens profils et les mises à niveau bloquées ; signalez les erreurs de stockage sans indiquer qu'une sauvegarde a réussi.
- Définir le comportement d'actualisation des onglets croisés : recharger les définitions au minimum lors d'un changement de focus ou de visibilité ; utiliser un canal d'événements compatible existant uniquement si nécessaire. Résoudre les écritures simultanées à l'aide de la révision/de l'horodatage de la définition et avertir avant d'écraser un brouillon de l'éditeur.

### 5.6 Fusion par titre : contrat validé

**Exigence validée :** les homes personnalisées doivent pouvoir fusionner des sections par titre dès la livraison initiale. Les titres restent des critères de regroupement, jamais des identités persistantes ni un moyen de remplacer une référence manquante.

**Contrat validé :** réglage `sectionMergeMode` par home (`none` / `title`), désactivé initialement et activable explicitement par l'utilisateur. En mode `title`, comparer le titre effectif (remplacement utilisateur non vide, sinon titre résolu) après normalisation Unicode NFC, suppression des espaces périphériques, réduction des espaces successifs et conversion en minuscules. Conserver accents et ponctuation ; ne pas traduire les titres ni fusionner les titres absents. Ne pas modifier la fusion de l'accueil agrégé existant.

- Inclure les rails des blocs de contenu de catégorie ; exclure les bannières et raccourcis de catégorie de cette règle.
- Placer le groupe à la première position de ses contributions ; conserver le premier titre d'affichage, entrelacer les contenus selon l'ordre des contributions et réutiliser la déduplication existante.
- L'entrelacement est un tour de rôle : pour deux contributions A et B, afficher `A1, B1, A2, B2, A3, B3, ...` ; pour trois, `A1, B1, C1, A2, B2, C2, ...`. Lorsqu'une contribution est épuisée, poursuivre avec les autres. Fusionner également les doublons intentionnels de rails lorsque l'option est active. La déduplication des médias existante reste applicable : sélectionner deux fois le même rail ne duplique pas ses cartes à l'affichage.
- Distinguer identité du groupe de rendu (fondée sur les identifiants des contributions), clé de regroupement par titre et préférences. Les préférences restent à portée de home et de groupe, pas globales au titre.
- Conserver des descripteurs, curseurs, erreurs et états de chargement propres à chaque contribution. Une pagination/nouvelle tentative doit cibler les contributions concernées. Ne pas reprendre telle quelle la fusion historique de `rustify.ts`, qui expose notamment un `currentPage` global calculé par maximum.
- Préserver les identités des sous-sections et leurs périodes ; ne pas fusionner des variantes temporelles distinctes au seul motif que leurs parents portent le même titre.
- Garder les blocs d'origine dans l'éditeur pour supprimer une contribution sans perdre les autres. Lorsque la fusion est active, présenter les blocs regroupés par titre effectif (remplacement personnalisé, sinon étiquette enregistrée), dans l'ordre de première apparition. Les flèches d'en-tête déplacent tout le groupe ; les flèches des entrées déplacent uniquement une contribution à l'intérieur du groupe. Une modification d'option d'affichage se propage à tous ses blocs en conservant leurs autres options. Pour un bloc de catégorie, utiliser le titre du bloc sans charger ses colonnes. Les changements restent dans le brouillon jusqu'à l'enregistrement ; fusion désactivée, les opérations restent individuelles.

## 6. Navigation latérale et expérience de l'éditeur

### 6.1 Hiérarchie de navigation requise

```text
homes personnalisées
  Mes documentaires
  Films et séries
  Enfants
  + Créer un foyer

lives

Sources [développer/réduire]
  Arte
  RTL Play
  Autres homes de sources disponibles
```

Conservez un accès direct et concis à la page d'accueil principale (par exemple, le lien de la marque) afin que les favoris `/` existants restent valides. N'insérez pas de groupe principal supplémentaire entre les pages d'accueil personnalisées et les lives. Le choix d'inclure ou non la page d'accueil principale dans le premier groupe reste à la discrétion du fabricant.

- La barre latérale liste les homes personnalisées dans un ordre persistant ; utilisez leurs identifiants stables, et non leurs noms, dans les itinéraires.
- Route personnalisée proposée : `/homes/:homeId`. Renommer une home ne modifie pas sa route.
- Lives suit immédiatement le groupe d'accueil personnalisé ; supprimez son raccourci de barre d'outils dupliqué après l'introduction de la barre latérale.
- Les homes sources sont regroupées dans un seul groupe pliable alimenté par des métadonnées/capacités, et non par des catalogues extraits.
- Ordinateur de bureau : barre latérale étendue permanente, avec un mode compact si la largeur de l’écran le justifie. Écrans étroits : panneau latéral superposé accessible depuis la barre d’outils, fermé lors de la navigation, avec possibilité de le fermer avec la touche Échap/arrière-plan et de lui rendre le focus.
Utilisez les variables CSS existantes, Vuetify et les ressources d'icônes. Conservez la navigation au clavier, l'attribut `aria-current` actif, les étiquettes explicites et les attributs `aria-expanded`/`aria-controls` pour les contrôles repliables.
- Conservez les boutons de recherche, de paramètres et de retour dans la barre supérieure. La barre latérale ne doit pas chevaucher les infobulles des paramètres, les notifications ou les commandes principales ; en mode plein écran, la barre latérale ne doit pas rester visible.
- Revérifiez actuellement l'éligibilité de l'application en arrière-plan en fonction des noms de route, des candidats d'arrière-plan source/personnalisé, des titres de documents, de la restauration du défilement et du comportement des onglets natifs.

### 6.2 Parcours d'édition

1. Créez un répertoire personnel vide avec un nom tronqué non vide, ou partez du répertoire personnel source actuel.
2. Choisissez les sources de bannières (zéro, une ou plusieurs).
3. Ajouter des raccourcis de catégorie et/ou des blocs de contenu de catégorie à partir de la découverte de sources.
4. Ajouter des sections d'accueil, y compris des sections en ligne/différées et une sélection de sous-sections facultative.
5. Réorganisez les homes et les blocs à l'aide des commandes de déplacement vers le haut/bas accessibles ; le glisser-déposer n'est pas nécessaire.
6. Possibilité de remplacer le titre d'un bloc et de choisir l'inclusion d'un signet.
7. Prévisualisez le brouillon, enregistrez-le de manière atomique en tant qu'enregistrement de définition unique, ou annulez-le sans modification persistante.
8. Renommez, modifiez et supprimez les logements existants avec confirmation de suppression. Après avoir supprimé le logement actif, revenez à un itinéraire valide ; ne sélectionnez pas automatiquement un autre fournisseur.

L'éditeur ne découvre une source qu'après sa sélection par l'utilisateur. Afficher suffisamment d'informations sur la source pour distinguer les sections/catégories portant le même nom. Afficher les sélections enregistrées même si leur source est temporairement indisponible.

### 6.3 Répartition proposée des responsabilités Vue

Les nouveaux fichiers proposés sont regroupés par fonctionnalité dans les répertoires existants ; aucune réorganisation de la structure des fichiers existants n'est prévue.

| Composant/module | Responsabilité et contrat |
| --- | --- |
| `App.vue` (existant) | Composez une barre d'outils, une barre latérale, une interface de navigation, un arrière-plan et des notifications ; aucune logique d'édition/de résolution. |
| `components/navigation/AppSidebar.vue` | Propriétés : `homes`, `sources`, `active destination`, `expanded state`. Émet : `toggle`, `create/edit request`, `destination selection`. |
| `components/navigation/SidebarSourceGroup.vue` | Propriétés : entrées de source et état réduit. Émet : mise à jour du repli et sélection de la source. |
| `views/HomeView.vue` (existant) | Lire le contexte source à partir des propriétés de la route et composer le catalogue. |
| `views/CustomHomeView.vue` | Charger l'ID de la page d'accueil et composer les points d'entrée du catalogue/éditeur résolus. |
| `components/home/CustomHomeEditor.vue` | Formulaire de brouillon personnalisé uniquement. Propriétés : définition facultative et sources disponibles. Émet : enregistrement validé, annulation. |
| `components/home/CustomHomeContentPicker.vue` | Propriétés : découvertes et sélections de sources. Émet : ajouts/suppressions de références ; aucune modification persistante directe. |
| `components/home/CustomHomeBlockList.vue` | Propriétés : blocs brouillons ordonnés. Émet : déplacer, supprimer, modifier le titre. |
| `composables/home/useCustomHomeCatalog.ts` | Résolution de définition personnalisée, gardes de requêtes, statuts de blocage et nouvelles tentatives. |
| `services/customHomes.ts` | Limite CRUD/validation IndexedDB typée utilisant l'assistant actuel. |
| `types/customHome.ts` | Définition persistante partagée et contrats de résolution/référence. |
| `useHomeCatalog` / `HomeCatalog` existant | Réutiliser le rendu et la pagination du catalogue avec une entrée explicite d'identité et de composition du catalogue ; conserver le comportement habituel de la page d'accueil/catégorie intact. |

Conservez `<script setup lang="ts">`, les props désactivées/les événements activés, l'état `computed` dérivé et les actions de mutation explicites. Une fonction de composition pure a sa place dans un service ou un utilitaire plutôt que dans un composant avec état. La répartition exacte des composants peut être réduite si l'implémentation reste simple ; évitez de déplacer toute la logique fonctionnelle dans `HomeCatalog` ou `App.vue`.

### 6.4 Actions d'ajout en mode édition

**Exigences validées :** ajouter le contrôle visuel de favoris utilisé pour les sections aux bannières des services, jamais aux bannières des homes personnalisées. Ajouter aussi ce contrôle aux catégories. Ces nouvelles icônes sont visibles et accessibles au clavier uniquement en mode édition de l'interface.

La revue du code montre que `/Users/jdecker/Downloads/arachnea/front/public-app/src/components/home/HomeSectionActions.vue` utilise `mdi-pin` / `mdi-pin-outline` pour les sections, et que `/Users/jdecker/Downloads/arachnea/front/public-app/src/components/HomeCatalog.vue` conditionne ces actions à `showSectionEditingButtons`. Réutiliser ce visuel et cette condition, sans créer un mode édition indépendant. Distinguer l'ajout à une home de l'épinglage actuel et des signets médias (`mdi-bookmark`).

- Une action sur une bannière sélectionne la collection complète du service identifié par `sourceId`, pas l'élément temporaire affiché. Dans l'agrégat, conserver la provenance pour identifier le service. Aucun contrôle d'ajout de bannière dans le contexte d'une home personnalisée, même si ses bannières proviennent de services.
- Une action de catégorie transmet sa référence stable et son descripteur. Choisir explicitement Raccourci ou Section ; toutes les contributions de la catégorie agrégée sont conservées sans étape supplémentaire de sélection.
- Contrat révisé : dans une home personnalisée, l'épingle active d'un raccourci de catégorie demande son retrait avec confirmation, et non sa copie vers une autre home. L'ajout reste disponible depuis les services et l'éditeur ; les définitions restent indépendantes.
- Parcours révisé validé : une seule fenêtre avec home cible/création, nom si création, Raccourci/Section pour une catégorie et Annuler/Ajouter. Ajouter enregistre atomiquement sans ouvrir l'éditeur ni changer de route ; Annuler n'écrit rien. Ne pas supposer une destination personnalisée implicite.
- Donner des libellés localisés explicites (« Ajouter les bannières de ce service à une home », « Ajouter cette catégorie à une home »). Définir l'état actif relativement à la home cible, pas à un favori global. Empêcher le double ajout accidentel d'une collection de bannières.
- `HomeCategoryStrip.vue` rend actuellement les catégories comme des liens : prévoir un bouton distinct, sans bouton imbriqué dans le lien et sans déclencher la navigation. L'action du héros ne doit pas déclencher lecture ou changement de bannière. Hors édition, ne laisser aucun contrôle invisible focalisable.
- `HomeHeroBanner` et `HomeCategoryStrip` restent présentatifs : props de visibilité/contexte et événements typés d'ajout vers l'orchestrateur, aucune écriture IndexedDB directe. Réutiliser le picker et l'éditeur proposés.

## 7. Lots d'implémentation progressifs

### ✅ Lot 0 : approuver les contrats — terminé

Les décisions par défaut et les cinq points de clarification sont validés, y compris l'option B pour les catégories des homes personnalisées. L'utilisateur a ensuite autorisé uniquement les lots 1A et 1B.

### ✅ Lot 1A : filtrage de la source du backend — implémenté

Livré : désérialisation stricte de `sources`, sélection triée/dédupliquée, court-circuit vide, erreurs par source inconnue/non chargée/sans capacité et résultats partiels sans repli. Les réponses filtrées en erreur n'exposent pas d'ETag. Capacités `supported_queries` calculées depuis le registre chargé et jointes aux métadonnées par identifiant exact. Aucun changement YAML ni réactivation de source.

Modifiez la requête, le gestionnaire, la façade et les appelants/tests existants concernés. Préservez le comportement de la réponse et du cache. Mettez à jour les fichiers `/Users/jdecker/Downloads/arachnea/docs/specifications/arachnea-stream-fr.md` et `/Users/jdecker/Downloads/arachnea/docs/specifications/arachnea-stream-en.md` avec les entrées REST/Tauri, les identifiants de source, le comportement en cas de valeurs vides ou invalides et les contraintes d'activation. Mettez à jour le fichier README du crate s'il documente ce point de terminaison.

Acceptation : seul l’ensemble de sources explicitement sélectionné sera exécuté ; les entrées omises préservent le comportement hérité ; les entrées vides ne peuvent pas déclencher une extraction à grande échelle ; les erreurs ne provoquent pas de repli.

### ✅ Lot 1B : interface utilisateur à source unique — implémenté

Livré : route `/source/:sourceId`, sélecteur léger `HomeSourceSelector`, propagation jusqu'à `loadHomeCatalog`, titre/logo, rafraîchissement des métadonnées et actions de nouvelle tentative. Les préférences utilisent `source:<identifiant-encodé>:<clé-héritée>` avec repli de lecture des styles globaux ; les signets sont filtrés sans modifier leur stockage. Les mouvements de sections sont limités au sous-ensemble visible. Les gardes de requête protègent aussi la pagination et le démontage ; le catalogue est remonté au changement de source. `/` conserve son agrégation.

Validation : `cargo check -p arachnea-stream`, compilation des tests (`--no-run`), test existant des métadonnées enrichi de contrôles de contrat hors réseau, `npm run type-check`, `npm run build-only` et `npm run lint:css`. Vite avertit de bundles volumineux. Le formatage Rust ciblé passe ; `cargo fmt --all -- --check` signale des écarts dans des fichiers non modifiés. La suite ciblée des résolveurs présente 3 succès et 6 échecs d'initialisation (`application data directory identifier is not configured`), hors du chemin modifié. La matrice manuelle navigateur/Tauri, l'entrelacement réseau de sources réelles et les scénarios REST/304 restent à vérifier en exécution interactive ; aucune nouvelle infrastructure de test n'a été ajoutée.

Modifiez l'itinéraire/la vue, les options de chargement/la clé de requête, les propriétés du catalogue et le sélecteur de source. Réutilisez les métadonnées et vérifiez le mappage des capacités/ID. Ajoutez les chaînes localisées dans les fichiers de langue FR/EN existants. Vérifiez la portée des préférences et les signets avant de déclarer qu'un répertoire source est totalement isolé.

Acceptation : l’URL source directe et l’actualisation fonctionnent ; la sélection de A puis de B ne peut pas afficher les résultats tardifs de A ; les catégories/bannières différées/pagination conservent leur source.

### ✅ Lot 2A : fondements de l'identité et de la persistance — implémenté

Fondations livrées, sans branchement de l'éditeur ou du moteur de composition :

- `types/customHome.ts` : définition version 1, références, blocs distincts, découverte et diagnostics de lecture.
- `services/customHomeValidation.ts` : validation stricte à l'exécution, refus des versions inconnues et des champs non déclarés, doublons d'ID de bloc interdits mais références répétées permises ; descripteurs liés à leur source et contrôle défensif des champs de credentials.
- `services/customHomes.ts` : brouillon vide sans effet persistant, CRUD dans `custom_homes` (clé `id`, index `navigationOrder`), sauvegarde/suppression avec comparaison atomique de `updatedAt` ; confirmation uniquement à la fin de la transaction. Les profils existants restent intacts. Les enregistrements invalides restent stockés et sont signalés séparément dans la liste. Aucune migration vers une version inconnue ni réinitialisation automatique.
- `loadHomeSourceDiscovery(sources)` : aucune requête pour une liste vide ; découverte sélectionnée sans fusion, sans chargement différé ; `available` distingue absence de ligne et home vide. Les erreurs d'agrégation utilisent les notifications existantes.
- `services/homeContentIdentity.ts` et `services/customHomeReferences.ts` : identité exacte à portée source, par clé/id fournisseur, puis descripteur canonique hors pagination, puis titre exact de secours. Aucun index de tableau. Collisions signalées ; références refusées si ambiguës. Les identités sont réservées à la découverte sans fusion, pas aux catalogues agrégés. Sous-sections identifiées dans la portée du parent ; sections en ligne conservées.
- L'assistant IndexedDB ouvre la version courante sans `databases()` disponible, ferme sur `versionchange`, et permet de réessayer après un upgrade bloqué. Pas de migration destructive ni de nouveau store Pinia : le service constitue la frontière persistante, les brouillons resteront dans l'éditeur du lot 2C.

Audit statique des YAML locaux (sans accès fournisseur) : Arte expose des clés de catégorie mais ses rails d'accueil en ligne reposent actuellement sur le titre ; RTL Play expose des clés pour ses rails en ligne. Les rails RTBF, M6 et TV5 et plusieurs rails des médias locaux exposent des liens utilisables comme secours de descripteur. Des rails en ligne TF1, TeleMB, Canal Zoom et Télésambre restent dépendants du titre lorsque ni clé/id ni descripteur n'est présent. Les sous-sections sont également contrôlées pour les collisions dans leur parent. Une clé présente n'est pas une garantie de stabilité côté fournisseur ; les sources utilisant des ancres partagées nécessitent une vérification au cas par cas. Aucune modification YAML n'est incluse dans ce lot : les corrections ciblées éventuelles restent soumises à approbation ; les références fragiles sont identifiables par `basis: 'label'`, et un titre changé devra être réparé plutôt que remappé silencieusement.

Validation : vérifications TypeScript/build/CSS et contrôles ponctuels hors ligne des fonctions pures. La persistance réelle après rechargement, les quotas, les upgrades bloqués et les transactions concurrentes navigateur/Tauri restent à valider interactivement avec les lots 2B/2C ; aucun nouveau fichier de test ni infrastructure ajouté.

Complément YAML autorisé après le lot 2A : clés standardisées `key: rail:<provider-id>` ajoutées aux rails Arte, TF1, France TV et RMC BFM Play ; LN24 reçoit `rail:category-content` pour son unique rail de catégorie. Les ancres partagées couvrent les catalogues de catégorie sans dupliquer les mappings. Les identifiants JSON Arte/TF1/France TV ont été vérifiés sur les réponses réelles ; RMC réutilise `/id`, déjà utilisé par sa déduplication interne. Le contrat de fixture TF1 exige désormais `sections > key`. Les clés précédemment publiées ne sont pas renommées et les titres continuent de piloter la fusion héritée.

Limite confirmée dans les réponses HTML locales : les recommandations Canal Zoom/Télésambre et certains rails featured TeleMB n'exposent pas de clé de conteneur persistante. Deux chargements Télésambre ont retourné des UIDs de slider différents pour les mêmes rails ; ces UIDs, les positions, les premiers médias et les titres normalisés sont donc exclus. Ces rails restent explicitement fragiles (ou non référençables si aucun titre/descripteur) et suivis dans le TODO. Aucune prétendue stabilisation à partir d'une donnée éphémère n'a été ajoutée.

Validation du complément : parsing des 15 YAML locaux de sources légales et du contrat de fixture TF1 ; vérification du format des mappings ajoutés ; unicité des identifiants sur les réponses récupérées (24 rails Arte, 28 TF1, 11 France TV). Le test Rust existant `test_query_load_home`, exécuté avec `ARACHNEA_TEST_QUERY_SOURCE=arachnea-stream/legal-stream/tf1-fr.yaml`, réussit et vérifie `sections > key`. RMC et LN24 n'ont pas été validés en extraction réelle dans cette passe. `git diff --check` ne signale pas d'erreur.

Introduire des types de référence/définition partagés, un stockage additif, une validation à l'exécution, la gestion des versions de schéma et des identités à portée source. Auditer les clés YAML manquantes comme une modification ciblée distincte. Fournir une représentation de découverte de source sans fusion sans modifier la normalisation agrégée existante.

Acceptation : les définitions survivent au rechargement ; les anciens signets/préférences sont conservés ; les rails portant le même nom provenant de différentes sources ou blocs restent distinguables ; les versions de schéma inconnues sont signalées sans réinitialisation destructive.

### ✅ Lot 2B : intégration de la composition et du moteur de rendu — implémenté

Livraison : `services/customHomeComposition.ts` compose les contributions dans l'ordre des blocs, avec fusion optionnelle NFC par titre effectif, entrelacement et déduplication média existante. L'identité du groupe et ses préférences dérivent de la home et des identifiants des contributions, jamais du titre. Les parents à sous-sections restent distincts pour préserver les variantes temporelles ; une sous-section explicitement sélectionnée peut participer à la fusion comme rail autonome.

`useCustomHomeCatalog` valide les définitions avant les requêtes, découvre uniquement les sources référencées, résout les identités exactes et charge uniquement les catégories choisies. Les sections et bannières sont hydratées à la demande avec concurrence limitée à quatre. Les requêtes en cours sont partagées sans partager les curseurs ; les blocs répétés gardent leurs progressions indépendantes. Les pages de catégorie utilisent `get_category`, les rails liés utilisent `get_section`. Les erreurs partielles conservent les contributions réussies ; une nouvelle tentative de groupe en erreur cible ses contributions défaillantes. Les références absentes sont signalées, sans substitution par titre ; leur formulaire de réparation appartient au lot 2C.

Le mode `custom` de `HomeCatalog` réutilise le rendu, les styles, les signets optionnels et les arrière-plans existants, mais désactive l'épinglage/réordonnancement global pour préserver l'ordre déclaré. La route `/homes/:homeId` permet de consulter les définitions déjà persistées ; aucune interface de création ni barre latérale n'est ajoutée. Les lectures sont rafraîchies au focus/retour de visibilité, et les opérations asynchrones sont protégées par révision.

Validation : type-check, lint CSS et build Vite réussis (avertissement de bundles volumineux). Contrôles ponctuels hors ligne exécutés sans ajout de tests au dépôt : définition vide sans requête, fusion/entrelacement/déduplication, partage des requêtes répétées, panne partielle et retry, curseurs indépendants de section/catégorie et réponses obsolètes. Validation interactive navigateur/Tauri et pagination réelle des fournisseurs encore à effectuer.

Inclure la fusion par titre de la section 5.6 ; vérifier identités, ordre, sous-sections, préférences, erreurs partielles et pagination indépendante des contributions fusionnées.

Résolvez les bannières, les raccourcis de catégories, les sections sources et les blocs de contenu de catégories ; isolez l’état des préférences et de la pagination ; intégrez le catalogue résolu au moteur de rendu existant. Implémentez les états de blocs partiels et les nouvelles tentatives, le chargement différé uniquement de certains éléments et la protection contre les résultats obsolètes.

Acceptation : seul le contenu sélectionné est hydraté ; les balises rails en ligne fonctionnent sans liens ; les balises rails de contenu par catégorie paginent selon leurs descripteurs réels ; une source défaillante laisse les autres blocs utilisables.

### ✅ Lot 2C : éditeur — implémenté

Livré : gestionnaire accessible depuis les homes agrégée/source/custom ; brouillons détachés, renommage, suppression confirmée, réorganisation atomique des homes et déplacement des bannières/raccourcis/blocs. Sélecteur avec découverte explicite d’une source, variantes de sections et distinction raccourci/contenu intégré. Réparation explicite des blocs avec conservation ID/position/titre, remplacement des raccourcis, retrait des sources indisponibles. Prévisualisation sans contrôles persistants, enregistrement sur commit et annulation sans écriture. Conflits signalés sans remplacement du brouillon. Icônes d’ajout visibles uniquement en édition : bannières services (jamais custom), catégories dont catégories custom, sections sources ; destination et contributions choisies avant sauvegarde. Liste actualisée au focus/visibilité et après mutations locales. Barre latérale réservée au lot 2D.

Validation : type-check, lint CSS, build Vite et contrôles ponctuels hors ligne des brouillons, annulation, réparation, quota/conflit et sauvegarde. Validation interactive navigateur/Tauri, stockage réel et accessibilité des dialogues encore à effectuer ; aucune nouvelle infrastructure de tests.

Inclure le réglage de fusion et les actions d'ajout de la section 6.4. Acceptation additionnelle : icônes uniquement en mode édition, aucune sur les bannières custom, aucune navigation/lecture parasite, annulation sans écriture et erreurs de sauvegarde explicites.

Implémentation des fonctionnalités de création, de renommage, de modification et de suppression, ainsi que de l'ordre d'accès, de la sélection de la source, de la prévisualisation des brouillons, de l'enregistrement et de l'annulation, de la validation et de la réparation des références manquantes. Aucune saisie d'URL arbitraire ni nouvelle dépendance au glisser-déposer.

Acceptation : l’annulation n’a pas d’effets secondaires persistants ; l’enregistrement d’un brouillon met à jour la navigation ; la suppression de la définition active laisse une route valide ; les erreurs ne peuvent pas se faire passer pour une persistance réussie.

### ✅ Lot 2D : finition de la barre latérale et de l’intégration — implémenté

Livré : navigation homes personnalisées → Lives → sources repliables, alimentée par définitions locales et métadonnées/capacités sans préchargement fournisseur. Création/modification/gestion depuis la barre latérale ; sélecteur source et boutons de gestion dupliqués retirés des vues. Marque vers `/`, Lives retiré de la barre supérieure ; recherche/paramètres/retour conservés. Barre permanente au-delà de 1000 px avec largeur réservée (260 px, 220 px jusqu’à 1280 px), sous la hauteur mesurée de la barre supérieure ; écrans étroits : dialogue modal natif avec confinement du focus, Échap, clic extérieur et fermeture à la navigation, restitution du focus au déclencheur. Masquage en plein écran et fermeture du tiroir via événements standard/préfixés. Groupe de sources persisté dans localStorage avec diagnostic d’erreur, synchronisation storage/focus ; définitions actualisées au focus/visibilité/mutations locales. Candidats d’arrière-plan vidés au changement de route ; titres et restauration de défilement existants conservés.

Validation : type-check, lint CSS, build Vite et vérification du diff ; contrôle ponctuel hors ligne de l’état responsive/plein écran/fermeture/focus et compilation des composants. Aucun nouvel outil de test. Validation interactive navigateur/Tauri (focus réel, dialogue mobile, plein écran Safari, paramètres/notifications, onglets natifs indépendants et stockage inter-vues) reste à effectuer ; ces scénarios ne sont pas déclarés validés par les seuls contrôles de compilation.

Introduire l'ordre de groupe requis ; déplacer Lives hors de la barre d'outils ; conserver le comportement de la recherche, des paramètres et du retour. Valider la mise en page adaptative, l'accessibilité, le comportement de l'arrière-plan et de l'icône principale, le mode plein écran, les onglets natifs et l'actualisation persistante des onglets croisés.

Acceptation : homes personnalisées -> lives -> homes sources pliables ; le focus/la fermeture du tiroir mobile fonctionne ; les vues plein écran et détaillées ne sont pas obstruées.

Chaque lot met à jour `/Users/jdecker/Downloads/arachnea/docs/TODO.md`, la documentation README/spécifications correspondante et les entrées regroupées par module dans `/Users/jdecker/Downloads/arachnea/CHANGELOG.md`. Il est préférable d'enregistrer les étapes franchies plutôt que de considérer la fonctionnalité comme terminée après la première étape.

## 8. Plan de validation

Aucun nouveau test n'est autorisé par cette demande d'analyse. Mettez à jour les tests existants concernés lors de la mise en œuvre. Si une nouvelle couverture déterministe est souhaitée, obtenez d'abord une autorisation explicite et utilisez l'infrastructure de test existante.

### 8.1 Vérifications existantes à exécuter pendant l'implémentation

```sh
cd /Users/jdecker/Downloads/arachnea/server
cargo fmt --all -- --check
cargo check -p arachnea-stream

cd /Users/jdecker/Downloads/arachnea/front/public-app
npm run type-check
npm run build-only
npm run lint:css
```

Identifiez les tests Rust existants pertinents et déterministes avant de les exécuter. Ne considérez pas la suite de scrapers des fournisseurs en direct comme un point de contrôle hors ligne et n'incitez pas à contacter chaque fournisseur configuré uniquement pour valider une modification de filtre de requêtes. Documentez les erreurs existantes et évitez de refactoriser des crates non liées pour les corriger.

### 8.2 Matrice manuelle des contrats et de l'interface

| Domaine | Scénarios |
| --- | --- |
| Requête source | Omise, vide, une/deux sources, doublons, espaces, vide, type malformé, nulle, inconnue, désactivée, aucune capacité `load_home`. |
| Transport/cache | Requête REST sur tableau JSON, appel Tauri, requête répétée/304, changement d'ensembles de sources, ordre d'ensemble stable, erreurs de fournisseur partiel. |
| Accueil de la source | URL directe, actualisation, retour/suivant, barre latérale intégrée, barre latérale différée, sous-section, bannières différées, ouverture de catégorie, signets de source uniquement. |
| Contenu personnalisé | Sources de bannières uniques ou multiples, vignette de catégorie, barres de catégories intégrées, sections à sources croisées, barre répétée dans deux blocs, référence manquante, définition vide. |
| Pagination | Curseurs de blocs indépendants, sources mixtes, échec puis nouvelle tentative, changement d'itinéraire rapide pendant le chargement, aucune récupération différée non sélectionnée. |
| Fusion par titre | Activée/désactivée, casse/espaces/Unicode, titres remplacés/absents, accents/ponctuation distincts, sections de catégories, doublons intentionnels, ordre, sous-sections temporelles, référence manquante, curseurs et préférences indépendants. |
| Actions d'ajout | Mode édition activé/désactivé, bannières de services vs custom, provenance dans l'agrégat, catégories multisources, raccourci vs contenu, destination/création, double clic, annulation, échec d'écriture, clavier et absence de navigation/lecture parasite. |
| Persistance | Base de données nouvelle/existante, rechargement, échec de quota/écriture, mise à niveau bloquée, schéma non pris en charge, modification simultanée, suppression de l'accueil actif, onglets du navigateur/natifs. |
| Navigation | Ordre des groupes obligatoire, persistance du repliement de la source, noms/logos longs manquants, tiroir mobile, clavier/focus, route active. |
| Régression | Accueil agrégé, itinéraires existants par catégorie/détail/recherche/lives, styles épinglés, signets, arrière-plans, vidéo principale, lecteur plein écran, titres des documents de bureau. |

Vérifiez la sélection de la source à l'aide des diagnostics backend/traces réseau, et pas seulement des cartes visibles : masquer les entrées indésirables ne prouve pas que les fournisseurs n'ont pas été appelés.

## 9. Décisions validées et statut de livraison

Décisions confirmées lors de la revue du 12 octobre 2026 :

1. Une liste source omise signifie tout ; une liste vide explicite signifie rien ; une entrée malformée/nulle est rejetée.
2. Définitions IndexedDB locales initialement ; pas de partage de serveur ni de synchronisation entre appareils.
3. Les raccourcis de catégorie et les blocs de contenu de catégorie intégrés sont disponibles et explicitement distingués dans l'éditeur.
4. La sélection des bannières se fait par collection source, et non par élément vedette temporaire ; intercalez-les dans l'ordre source configuré.
5. Gardez l'agrégat hérité `/` accessible via la marque ; la barre latérale commence par les homes personnalisées, puis les lives, puis les sources.
6. Préférences de la source/de la page d'accueil personnalisée ; les signets de la source sont filtrés, les signets personnalisés sont optionnels.
7. Retenir la découverte des capacités depuis le registre dans l'étape 1 plutôt que l'affichage des sources non prises en charge.
8. Inclure une option de fusion par titre activable par home, désactivée initialement. Comparer les titres effectifs selon la section 5.6, inclure les rails issus des contenus de catégories et les doublons intentionnels, puis entrelacer les contributions en tour de rôle comme actuellement. Conserver les identités persistantes et la réparation des références.
9. Ajouter aux bannières des services et aux catégories le contrôle visuel utilisé pour les sections, uniquement en mode édition. Exclure les bannières des homes personnalisées.
10. Contrat révisé validé : l'ajout rapide utilise une seule fenêtre avec destination existante ou création (nom), choix Raccourci/Section pour une catégorie et boutons Annuler/Ajouter. Ajouter résout toutes les contributions du contenu cliqué et enregistre atomiquement sans éditeur ni navigation. Les options avancées restent accessibles depuis la home. L'annulation n'écrit rien ; les erreurs gardent la fenêtre ouverte.
11. Option B retenue : autoriser aussi la copie de références de catégories depuis les homes personnalisées, uniquement en mode édition, vers une autre home via le parcours d'ajout validé. Les définitions restent indépendantes ; l'exclusion des bannières personnalisées demeure inchangée.

**Lots 1A et 1B autorisés et implémentés ; lots 2 en attente.** Les clarifications demandées sont closes. Vérifier les scénarios manuels de l'étape 1 avant la composition personnalisée et la navigation. Aucun nouveau test n'est demandé ; les tests existants concernés sont adaptés.

## 10. Clarifications closes : copie des catégories depuis les homes personnalisées

Les cinq points de la revue sont validés et intégrés aux sections 5.6, 6.4 et 9. Aucun point de décision identifié dans cette revue ne reste ouvert ; les lots 2 demeurent en attente.

**Périmètre validé :** une vignette/raccourci de catégorie déjà ajouté à une home personnalisée, et non les cartes médias des sections issues d'un bloc de contenu de catégorie. Exemple : la home « Mes documentaires » contient un raccourci vers la catégorie « Sciences » d'Arte.

**Contrat révisé après livraison :** en mode édition, le raccourci conserve une épingle active, désormais utilisée pour retirer cette occurrence de sa home après confirmation. Elle n'ouvre plus le parcours de copie (ancienne option B). Les références et définitions des autres homes restent inchangées. Les ajouts se font depuis les services ou l'éditeur.

### Ajustements d'interface validés après le lot 2D

- La réorganisation des groupes n'est configurable que dans les homes personnalisées, via l'éditeur et les commandes haut/bas des sections en mode édition. Les sources et `/` conservent l'ordre du catalogue ; les anciennes préférences d'épinglage restent stockées mais ne réordonnent plus ces pages.
- Les actions d'ajout bannières/catégories/sections réutilisent un petit bouton rond de 36 px, icône 18 px et fond semi-transparent, uniquement en mode édition. L'épingle de catégorie est superposée à l'intérieur visuel de sa carte, sans imbrication de contrôles interactifs ni navigation parasite.
- Le titre « Homes personnalisées » porte une icône Home et une icône d'édition ; les homes individuelles n'ont pas d'icône Home. La ligne séparée « Gérer les homes personnalisées » est supprimée.
- Ajout en une fenêtre : destination/création, Raccourci/Section pour les catégories, Annuler/Ajouter. Les contributions multisources sont toutes conservées ; une résolution ambiguë/manquante refuse l'ajout complet plutôt que de choisir silencieusement une autre référence.

Les bannières des homes personnalisées n'affichent jamais l'action d'ajout ; les nouvelles icônes sont absentes hors mode édition. Dans les homes personnalisées, les épingles actives des sections et catégories retirent leurs références après confirmation. Une section fusionnée impose le choix d'un seul bloc propriétaire ; son déplacement ne déplace pas implicitement tout le groupe. Le retrait d'un rail de contenu de catégorie retire son bloc complet et toutes ses sections, avec avertissement explicite. Les mouvements et retraits enregistrent atomiquement la révision affichée ; les conflits/erreurs gardent le dialogue ouvert sans mutation optimiste. Les signets et prévisualisations n'exposent pas ces mutations.

### Bilan de la revue

L'approche en deux étapes et la réutilisation du filtre backend, du rendu et du stockage existants restent cohérentes. Les risques principaux sont la séparation identité/fusion/préférences, les curseurs indépendants des rails fusionnés et la distinction épinglage/ajout. La revue corrige la contradiction sur les identifiants vides, la syntaxe de l'esquisse TypeScript et les commandes Cargo, et ajoute les critères d'acceptation des nouvelles interactions. L'audit des clés YAML reste ciblé ; présenter ses modifications nécessaires avant implementation, sans refonte globale.
