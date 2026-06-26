# Analyse : sorties typées pour `arachnea-scrapyfy`

## Objectif

Le moteur `arachnea-scrapyfy` expose aujourd'hui la plupart des valeurs extraites sous forme de `string[]`. Le front doit donc deviner si un champ représente une chaîne, un nombre, un booléen, une liste ou un objet, puis reconvertir ces valeurs dans `front/src/services/rustify.ts`.

L'objectif demandé est de déplacer ce contrat dans les YAML : chaque champ doit pouvoir déclarer le format de sortie attendu, par exemple `number`, `boolean`, `string`, `number[]`, `boolean[]`, `string[]`, `object`, `object[]`. Le backend doit ensuite renvoyer directement une réponse JSON typée, et le front ne devrait plus faire de conversion de bas niveau pour corriger le format historique.

Cette note analyse le travail à prévoir, les zones sensibles et une trajectoire de migration.

## Etat actuel

### Rust backend

Le coeur du contrat se trouve dans `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_data_node.rs`.

`ScraperDataNode` stocke les feuilles sous forme de `Vec<String>` :

- `values: Vec<String>` pour les valeurs scalaires répétées;
- `children: HashMap<String, ScraperDataNode>` pour les objets imbriqués;
- `items: Vec<ScraperDataNode>` pour les listes explicites d'objets.

La sérialisation choisit ensuite une forme compacte :

- `null` si le noeud est vide;
- `Vec<String>` pour une feuille;
- objet pour des enfants;
- tableau d'objets pour `items`;
- tableau plat d'objets quand plusieurs enfants feuilles ont des listes alignées.

Les extracteurs alimentent ce modèle uniquement en chaînes :

- `HtmlScraperEntry::apply_to` applique des actions et pousse des `String`.
- `JsonScraperEntry::apply_to` convertit toute valeur JSON via `json_value_to_strings`; les nombres et booléens deviennent donc des chaînes, les objets deviennent une chaîne JSON.
- `StaticScraperEntryRaw::apply_to` convertit aussi les valeurs YAML en chaînes.
- `ScraperAction::apply` prend et retourne toujours `Vec<String>`.

Les post-processors sont également string-based :

- `derive_pagination` lit des entiers et booléens depuis des chaînes, puis réécrit `have_more` sous forme de chaîne `"true"` ou `"false"`;
- `compute_items_field` calcule un nombre, mais le stocke en chaîne;
- `append_static_items`, `extract_regex_items`, `fetch_regex_items_from_items` et `pivot_items_by_index` produisent aussi des `ScraperDataNode::from_values(vec![...])`.

`server/crates/arachnea-stream/src/stream_scraper.rs` expose ces noeuds directement dans les endpoints `search`, `load_home`, `get_entry`, `get_season`, `get_category`, `get_section`, `list_lives`, `get_live` et `get_service`. Les facades ne font pas de shaping spécifique, ce qui est cohérent avec les consignes du dépôt.

### YAML

Les YAML ne portent pas de type de sortie sur les entries. Les champs sont déclarés par `name`, `pointer` ou `selector`, `select`, `actions`, et parfois `entries` pour produire des groupes.

Une cartographie rapide des YAML existants montre les familles de champs les plus touchées :

| Famille | Exemples | Type cible probable |
|---|---|---|
| Pagination | `current_page`, `total_pages`, `page_size` | `number` |
| Booléens | `have_more`, `infer_have_more_from_full_page` | `boolean` |
| Mesures | `rating`, `year`, `count-season`, `count-episodes` | `number` |
| Durées | `duration` | `string` |
| Listes de libellés | `theme`, `genre`, `casting`, `director`, `media-type` | `string[]` |
| Images | `img/poster > link`, `img/landscape > link`, `img/logo > link` | objet parent, `link: string` |
| Groupes | `entries`, `sections`, `banners`, `players`, `season`, `episodes` | `object[]` |
| Resolver/player | `resolver`, `resolver > stream`, `storyboard` | `object`, avec dimensions en `number` |
| Metadata service | `description`, `search_themes` | potentiellement `object` / `object[]` |

Les champs les plus fréquents restent textuels (`title`, `description`, `link`, `web-link`, `source`, `label`), mais plusieurs champs actuellement parsés côté front sont clairement numériques ou booléens.

### Frontend

Le front a déjà des types applicatifs propres dans `front/src/types/*`, par exemple :

- `MediaItem.rating: number | null`;
- `EntryEpisodePage.currentPage: number`;
- `EntryEpisodePage.haveMore: boolean`;
- `EntryPlayerStoryboard.width/height/columns/interval: number`.

La dette est concentrée dans `front/src/services/rustify.ts`, qui contient des helpers de compatibilité :

- `readStringList` aplati des tableaux de chaînes et des noeuds `{ "_": [...] }`;
- `firstNonEmptyString` récupère la première chaîne non vide;
- `firstNumber` parse une chaîne en nombre;
- `readBoolean` et `readOptionalBoolean` convertissent `"true"` / `"false"`;
- `readStringMap` transforme un objet de noeuds scraper en `Record<string, string>`;
- `readRecordList`, `readBannerList`, `expandIndexedBannerRecords` tolèrent plusieurs formes historiques.

Le front normalise ensuite vers ses modèles UI. Cette normalisation restera utile, mais elle ne devrait plus corriger le typage de base des réponses.

## Problème à résoudre

Le changement n'est pas seulement "parser un nombre". Il faut ajouter un contrat de sortie stable au moteur :

1. Les YAML doivent déclarer le format attendu pour chaque champ.
2. Le runtime doit transporter ce format jusqu'au `ScraperDataNode`.
3. La sérialisation doit convertir les valeurs dans le type JSON attendu.
4. Les post-processors doivent produire des champs typés ou accepter une déclaration de type.
5. Les YAML existants doivent être migrés vers ce nouveau contrat.
6. Le front doit consommer les nouvelles formes typées directement, avec `null` pour les champs déclarés mais non trouvés, pas avec une reconversion défensive des anciens `string[]`.

La contrainte importante du dépôt est que le shaping doit rester générique dans les YAML, actions ou post-processors, pas dans `stream_scraper.rs`.

## Approches possibles

### Approche A : conversion typée au bord de la sérialisation

On garde les actions et les extracteurs internes en `Vec<String>`, mais on ajoute une métadonnée de type sur les noeuds. La conversion vers `number`, `boolean`, `object`, etc. se fait quand `ScraperDataNode` est sérialisé.

Avantages :

- changement limité;
- pas besoin de réécrire toutes les actions;
- compatible avec les post-processors actuels qui travaillent déjà sur des chaînes;
- permet de garder les actions internes string-based au début tout en produisant une réponse JSON typée.

Inconvénients :

- les valeurs JSON natives sont déjà perdues par `json_value_to_strings`;
- `object` doit être produit par des sous-groupes `entries`, donc un objet source JSON ne peut pas être exposé tel quel sans mécanisme explicite d'import vers `ScraperDataNode`;
- les erreurs de cast apparaissent tard, au moment de produire la réponse;
- les actions restent string-based, donc ce n'est pas une vraie pile typée interne.

C'est l'approche recommandée pour une première migration, car elle répond au besoin du front avec le moins de risque.

### Approche B : rendre `ScraperDataNode` et les actions vraiment typés

On remplace `Vec<String>` par un type comme `Vec<ScraperValue>`, proche de `serde_json::Value`, puis chaque action convertit explicitement ses entrées et sorties.

Avantages :

- modèle plus propre à long terme;
- les JSON source conservent leurs nombres, booléens et objets;
- `object` / `object[]` deviennent naturels.

Inconvénients :

- changement beaucoup plus large;
- toutes les actions doivent être auditées;
- les post-processors doivent être réécrits ou adaptés;
- risque plus élevé sur les sources existantes.

Cette approche peut devenir intéressante plus tard, mais elle est trop large pour le premier palier.

### Approche C : ajouter seulement un schéma côté front

On documente les types attendus côté front et on continue à reconvertir dans `rustify.ts`.

Avantages :

- faible impact backend.

Inconvénients :

- ne répond pas au besoin principal;
- laisse le contrat réel implicite;
- maintient la dette côté front.

Cette approche n'est pas suffisante.

## Recommandation de conception

Ajouter une notion de type de sortie déclarée dans les YAML avec le champ `type`.

Raison :

- `format` existe déjà dans plusieurs actions (`regex_find_all`, `get_date`) avec un autre sens;
- `type` est le nom préféré pour le contrat YAML, même s'il existe déjà dans les actions et post-processors;
- le contexte YAML désambiguïse les usages : `scraper_type` reste le type de query, `actions[].type` reste le type d'action, `post_process[].type` reste le type de post-processor, et `entries[].type` devient le type JSON produit par l'entry.

Exemple :

```yaml
- name: current_page
  type: number
  actions:
    - type: format_text
      argument: "{page}"

- name: have_more
  type: boolean
  pointer: /hasNext

- name: theme
  type: string[]
  pointer: /categories/*/label
  select: all

- name: img/poster > link
  type: string
  pointer: /image/url
  select: first

- name: players
  type: object[]
  entries:
    - name: embed-link
      type: string
      pointer: /embedUrl
    - name: storyboard
      type: object
      entries:
        - name: width
          type: number
          pointer: /storyboard/width
        - name: height
          type: number
          pointer: /storyboard/height
```

Sémantique proposée :

- `type` est obligatoire pour chaque entry qui produit un champ de sortie. Il n'est pas nécessaire pour les entries purement internes ou contextuelles qui ne sont jamais sérialisées dans la réponse finale.
- `select` continue de contrôler combien de valeurs sont extraites.
- `type` contrôle la forme JSON finale.
- Si une entry est déclarée dans le YAML mais ne trouve aucune valeur, le champ doit être sérialisé à `null`. Cela rend les YAML en cours de création plus faciles à déboguer qu'un champ silencieusement absent.
- Pour un type scalaire (`string`, `number`, `boolean`), la sérialisation attend une seule valeur. Si plusieurs valeurs sont produites, c'est une erreur de configuration sauf si l'entry utilise explicitement `select: first` ou une action qui agrège les valeurs.
- Pour un type tableau (`string[]`, `number[]`, `boolean[]`, `object[]`), toutes les valeurs valides sont conservées.
- `object` et `object[]` s'appliquent au noeud `ScraperDataNode` produit par l'entry. Ils ne doivent pas être compris comme un cast générique d'une valeur JSON brute.
  - par défaut, un groupe objet est sérialisé comme un tableau d'objets `[{ "key": value }]`;
  - avec `select: first`, ce même groupe est sérialisé comme un objet simple `{ "key": value }`.
- `object[]` reste accepté comme alias explicite pour documenter qu'un groupe doit sortir en tableau d'objets. Il ne doit pas être combiné avec `select: first`, car ce serait contradictoire avec la forme objet simple.
- Pour un groupe YAML avec `entries`, la cardinalité objet dépend donc de `select`: `select: first` produit un objet, tandis que `select: all` ou l'absence de `select: first` produit une liste d'objets.
- Pour une entry JSON qui pointe vers un objet source brut, le comportement recommandé est de créer un groupe YAML avec `entries` afin de matérialiser cet objet sous forme de `ScraperDataNode`. Le `type: object` reste alors porté par le noeud scraper, pas par la valeur `serde_json::Value` sélectionnée.
- Un cast impossible ou un `type` manquant doit faire échouer le chargement ou l'exécution de la query avec le format d'erreur actuel, mais avec un message détaillé : nom de query, chemin YAML/entry, champ de sortie, type attendu et valeur reçue quand elle peut être affichée sans bruit. Une valeur présente avec le mauvais type ne doit pas être silencieusement gardée en chaîne.

## Changements Rust à prévoir

### 1. Ajouter un type de contrat de sortie

Créer un enum générique dans `arachnea-scrapyfy`, par exemple :

```rust
pub enum ScraperOutputType {
    String,
    Number,
    Boolean,
    Object,
    StringArray,
    NumberArray,
    BooleanArray,
    ObjectArray,
}
```

Il doit être désérialisable depuis les libellés YAML `string`, `number`, `boolean`, `object`, `string[]`, etc.

Le champ Rust peut rester nommé `output_type` pour éviter les confusions internes, tout en étant exposé en YAML avec `#[serde(rename = "type")]`.

### 2. Porter le type dans `ScraperDataNode`

`ScraperDataNode` doit probablement gagner un champ optionnel :

```rust
pub output_type: Option<ScraperOutputType>
```

Ce type doit vivre sur le noeud lui-même, pas seulement sur les feuilles. C'est indispensable pour que `object` et `object[]` puissent contrôler la sérialisation des sous-groupes (`children` vs `items`) au lieu de laisser cette décision aux heuristiques actuelles.

Les méthodes d'insertion doivent pouvoir poser ce type :

- soit via `push_value_typed(path, value, output_type)`;
- soit via `set_output_type(path, output_type)` appelée par les entries avant insertion.

Points sensibles :

- `merge` doit conserver le type si un seul côté le connaît;
- `merge` doit refuser ou signaler les conflits si deux entries écrivent le même chemin avec des types incompatibles;
- `copy_field`, `set_node`, `prepare_copy_fields` doivent copier le type avec le noeud;
- `keep_first_values` ne doit pas supprimer une liste qui a volontairement un type tableau (`string[]`, `number[]`, `boolean[]`, `object[]`).

### 3. Modifier la sérialisation

`ScraperDataNodeRaw` est actuellement un intermédiaire de sérialisation très lié au contrat historique :

- `Values(Vec<String>)` impose des feuilles sous forme de tableaux de chaînes;
- `FlatArray(Vec<HashMap<String, String>>)` impose des objets dont toutes les propriétés sont des chaînes;
- l'ordre des variantes de l'enum `untagged` encode des heuristiques implicites;
- le choix objet vs tableau d'objets dépend de `items` et de `array_len()`, pas d'un contrat YAML explicite.

Avec le nouveau champ `type`, il n'est pas évident qu'il faille encore utiliser `ScraperDataNodeRaw` pour sérialiser. Une approche plus claire serait d'ajouter un renderer explicite :

```rust
impl ScraperDataNode {
    fn to_json_value(&self) -> serde_json::Value {
        // inspecte self.output_type, values, children et items
    }
}

impl Serialize for ScraperDataNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json_value().serialize(serializer)
    }
}
```

Cette approche laisse `ScraperDataNode` décider de la forme JSON à partir du `type` déclaré. `ScraperDataNodeRaw` pourrait alors être supprimé du chemin de sérialisation, ou conservé uniquement pour la désérialisation de tests/fixtures s'il reste réellement utile.

Si on décide malgré tout de garder `ScraperDataNodeRaw`, il devra être profondément réécrit :

- remplacer `Vec<String>` par des valeurs typées;
- remplacer `FlatArray(Vec<HashMap<String, String>>)` par un tableau d'objets typés;
- ajouter des variantes scalaires `number`, `boolean`, `string`, `object`;
- faire dépendre le choix `Object` vs `Array` du `type` déclaré, pas uniquement de la structure interne.

A ce stade, un `ScraperDataNodeRaw` réécrit ressemblerait beaucoup à `serde_json::Value`. La recommandation est donc de ne plus l'utiliser comme étape centrale de sérialisation et de passer par un renderer `ScraperDataNode -> serde_json::Value`.

Cas à traiter :

- feuille `string` -> `"foo"`;
- feuille `string[]` -> `["foo", "bar"]`;
- feuille `number` -> `42` ou `4.2`;
- feuille `number[]` -> `[1, 2]`;
- feuille `boolean` -> `true`;
- groupe objet avec `select: first` -> les enfants du noeud sérialisés comme `{ key: value }`;
- groupe objet sans `select: first` ou avec `type: object[]` -> les `items` du noeud, ou les enfants alignés, sérialisés comme `[{ key: value }]`;
- flat array aligné -> chaque propriété doit utiliser le type scalaire de son enfant.

Le cas des flat arrays devra être redéfini autour de `type: object[]`. C'est important pour les bannières et les structures alignées : elles ne doivent plus dépendre d'une heuristique de sérialisation implicite.

### 4. Etendre les entries YAML

Ajouter le champ YAML `type` aux raw structs, stocké côté Rust comme `output_type` ou équivalent :

- `HtmlScraperEntryRaw`;
- `JsonScraperEntryRaw`;
- `StaticScraperEntryRaw`;
- `ScraperRegexItemEntry` pour les post-processors regex.

Les `apply_to` correspondants doivent transmettre le type au noeud cible.

Pour `JsonScraperEntry`, il y a un point particulier : aujourd'hui `json_value_to_strings` détruit le type source. Pour `number` et `boolean`, ce n'est pas bloquant techniquement car les chaînes se recastent, mais ce n'est pas idéal.

Pour `object` et `object[]`, il ne faut pas traiter le type comme un simple passthrough de `serde_json::Value`. Le type s'applique au `ScraperDataNode` final. Le chemin nominal doit donc rester :

1. une entry de groupe sélectionne un objet ou une liste d'objets source;
2. ses `entries` construisent un sous-arbre `ScraperDataNode`;
3. `type: object` ou `type: object[]` indique comment ce sous-arbre doit être rendu en JSON.

Si un jour on veut supporter un import direct d'un objet JSON source sans lister ses sous-champs en YAML, cela devrait être une fonctionnalité explicite qui convertit récursivement le `serde_json::Value` sélectionné en `ScraperDataNode`. Même dans ce cas, le renderer final resterait `ScraperDataNode -> serde_json::Value`; il ne faudrait pas mélanger dans un même noeud des valeurs JSON brutes et des enfants scraper.

### 5. Adapter les post-processors

Les post-processors doivent être alignés avec le nouveau contrat :

- `derive_pagination` devrait écrire `have_more` comme `boolean` et éventuellement `source_params` comme `object[]`.
- `compute_items_field` devrait produire `number` par défaut, ou accepter `type`.
- `pivot_items_by_index` doit copier les types des champs source et générer les champs configurés avec leur `type`.
- `append_static_items` devrait pouvoir typer ses champs statiques.
- `extract_regex_items` et `fetch_regex_items_from_items` doivent typer leurs `entries`.

Le risque principal est de typer seulement les entries normales et d'oublier les champs générés. Le front continuerait alors à recevoir un mélange incohérent.

### 6. Validation de configuration

Ajouter une validation lors du chargement YAML :

- `type` connu;
- `object` / `object[]` cohérent avec un noeud ayant des `entries`, des `children` ou des `items`;
- `object[]` ne doit pas être combiné avec `select: first`;
- conflit de type si plusieurs entries écrivent le même `name`;
- avertissement si un champ scalaire est déclaré avec `select: all`;
- avertissement ou erreur si un champ tableau primitif (`string[]`, `number[]`, `boolean[]`) est déclaré avec `select: first` mais cela peut être volontaire.

Cette validation doit rester générique et ne pas introduire de logique spécifique par source.

## Changements YAML à prévoir

La migration peut être franche, car les YAML existants sont encore des prototypes. Il vaut mieux éviter une couche de rétrocompatibilité globale si elle alourdit `ScraperDataNode`, les actions ou le front.

Concrètement :

- toutes les entries qui produisent une sortie devraient déclarer `type`;
- les groupes doivent déclarer la famille objet; par défaut ils produisent un tableau d'objets, et `select: first` produit un objet simple;
- les champs générés par post-processors doivent aussi être typés;
- les fixtures sous `server/data-test/` doivent être migrées dans le même mouvement.

Priorité 1 : pagination et champs numériques évidents.

- `current_page`: `number`
- `total_pages`: `number`
- `page_size`: `number`
- `total`: `number`
- `have_more`: `boolean`
- `year`: `number`
- `count-season`: `number`
- `rating`: `number`
- `storyboard > width`, `height`, `columns`, `interval`: `number`
- `duration`: `string`, sans tableau, car la durée reste un libellé ou une forme source à afficher

Priorité 2 : listes et groupes transversaux.

- `theme`, `genre`, `casting`, `director`, `media-type`: `string[]`
- `entries`, `sections`, `banners`, `categories`, `players`, `season`, `episodes`: groupes objets, tableau d'objets par défaut
- `resolver`, `resolver > stream`, `storyboard`: groupes objets avec `select: first` pour obtenir `{ key: value }`
- liens d'image sous objet : `string`

Priorité 3 : champs ambigus à traiter source par source.

- `content-advisor`: plutôt `string`, car ce n'est pas toujours numérique.
- `release-date` et `expire`: rester `string` tant que le front attend des dates ISO ou pseudo-ISO. Un type `date` pourrait être discuté plus tard, mais il n'est pas dans la demande initiale.
- `source_params`: `Record<string, string>[]`, car ce sont des paramètres de requête.

L'usage d'ancres YAML sera utile pour éviter de répéter `type` sur des groupes réutilisés, mais la lisibilité doit rester prioritaire.

## Changements front à prévoir

Le front ne devrait plus reconstruire les types de base, mais il continuera à normaliser les données backend vers les modèles UI.

Changements probables dans `front/src/services/rustify.ts` :

- supprimer les reconversions comme `firstNumber(record.rating)` ou `firstNumber(record.year)`, car ces champs doivent déjà être des `number` ou `null`;
- remplacer `readBoolean(record.have_more)` par une lecture directe de `boolean | null`;
- remplacer `readStringList` par des lectures de tableaux déjà typés (`string[]`) pour les champs de liste;
- simplifier `readStringMap` pour lire de vrais objets JSON;
- supprimer le parsing JSON de `description` dans `normalizeServiceDescription` si le backend renvoie un objet;
- garder `duration` comme `string | null`, sans tableau; `formatDurationLabel` ne doit plus récupérer la première chaîne d'un `string[]`, seulement formater une chaîne déjà typée;
- garder seulement des garde-fous de validation/narrowing TypeScript pour `unknown`, pas des convertisseurs destinés à compenser l'ancien contrat.

Il ne faut pas confondre deux responsabilités :

- convertir `"123"` en `123` doit disparaître du front;
- transformer `123` secondes en un label `02:03` reste une logique de présentation front légitime.

Les types dans `front/src/types/*` sont déjà majoritairement bons. Il faudra surtout introduire des types pour les réponses backend brutes afin que TypeScript vérifie le nouveau contrat entre `call_api` et les normalizers. Si une réponse backend ne respecte pas ce contrat, le front doit échouer clairement ou ignorer le champ optionnel, pas tenter de deviner un ancien format.

## Contrats backend attendus par méthode front

Le tableau ci-dessous part des appels faits par `front/src/services/rustify.ts`. La colonne "format actuel" décrit ce que le front accepte aujourd'hui par compatibilité avec la sérialisation historique. La colonne "nouveau format" décrit le contrat que le Rust devrait renvoyer après migration : les scalaires doivent être des scalaires JSON, les groupes doivent être des objets ou tableaux d'objets, et les champs déclarés mais non trouvés doivent être à `null`, jamais cachés dans `[""]`.

| Méthode backend | Fonction front | Format actuel consommé | Nouveau format Rust attendu |
|---|---|---|---|
| `search` | `searchMediaItemsPage` | Soit une liste de groupes contenant `entries`, soit une liste plate que le front regroupe par `source`. Les champs de pagination (`current_page`, `total_pages`, `have_more`) sont relus avec `firstNumber` et `readOptionalBoolean`; `source_params` est reconstruit avec `readStringMap`; chaque entry est normalisée depuis des feuilles `string[]`. | `SearchGroup[]` avec `source: string`, `entries: MediaEntry[]`, `current_page: number`, `have_more: boolean`, `total_pages: number/null`, `next_value: string/null`, `next_param: string/null`, `source_params: Record<string, string>[]`. Si `have_more` est `false`, le front ne doit plus recalculer une page suivante depuis une chaîne. |
| `load_home` | `loadHomeCatalog` | Liste ou objet de lignes catalogue. `source`, `banners`, `categories` et `sections` peuvent être des noeuds scraper, des objets uniques, des tableaux, ou des formes alignées que `readBannerList` doit réparer. | `CatalogRow[]` avec `source: string`, `banners: Banner[]`, `categories: Category[]`, `sections: Section[]`. Les groupes `banners`, `categories` et `sections` doivent être déclarés `type: object[]` dans les YAML pour produire directement `[{ key: value }]`. |
| `get_service` | `loadServiceMetadata` | Tableau de métadonnées. `description` peut être un objet ou une chaîne JSON parsée côté front. `themes` ou `search_themes` peuvent être `string[]` ou une liste d'objets à reconvertir. | `ServiceMetadataPayload[]` avec `id: string`, `title: string`, `logo: string/null`, `description: object`, `themes: ServiceTheme[]`. `description` doit être un objet de type `{ locale: text }`; `themes` doit être `object[]` avec `code: string` et `service_code: string`. |
| `get_category` | `getCategoryCatalog` | Même forme tolérée que `load_home`, avec pagination de section parfois encodée en chaînes. | Même contrat que `load_home`: `CatalogRow[]`. Les sections paginables doivent porter `current_page: number`, `have_more: boolean`, `link: string/null` et `request: Record<string, string>/null` directement typés. |
| `get_section` | `loadHomeSectionPage` | Objet direct ou objet contenant `sections`; le front prend `sections[0]` si présent. `entries`, `current_page` et `have_more` sont extraits puis reconvertis. | Objet direct `SectionPage` avec `entries: MediaEntry[]`, `current_page: number`, `have_more: boolean`, et éventuellement `source_params: Record<string, string>[]` si la pagination doit rester spécifique par source. Ne plus renvoyer un wrapper ambigu `{ sections: [...] }` pour ce cas. |
| `get_entry` | `getEntryDetails` | Tableau `unknown[]`; seul `response[0]` est utilisé. Les champs scalaires, les saisons, les épisodes, les players, le resolver et le storyboard sont relus via `readStringList`, `readRecordList`, `firstNumber` et des alias historiques. | Objet direct `EntryDetailsPayload`, pas un tableau. Les listes `season`, `episode` et `players` doivent être des tableaux d'objets; `resolver`, `resolver.stream` et `storyboard` doivent être des objets; `rating`, `year`, `count-season` et dimensions de storyboard doivent être `number/null`; `duration` doit être `string/null`; `theme`, `genre`, `casting`, `director`, `lang/audio` et `lang/subtitles` doivent être `string[]`. |
| `list_lives` | `listLiveMediaItems` | Tableau d'entries normalisées comme des `MediaEntry`, avec les mêmes conversions de chaînes vers listes, nombres et objets image. | `MediaEntry[]` typé directement. Pour les lives, `media-type` devrait être `string[]` contenant une valeur comme `video/live`; les liens et images restent des `object` imbriqués quand ils ont des sous-champs. |
| `get_live` | `getLivePlayers` | Objet contenant `players`, où chaque player est relu comme objet ou liste d'objets. Les liens, le resolver et le storyboard peuvent être des noeuds historiques. | Objet `{ players: Player[] }`. Chaque `Player` doit avoir des chaînes/null pour `embed-link`, `direct-link`, `name`, `lang`; `resolver: object/null`; `storyboard: object/null` avec dimensions numériques. |
| `get_season` | `getSeasonEpisodes` | Objet contenant `episodes`, `current_page` et `have_more`; pagination relue via `firstNumber` et `readBoolean`; épisodes normalisés avec les mêmes conversions que `get_entry`. | Objet `SeasonEpisodePage` avec `current_page: number`, `have_more: boolean`, `episodes: Episode[]`. Chaque épisode doit déjà contenir ses listes et sous-objets typés, notamment `players: Player[]` et `duration: string/null`. |
| `resolve_player_stream` | `resolvePlayerStream` | Objet déjà proche d'un format typé, mais le front accepte encore les alias `streamUrl`, `manifestType`, `licenseUrl`, `licenseHeaders` et reconstruit `license_headers` avec `readStringMap`. | Objet strict `{ stream_url: string, manifest_type: string, license_url: string/null, license_headers: object }`. Les headers doivent être un vrai `Record<string, string>`, sans noeuds `string[]` ni alias camelCase. |

Structures communes cibles utilisées par le tableau :

- `MediaEntry` : objet carte média avec `source: string/null`, `link: string/null`, `web-link: string/null`, `title: string/null`, `title/alt: string/null`, `label: string/null`, `description: string/null`, `overview: string/null`, `media-type: string[]`, `theme: string[]`, `genre: string[]`, `lang: string[]` ou `string/null` selon le YAML, `duration: string/null`, `rating: number/null`, `release-date: string/null`, `expire: string/null`, `episode: object/null`, et images en objets comme `img/poster: { link: string/null }`.
- `Banner` : objet avec `id`, `key`, `title`, `subtitle`, `description`, `image`, `logo`, `video`, `link`, `entry_url`, `web_url` en `string/null`, plus `source: string/null` si le parent ne suffit pas.
- `Category` : objet avec `id`, `key`, `label`, `image`, `description` en `string/null`, et `source` ou `request` en `Record<string, string>` pour les paramètres de requête.
- `Section` / `SectionPage` : objet avec `id`, `key`, `label`, `link`, `query_url` en `string/null`, `request: Record<string, string>/null`, `entries: MediaEntry[]`, `current_page: number`, `have_more: boolean`.
- `Player` : objet avec `embed-link`, `direct-link`, `name`, `lang` en `string/null`, `resolver: { kind: string, target_id: string, stream: { kind: string/null } }/null`, et `storyboard: { link: string, width: number, height: number, columns: number, interval: number }/null`.
- `Episode` : objet proche de `MediaEntry`, avec en plus `players: Player[]`, `season-name: string/null`, `img/preview: object[]` ou `img/preview: { link: string/null }` selon le besoin YAML, et `duration: string/null`.

Conséquence côté front : les normalizers pourront rester responsables de la présentation (`formatDurationLabel`, libellés traduits, URL absolues), mais plus de la récupération de la "première valeur" ni du cast de `"true"` en `true`, de `"42"` en `42`, ou de chaînes JSON en objets.

## Risques et points d'attention

### Cardinalité : valeur unique vs tableau

Aujourd'hui tous les scalaires sont des listes. Avec le nouveau contrat, un champ `string`, `number` ou `boolean` ne devrait pas recevoir plusieurs valeurs.

Pour les groupes objets, la cardinalité dépend de `select` : plusieurs items donnent un tableau d'objets par défaut; `select: first` donne un objet simple.

Recommandation : rendre invalide le cas où un scalaire reçoit plusieurs valeurs, sauf si l'entry déclare explicitement `select: first`, ou si une action/post-processor agrège volontairement plusieurs valeurs en une seule.

### Champs écrits plusieurs fois

Beaucoup de YAML ont plusieurs entries avec le même `name` pour gérer des fallbacks. Exemple : plusieurs sources d'image ou plusieurs pointeurs pour `description`.

Il faudra imposer que toutes les entries qui écrivent le même chemin déclarent le même `type`.

### `object` depuis une source JSON

Le type `object` n'est pas un type de valeur source. Il décrit la sérialisation d'un noeud `ScraperDataNode`. Pour un scraper JSON, le cas nominal doit donc rester un groupe YAML : le pointer sélectionne l'objet source, les `entries` construisent les enfants du noeud, puis `type: object` ou `type: object[]` sérialise ce noeud.

Il vaut mieux éviter un fallback qui parserait la chaîne JSON produite par `json_value_to_strings`. Ce serait fragile, tardif et incohérent avec le modèle où `object` est porté par le noeud scraper. Si un besoin réel apparaît pour exposer un objet JSON source sans décrire ses sous-champs, il faudrait ajouter une fonctionnalité explicite d'import récursif `serde_json::Value -> ScraperDataNode`, avec validation de type sur le noeud final.

### Post-processors oubliés

Si seuls les entries YAML normales sont typées, les champs produits par `derive_pagination` ou `compute_items_field` resteront en chaînes. Ce serait visible immédiatement côté front sur `have_more`, `current_page`, `total_pages`, etc.

Recommandation : traiter les post-processors dans le même lot que le type runtime.

### Désérialisation de `ScraperDataNode`

`ScraperDataNode` dérive aussi `Deserialize` via `ScraperDataNodeRaw`. Aujourd'hui cette désérialisation attend des tableaux de chaînes ou des objets de chaînes. Avec des sorties typées, il faudra décider si le runtime doit savoir relire ses nouvelles formes JSON typées, ou si la désérialisation n'est utilisée que pour des tests/fixtures historiques.

### Documentation existante

`docs/response-format.md` documente explicitement que les champs scalaires sont des `string[]`. Cette documentation deviendra fausse dès que la migration commencera. Elle devra être mise à jour dans le même changement que le contrat effectif.

### Pas de double contrat frontend

Comme les YAML sont encore des prototypes, le front ne devrait pas porter durablement deux contrats. Une fois le moteur et les YAML migrés, `rustify.ts` doit lire des valeurs déjà typées ou `null`, et les helpers historiques (`firstNumber`, `readBoolean` string-based, parsing JSON de champs objet) doivent disparaître.

## Plan de migration proposé

1. Ajouter `ScraperOutputType`, exposé en YAML par le champ `type`, et la sérialisation typée dans `ScraperDataNode`.
2. Remplacer le chemin principal de sérialisation basé sur `ScraperDataNodeRaw` par un renderer explicite `ScraperDataNode -> serde_json::Value`.
3. Propager `type` depuis les entries HTML, JSON et static, puis valider les conflits de type sur un même chemin.
4. Adapter les post-processors pour produire ou propager des types, surtout `derive_pagination` et `compute_items_field`.
5. Migrer les YAML et fixtures vers le contrat strict : scalaires typés, listes typées, groupes `object` / `object[]`.
6. Adapter `front/src/services/rustify.ts` pour consommer directement les champs typés et retirer les conversions basées sur l'ancien `string[]`.
7. Mettre à jour `docs/response-format.md` avec le nouveau contrat.

## Validation recommandée

Même si aucune nouvelle infrastructure de tests n'est nécessaire par défaut, l'implémentation devrait au minimum utiliser les checks existants :

- `cargo test` ou tests ciblés sur `arachnea-scrapyfy` et `arachnea-stream`;
- validation de chargement de tous les YAML `server/services/*.yaml` et `server/services/darkstream/*.yaml`;
- comparaison manuelle ou snapshot des endpoints principaux : `get_service`, `load_home`, `search`, `get_entry`, `get_season`;
- `npm run type-check` côté `front` si disponible;
- vérification UI sur les zones qui utilisent des nombres/booléens : pagination, badges de durée/rating, storyboard player, saisons.

## Décisions actées avant implémentation

- En cas de cast impossible, la query doit échouer avec un détail exploitable du problème. Le message doit permettre d'identifier le champ de sortie, le type attendu, la valeur reçue et la query ou entry concernée.
- Si une entry est déclarée dans le YAML mais qu'aucune valeur n'est trouvée, le champ doit être présent à `null` dans la sortie.
- Quand un scalaire reçoit plusieurs valeurs, le comportement attendu est une erreur, sauf si l'entry déclare explicitement `select: first`.
- Pour les groupes objets, la sortie est un tableau d'objets par défaut; `select: first` force l'objet simple.
- `object[]` reste accepté comme alias explicite pour un tableau d'objets, mais `object[]` avec `select: first` doit être refusé comme configuration contradictoire.
- `source_params` et les paramètres de requête restent des `Record<string, string>`.
- `year` et `count-season` sont des `number`; `duration` est un `string` sans tableau.
- Le format de gestion d'erreur existant est conservé; seuls les messages doivent être enrichis avec le détail du problème.
- `ScraperDataNodeRaw` ne doit plus être utilisé comme chemin principal de sérialisation. Le moteur doit générer directement un `serde_json::Value` depuis `ScraperDataNode`.
- `object` et `object[]` sont des types de sérialisation de `ScraperDataNode`, pas des casts directs de valeurs JSON sources. Pour exposer un objet source JSON, il faut le matérialiser via des `entries` de groupe. Un import direct `serde_json::Value -> ScraperDataNode` peut rester une évolution future explicite, mais pas un comportement implicite du premier palier.

## Etat d'implémentation

L'implémentation retenue suit l'approche A :

- `ScraperOutputType` porte le contrat YAML `type`.
- `ScraperDataNode` sérialise désormais directement vers `serde_json::Value`; `ScraperDataNodeRaw` reste disponible pour la désérialisation historique, mais n'est plus le chemin principal de sortie.
- Les entries HTML, JSON, static et les post-processors générateurs de champs propagent les types dans les noeuds.
- Les YAML `server/services/**/*.yaml` sont migrés vers `type`, avec `select: first` explicite pour les scalaires.
- Les sorties `get_entry`, `get_season`, `get_live` sont des objets simples; les endpoints agrégés restent des tableaux de groupes ou d'items.
- Le front lit les nombres et booléens déjà typés (`number`, `boolean`) au lieu de parser les anciens tableaux de chaînes.
- `docs/response-format.md` décrit le nouveau contrat typé.

## Détails d'implémentation utiles au debug

### Chemin principal de sérialisation

Le point central est `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_data_node.rs`.

Le flux attendu est :

1. Une entry YAML est désérialisée avec `type`, stocké côté Rust dans `output_type`.
2. L'extracteur ou le post-processor pose ce type sur le noeud cible via `set_output_type`, `push_value_typed`, `set_value_typed` ou `push_node_typed`.
3. `ScraperDataNode::serialize` appelle `to_json_value()`.
4. `to_json_value_at(path)` choisit le renderer selon `output_type`.
5. Les erreurs de cast sont remontées comme erreurs serde, donc elles conservent le format d'erreur backend existant.

`ScraperDataNodeRaw` n'est plus utilisé pour produire la sortie JSON. Il reste seulement le chemin de désérialisation historique via `#[serde(try_from = "ScraperDataNodeRaw")]`.

### Où le type est attaché

Les entries HTML et JSON ont un champ Rust `output_type`, exposé en YAML par `#[serde(rename = "type")]`.

Fichiers concernés :

- HTML : `scraper_html/entry.rs`;
- JSON : `scraper_json/entry.rs`;
- static : `scraper_static/query.rs`;
- post-processors : `post_processes/types.rs`, `regex_helpers.rs`, `derive_pagination.rs`, `compute_items_field.rs`, `append_static_items.rs`, `pivot_items_by_index.rs`, `extract_regex_items.rs`, `fetch_regex_items_from_items.rs`.

Pour un champ feuille :

- `select: first` utilise `set_value_typed`;
- `select: all` ou le défaut utilise `push_value_typed`;
- le type doit être scalaire ou tableau primitif (`string`, `number`, `boolean`, `string[]`, `number[]`, `boolean[]`).

Pour un groupe avec `entries` :

- le type YAML doit être `object` ou `object[]`;
- `type: object` + `select: first` garde `object`;
- `type: object` sans `select: first` est normalisé en `object[]`;
- `type: object[]` reste `object[]`;
- `type: object[]` + `select: first` est refusé.

### Champs déclarés mais absents

Une entry typée pose son `output_type` avant d'ajouter des valeurs. Si aucune valeur n'est trouvée, le noeud existe donc avec son type mais sans valeur, et la sérialisation renvoie `null`.

Si un champ attendu manque complètement dans la réponse, vérifier en priorité que l'entry appelle bien `set_output_type` avant extraction. C'est le cas des entries HTML/JSON/static normales; si le champ vient d'un post-processor custom, il faut contrôler ce post-processor.

### Erreurs fréquentes et origine probable

| Message ou symptôme | Cause probable | Correction habituelle |
|---|---|---|
| `entry X must define type` | Entry YAML ou entrée de post-process sans `type`. | Ajouter `type` sur l'entry qui produit la sortie. |
| `uses object[] but does not define entries` | Champ feuille typé objet sans sous-groupe. | Soit ajouter `entries`, soit remplacer par un type scalaire/tableau. Exception : les champs avec `sub_queries` peuvent être transitoires. |
| `cannot combine type object[] with select: first` | Contrat contradictoire. | Utiliser `type: object` avec `select: first`, ou retirer `select: first`. |
| `expected one value, received [...]` | Un champ `string`, `number` ou `boolean` a reçu plusieurs valeurs. | Ajouter `select: first` si une seule valeur est voulue, ou passer en `string[]`/`number[]`/`boolean[]`. |
| `invalid number` | Une valeur typée `number` n'est pas parsable. | Corriger l'action YAML, le pointer, ou garder `type: string` si le format n'est pas numérique strict. |
| `invalid boolean` | Une valeur typée `boolean` n'est pas reconnue. | Produire `true`, `false`, `1`, `0`, `yes`, `no`, `y` ou `n`, ou garder `type: string`. |
| Un objet attendu devient `[{}]` ou `{}` | Le groupe existe mais ses sous-entries ne trouvent rien. | Vérifier le `pointer`/`selector` du groupe et des enfants. |
| Un parent implicite comme `players` sort en objet au lieu de liste | Les enfants n'ont pas de cardinalité alignée ou un seul enfant produit une seule valeur. | Déclarer un vrai groupe `players` en `object[]`, ou vérifier les champs enfants produits par post-process. |

### Cas particulier des `sub_queries`

Un champ sans `entries` mais avec `sub_queries` peut être typé `object[]`. Il sert alors de valeur transitoire : la valeur initiale peut être une URL ou un identifiant, puis le résultat de la sous-query remplace ou enrichit ce champ.

C'est utilisé pour des champs comme `players` ou `video` qui extraient d'abord une URL de follow-up. La validation autorise donc les types objet sur ces champs même sans `entries`, uniquement parce que `sub_queries` est présent.

Si une erreur de type apparaît sur un champ de ce genre, il faut regarder deux endroits :

- la valeur initiale extraite par l'entry parent, utilisée pour déclencher la requête;
- le résultat fusionné par `execute_entry_sub_queries` dans `scraper/query_executor.rs`.

### Groupes implicites avec chemins `>`

Les chemins comme `img/poster > link`, `resolver > stream > kind` ou `players > direct-link` créent des parents implicites.

Ces parents implicites n'ont généralement pas de `type` YAML propre. Leur sérialisation reste possible parce que :

- un noeud non typé avec `children` se sérialise en objet;
- si plusieurs enfants feuilles ont des valeurs alignées, `array_len()` permet de sérialiser en tableau d'objets.

Ce second point est important pour les post-processors qui produisent par exemple :

```yaml
- name: players > name
  type: string
- name: players > direct-link
  type: string
```

Si `name` et `direct-link` produisent chacun deux valeurs, la sortie peut devenir :

```json
"players": [
  { "name": "HD", "direct-link": "..." },
  { "name": "SD", "direct-link": "..." }
]
```

Pour les nouveaux YAML, il reste préférable de déclarer un vrai groupe `players` avec `type: object[]` quand c'est possible; les groupes implicites sont surtout un chemin de compatibilité contrôlé.

### Static queries et objets

Les static queries utilisent aussi `type`.

Pour les métadonnées service, `description` est maintenant un objet typé. Le cas `value: "{service_description}"` est traité explicitement dans `scraper_static/query.rs` : la chaîne JSON rendue est parsée puis convertie récursivement en `ScraperDataNode`.

Conséquence de debug :

- `get_service` doit exposer `description: { "fr": "..." }`, pas une chaîne JSON;
- les tests de champs attendent `description > fr`;
- si le JSON statique est invalide, l'erreur pointe vers l'entry static concernée.

### Post-processors

Les post-processors qui créent des champs doivent poser ou propager le type.

Points à vérifier en cas de bug :

- `derive_pagination` produit `current_page`/`total_pages` en `number`, `have_more` en `boolean` et `source_params` en `object[]`;
- `compute_items_field` produit un `number`;
- `extract_regex_items` et `fetch_regex_items_from_items` exigent `type` sur chaque `entries[]`;
- `pivot_items_by_index` tente de propager le type élémentaire depuis la source vers `nested_value_field`;
- `append_static_items` produit un `object[]` cible et des enfants string.

Une erreur comme `fetch_regex_items_from_items for get_season entry X must define type` signifie que l'entrée `entries:` du post-process YAML n'a pas encore été migrée.

### Front et endpoints

`server/crates/arachnea-stream/src/stream_scraper.rs` conserve le shaping générique sauf pour les endpoints qui retournent naturellement un seul objet :

- `get_entry` dépile maintenant le premier résultat et renvoie un objet direct;
- `get_season` et `get_live` faisaient déjà ce genre de dépilage;
- `search`, `load_home`, `get_category` et `list_lives` restent des tableaux.

Côté front, `front/src/services/rustify.ts` garde la normalisation UI, mais ses helpers de bas niveau sont volontairement plus stricts :

- `firstNumber` ne parse plus les chaînes, il lit seulement un `number`;
- `readBoolean` et `readOptionalBoolean` lisent seulement un `boolean`;
- `normalizeServiceDescription` attend un objet typé;
- `readStringParamMap` est le helper dédié pour transformer les primitives déjà typées en `Record<string, string>` quand le front doit renvoyer des paramètres au backend.

Si un champ devient vide côté UI après cette migration, vérifier d'abord la forme JSON réelle renvoyée par le backend. Le front ne masque plus autant les erreurs de type qu'avant.

### Commandes de validation utiles

Depuis `server/` :

```bash
cargo test config_yaml_to_json --lib -p arachnea-scrapyfy
cargo test -p arachnea-scrapyfy
cargo check -p arachnea-stream
ARACHNEA_TEST_QUERY_SOURCE=darkstream/anime-sama cargo test -p arachnea-stream test_query_service_stream_metadata -- --nocapture
```

`config_yaml_to_json` valide surtout les YAML top-level sous `server/services/*.yaml`. Pour les YAML en sous-dossier comme `server/services/darkstream/*.yaml`, utiliser les tests `arachnea-stream` avec `ARACHNEA_TEST_QUERY_SOURCE=darkstream/<service>`.

Depuis `front/` :

```bash
npm run type-check
```

Si cette commande échoue avec `vue-tsc: command not found`, installer les dépendances front avant de conclure sur une erreur TypeScript.

## Conclusion

La migration est faisable sans refonte complète si le premier palier ajoute un contrat de sortie typé autour de `ScraperDataNode`, tout en gardant les actions internes en chaînes. Le point critique est de ne pas typer uniquement les feuilles : `object` / `object[]` doivent aussi contrôler la sérialisation des sous-groupes, et les post-processors doivent produire des champs typés.

La stratégie la plus simple, puisque les YAML sont encore des prototypes, est de migrer vers un contrat strict : support générique dans Rust, YAML entièrement typés, front qui lit des valeurs déjà bonnes ou `null`, et suppression des compatibilités qui ne serviraient qu'à maintenir l'ancien `string[]`.
