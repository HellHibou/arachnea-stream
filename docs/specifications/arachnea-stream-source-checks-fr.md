# Tests YAML du service Stream : rédaction et exécution

Ceci est le guide de rédaction complet pour la **version de schéma 1**. Il décrit
le chargeur et l'exécuteur implémentés. Il s'adresse aux humains et aux agents IA.
Renommer ce guide ne change pas `schema_version: 1`.

## 1. Commandes : exécuter les tests

Exécutez chaque commande de cette section **depuis la racine du dépôt**. Cargo doit
être installé et présent dans le `PATH`. Les tests Rust existants effectuent la
découverte et l'exécution ; il n'y a aucun lanceur JavaScript, binaire
supplémentaire ou alias Cargo à installer.

### Un point d'entrée Rust ; sélection via l'environnement

Utilisez `stream_scraper::tests::test_all_services` pour un scénario d'intégration
complet. `ARACHNEA_TEST_QUERY_SOURCE` contrôle sa portée :

| Valeur d'environnement | Sélection de la source |
| --- | --- |
| Variable non définie | Tous les fichiers YAML de production découverts récursivement |
| Exactement `*` | Tous les fichiers YAML de production découverts récursivement |
| ID d'un service de production, tel que `anime-sama` | Uniquement ce service |
| Chemin relatif à `services`, tel que `arachnea-stream/dark-stream/anime-sama.yaml` | Uniquement cette configuration |
| Sélection vide, composée d'espaces, non Unicode, inconnue ou ambiguë | Erreur ; ne s'élargit jamais silencieusement à toutes les sources |

Pour **un service**, exécutez :

```sh
ARACHNEA_TEST_QUERY_SOURCE=anime-sama \
  cargo test --manifest-path server/Cargo.toml -p arachnea-stream --lib \
  stream_scraper::tests::test_all_services -- --exact --ignored --nocapture
```

Remplacez `anime-sama` par l'`id` du YAML de production ou par son chemin relatif à
`services`. Cela exécute toutes les étapes du contrat pour cette source, y compris
la navigation déclarée.

Pour **toutes les sources**, laissez `ARACHNEA_TEST_QUERY_SOURCE` non définie et
exécutez :

```sh
cargo test --manifest-path server/Cargo.toml -p arachnea-stream --lib \
  stream_scraper::tests::test_all_services -- --exact --ignored --nocapture
```

Pour sélectionner explicitement toutes les sources même si un sélecteur a été
défini auparavant, utilisez le joker littéral. Mettez `*` entre guillemets comme
indiqué :

```sh
ARACHNEA_TEST_QUERY_SOURCE='*' \
  cargo test --manifest-path server/Cargo.toml -p arachnea-stream --lib \
  stream_scraper::tests::test_all_services -- --exact --ignored --nocapture
```

Ce sont des commandes d'intégration en direct. Le test est ignoré par un
`cargo test` ordinaire ; « toutes par défaut » ne s'applique que lorsque ce test
d'intégration est explicitement exécuté. Depuis `server/`, les mêmes commandes
fonctionnent après avoir omis `--manifest-path server/Cargo.toml`.

Rust analyse récursivement `server/services/arachnea-stream/` à la recherche de
fichiers `.yaml`. Cela inclut les sources désactivées et les fichiers absents de
`services.json`. Aucune liste de chemins de fournisseurs n'est maintenue. L'analyse
rejette les liens symboliques.

Chaque source est préparée indépendamment avant les requêtes au fournisseur. Un
contrat de test manquant est signalé comme `not_tested` avec `coverage_missing`
comme raison ; un contrat invalide est signalé comme un échec de source. **Les
autres services valides s'exécutent quand même.** Ne sautez pas silencieusement les
contrats manquants et ne déduisez pas les attentes de la production. Le résumé final
liste chaque service sélectionné et sa raison. Cargo ne signale un échec agrégé
qu'après la fin des services restants si une source a échoué ou n'a pas été testée,
ce qui préserve le signal de couverture incomplète.
Le 30 octobre 2026, l'inventaire comptait 21 configurations et neuf contrats : les
12 autres contrats étaient volontairement différés. Il s'agit d'un instantané de
l'inventaire, pas d'une limite fixe. Sélectionnez un service couvert pendant que
vous rédigez un contrat manquant.

### Vérifier un contrat via son opération de métadonnées

```sh
ARACHNEA_TEST_QUERY_SOURCE=anime-sama \
  cargo test --manifest-path server/Cargo.toml -p arachnea-stream --lib \
  stream_scraper::tests::test_query_service_stream_metadata -- --exact --nocapture
```

Cela charge et valide le **contrat sélectionné dans son ensemble**, vérifie ses
capacités de production, puis exécute uniquement les métadonnées. Cela n'exécute pas
les étapes de navigation, de recherche ou d'entrée. Les requêtes de métadonnées de
production existantes utilisent `scraper_type: static` ; vérifiez-le avant de
traiter la vérification des métadonnées d'une nouvelle source comme une
vérification locale, sans fournisseur. Ce n'est pas une commande réservée au
parseur.

Pour valider tous les contrats versionnés et exécuter leurs métadonnées, laissez
`ARACHNEA_TEST_QUERY_SOURCE` non définie ou réglez-la sur `*`, puis lancez la même
commande Cargo de métadonnées.
Elle découvre les contrats récursivement sous `tests/fixtures/services/` dans la
crate Stream. Elle ne détecte **pas** les fichiers YAML de production sans contrat ;
utilisez la commande d'intégration complète `test_all_services` sans sélecteur ou
avec `*` pour cette vérification de couverture.

### Exécuter une opération en direct et ses prérequis

```sh
ARACHNEA_TEST_QUERY_SOURCE=anime-sama \
  cargo test --manifest-path server/Cargo.toml -p arachnea-stream --lib \
  stream_scraper::tests::test_query_search -- --exact --ignored --nocapture
```

Utilisez exactement l'un de ces noms de test existants :

| Vérification demandée | Nom complet du test Cargo | Nécessite `--ignored` ? |
| --- | --- | --- |
| Métadonnées | `stream_scraper::tests::test_query_service_stream_metadata` | Non |
| Accueil | `stream_scraper::tests::test_query_load_home` | Oui |
| Recherche | `stream_scraper::tests::test_query_search` | Oui |
| Entrée/détail | `stream_scraper::tests::test_query_get_entry` | Oui |
| Scénario d'intégration complet, une source ou toutes selon le sélecteur | `stream_scraper::tests::test_all_services` | Oui |
| Point d'entrée de compatibilité pour un scénario complet, mêmes règles de sélection | `stream_scraper::tests::test_service` | Oui |

Les opérations de catégorie, de section et de lecteur n'ont pas de fonction de test
dédiée. Exécutez-les via le contrat complet ou comme dépendances d'une vérification
d'entrée. N'inventez pas de noms de test Cargo : un filtre correspondant à zéro test
ne valide rien.
Les opérations en direct filtrées utilisent la même table de sélection de source.
Définissez un ID de source pour éviter de sélectionner toutes les configurations de
production. Préférez `test_all_services` au nom de compatibilité `test_service`, et
utilisez `--exact` pour éviter d'exécuter les deux points d'entrée de scénario
complet.

### Exécuter les tests locaux de routine

```sh
cargo test --manifest-path server/Cargo.toml -p arachnea-stream --all-features
cargo test --manifest-path server/Cargo.toml --workspace
```

Les cinq points d'entrée d'intégration en direct sont ignorés par défaut. Les
commandes de routine n'ont besoin d'aucune liste `--skip` et ne qualifient pas la
santé des sources en direct. Conservez le nombre d'ignorés dans les résumés.
`--offline`, s'il est ajouté à Cargo, n'affecte que l'accès aux dépendances ; il ne
désactive **pas** les requêtes HTTP au fournisseur lorsqu'un test en direct est
sélectionné.

Cargo rapporte directement le statut du test Rust existant : un succès renvoie 0 et
un échec de test renvoie normalement 101. Aucun wrapper ne remappe ce résultat.
Inspectez les diagnostics de sélection de source et le résultat final du test, pas
seulement le statut du processus.

## 2. Procédure de rédaction : suivez ces étapes dans l'ordre

1. Lisez le fichier visé sous `server/services/arachnea-stream/`. Notez son `id`
   exact, son chemin relatif à `services` et les noms de requêtes sous `queries`.
2. Inspectez ses formes de sortie publiques, y compris les déclarations de `type`
   de production et les réponses réelles lorsqu'elles sont disponibles. Incluez le
   post-traitement de production pertinent, pas seulement les déclarations
   d'entrée d'origine. Ne supposez pas que chaque fournisseur a une recherche, des
   catégories, des sections, des pages de détail ou les mêmes types de champs.
3. Créez exactement un contrat au chemin reflété décrit à la section 3. Ne créez
   ni ne modifiez le YAML de production pour faire passer le test dans le cadre de
   la rédaction du contrat. Si la production nécessite une correction, signalez-la
   séparément.
4. Choisissez les opérations prises en charge à vérifier. Incluez toujours une
   étape de métadonnées et au moins une opération de contenu. Décrivez un chemin
   réel et utile à travers la source.
5. Écrivez les étapes dans l'ordre des dépendances. Choisissez des minima de
   données utiles réduits et uniquement les champs réellement indispensables.
   Ajoutez délibérément des contraintes sur les champs optionnels.
6. Vérifiez chaque règle des schémas ci-dessous. N'inventez ni clés ni opérations.
7. Exécutez la commande de métadonnées pour cette source. Corrigez d'abord les
   erreurs de configuration.
8. Exécutez la commande d'intégration complète `test_all_services` avec
   `ARACHNEA_TEST_QUERY_SOURCE` réglée sur l'ID du service visé. Inspectez les
   décomptes, les champs invalides, les erreurs et les lacunes de couverture de
   chaque étape. Une réussite limitée aux métadonnées est insuffisante.
9. Rapportez la commande exacte, la source, les références d'entrée et le résultat
   observé. Gardez visibles les limitations d'accès et les échecs de contrat. Ne
   relâchez pas les assertions simplement pour obtenir un résultat vert. Mettez à
   jour le changelog et les suivis de couverture dans `docs/TODO.md` lorsque les
   instructions `AGENTS.md` du dépôt l'exigent.

Pour les détails du format de production et des sorties publiques, consultez la
[spécification Scrapyfy YAML](arachnea-scrapyfy-fr.md) et la
[spécification Stream](arachnea-stream-fr.md). Elles décrivent le comportement de
production ; les clés autorisées dans un fichier de test sont définies uniquement
dans ce guide.

**Pour les agents IA :** rédigez uniquement le ou les contrats de service demandés,
plus les mises à jour requises de documentation, de changelog et de suivi. Gardez
tous les termes propres à la source, les URL, les chemins de champs et la navigation
dans le YAML. N'ajoutez pas de branches Rust propres à un fournisseur, de nouvelle
fonction de test, de nouveau framework ni de liste de fournisseurs maintenue. Ne
créez pas tous les contrats restants sauf demande explicite. Ne prétendez jamais
qu'une étape en direct non exécutée a réussi.

## 3. Emplacement des fichiers et identité

Ces quatre valeurs doivent concorder :

| Élément | Exemple |
| --- | --- |
| Fichier de production | `server/services/arachnea-stream/legal-stream/m6play-fr.yaml` |
| `id` de production et `source` du contrat | `m6play-fr` |
| `source_config` du contrat | `arachnea-stream/legal-stream/m6play-fr.yaml` |
| Fichier de test | `server/crates/arachnea-stream/tests/fixtures/services/arachnea-stream/legal-stream/m6play-fr.yaml` |

Règles :

- `source_config` commence relativement à `server/services/`, et non à la racine du
  dépôt, à la crate Stream ou au répertoire des fixtures. Incluez
  `arachnea-stream/`.
- Préservez chaque sous-répertoire sous `server/services/` dans le chemin de la
  fixture. Ne placez pas les contrats directement sous `tests/fixtures/services/`.
- Le nom de fichier est `<source>.yaml` ; sa racine doit être exactement égale à
  l'`id` de production et à la `source` du contrat. Les ID doivent être non ambigus
  pour la sélection par ID.
- Utilisez `.yaml`, et non `.yml` ou `.json`. Les chemins absolus, les composants
  `.`/`..` et les configurations sortant de la racine des services sont rejetés.
  La découverte des répertoires rejette les liens symboliques ; ne les utilisez pas
  pour les fichiers de production ou de test.
- Le répertoire historique `server/data-test/` n'est pas utilisé. Ne le recréez pas.

Le fichier de production définit **comment scraper**. Le contrat définit **quelle
sortie utile doit rester disponible pour les entrées de référence choisies**. Lisez
la production pour comprendre les opérations et les formes disponibles, mais ne
copiez pas mécaniquement tous les noms de champs de production dans les attentes.
Les deux documents ont des schémas différents. Les clés de production telles que
`queries`, `scraper_type`, `row_selector`, `entries` et `actions` ne sont pas des
clés de premier niveau valides dans un contrat de test. Des noms comme `entries`
peuvent toutefois apparaître comme segments de chemin.

## 4. Exemple complet fonctionnel

Voici le contrat M6 existant. Il exerce les métadonnées, la navigation d'accueil,
une catégorie, une section différée, la recherche et une page de détail. C'est un
fichier complet, pas un fragment. Ses identifiants de source, son terme de recherche
et ses chemins appartiennent à M6.

**Lors de la rédaction d'un autre service, ne copiez pas ces valeurs telles
quelles.** Remplacez l'identité et choisissez les opérations, les entrées de
référence et les chemins à partir du contrat réel de ce service. Par exemple, une
source sans recherche ne doit pas hériter de l'étape `search` de M6.

Fichier de production : `server/services/arachnea-stream/legal-stream/m6play-fr.yaml`.
Fichier de test : `server/crates/arachnea-stream/tests/fixtures/services/arachnea-stream/legal-stream/m6play-fr.yaml`.

```yaml
# Reference expectations; live qualification is recorded separately.
schema_version: 1
source: m6play-fr
source_config: arachnea-stream/legal-stream/m6play-fr.yaml

capabilities:
  expected: ["service_stream_metadata","load_home","get_category","get_section","search","get_entry"]
  not_expected: []

steps:
  - id: metadata
    operation: service_stream_metadata
    expect:
      items_path: []
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["id"]
        - ["title"]
        - ["logo"]

  - id: home
    operation: load_home
    expect:
      items_path: ["categories"]
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["label"]
        - ["request","query_url"]

  - id: category
    operation: get_category
    inputs:
      query_url:
        from_step: home
        items_path: ["categories"]
        select: first_valid
        value_path: ["request","query_url"]
    expect:
      items_path: ["sections"]
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["label"]
        - ["link"]

  - id: section
    operation: get_section
    inputs:
      query_url:
        from_step: category
        items_path: ["sections"]
        select: first_valid
        value_path: ["link"]
    expect:
      items_path: ["entries"]
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["title"]
        - ["link"]
      optional_fields:
        - path: ["web-link"]
          type: string

  - id: search
    operation: search
    inputs:
      query: "journal"
    expect:
      items_path: ["entries"]
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["title"]
        - ["link"]
      optional_fields:
        - path: ["description"]
          type: string
        - path: ["web-link"]
          type: string

  - id: entry
    operation: get_entry
    inputs:
      query_url:
        from_step: section
        items_path: ["entries"]
        select: first_valid
        value_path: ["link"]
    expect:
      items_path: []
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["title"]
      optional_fields:
        - path: ["description"]
          type: string
```

Un service plus simple peut ne nécessiter que les métadonnées plus la recherche, ou
les métadonnées plus l'accueil et l'entrée. Un service basé sur des catégories
nécessite une navigation par catégorie lorsque l'accueil ne contient que des
descripteurs de catégorie. Le chemin utile exact est une décision propre à chaque
service.
Les sections suivantes constituent le schéma autorisé complet ; les extraits plus
petits ci-dessous remplacent la partie indiquée d'un fichier et ne sont pas des
contrats autonomes.

## 5. YAML et schéma de premier niveau

Utilisez un seul document YAML. Utilisez de vrais nombres/booléens pour les champs
numériques/booléens et des chaînes pour les ID, les opérations, le texte d'entrée et
les segments de chemin. Mettez entre guillemets les textes ambigus tels que `"true"`,
`"123"` ou `"*"` lorsqu'une chaîne est voulue. Les valeurs vides telles que
`query:` sont nulles, pas des chaînes valides. Préférez `[]` ou `{}` pour les
collections vides explicites lorsque le schéma les autorise.

Les clés inconnues, les clés de mapping dupliquées, les clés de mapping non
chaînes, la coercition implicite des scalaires et les valeurs nulles explicites sont
rejetées. N'utilisez pas de balises YAML et ne comptez pas sur les balises pour
convertir les types ; le lecteur strict rejette les valeurs étiquetées non prises en
charge. N'ajoutez pas de champs déguisés en commentaires tels que `description`,
`enabled`, `cache`, `retries`, `skip` ou `nullable` au schéma. Les commentaires YAML
ordinaires sont autorisés. Omettez une clé optionnelle pour utiliser sa valeur par
défaut.

| Clé de premier niveau | Requise ? | Type et règle exacte |
| --- | --- | --- |
| `schema_version` | Oui | Entier, exactement `1` |
| `source` | Oui | Chaîne non vide égale à l'`id` de production et à la racine du nom de fichier |
| `source_config` | Oui | Chemin `.yaml` existant relatif à `services`, reflété par le fichier de test |
| `capabilities` | Oui | Mapping contenant les deux listes décrites à la section 6 |
| `steps` | Oui | Liste ordonnée de 1 à 32 mappings d'étape, y compris les métadonnées |
| `limits` | Non | Mapping de la section 11 ; vaut par défaut 60 secondes par étape et 300 par source |
| `prerequisites` | Non | Liste de la section 11 ; vaut `[]` par défaut |

Aucun champ obligatoire n'est déduit de la production ni rempli à partir d'une autre
source.

## 6. Capacités : attendues, non attendues et non couvertes

```yaml
capabilities:
  expected: [service_stream_metadata, load_home, get_entry]
  not_expected: [search]
```

Ce fragment n'est valide que si la production déclare les trois opérations attendues
et ne déclare **pas** `search`.

- `expected` et `not_expected` sont tous deux des listes obligatoires de noms
  d'opérations distincts et non vides. `not_expected: []` est valide ; l'omettre ne
  l'est pas.
- Seuls les sept noms d'opération de la section 7 peuvent figurer dans l'une ou
  l'autre liste.
- Les listes ne peuvent pas se chevaucher.
- `service_stream_metadata` doit être dans `expected` et doit avoir une étape.
- Chaque opération de `expected` doit exister en production et avoir au moins une
  étape. L'opération de chaque étape doit être dans `expected`.
- `not_expected` signifie **absent de la production**, et non « présent mais non
  testé » ou « temporairement cassé ». Si la production le déclare, la validation
  échoue.
  Ces opérations sont exclues du tableau des résultats et des décomptes de sévérité.
- Les opérations de production qui ne figurent dans aucune des deux listes sont
  signalées comme `coverage_missing`, avec le statut `Skip` et la sévérité
  `Warning` dans le tableau. Cela inclut les opérations non prises en
  charge par le schéma v1, telles que `get_season`, `list_lives` ou
  `resolve_stream`. Ne placez pas de noms non pris en charge dans l'une ou l'autre
  liste.
- Une opération supplémentaire non couverte n'invalide pas les étapes déclarées
  réussies, mais empêche de revendiquer une couverture complète des capacités.
- Un contrat de source manquant est signalé comme `not_tested`. Les autres sources
  valides s'exécutent ; l'agrégat reste non réussi pour couverture incomplète après
  le résumé final.

Pour terminer avec succès une exécution complète de source, au moins une étape de
**contenu** exécutée doit réussir : `search`, `get_category`, `get_section`,
`get_entry` ou `get_players`. Les métadonnées et l'accueil seuls ne peuvent pas
produire un résultat vert pour une source complète.

## 7. Étapes et opérations prises en charge

Chaque étape accepte exactement ces clés :

| Clé d'étape | Requise ? | Règle |
| --- | --- | --- |
| `id` | Oui | Chaîne non vide unique dans ce fichier ; les références utilisent cet ID |
| `operation` | Oui | Un nom exact pris en charge dans le tableau ci-dessous |
| `inputs` | Non | Mapping, `{}` par défaut ; uniquement les clés autorisées pour cette opération |
| `expect` | Oui | Mapping décrit à la section 10 |

Plusieurs étapes peuvent utiliser la même opération, par exemple une entrée
découverte et une entrée de référence séparée. Elles doivent avoir des ID d'étape
différents. Chaque étape s'exécute une fois ; il n'y a ni nouvelle tentative, ni
boucle, ni pagination automatique dans l'exécuteur.

| Opération | `inputs` autorisés | Racines de charge utile publique fournies aux assertions | Mode de cache |
| --- | --- | --- | --- |
| `service_stream_metadata` | Aucune entrée | Objets de métadonnées individuels | `NoCache` |
| `load_home` | Aucune entrée | Objets d'accueil individuels | `FullCache` |
| `search` | `query` requis : chaîne non vide ou référence | Objets de groupe de recherche publics individuels, contenant généralement `entries` | `ClientCache` |
| `get_category` | `query_url` requis : chaîne non vide ou référence ; `page` optionnel : entier positif, `1` par défaut | Objets de catégorie individuels | `FullCache` |
| `get_section` | `query_url` requis : chaîne non vide ou référence ; `page` optionnel : entier positif, `1` par défaut | Un objet de section | `FullCache` |
| `get_entry` | `query_url` requis : chaîne non vide ou référence | Un objet d'entrée/détail | `ClientCache` |
| `get_players` | `query_url` requis : chaîne non vide ou référence | Un objet de résultat de lecteur | `ClientCache` |

`page` doit être un entier littéral, pas une chaîne ni une référence dynamique. Ne
placez pas `page` sur la recherche : la recherche s'exécute toujours sur la page 1
avec des filtres de type de média/thème vides.
Les URL et les identifiants de requête internes sont passés comme des chaînes
définies par la source ; le format de test n'exige pas que toutes les entrées
`query_url` soient des URL HTTP.

L'adaptateur choisit la source à partir du contrat. Pour `get_category`, il construit
le descripteur typé `{name: source, query_url: resolved_value}` et passe des
paramètres de source supplémentaires vides. Les appels de section, de détail et de
lecteur ciblent également la même source. N'ajoutez pas `source`, `name`, `channel`,
`source_params`, d'en-têtes de requête, d'identifiants, de filtres de média ni de
paramètres arbitraires aux `inputs`. Si l'opération de production exige une entrée
non prise en charge, signalez la limitation au lieu d'inventer une clé ou d'ajouter
une exception de fournisseur en Rust.

Les assertions examinent la `data` publique sérialisée, **pas** l'enveloppe, les
`ScraperDataNode.values/children/items` internes ni une réponse reconstruite. Ne
commencez pas les chemins par `data`. Les réponses vectorielles sont séparées en
objets racines individuels ; les réponses singleton restent un seul objet racine.
La recherche conserve son objet de groupe public : utilisez `items_path: [entries]`,
et non `[]`, pour compter les résultats de recherche.

Chaque enveloppe de façade doit être exempte d'erreur, même si elle contient des
données partielles utiles. Un champ `source` retourné doit identifier la source
sélectionnée. Les métadonnées doivent identifier exactement une source sélectionnée
et annoncer les capacités chargées. Une source sans rapport ou un résultat de repli
ne peut pas satisfaire le contrat.

## 8. Syntaxe des chemins : clés et parcours explicite des tableaux

Un chemin est une liste YAML de chaînes, pas du JSONPath ni une chaîne délimitée par
des barres obliques.

| Chemin | Signification |
| --- | --- |
| `[]` | Chaque objet racine public lui-même |
| `[entries]` | Éléments du tableau `entries` de la racine |
| `[categories]` | Éléments du tableau `categories` de la racine |
| `[sections, "*", entries]` | Entrées de toutes les sections de la racine |
| `[sections, "*", subsections, "*", entries]` | Entrées des sous-sections explicitement imbriquées |
| `["img/poster", link]` | Clé littérale `img/poster`, puis clé `link` |
| `[request, query_url]` | Clé `query_url` dans l'objet `request` |

Les règles de parcours sont exactes :

1. Commencez à chaque racine publique.
2. Un segment littéral lit une clé d'objet. Il ne traverse jamais implicitement un
   tableau.
3. `"*"` développe un niveau de tableau. Mettez-le entre guillemets pour éviter la
   syntaxe d'alias YAML.
4. À l'emplacement final `items_path` uniquement, développez un tableau une fois en
   ses éléments. Une valeur finale non tableau est comptée comme un élément
   sélectionné.
5. Une clé manquante contribue pour zéro valeur sélectionnée. Une valeur
   intermédiaire existante du mauvais type est une erreur de forme, même si
   d'autres branches fournissent assez de données.
6. Les éléments sélectionnés doivent être des objets pour être valides. Les
   scalaires, les valeurs nulles et les éléments de tableaux imbriqués comptent
   quand même dans `total`, mais sont des objets invalides.

Exemple de charge utile publique :

```json
{
  "sections": [
    {"entries": [{"title": "First", "link": "entry-1"}]},
    {"entries": [{"title": "Second", "link": "entry-2"}]}
  ]
}
```

`items_path: [sections, "*", entries]` sélectionne deux objets. Leurs chemins de
champs obligatoires `[title]` et `[link]` sont relatifs à chaque entrée, pas à la
racine. `[sections, entries]` est incorrect car `sections` est un tableau.

`/`, `>`, les points et les crochets à l'intérieur d'une clé littérale ne sont pas
des séparateurs. Un chemin vide est autorisé ; un segment vide ou composé uniquement
d'espaces ne l'est pas. Seul `items_path` autorise les jokers. Les chemins des
champs obligatoires, optionnels et des valeurs d'entrée utilisent uniquement des
clés littérales. Il n'y a ni joker récursif, ni indexation numérique de tableau, ni
expression JSONPath, ni descente automatique dans les sous-sections.

## 9. Entrées : littéraux et dépendances

### Entrées littérales

Un terme de recherche doit être délibéré et propre à la source :

```yaml
inputs:
  query: "journal"
```

Une entrée peut utiliser une référence maintenue au lieu de la découverte :

```yaml
inputs:
  query_url: "https://example.invalid/reference-entry"
```

Cette dernière URL est illustrative et doit être remplacée par une référence
vérifiée pour la source. N'interprétez pas une référence disparue comme un test
réussi. Les entrées littérales doivent être des chaînes concrètes : l'exécuteur de
test n'étend pas lui-même les espaces réservés de production ni la syntaxe des
variables d'environnement dans le document de test. Ne supposez pas que
`{base_url}`, `${VARIABLE}` ou un champ YAML `value` seront résolus comme entrée de
référence.

### Entrée dynamique : les quatre clés de référence sont obligatoires

```yaml
inputs:
  query_url:
    from_step: home
    items_path: [categories]
    select: first_valid
    value_path: [request, query_url]
```

| Clé de référence | Règle exacte |
| --- | --- |
| `from_step` | ID d'une étape apparaissant plus tôt dans ce même fichier |
| `items_path` | Doit être exactement égal au `expect.items_path` de l'étape amont |
| `select` | Exactement `first_valid` ; aucune autre stratégie n'existe |
| `value_path` | Chemin littéral relatif à un objet amont sélectionné ; doit se résoudre à l'exécution en une chaîne JSON non vide |

Les dépendances manquantes, avant, sur soi-même et cycliques sont invalides avant
tout appel réseau. N'utilisez pas un nom d'opération de production comme
`from_step` sauf si c'est aussi l'ID réel de l'étape amont.

`first_valid` parcourt les objets amont sélectionnés dans l'ordre de la réponse et
choisit le premier objet satisfaisant toutes les assertions amont par objet **et**
possédant la chaîne non vide demandée. Il ne choisit pas simplement le premier
élément. Il n'essaie pas un autre candidat après l'échec de la requête aval. L'étape
amont entière doit avoir réussi ; sinon ses dépendantes sont bloquées, même si
certaines données étaient disponibles. Une étape amont réussie sans valeur d'entrée
utilisable bloque aussi le consommateur et signale le chemin de valeur manquant.

### Choisir une recette de navigation à partir de la sortie réelle

| Forme de la source | Étapes et chemins d'éléments | `value_path` de l'entrée suivante |
| --- | --- | --- |
| L'accueil contient directement des entrées | `home: [sections, "*", entries]` → `entry: []` | `[link]` depuis l'accueil |
| L'accueil contient des catégories avec du contenu derrière | `home: [categories]` → `category: [sections, "*", entries]` → `entry: []` | `[request, query_url]` depuis l'accueil, puis `[link]` depuis la catégorie |
| La catégorie contient des sections différées | `home: [categories]` → `category: [sections]` → `section: [entries]` → `entry: []` | `[request, query_url]`, puis `[link]`, puis `[link]` |
| L'accueil contient des sections différées | `home: [sections]` → `section: [entries]` → `entry: []` | `[link]` depuis l'accueil, puis `[link]` depuis la section |
| Des sous-sections imbriquées explicites contiennent des entrées | `home: [sections, "*", subsections, "*", entries]` → `entry: []` | `[link]` depuis l'accueil |

Ce sont des recettes, pas des garanties universelles de fournisseur. Si un chemin
sélectionné est absent ou si un identifiant interne a une signification différente,
utilisez la forme réelle de la source. Une étape `load_home` dont le chemin
d'éléments commence par `categories` doit avoir une étape de contenu dépendante,
directement ou transitivement. Les descripteurs de navigation statiques seuls ne
démontrent pas une extraction de contenu utile. L'exemple M6 montre la route
catégorie/section différée sans aucune branche Rust propre au fournisseur.

## 10. Assertions et décomptes de données utiles

Chaque `expect` accepte exactement ces champs :

| Clé | Requise ? | Type, valeur par défaut et règle |
| --- | --- | --- |
| `items_path` | Oui | Liste de segments de chaînes ; peut être `[]` ; jokers explicites autorisés |
| `min_count` | Oui | Entier positif ; zéro est invalide |
| `min_valid_count` | Oui | Entier positif, pas supérieur à `min_count` |
| `required_non_empty` | Oui | Liste non vide de chemins de champs littéraux distincts |
| `optional_fields` | Non | Liste de mappings décrits ci-dessous ; vaut `[]` par défaut |
| `all_valid` | Non | Booléen, `false` par défaut |

Les valeurs de départ recommandées sont `min_count: 1`, `min_valid_count: 1` et
`all_valid` omis. N'exigez jamais une taille de catalogue en direct exacte, un ordre
éditorial stable ou le texte complet d'un titre sans besoin précis et justifié.

### Champs obligatoires

```yaml
expect:
  items_path: [entries]
  min_count: 1
  min_valid_count: 1
  required_non_empty:
    - [title]
    - [link]
```

Une valeur obligatoire est invalide lorsqu'elle est manquante, nulle, une chaîne
vide ou composée uniquement d'espaces, un tableau vide ou un objet vide. Le zéro
numérique et le booléen faux sont non vides. Cette assertion vérifie la
présence/non-vacuité ; elle n'exige **pas** une chaîne.

Au moins un chemin obligatoire doit être non vide et différent de `[source]` et
`[id]`. Pour les métadonnées, `[title]` est utile. Pour une entrée, `[title]` peut
être le minimum. Pour la navigation, exigez le lien ou l'entrée de requête réel
suivant. L'identité racine seule ne doit jamais servir de preuve d'extraction de
contenu. `[]` peut en outre vérifier l'objet sélectionné lui-même, mais ne satisfait
pas cette exigence de champ utile.

### Champs optionnels et types

```yaml
optional_fields:
  - path: [description]
    type: string
  - path: ["img/poster", link]
    type: string
    non_empty: true
```

Chaque mapping de champ optionnel exige `path` et `type`. `non_empty` est un
booléen optionnel valant `false` par défaut. Les types autorisés sont exactement
`string`, `number`, `boolean`, `object` et `array`. Il n'y a ni coercition de type,
ni type nullable, ni union.

Une clé optionnelle absente est valide. Une valeur intermédiaire existante du
mauvais type n'est pas une absence : par exemple `img/poster: null` ne peut pas
fournir un `link` imbriqué. Lorsque le champ terminal existe, il doit correspondre
au type déclaré, y compris lorsque sa valeur est nulle :

| Valeur publique | `type: string` optionnel | Avec aussi `non_empty: true` |
| --- | --- | --- |
| Clé absente | Valide | Valide |
| `null` | Invalide | Invalide |
| `""` ou `"   "` | Valide | Invalide |
| `"A description"` | Valide | Valide |
| `42`, `false`, `[]`, `{}` | Invalide | Invalide |

Cette distinction est importante : la recherche qualifiée de RTL Play a retourné
`description: null` pour deux éléments, donc sa contrainte de chaîne optionnelle a
échoué. Ne supprimez pas la contrainte, ne convertissez pas null en absence et
n'affaiblissez pas le schéma uniquement pour masquer un tel échec. Signalez si
l'attente, la sortie publique ou la sémantique prise en charge nécessite une
correction justifiée séparément.

Un chemin ne peut pas être dupliqué dans les champs obligatoires, dans les champs
optionnels, ni entre les deux. Ne déclarez pas le même chemin comme obligatoire et
optionnel pour ajouter une vérification de type obligatoire : le schéma v1 n'a pas
d'assertion de type obligatoire distincte. Les nouveaux champs non déclarés ne font
pas échouer une vérification. Les champs optionnels n'ont pas besoin d'apparaître
uniformément sur les lignes.

### Décompte et succès

Chaque élément sélectionné contribue une fois à `total`. Un objet contribue à
`valid` uniquement si toutes les assertions obligatoires et optionnelles présentes
réussissent. Tout le reste contribue à `invalid`.

Une étape réussit uniquement lorsque toutes ces conditions sont remplies :

1. Il n'y a aucune erreur d'enveloppe ni de forme.
2. `total >= min_count`.
3. `valid >= min_valid_count`.
4. Si `all_valid: true`, `invalid == 0`.

Avec des minima de un, trois éléments contenant un objet valide peuvent réussir
lorsque `all_valid` est faux ; les deux objets invalides restent visibles dans les
diagnostics. Avec zéro élément extrait, une étape échoue toujours. Les formes
intermédiaires incorrectes restent des échecs même si une autre branche fournit
assez d'objets.

## 11. Limites et prérequis optionnels

Omettez les deux sections lorsqu'elles ne sont pas nécessaires. Pour les définir
explicitement :

```yaml
limits:
  step_timeout_seconds: 60
  source_timeout_seconds: 300
prerequisites:
  - env: EXAMPLE_ACCESS_READY
    reason: "This source requires an independently configured access prerequisite"
```

Le nom d'environnement et la raison ci-dessus sont des exemples, pas un mécanisme
d'authentification intégré. Remplacez-les par un vrai prérequis uniquement si
nécessaire.

| Limite | Valeur par défaut | Valeurs autorisées |
| --- | --- | --- |
| Nombre d'étapes | Aucune étape implicite ajoutée | 1 à 32, métadonnées comprises |
| `step_timeout_seconds` | 60 | Entier de 1 à 120 |
| `source_timeout_seconds` | 300 | Entier compris entre le délai d'étape et 900 |

Chaque prérequis possède exactement `env` et `reason`, tous deux des chaînes non
vides. Les noms d'environnement ne peuvent pas se répéter. L'exécuteur exige une
valeur Unicode présente et non vide, mais ne journalise jamais cette valeur. Un
prérequis n'injecte pas d'identifiants, ne configure pas de proxy, n'effectue pas de
connexion et ne prouve pas que l'accès fonctionne. Les prérequis manquants échouent
comme `prerequisite_missing` ; ils ne sont jamais comptés comme une récupération
réussie.

L'exécution est séquentielle. Au plus 32 appels de façade de scénario s'exécutent
par source ; les assertions de métadonnées préservées vérifient en outre les
sélections d'accueil vides/inconnues sans envoyer de requêtes au fournisseur. Le
contrat ne plafonne pas le fan-out HTTP interne, les redirections ni les nouvelles
tentatives de transport. Il n'y a aucune option de nouvelle tentative YAML.
L'arrêt du cache a une limite distincte de cinq secondes et peut prolonger le
nettoyage au-delà d'une échéance de requête ou de source. L'annulation d'une étape
ne garantit pas l'arrêt immédiat d'un travail de transport déjà détaché.

## 12. Surcharges de diagnostic des entrées

Les tests Rust lisent directement ces variables d'environnement. Supprimez les
surcharges de diagnostic de recherche/détail lors de la qualification du contrat de
référence inchangé.

| Variable | Effet |
| --- | --- |
| `ARACHNEA_TEST_QUERY_SOURCE` | Sélecteur de source : non défini ou `*` sélectionne tout ; un ID ou un chemin relatif à `services` sélectionne une source. Les vérifications de métadonnées appliquent la sélection « tout » uniquement aux contrats de fixture existants |
| `ARACHNEA_TEST_SEARCH_TERM` | Remplace `query` pour les étapes de recherche exécutées |
| `ARACHNEA_TEST_ENTRY_URL` | Remplace `query_url` pour les étapes d'entrée/détail exécutées |

Les valeurs de diagnostic vides ou non Unicode sont invalides. Une surcharge sans
opération exécutée correspondante est aussi invalide. Les exécutions filtrées
évaluent la pertinence après résolution de la sélection effective des étapes. Les
surcharges sont étiquetées comme diagnostiques et utilisent des empreintes de
référence masquées dans la sortie.

Une référence remplacée supprime uniquement la dépendance de cette entrée. Ainsi,
une vérification d'entrée filtrée avec une URL explicite peut s'exécuter sans ses
étapes de découverte. Une exécution complète de service exécute toujours ses autres
étapes déclarées. Les surcharges ne lèvent jamais les assertions d'enveloppe ou de
champ, ne changent jamais la source et ne modifient pas le fichier de contrat.

## 13. Échecs de lecture et rapport honnête de la couverture

| Résultat | Signification et action suivante |
| --- | --- |
| `passed` | L'opération de cette étape et les assertions déclarées ont réussi ; inspectez les étapes restantes de la source et les lacunes de couverture |
| `invalid_configuration` | Corrigez le YAML, l'identité, les entrées, les capacités, les références ou les limites avant d'interpréter un résultat de santé de source |
| `coverage_missing` | Un contrat manquant rend ce service `not_tested` ; les autres services continuent. Les opérations supplémentaires non couvertes n'invalident pas les étapes déclarées réussies |
| `not_tested` | Résultat au niveau de la source pour un contrat absent ; aucun appel pour cette source, la raison étant conservée dans le résumé final |
| `prerequisite_missing` | Fournissez la configuration d'accès requise indépendamment ; aucun succès de récupération n'est revendiqué |
| `retrieval_failed` | Échec de transport typé ou délai d'expiration ; conservez la cause contextuelle |
| `extraction_failed` | Aucune erreur de forme, mais moins d'éléments que `min_count` ; examinez la référence, l'accès et l'extraction sans supposer une cause |
| `contract_failed` | L'enveloppe, la forme ou les assertions d'objet ont échoué ; inspectez l'erreur, les chemins invalides et les types JSON observés |
| `blocked` | L'étape n'a pas été appelée car son entrée n'a pas pu être découverte, une étape amont a échoué ou l'échéance de la source a expiré |
| `not_applicable` | L'opération est explicitement absente de la production et n'est pas exécutée ; zéro appel n'est pas un contrôle en direct réussi |

Les étapes indépendantes continuent après un échec dans la limite de l'échéance de
la source. Les dépendantes d'une étape échouée sont bloquées, même lorsque des
données amont partielles existent. La source et l'agrégat échouent en cas d'échec
d'étapes attendues, d'étapes bloquées ou de prérequis manquants.
Une exécution complète de source sans étape de contenu réussie ne peut pas être
verte.

L'enveloppe publique ne conserve pas toutes les causes de transport typées. Une
erreur d'enveloppe backend opaque reste `contract_failed` ; ne la reclassez pas en
devinant à partir d'une charge utile vide. Par exemple, l'entrée qualifiée de LN24 a
conservé une erreur de route proxy belge malgré un repli direct ; l'exécuteur a
rejeté l'enveloppe au lieu de revendiquer une régression d'extraction ou une
vérification de détail réussie.

La sortie inclut la source/l'étape, l'opération, les durées, les chemins, les
minima, les décomptes, les chemins invalides, les types JSON observés, les
exclusions et les capacités non couvertes. `partial_coverage` est vrai pour les
exécutions filtrées par opération, les capacités de production non couvertes ou les
sources qui n'ont pas pu être préparées (y compris les contrats manquants). Les
métadonnées filtrées peuvent réussir leurs propres assertions sans qualifier la
santé complète de la source.
L'empreinte de référence et l'origine de l'URL sont rapportées sans identifiants,
chemins d'URL, chaînes de requête, fragments ni corps de réponse. N'inventez pas de
statut HTTP, d'URL effective ni de décomptes de charge utile affirmés lorsque la
façade ne les expose pas.

### Résumé final par source

Après la préparation et l'exécution, l'exécuteur liste toujours les résultats par
source pour les services découverts ayant atteint l'exécuteur commun :

| Résultat de la source | Signification |
| --- | --- |
| `passed` | Les vérifications de la source sélectionnée ont réussi |
| `failed` | Configuration invalide de la source, échec de prérequis/d'exécution/d'assertion, ou panique Rust interceptée |
| `not_tested` | Aucun contrat indépendant n'existe pour cette source ; le message explique le contrat manquant |
| `not_applicable` | Une opération filtrée n'est explicitement pas attendue ; aucune vérification en direct réussie n'est revendiquée |

`source_check_summary` rapporte `selected`, `attempted`, `passed`, `failed`,
`not_tested`, `not_applicable` et `partial_coverage`. `attempted` compte les sources
dont la sélection d'opérations non vide a atteint l'exécution, à l'exclusion des
échecs de préparation et des sélections non applicables. `source_check_result`
liste ensuite le résultat et la raison de chaque service. Les résultats de source et
les résultats d'étape sont distincts.

Après le résumé lisible par machine, l'exécuteur imprime un tableau aligné
`Source check results` regroupant chaque ligne source/opération enregistrée :
source, opération, statut (`Passed`, `Skip`, `Blocked`, `Failed`), sévérité
(`Ok`, `Warning`, `Error`) et un message pour les lignes non réussies. `passed`
correspond à `Passed`/`Ok` ; `coverage_missing` et `not_tested` correspondent à
`Skip`/`Warning` : l'opération ou la source existe en production, mais son test
n'est pas implémenté. Cela inclut les opérations de production non prises en
charge par le schéma v1. Les étapes bloquées correspondent à `Blocked`/`Error` ;
les autres échecs et erreurs de configuration/préflight à `Failed`/`Error`.
Les opérations explicitement absentes via `capabilities.not_expected` sont
vérifiées comme absentes en production, jamais appelées et exclues des lignes du
tableau et des décomptes de sévérité. Leur liste reste informative dans les
diagnostics ; une opération absente explicitement demandée conserve son résultat
`not_applicable` au niveau de la source. Les libellés de
sévérité utilisent des couleurs ANSI uniquement sur un terminal interactif et sont
désactivés par `NO_COLOR` (ou `CLICOLOR=0`). Le tableau est suivi du nombre total de
lignes et des décomptes `Ok`/`Warning`/`Error`, puis d'une ligne vide.

Une erreur par source n'arrête pas les services suivants. Les paniques Rust
récupérables pendant la préparation/l'exécution d'une source sont enregistrées comme
des échecs et l'état de cette source est abandonné. Les contrats manquants/invalides
ne provoquent jamais de requêtes fabriquées pour cette source. L'agrégat reste non
réussi si une source a échoué ou n'a pas été testée ; son assertion finale a lieu
**après** le résumé et les exécutions des sources restantes.
Les erreurs de configuration partagée, telles qu'un manifeste de premier niveau
illisible ou une surcharge globale invalide, peuvent encore empêcher l'exécution
avant que l'exécution par source ne commence.

## 14. Garanties de cache et de portée

La façade contient exactement la source Stream sélectionnée, avec des stockages
d'activation/session/application en mémoire isolés et un nouveau fichier
d'identifiants temporaire. Les surcharges de paramètres du manifeste s'appliquent
lorsqu'elles sont enregistrées ; un YAML analysé non enregistré utilise ses propres
valeurs par défaut. Le test n'importe ni les identifiants ni les préférences de
l'utilisateur.

Chaque opération de scénario reçoit un contexte de requête neuf sans
`If-None-Match` client et un répertoire de cache de données vide neuf. Les handles du
cache serveur se ferment avant le nettoyage. La réutilisation de session/cookie au
sein d'une exécution de source est distincte de la réutilisation de données
extraites. Les diagnostics montrent des ID de cache distincts, une utilisation
mémoire initiale nulle, l'utilisation mémoire finale et le statut de validation de
la façade. La qualification du 29 octobre 2026 a observé des caches neufs et
distincts à travers des exécutions répétées ; voir le rapport lié ci-dessous.

Il n'y a pas de bascule de cache dans le YAML de test. Ne supprimez pas les caches
ou sessions de l'application, ne définissez pas un indicateur fictif `no_cache` et
ne modifiez pas les API de façade de production pour rédiger un contrat. Ces
vérifications ne prouvent pas le contournement du cache CDN, la lecture vidéo
complète, la validité DRM/licence ni la disponibilité future de chaque segment.

Les fonctionnalités non prises en charge du schéma v1 incluent les
scripts/sélecteurs/transformations arbitraires, les conditionnels, les boucles, les
nouvelles tentatives, les dépendances entre sources, les replis vers une référence
alternative, l'indexation numérique de chemin, JSONPath, les assertions de
valeur constante/regex, les types nullables/union, les paramètres d'opération
arbitraires, les saisons, la lecture en direct et les opérations de résolution.
Signalez séparément un besoin réel d'extension générique ; n'ajoutez pas de YAML non
pris en charge ni de raccourci Rust propre à une source.

## 15. Liste de contrôle finale pour l'auteur ou l'agent IA

- [ ] Seuls les contrats de service demandés et les mises à jour requises de
  documentation/suivi ont été créés/modifiés ; la production et Rust sont restés
  intacts sauf autorisation.
- [ ] Le chemin reflète `source_config` ; l'`id` de production, la `source` et la
  racine du nom de fichier correspondent exactement.
- [ ] `schema_version` est l'entier `1` ; tous les mappings/listes obligatoires sont
  présents.
- [ ] Chaque opération nommée est prise en charge par le schéma v1. Les opérations
  attendues existent en production ; les opérations non attendues sont réellement
  absentes.
- [ ] Chaque opération attendue a une étape, les métadonnées sont incluses et une
  vraie opération de contenu peut établir une exécution complète réussie.
- [ ] Les ID d'étape sont uniques ; les références pointent vers l'arrière et
  copient exactement le `expect.items_path` amont.
- [ ] Les chemins utilisent des segments de chaînes littéraux et des jokers
  explicites entre guillemets. Les chemins de valeur et de champ n'ont pas de joker
  et n'incluent pas la clé `data` de l'enveloppe.
- [ ] Chaque attente a des minima positifs et un champ obligatoire utile au-delà de
  `source`/`id` ; les décomptes n'imposent pas une taille de catalogue volatile.
- [ ] Les champs optionnels utilisent des types JSON publics vérifiés ; les valeurs
  absentes et nulles sont traitées selon les règles documentées. Aucun chemin de
  champ n'est déclaré deux fois.
- [ ] Les entrées sont des valeurs de référence délibérées ou des références de
  découverte bornées ; les surcharges d'environnement non pertinentes sont
  supprimées pour la qualification.
- [ ] La commande de métadonnées propre à la source a réellement sélectionné et
  exécuté son test.
- [ ] La commande en direct pour un service a été exécutée si l'accès était
  disponible ; sinon les étapes non exécutées et la raison sont explicitement
  rapportées.
- [ ] Les échecs, objets invalides, limites d'accès et opérations non couvertes
  restent visibles ; aucun succès ni couverture complète n'est inventé.

## 16. Références de code et qualification historique

Lorsque la documentation et le comportement semblent incohérents, inspectez ces
implémentations avant de proposer un changement. Ne modifiez pas silencieusement le
format pendant la rédaction d'un fichier.

| Responsabilité | Fichier |
| --- | --- |
| Points d'entrée des tests Rust existants | [stream_scraper.rs](../../server/crates/arachnea-stream/tests/stream_scraper.rs) |
| Découverte, sélection, chargement des fixtures reflétées et configuration isolée | [stream_scraper.rs](../../server/crates/arachnea-stream/tests/support/stream_scraper.rs) |
| Schéma strict du contrat et validation des dépendances | [source_checks.rs](../../server/crates/arachnea-stream/tests/support/source_checks.rs) |
| Parcours JSON et assertions | [assertions.rs](../../server/crates/arachnea-scrapyfy/tests/support/scraper_manager/assertions.rs) |
| Lecteur YAML strict | [yaml.rs](../../server/crates/arachnea-scrapyfy/tests/support/scraper_manager/yaml.rs) |
| Sélection des opérations, entrées, délais d'expiration et rapport | [runner.rs](../../server/crates/arachnea-stream/tests/support/source_checks/runner.rs) |
| Appels de façade typés et vérifications d'enveloppe | [execution.rs](../../server/crates/arachnea-stream/tests/support/source_checks/execution.rs) |
| Version de schéma et bornes de l'exécuteur | [lib.rs](../../server/crates/arachnea-stream/src/lib.rs) |
