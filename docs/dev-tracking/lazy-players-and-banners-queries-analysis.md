# Analyse : chargement hybride des players et des bannières

## Objectif

Réduire le temps de réponse de `load_home`, `get_entry` et `get_season` en remplaçant les sous-requêtes coûteuses par des requêtes explicites déclenchées par le front lorsque les données sont nécessaires.

Le contrat reste hybride et aligne les collections différées sur le format déjà employé par une section :

- lorsqu’une query possède déjà les données nécessaires, elle les place dans `entries` ;
- lorsqu’un appel supplémentaire est requis, le même objet porte une `source` et un `link` ;
- le front appelle ensuite la query adaptée avec ce descripteur.

Ce mécanisme est celui déjà utilisé par les sections : `load_home` renvoie une source de section avec une `source` et un `link`, puis le front appelle `get_section` à la demande.

Le `link` est une donnée transmise au backend, utilisée comme paramètre de la query YAML de la `source` sélectionnée. Il ne s’agit pas d’une URL appelée directement par le navigateur.

Le contrat cible commun est le suivant :

```text
Collection<T>
├── entries: T[]             données déjà chargées ; tableau vide si différé
├── source: string            service YAML qui doit charger la collection
└── link: string?             présent si la collection doit être chargée
```

`sections` reste un tableau de collections parce qu’il existe plusieurs rails. `banners` et `players` deviennent chacun une collection unique, au lieu d’être directement des tableaux. C’est un changement de contrat volontairement structurant.

## Nouvelles queries proposées

### `get_banners`

But : charger les bannières différées d’une seule source.

Entrée backend proposée :

```text
source: nom du service YAML
link: lien interne de la ressource de bannières
```

Sortie : une liste normalisée de bannières. Chaque bannière peut contenir directement ses métadonnées et ses visuels. Si l’obtention de sa vidéo exige une autre requête, elle expose alors une collection `players` pour `get_players` au lieu d’une URL vidéo finale.

Contrat hybride pour `load_home` :

```text
load_home
└── banners
    ├── entries      bannières immédiatement disponibles
    ├── source        source à utiliser pour le chargement différé
    └── link          lien à utiliser par get_banners si nécessaire
```

Le front lance `get_banners` pour chaque collection `banners` portant un `link`, en parallèle. Les bannières sont ajoutées à la fin de la liste globale au fur et à mesure de l’arrivée des réponses : la première bannière disponible est donc affichable immédiatement. La fusion reste idempotente par `source + link` pour éviter les doublons lors d’un rafraîchissement.

### `get_players`

But : récupérer les descripteurs de lecture d’un média ou d’une bannière lorsque ceux-ci ne sont pas déjà présents.

Entrée backend proposée :

```text
source: nom du service YAML
link: lien interne permettant d’obtenir le ou les players
```

Sortie : des objets `players` normalisés (`resolver`, `target`, lien d’intégration, etc.). La résolution finale du flux reste confiée à `get_stream`, déjà existant.

Contrat hybride pour une entrée, un épisode ou une bannière :

```text
objet
└── players
    ├── entries      players déjà disponibles
    ├── source        source à utiliser pour le chargement différé
    └── link          lien à utiliser par get_players si nécessaire
```

Le front appelle `get_players` uniquement au clic sur Lecture. Aucun préchargement au survol ou au focus n’est prévu.

## Réutilisation des queries existantes

| Service et query actuelle | Sous-requête actuelle | Remplacement proposé | Query réutilisée ou créée |
| --- | --- | --- | --- |
| CoFlix `get_entry` | `players → target → page de l’hébergeur` | `get_entry` renvoie `players: { entries: [], source, link }`; le clic lance le chargement. | Nouvelle `get_players`, puis `get_stream` existante. |
| CoFlix `get_season` | `episodes → players → page de l’hébergeur` pour chaque épisode | Chaque épisode renvoie `players: { entries: [], source, link }`; aucun player n’est résolu tant qu’un épisode n’est pas choisi. | Nouvelle `get_players`, puis `get_stream` existante. |
| M6 Play `load_home` | Chargement des bannières depuis l’API programmes | `load_home` renvoie `banners: { entries: [], source, link }`. | Nouvelle `get_banners`. |
| RTBF `load_home` et `get_category` | Catégories, bannières, puis chaîne d’autorisation HLS pour les promobox | Les métadonnées sont chargées dans `banners.entries`; une bannière vidéo porte `players: { entries: [], source, link }`. | Nouvelle `get_banners`; nouvelle `get_players` pour les promobox; `get_stream` existante pour la lecture. |
| RTBF `get_entry` | Chargement des saisons puis des épisodes (pages 1 et 2) | La chaîne actuelle est conservée dans ce périmètre. | Aucune migration vers une nouvelle query : les sous-requêtes existantes sont maintenues. |
| TF1 `load_home` | Deux sous-requêtes lisent la même ressource de couvertures | Une seule `get_banners` par source lit les couvertures de programmes et de vidéos dans la même réponse distante. | Nouvelle `get_banners`. |
| TF1 `get_entry` | Vidéo de liste éditoriale et recherche de secours | `get_entry` fournit `players: { entries: [], source, link }`; `get_players` applique le chemin éditorial et seulement ensuite le fallback de recherche. | Nouvelle `get_players`, puis `get_stream` existante. |

### Cas RTBF : `get_entry` hors périmètre

Les deux nouvelles queries suffisent pour différer les bannières et les players, mais `RTBF get_entry` conserve ses sous-requêtes actuelles dans ce périmètre.

La chaîne actuelle est :

```text
get_entry → program.path → saisons (contentPath) → épisodes (contentPath, pages 1 et 2)
```

`get_season` reste réutilisable à l’avenir pour charger une liste d’épisodes à partir d’un lien de saison. Toutefois, aucune migration de cette chaîne n’est planifiée ici : ni `get_entry_seasons`, ni le remplacement des sous-requêtes existantes ne font partie de ce changement.

## Impact sur les performances

| Élément | Effet attendu |
| --- | --- |
| Temps de réponse de `load_home` | Réduction : les appels de bannières et, surtout, les autorisations vidéo RTBF ne bloquent plus la réponse initiale. |
| Temps de réponse de `get_entry` / `get_season` | Réduction importante pour CoFlix : les appels par épisode ne sont plus exécutés en avance. Réduction pour TF1 : les recherches de vidéo sont différées. |
| Temps au clic sur Lecture | Augmentation d’un appel frontend → backend vers `get_players`, plus les appels distants déjà nécessaires. Ce coût est volontairement déplacé au moment où l’utilisateur a choisi un contenu. |
| Charge backend et appels fournisseurs | Réduction lorsque l’utilisateur ne consulte pas toutes les bannières ou tous les épisodes. Les flux et autorisations inutilisés ne sont plus demandés. |
| Fraîcheur des flux | Amélioration : les URLs HLS et jetons courts sont obtenus au plus près de la lecture. |

Pour `get_banners`, les appels doivent être parallèles mais limités (par exemple quatre requêtes simultanées). Une requête en échec doit seulement laisser la source concernée vide ou en erreur, sans empêcher l’affichage des autres sources.

## Adaptations backend et frontend

Les nouvelles queries ne peuvent pas être ajoutées uniquement aux YAML : les commandes API sont enregistrées explicitement côté backend.

Backend :

- ajouter les requêtes `get_banners` et `get_players` dans `StreamScraper` ;
- enregistrer les deux commandes dans le contrôleur, comme `get_section` ;
- accepter `source` et `link`, et exécuter uniquement la source demandée ;
- ajouter les queries correspondantes dans les YAML concernés.

Frontend :

- remplacer les tableaux `HomeBanner[]` et `EntryPlayer[]` exposés par le contrat brut par des collections `{ entries, source, link }` ;
- adapter les normaliseurs afin de conserver les `entries` déjà incluses et d’extraire la `source` et le `link` lorsqu’ils existent ;
- appeler `get_banners` en parallèle avec déduplication par `source + link` ;
- appeler `get_players` au clic sur Lecture, avec déduplication des requêtes simultanées pour un même objet ;
- ajouter les `entries` de bannières à la fin de la liste globale dans l’ordre d’arrivée des réponses, de manière idempotente afin d’éviter les doublons lors d’un rafraîchissement.

Les composants ne devraient pas avoir à connaître le format brut du YAML. Les normaliseurs peuvent continuer à exposer une liste de bannières et une liste de players aux composants, tout en conservant le descripteur de chargement dans l’état de données du catalogue ou de l’entrée.

## Plan d’implémentation

1. ✅ **Valider le contrat structurant.** Définir les schémas bruts de `banners` et `players` avec `entries`, `source` et `link`, ainsi que les règles de présence de ces champs. Migrer directement tous les producteurs et consommateurs vers ce format, sans couche de compatibilité pour l’ancien format.
   - `Collection<T>` ajoutée dans `front/src/types/media.ts:147`
   - `HomeCatalogData.banners` passe de `HomeBanner[]` à `Collection<HomeBanner>` (`front/src/types/home.ts:97`)
   - `EntryDetails.players` et `EntryPlayableItem.players` passent de `EntryPlayer[]` à `Collection<EntryPlayer>` (`front/src/types/entry.ts:10,194`)
   - Normaliseurs mis à jour : `normalizeHomeCatalog`, `mergeNormalizedCatalogs`, `normalizeEntryDetails`, `normalizeEntryEpisode`
   - Consommateurs mis à jour : `useHomeCatalog`, `entryVideoPlayer`, `entryEpisodeSelection`, `entryDetailsMediaItems`, `HomeCatalog.vue`, `ProgramEntryDetails.vue`
   - Aucune couche de compatibilité – l’ancien format n’est plus accepté.
2. ✅ **Créer les commandes backend.** Ajouter `get_banners` et `get_players` à `StreamScraper`, leurs entrées de requête et leur enregistrement dans le contrôleur. Reprendre la sélection d’une seule source et le passage de `source`/`link` de `get_section`.
   - `GetBannersRequest` et `GetPlayersRequest` ajoutés dans `server/crates/arachnea-stream/src/stream_scraper.rs:127-138`
   - `StreamScraper::get_banners` et `StreamScraper::get_players` méthodes publiques (lignes 636-716), suivant le même patron que `get_section` : exécutent la query YAML de la source sélectionnée avec `query_url` et `link`, fusionnent les lignes via `merge_first`/`keep_first_values`
   - Commandes enregistrées dans `register_service` sous les noms `"get_banners"` et `"get_players"`
   - Fonctions frontend `getBanners` et `getPlayers` ajoutées dans `front/src/services/rustify.ts`
   - Compilation backend (`cargo check -p arachnea-stream`) et frontend (`vue-tsc --noEmit`) validées
3. **Créer l’infrastructure frontend.** Ajouter les types de collections différées, les normaliseurs et les chargeurs dédiés. Implémenter la déduplication des appels en cours, la limitation de concurrence pour les bannières, les états de chargement/erreur par source et l’ajout des bannières dans l’ordre d’arrivée.
4. **Migrer TF1 `load_home`.** Remplacer les deux sous-requêtes de couvertures par une seule `get_banners` qui lit les deux types depuis une réponse distante. C’est le premier cas de validation du contrat et un gain sans changement de comportement fonctionnel.
5. **Migrer CoFlix.** Remplacer les sous-requêtes de players de `get_entry` et `get_season` par des collections `players` différées. Le front appelle `get_players`, puis la commande existante `get_stream` au moment de la lecture.
6. **Migrer RTBF et M6 Play.** Déplacer les bannières de RTBF et de M6 Play vers `get_banners`; déplacer l’autorisation HLS de promobox RTBF vers `get_players`. Ne pas modifier les sous-requêtes de `RTBF get_entry` dans ce chantier.
7. **Valider et mesurer.** Mettre à jour les tests existants affectés, vérifier la conformité des réponses YAML au nouveau contrat et comparer les mesures listées ci-dessous avant de supprimer les sous-requêtes migrées.

## Mesures à comparer avant et après

- durée backend de `load_home`, `get_entry`, `get_season`, `get_banners` et `get_players` par source ;
- nombre d’appels HTTP sortants par affichage de page et par lecture effective ;
- temps entre le clic sur Lecture et le démarrage du flux ;
- taux d’échec des chargements différés ;
- taux de réutilisation du cache des bannières et taux de déduplication des appels en cours.
