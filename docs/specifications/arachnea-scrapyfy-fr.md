# Spécification du format YAML arachnea-scrapyfy

Ce document est la référence commune pour rédiger les YAML de services. Il se lit
avec la spécification du groupe de services à créer :

| Responsabilité | Spécification |
|---|---|
| Catalogues de streaming, navigation et descripteurs de lecteurs | [arachnea-stream](arachnea-stream-fr.md) |
| Reconnaissance des hébergeurs vidéo externes et extraction des flux | [arachnea-stream-hoster](arachnea-stream-hoster-fr.md) |
| Fournisseurs de listes de proxys | [arachnea-proxies](arachnea-proxies-fr.md) |
| Fournisseurs de pays pour une IP et pour la connexion sortante | [arachnea-ip-countries](arachnea-ip-countries-fr.md) |
| Contrats de contrôle des sources stream | [Contrôles des sources](arachnea-stream-source-checks-fr.md) |

Les spécifications commune et spécialisée constituent la référence de rédaction.
Aucune recherche dans le code source n'est nécessaire. Les identifiants et noms
YAML sont sensibles à la casse. Copier les noms exactement, y compris les
orthographes historiques `get_attribut` et `origine`. Les exemples contenant
`...` ou des alternatives séparées par `|` sont des schémas de syntaxe, pas des
fichiers complets. Les exemples complets sont explicitement indiqués.

## Choisir une stratégie d'extraction

Examiner les URL cibles et des réponses représentatives avant de rédiger les
règles. Identifier les requêtes qui fournissent le contenu attendu, ainsi que les
liens ou identifiants reliant les étapes de navigation. Préférer une API JSON
stable ou des données intégrées aux sélecteurs CSS positionnels si elles exposent
les mêmes informations nécessaires.

| Contenu observé | Stratégie YAML |
|---|---|
| Métadonnées fixes ou reconnaissance d'une chaîne d'entrée | `scraper_type: static` |
| JSON retourné par un point d'entrée | `scraper_type: json` avec un `row_pointer` adapté |
| HTML contenant le `__NEXT_DATA__` de Next.js | `scraper_type: json` avec `extract_next_data: true` |
| Contenu dans les éléments ou attributs HTML | `scraper_type: html` avec sélecteurs CSS et actions |
| Enregistrements de texte brut délimité | `scraper_type: text` avec délimiteurs de lignes et, si nécessaire, de champs |
| Données supplémentaires derrière des liens ou points d'entrée | Sous-requête HTML/JSON documentée à un emplacement pris en charge |

Implémenter les capacités demandées et effectivement exposées par les réponses de
la source. `search` et `get_entry` sont des opérations de catalogue, pas un socle
pour les hébergeurs ou fournisseurs de proxys et de pays. Utiliser les noms
d'opérations, entrées d'exécution et contrats de sortie du groupe choisi.

Avant de conclure que le YAML ne suffit pas, vérifier les en-têtes, GET/POST et
la construction du corps en section 5, les types d'extraction en sections 6–9,
l'exécution navigateur en section 10, les actions en section 11 et les
post-traitements en section 12. Une authentification ou une protection anti-bot
ne prouve pas à elle seule qu'une nouvelle primitive est nécessaire. Déclarer
`credentials` décrit le besoin de compte ; cela n'implémente pas la connexion.
L'exécution navigateur dépend du support et de la configuration documentés ;
elle ne garantit pas l'accès à toute source protégée. Identifier précisément
l'opération manquante si les capacités existantes ne permettent pas d'exprimer
le parcours observé.

Les YAML de services existants sont des exemples facultatifs propres aux
fournisseurs, pas un remplacement de cette spécification. Réutiliser seulement
les modèles pertinents et les vérifier avec la réponse actuelle de la cible.
Les guides de groupes indiquent des exemples utiles ; leur lecture n'est pas
nécessaire pour comprendre le schéma ou le contrat de sortie.

## Procédure de rédaction

1. Choisir le groupe de services et lire son contrat d'entrée et de sortie.
2. Choisir un `id` de collection unique dans le groupe. Le nom du fichier peut
   différer de cet ID ; les appelants utilisent l'ID. Choisir des noms de requêtes
   uniques dans le fichier.
3. Déclarer les paramètres de type chaîne dans l'ordre de dépendance. Placer les
   blocs partagés sous `shared` et les réutiliser avec les ancres et alias YAML.
   `shared` est une convention de réutilisation YAML, pas une requête exécutable
   ni un champ de sortie.
4. Choisir `static`, `html`, `json` ou `text` pour chaque requête. Définir les
   champs obligatoires indiqués ci-dessous ; ne pas copier aveuglément les options
   d'un autre type de scraper.
5. Extraire une réponse représentative. Utiliser `type: object[]` pour les objets
   répétés et `type: string[]` pour les scalaires répétés. Définir explicitement
   `select: first` pour un scalaire si plusieurs éléments peuvent correspondre.
6. Appliquer les actions dans l'ordre, puis les sous-requêtes et post-traitements
   nécessaires. Vérifier le JSON final selon le contrat du groupe, y compris
   les résultats vides.
7. Inscrire le fichier dans `services.json` ; un fichier non inscrit n'est pas
   rendu disponible par sa seule présence. Choisir son activation délibérément.
8. Valider le chargement et lancer les contrôles existants pertinents. Pour les
   contrôles de sources stream, suivre la spécification dédiée. Un chargement
   réussi ne prouve pas qu'une API distante, un sélecteur ou une URL vidéo fonctionne.

## Valider un service

Vérifier séparément les niveaux suivants et indiquer ceux réellement exécutés :

| Niveau | Ce qu'il établit | Ce qu'il ne prouve pas |
|---|---|---|
| Chargement YAML et conversion sémantique | La configuration est acceptée par le chargeur | Que chaque clé a un effet ou que les données distantes sont extractibles |
| Inscription et activation du manifeste | Les chemins, ID, surcharges et activations voulues sélectionnent le service | La disponibilité distante |
| Contrat JSON extrait | Les champs, types et cardinalités respectent l'opération | La navigation complète ou la lecture média |
| Métadonnées statiques, si présentes | La requête s'exécute et retourne le contenu attendu | Le fonctionnement réel du catalogue, de l'hébergeur ou du fournisseur |
| Opération en ligne | La requête et l'extraction observées fonctionnent pour l'entrée vérifiée | Les autres entrées, pages suivantes ou la lecture si elles ne sont pas vérifiées |
| Lecture, pour les services vidéo | Le média vérifié et les ressources nécessaires sont accessibles au lecteur | La disponibilité future du fournisseur |

Inclure des réponses représentatives remplies et vides, des valeurs facultatives
absentes, ainsi que les continuations ou requêtes différées utilisées par
l'opération. Un chargement réussi peut ignorer une clé YAML inconnue ; comparer
les clés au schéma en plus de valider le JSON. Distinguer les erreurs réseau,
d'authentification et de contrat. Indiquer comme non vérifié tout contrôle non
exécuté.

Pour les catalogues de streaming, suivre [le guide des contrôles de sources](arachnea-stream-source-checks-fr.md)
pour les contrats, métadonnées et commandes en ligne. Ses opérations de catalogue
ne constituent pas un contrat de test d'hébergeur ou de fournisseur. Pour les
hébergeurs, vérifier reconnaissance, flux extraits et lecture selon le guide
hoster. Pour les fournisseurs de proxys et de pays, vérifier leurs propres
entrées, lignes aplaties et sorties de pays. Utiliser les contrôles existants
pertinents ; l'accès, les identifiants ou le réseau peuvent laisser le comportement
en ligne non vérifié. Une réponse de production échouée ne justifie pas d'affaiblir
le contrat attendu sans établir que ce contrat est lui-même incorrect.

## 1. Structure générale du fichier

Un fichier YAML décrit une **collection de requêtes** pour une source de données. La structure racine est :

```yaml
id: mon-identifiant-source
title: "Titre optionnel"
logo: "https://example.com/logo.png"
description:
  fr: "Description en français"
  en: "Description in English"
parameters:
  - name: base_url
    value: "https://example.com"
    description: "URL de base de la source"
http:
  mode: auto
  user_agent_profile: chrome
proxy_insecure_tls_hosts:
  - media.example.invalid
queries:
  - name: ma_query
    scraper_type: html
    # ... champs spécifiques au type
```
Le champ de collection optionnel proxy_insecure_tls_hosts autorise une exception TLS explicite pour des hôtes HTTPS exacts. La vérification reste activée par défaut : le proxy normalise la liste côté serveur, la contrôle à chaque requête et après chaque redirection, et refuse tout autre hôte. Ne l’utiliser que pour un défaut temporaire connu du fournisseur, puis retirer l’entrée après renouvellement du certificat.

---

## 2. Champs racine

### `id` (obligatoire)
Identifiant unique de la source. Utilisé comme clé de recherche.
Alias YAML : `name` (interchangeable avec `id`).

```yaml
id: m6play-fr
```

### `title` (optionnel)
Titre lisible de la source. Si absent, la valeur de `id` est utilisée.

```yaml
title: "M6 Play France"
```

### `logo` (optionnel)
URL ou chemin vers le logo de la source.

```yaml
logo: "https://example.com/logo.png"
```

### `description` (optionnel)
Carte multilingue de descriptions, clé = code langue, valeur = texte.

```yaml
description:
  fr: "Service de streaming M6"
  en: "M6 streaming service"
```

### `credentials` (optionnel)
Déclare si l'utilisation du service requiert un compte et, facultativement,
l'URL de création du compte. Sans ce bloc, aucun identifiant n'est requis.

```yaml
credentials:
  required: true
  signup_url: "https://example.org/register"
```

| Champ | Type | Obligatoire | Description |
|-------|------|-------------|-------------|
| `required` | booléen | non | Vaut `false` par défaut ; le service requiert des identifiants lorsqu'il vaut `true`. |
| `signup_url` | chaîne | non | URL publique d'inscription affichée par les clients d'administration. |

### `parameters` (optionnel)

La clé racine accepte aussi l’alias de compatibilité `parametres`.
Liste de paramètres par défaut de la collection. Chaque paramètre a :

| Champ | Type | Obligatoire | Description |
|-------|------|-------------|-------------|
| `name` | string | oui | Nom utilisé comme `{placeholder}` dans les templates |
| `value` | string | oui | Valeur par défaut (peut contenir des `{placeholders}`) |
| `description` | string | non | Description du paramètre |
| `actions` | array | non | Actions appliquées à la valeur résolue |

Les paramètres sont résolus séquentiellement dans l'ordre de déclaration YAML. Un paramètre peut référencer les paramètres précédents via `{placeholder}`.

Les placeholders non déclarés sont préservés au chargement de la collection afin d'être fournis par l'appelant à l'exécution. Un placeholder requis mais absent provoque alors une erreur contextuelle lors de la résolution de la requête, et non au chargement du YAML.

Le préfixe `@` est réservé aux variables dynamiques produites pendant l'extraction d'une requête. Ces variables ne sont pas des paramètres de collection et se référencent avec la syntaxe `{@nom}`. Les paramètres standards ne doivent pas utiliser de nom commençant par `@`.

Pour les entrées textuelles, le chargement parcourt les entrées imbriquées sans réécrire ni valider les paramètres des actions. Ces paramètres sont résolus à l’exécution des actions ; les options communes des requêtes conservent leur résolution au niveau de la collection pendant le chargement.

```yaml
parameters:
  - name: base_url
    value: "https://www.m6play.fr"
  - name: query_url
    value: "{base_url}/api/search"
```

**Ajouts automatiques** : Les paramètres suivants sont automatiquement injectés lors de l'exécution :
- `source` — identifiant de la collection (`id`)
- `service_id` — identifiant de la collection
- `service_title` — titre de la collection
- `service_logo` — logo de la collection
- `service_description` — description sérialisée en JSON
- `page_index` — dérivé de `page` (`page - 1`)
- `offset` — dérivé de `(page - 1) * page_size` lorsque les deux valeurs sont positives
- `query_separator` — `?`, `&` ou `""` selon l'URL
- `url_fragment` — partie après `#` dans le `query_url` résolu, définie uniquement quand l'URL porte un fragment non vide (elle n'est jamais envoyée au serveur)

---

## 3. Configuration HTTP (`http`)

Applicable au niveau collection, requête et sous-requête. La configuration de la collection est héritée par chaque requête, et celle de la requête par ses sous-requêtes.

```yaml
http:
  mode: auto                    # auto | direct | cloudflare_smart | cloudflare_browser
  user_agent_profile: chrome    # chrome | chrome_stable | firefox | firefox_stable
  user_agent: "Custom UA"       # optionnel : surcharge complète du User-Agent
  proxy_countries: ["US", "CA"] # optionnel : pays proxy ordonnés
  proxy_domain_group: ["m6.fr", "6play.fr", "6cloud.fr", "bedrock.tech"] # groupe optionnel de domaines
  proxy_affinity: "opaque-session-id" # optionnel : conserve le même proxy admissible
  proxy_rejection_statuses: [403] # optionnel : rejet par destination ; reprises selon le budget partagé
  max_redirects: 16             # optionnel : nombre max de redirections (défaut global: 16)
```
`proxy_countries` est normalisé en codes ISO alpha-2 ordonnés et en majuscules,
sans doublon. Chaque élément de `proxy_countries` accepte l'interpolation de
template runtime. Une liste JSON runtime peut être développée avec
`http.proxy_countries: ["{proxy_countries}"]` et combinée avec des entrées
statiques. Un pays unique doit aussi employer une liste à un élément, par exemple
`http.proxy_countries: ["US"]`. Le champ singulier `proxy_country` n'est pas
accepté.

`proxy_domain_group` est une liste statique optionnelle de noms de domaines.
Le proxy normalise la casse, IDNA et le point final, ignore les entrées invalides
et déduplique les noms. Le groupe ne s’applique qu’à ses domaines et sous-domaines :
un proxy accepté par un autre membre admissible fournit un candidat par défaut
avant la recherche hiérarchique et l’exploration générale. Préférences exactes et
affinité explicite admissible restent prioritaires. Pays, capacités, exclusions de
cible, callbacks et budgets restent applicables. Une liste enfant vide hérite du
parent ; une liste non vide le remplace. Ce paramètre n’impose ni une IP commune ni
un changement du routage des URL média retournées ; il est sans effet sans routage
proxy dynamique.

`proxy_affinity` est un identifiant opaque limité au processus. Les requêtes qui
partagent cet identifiant réutilisent le même proxy dynamique tant qu'il reste
admissible pour le pays, le protocole et la destination. L'identifiant n'expose
pas l'adresse du proxy et sa liaison expire après 15 minutes d'inactivité.

`proxy_rejection_statuses` contient les statuts de réponse de l'origine qui
invalident le proxy dynamique actuellement sélectionné pour cette destination.
Les reprises suivent l’échéance partagée et la politique de rejeu décrites ci-dessous. La liste est vide par défaut et reste sans effet sans route proxy
dynamique intégrée au processus. Le rejet est persisté dans l'inventaire des
proxys pendant 24 heures et reste limité au triplet exact protocole, hôte et
port. Après une réponse acceptée (2xx par défaut), le proxy sélectionné devient le candidat
prioritaire pour cette destination exacte tout en restant soumis à tous les
contrôles d'éligibilité ; chaque destination conserve une préférence distincte.

Avec la feature `arachnea-proxy`, `http.proxy_response_classifier` sélectionne
une politique connue et remplace la liste de rejet, sans composition implicite :

```yaml
http:
  proxy_response_classifier:
    type: statuses
    rejected_statuses: [403]
    accepted_statuses: [200, 206]
    cooldown_seconds: 86400
```

Les types sont `default` (2xx acceptés), `http403` (403 rejeté, autres réponses
selon la politique prudente) et `statuses`. Les statuts doivent appartenir à
100..599, les listes acceptées/rejetées être disjointes et le cooldown explicite
être strictement positif et représentable. Sans `accepted_statuses`, tous les
2xx sont acceptés ; une liste vide n’accepte rien. Sans cooldown, la valeur de
l’inventaire s’applique. Une exclusion active n’est jamais raccourcie.
La configuration enfant remplace celle du parent ; `type: default` permet une
réinitialisation explicite. La priorité est handler injecté par code, politique
connue, liste historique, puis politique prudente. Les 4xx/5xx non classifiés et
les redirections restent inconclusifs : ni exclusion ni nouveau succès.

La classification utilise les en-têtes originaux avant transformation et le jeton
de la destination réellement observée, sans valider les origines précédentes
d’une redirection. Les handlers de statuts ne lisent aucun corps. Un handler
injecté peut demander au maximum 64 Kio, avec une lecture limitée à trois
secondes et au timeout restant ; les octets sont restitués. Un extrait limité,
indisponible ou insuffisant ne démontre pas l’absence d’un marqueur. Les erreurs
du handler sont journalisées et inconclusives. Les moteurs sans observation
précoce corrélée ne produisent pas de préférence ou d’exclusion par déduction.
La réponse HTTP publique reste matérialisée. Les appels HTTP géolocalisés
partagent une cible d'acceptation de 120 secondes, avec au plus 15 secondes pour
terminer une tentative métier déjà engagée. Toutes les couches de reprise partagent
la même échéance, sans plafond de nombre de précontrôles, validations ou points
d'entrée métier distincts ; la concurrence reste bornée. Aucun nouveau travail
ne commence pendant le délai supplémentaire. La collecte du corps accepté utilise
le délai HTTP ordinaire distinct (25 secondes dans Scrapyfy).

GET/HEAD peuvent réessayer après une erreur de transport pertinente ou un rejet
configuré. Une requête modificatrice dont l'envoi est incertain n'est jamais
rejouée par défaut, même après un rejet configuré. `proxy_retry_replayable: true`
garantit explicitement que l'opération et son corps sont rejouables ; cette option
hérite normalement et `false` remplace le parent. Une erreur prouvée avant envoi
peut permettre une reprise sans ce drapeau. Un résultat inconclusif ou une erreur
de collecte du corps ne fait pas changer de proxy. Les moteurs navigateur ou
personnalisés sans corrélation précoce refusent le mode de recherche HTTP
géolocalisée à portée limitée ; l'exécution explicite de page reste distincte.
Les appels Rquest contextuels conservent les préférences par destination et
peuvent réutiliser des transports inactifs compatibles entre clients dérivés.
Chaque appel resélectionne sa route et réassocie exclusivement le transport à son
nouveau jeton et à son échéance. Le pool garde au plus deux transports inactifs
par route et 32 globalement, et écarte à l'accès ceux inactifs depuis 60 secondes.
Seules les tentatives complètes acceptées ou les redirections suivies explicitement
autorisées sont conservées. Cookies et routages mutables restent indépendants.
La connexion dépend du serveur ; les reconnexions utilisent le contexte courant.
Les budgets et règles de rejeu restent inchangés ; aucune nouvelle option YAML.

### Héritage HTTP et valeurs par défaut

Une option facultative absente hérite de son parent. Un scalaire explicite
remplace le parent, y compris `proxy_retry_replayable: false`. Les listes
`proxy_countries`, `proxy_domain_group`, `proxy_rejection_statuses` et
`empty_on_statuses` héritent lorsqu'elles sont absentes **ou vides** ; une liste
enfant non vide remplace celle du parent. `[]` ne permet pas de vider ces listes
héritées. `proxy_response_classifier: {type: default}` réinitialise une politique
héritée. Les objets de configuration du navigateur remplacent entièrement
l'objet parent correspondant.

Sans mode explicite, la couche HTTP utilise `auto`. Sans profil explicite, elle
utilise Chrome ; un `user_agent` personnalisé est prioritaire. Les requêtes HTTP
ordinaires utilisent un délai de 25 secondes ; la limite de redirections par
défaut est 16. Il n'existe pas de champ HTTP de scraper YAML `timeout`,
`timeout_ms` ou `proxy_country`. L'option `exec_js.timeout_ms` appartient à cette
action, pas aux requêtes HTTP.

### Modes HTTP (`mode`)

| Valeur | Description |
|--------|-------------|
| `auto` | Mode par défaut, choix automatique |
| `direct` | Requête directe sans contournement |
| `cloudflare_smart` | Tente de contourner Cloudflare intelligemment |
| `cloudflare_browser` | Utilise un navigateur pour contourner Cloudflare |

### Profils User-Agent (`user_agent_profile`)

| Valeur | Description |
|--------|-------------|
| `chrome` | Profile Chrome standard |
| `chrome_stable` | Profile Chrome stable |
| `firefox` | Profile Firefox standard (identique à `firefox_stable` en pratique) |
| `firefox_stable` | Profile Firefox stable |

---

## 4. Type de scraper : `scraper_type`

Chaque requête dans `queries` doit définir `scraper_type` avec l'une des valeurs :
- `html` — extraction depuis du HTML avec des sélecteurs CSS
- `json` — extraction depuis du JSON avec des pointeurs JSON
- `static` — données déclarées statiquement dans le YAML (pas de requête HTTP)
- `text` — extraction depuis du texte brut découpé par délimiteurs

---

## 5. Structure commune aux scrapers HTTP (`html`, `json`, `text`)

Les scrapers `html`, `json` et `text` partagent ces champs via `ScraperQueryCommon` :

```yaml
name: "identifiant-requete"          # obligatoire
scraper_type: html|json|text          # obligatoire
base_url: "https://example.com"       # obligatoire : URL de base, disponible comme {base_url}
media_types:                          # optionnel : types de contenu produits
  - movie
  - series
query_url: "/api/data"                # obligatoire : template d'URL
request_url_actions: []                 # optionnel : actions après résolution de l'URL
request_method: get                   # optionnel : get (défaut) | post
request_body_pointer: "/data/payload" # optionnel : pointeur JSON pour le corps POST
request_body_select: first            # optionnel : first | all (défaut: all)
request_body_actions: []              # optionnel : actions sur le corps avant envoi
request_headers:                      # optionnel : en-têtes HTTP
  - name: "Authorization"
    pointer: "/auth/token"
    select: first
    actions: []
http:                                # optionnel : configuration HTTP (surcharge)
  empty_on_statuses: []              # statuts HTTP traités comme une réponse vide
query_param_mappings: []              # optionnel : mappings de paramètres
result_item_field: "entries"          # optionnel : champ groupe à aplatir en lignes
pre_process: []                       # optionnel : transformations du corps avant parsing et hash ETag de fallback
post_process: []                      # optionnel : post-traitements
```

### `http.empty_on_statuses`

`http.empty_on_statuses` contient les codes HTTP qui représentent une absence de
résultat valide plutôt qu'un échec de requête. Le corps retourné est alors
ignoré et remplacé par une réponse vide adaptée au scraper : `[]` pour `json`,
ou une chaîne vide pour `html` et `text`. Par défaut, la liste est vide et les
statuts non configurés conservent le comportement normal, notamment les erreurs
de parsing. Comme les autres options `http`, ce champ est hérité de la collection
vers les requêtes et sous-requêtes, sauf surcharge locale.

```yaml
http:
  empty_on_statuses: [404]
```

### `pre_process`

Le pré-traitement s'applique au corps final retourné par une requête HTTP,
après les redirections et avant le parsing HTML, JSON ou texte. Il est aussi
appliqué avant le hash de contenu de fallback utilisé pour la validation
conditionnelle. Les validateurs distants `ETag` et `Last-Modified` restent
prioritaires lorsqu'ils sont disponibles. Sinon, le corps transformé produit
le fragment `C:` ; une correspondance arrête le parsing et toutes les
sous-requêtes dépendantes. Chaque requête racine et sous-requête possède sa
propre liste, sans héritage vers les enfants. Il ne s'applique pas à
`input_html`. Les valeurs `start` et `end` peuvent utiliser `{base_url}`, résolu
depuis le paramètre à l'exécution lorsqu'il est présent, ou depuis l'URL de base
de la requête sinon.

```yaml
pre_process:
  - type: remove_text_blocks
    start: '<script data-volatile="true">'
    end: '</script>'
```

`remove_text_blocks` retire tous les blocs littéraux, délimiteurs inclus.
L'absence de `start` ne modifie pas le contenu. Un `start` trouvé sans `end`
suivant produit une erreur à l'exécution. Les deux délimiteurs doivent être
non vides.

### `request_headers`

Chaque en-tête est défini par :

| Champ | Type | Description |
|-------|------|-------------|
| `name` | string | Nom de l'en-tête (ex: `Authorization`) |
| `pointer` | string (opt.) | Pointeur JSON pour extraire la valeur du contexte |
| `select` | enum | `first` (défaut) ou `all` (valeurs jointes avec ", ") |
| `actions` | array | Actions appliquées à la valeur résolue |

```yaml
request_headers:
  - name: "Authorization"
    pointer: "/data/token"
    select: first
    actions:
      - type: format_text
        argument: "Bearer {}"
```

### `query_param_mappings`

Transforme les paramètres source en paramètres cible via une table de traduction :

| Champ | Type | Description |
|-------|------|-------------|
| `target_param` | string | Nom du paramètre de sortie |
| `source_param` | string | Nom du paramètre d'entrée (valeurs séparées par virgules) |
| `values` | map | Table de correspondance : clé source → valeur cible. `{}` = valeur encodée URL |
| `item_suffix` | string | Suffixe ajouté à chaque élément traduit |

```yaml
query_param_mappings:
  - target_param: "category_ids"
    source_param: "categories"
    values:
      movie: "cat_01"
      series: "cat_02"
    item_suffix: ","
```

### `result_item_field`

Quand une requête produit un groupe dont les items doivent devenir les lignes de résultat, ce champ indique le nom du groupe à aplatir :

```yaml
result_item_field: "entries"
```

Avec ce réglage, si la requête produit `{ entries: [ { id: 1 }, { id: 2 } ] }`, le résultat sera `[ { id: 1 }, { id: 2 } ]`.

---

## 6. Scraper HTML (`scraper_type: html`)

### Champs spécifiques

```yaml
name: recherche
scraper_type: html
row_selector: "div.result-item"     # obligatoire : sélecteur CSS pour chaque ligne
row_concurrency: 4                  # optionnel : nombre max de lignes traitées en parallèle (défaut: 4)
input_html: "{html}"                # optionnel : modèle résolu en HTML, contourne la requête HTTP
entries: []                         # obligatoire : extracteurs de champs
```

Quand `input_html` est défini, le modèle est résolu avec les paramètres
d'exécution (typiquement `{html}`) et le HTML obtenu est utilisé comme corps
de réponse sans effectuer de requête HTTP. `query_url` reste utilisé comme
URL de contexte pour les actions, en-têtes et messages d'erreur. Lorsque le
modèle se résout en une chaîne vide, la récupération HTTP normale est utilisée.
Un `{html}` absent est une erreur de modèle ; déclarer un paramètre de collection
`html` avec `value: ""` si la même requête doit accepter du HTML fourni et un GET.

`input_html` nécessite au moins un `query_url` ou un paramètre d'exécution
`{url}` pour la résolution de l'URL de contexte.

`row_selector` est résolu comme un modèle avec les paramètres d'exécution avant
l'extraction des lignes, ce qui permet à une valeur fournie par l'appelant de
restreindre l'extraction. C'est le rôle de `{url_fragment}` : un lien tel que
`https://example.com/page.html#ep3` sélectionne les lignes de cet onglet ou de
ce bloc alors que la source n'expose aucune URL pour celui-ci. Le sélecteur doit
rester analysable tel quel (par exemple
`div[class~="{url_fragment}vs"]`) : la configuration est validée au chargement,
et un sélecteur dont les placeholders restent non résolus ne correspond à
aucune ligne.

### Entrées HTML (`entries`)

Chaque entrée peut être un **champ** (feuille) ou un **groupe** (contenant des sous-entrées).

```yaml
entries:
  # Champ simple
  - name: title
    type: string
    selector: "h2 a"                # sélecteur CSS optionnel
    select: first                   # first | all (défaut)
    actions:                        # optionnel : pipeline d'actions
      - type: get_text

  # Groupe
  - name: images
    type: object[]                   # object | object[] (défaut: object[])
    selector: "img.poster"
    select: all
    entries:                         # sous-entrées du groupe
      - name: url
        type: string
        actions:
          - type: get_attribut
            argument: src
          - type: resolve_url
            proxy: true
```

#### `post_build` des groupes

Un groupe objet HTML peut déclarer des transformations ordonnées exécutées après
l'extraction de tous ses champs enfants.

##### `math_formula`

Calcule un champ enfant numérique depuis les champs frères scalaires numériques
du même objet.

| Champ | Type | Description |
|-------|------|-------------|
| `target` | string | Chemin enfant séparé par `>` recevant le nombre calculé |
| `expression` | string | Expression mathématique dont les placeholders `{chemin}` lisent les champs frères scalaires |

Les expressions sont analysées par le moteur mathématique déterministe intégré.
Tous les placeholders doivent résoudre vers des champs frères numériques à
l'exécution, sinon la cible est omise. L'expression est validée au chargement
du YAML.

##### `remove_fields`

Retire des champs frères temporaires après leur consommation par une
transformation post-build précédente.

| Champ | Type | Description |
|-------|------|-------------|
| `fields` | string[] | Chemins enfants séparés par `>` à supprimer |

```yaml
- name: storyboard
  type: object
  select: first
  entries:
    - name: count_per_image
      type: number
      actions: [ ... ]
    - name: count_per_row
      type: number
      actions: [ ... ]
  post_build:
    - type: math_formula
      target: rows
      expression: "{count_per_image} / {count_per_row}"
    - type: remove_fields
      fields: [count_per_image, count_per_row]
```

Si une entrée de formule est absente ou non numérique à l'exécution, sa cible est omise.

#### Règles de validation des entrées HTML :

- Un champ doit définir `actions` ou `sub_queries` (ou les deux), mais pas `entries`.
- Un groupe doit définir `entries` mais pas `actions`.
- `post_build` est pris en charge uniquement par les groupes HTML, jamais par les champs feuilles.
- `type` est obligatoire pour les deux.
- Pour un groupe :
  - `type: object` avec `select: first` → un objet unique
  - `type: object` avec `select: all` → tableau d'objets
  - `type: object[]` → tableau d'objets (incompatible avec `select: first`)
- Les noms supportent les chemins hiérarchiques avec `>` :
  ```yaml
  - name: parent>enfant>champ
    type: string
  ```

### `select`

| Valeur | Description |
|--------|-------------|
| `first` (défaut pour certains contextes) | Seulement le premier élément correspondant |
| `all` (défaut général) | Tous les éléments correspondants |

---

## 7. Scraper JSON (`scraper_type: json`)

### Champs spécifiques

```yaml
name: recherche-api
scraper_type: json
row_pointer: "/data/results/*"           # obligatoire : pointeur JSON pour chaque ligne
extract_next_data: false                 # optionnel : parser le HTML comme Next.js __NEXT_DATA__
filters:                                 # optionnel : filtres racine
  status: ["active", "published"]
sibling_sub_query_concurrency: 4         # optionnel : parallélisme sous-requêtes (défaut: 4)
sub_query_context_concurrency: 4         # optionnel : parallélisme contextes (défaut: 4)
sub_query_fetch_concurrency: 8           # optionnel : parallélisme requêtes HTTP (défaut: 8)
entries: []                              # obligatoire : extracteurs de champs
sub_queries: []                          # optionnel : sous-requêtes
```

### Syntaxe des pointeurs JSON

Le système de pointeurs JSON supporte :

| Syntaxe | Description |
|---------|-------------|
| `/data/title` | Chemin simple dans un objet |
| `/data/*` | Tous les éléments d'un tableau ou toutes les valeurs d'un objet |
| `/data/0` | Index numérique dans un tableau |
| `/data/*[role=mea]` | Filtre : éléments du tableau dont `role` = `"mea"` |
| `/data/*[content/0/idType=8]` | Filtre avec chemin imbriqué : éléments dont `content[0].idType` = `8` |
| `/data/0[status=active]` | Filtre sur un index spécifique |
| `/data/*/external_key` | Descend dans tous les sous-éléments |

```yaml
# Exemple : filtrer par role=cover
row_pointer: "/images/*[role=cover]"

# Exemple : filtrer par un champ imbriqué dans un objet du tableau
row_pointer: "/*[content/0/idType=8]"
```

### Entrées JSON (`entries`)

Chaque entrée peut être un **champ** ou un **groupe**.

```yaml
entries:
  # Champ simple
  - name: title
    type: string
    pointer: "/title"                    # pointeur JSON optionnel
    select: first                        # first | all
    actions:
      - type: get_text

  # Groupe
  - name: episodes
    type: object[]
    pointer: "/episodes/*"
    select: all
    entries:
      - name: id
        type: string
        pointer: "/id"
      - name: title
        type: string
        pointer: "/title"
```

**Contrainte** : En JSON, l'entrée `type` est **obligatoire** pour tous les champs et groupes.

**Pointeur absent** : Si `pointer` n'est pas défini, l'extraction opère sur la ligne elle-même.

---

## 8. Scraper Static (`scraper_type: static`)

Ne fait **aucune requête HTTP**. Les données sont déclarées dans le YAML et les templates `{placeholders}` sont résolus au moment de l'exécution.

### Champs spécifiques

```yaml
name: metadata
scraper_type: static
media_types:                            # optionnel
  - metadata
result_item_field: "entries"            # optionnel : groupe à aplatir en lignes
entries: []                             # obligatoire : entrées statiques
```

`result_item_field` est aussi pris en charge par les requêtes statiques. Il
aplatit les objets du groupe déclaré en lignes de résultat, comme pour les
requêtes HTML, JSON et texte.

### Entrées statiques (`entries`)

Chaque entrée peut avoir :

| Champ | Type | Description |
|-------|------|-------------|
| `name` | string | Nom du champ (supporte `>` pour chemins hiérarchiques) |
| `type` | string | Type de sortie JSON (`string`, `number`, `boolean`, `object`, `object[]`) |
| `value` | YAML value | Valeur scalaire ou structure (template `{placeholder}` résolu) |
| `actions` | array | Actions appliquées après résolution du template |
| `items` | array | Liste d'objets statiques (template `{placeholder}` dans chaque valeur) |

**Règles** :
- `type` est obligatoire
- `value` et `items` sont mutuellement exclusifs
- Si `items` est présent, `type` doit être `object` ou `object[]`
- Les champs `value` et les valeurs dans `items` sont rendus via les paramètres d'exécution

```yaml
entries:
  # Valeur scalaire avec template
  - name: logo
    type: string
    value: "{base_url}/logo.png"
    actions:
      - type: resolve_url
        proxy: true

  # Valeur objet
  - name: platform
    type: object
    value:
      name: "M6 Play"
      url: "{base_url}"

  # Liste d'items statiques
  - name: quick_links
    type: object[]
    items:
      - label: "Accueil"
        url: "{base_url}/home"
      - label: "Recherche"
        url: "{base_url}/search"
```

---

## 9. Scraper Text (`scraper_type: text`)

Extrait des données d'un corps de réponse texte en découpant par lignes (`row_delimiter`) puis en champs (`field_delimiter`).

### Champs spécifiques

```yaml
name: fichiers-csv
scraper_type: text
row_delimiter: "\n"                    # obligatoire : délimiteur de lignes
field_delimiter: "|"                   # optionnel : délimiteur de champs
entries: []                            # obligatoire : extracteurs de champs
```

### Entrées text (`entries`)

| Champ | Type | Description |
|-------|------|-------------|
| `name` | string | Nom du champ (supporte `>` pour chemins hiérarchiques) |
| `type` | string | Type de sortie JSON (obligatoire pour les champs) |
| `field` | int (opt.) | Index 1-based du champ après split par `field_delimiter` |
| `actions` | array | Pipeline d'actions appliquées à la valeur extraite |
| `entries` | array | Sous-entrées (groupe) |

```yaml
row_delimiter: "\n"
field_delimiter: "|"
entries:
  - name: id
    type: number
    field: 1
  - name: title
    type: string
    field: 2
  - name: duration
    type: number
    field: 3
    actions:
      - type: normalize_duration
```

Avec une ligne comme `42|Mon Film|1h30`, cela produit :
```json
{ "id": 42, "title": "Mon Film", "duration": 5400 }
```

---

## 10. Sous-requêtes (`sub_queries`)

Les sous-requêtes permettent d'enchaîner des requêtes HTTP supplémentaires à partir des valeurs extraites. Elles sont disponibles dans les scrapers `html` et `json`. Une sous-requête attachée à un champ dans un groupe d'objets s'exécute séparément pour chaque objet extrait : ses templates utilisent les champs de cet objet et son résultat remplace uniquement l'URL source de ce champ.

Les entrées HTML et JSON acceptent les sous-requêtes. Seules les requêtes racines
JSON exposent une liste sœur `sub_queries` ; une requête racine HTML doit attacher
les requêtes supplémentaires à ses entrées. Les sous-requêtes HTML/JSON imbriquées
peuvent à leur tour déclarer `sub_queries`.

### Structure commune (`SubQueryCommon`)

```yaml
sub_queries:
  - scraper_type: html|json            # obligatoire : type de la sous-requête
    # Champs communs :
    context_pointer: "/data/items/*"   # optionnel : pointeur pour les lignes de contexte
    context_select: all                # optionnel : first | all
    filters:                           # optionnel : filtres sur le contexte
      type: ["movie", "series"]
    row_filters:                       # optionnel : filtres sur les lignes récupérées
      lang: ["fr", "en"]
    target: "parent>details"           # optionnel : chemin de destination dans le résultat
    request_pointer: "/url"            # optionnel : pointeur pour l'URL de la sous-requête
    request_select: first              # optionnel : first | all
    request_actions: []                # optionnel : actions sur l'URL avant requête
    request_method: get                # optionnel : get | post
    request_headers: []                # optionnel : en-têtes de la sous-requête
    http: {}                           # optionnel : config HTTP
    pre_process: []                    # optionnel : transformations du corps avant parsing
    post_process: []                   # optionnel : post-traitements
    # Champs spécifiques selon scraper_type
```

### `target`

Chemin où les résultats de la sous-requête sont imbriqués. Format : `"parent>enfant"` (séparé par `>`).
Si absent, les résultats sont fusionnés au niveau racine.

```yaml
target: "details>episodes"
```

### Requête dans une page navigateur

Les requêtes et sous-requêtes HTML peuvent utiliser une page navigateur
conservée au lieu du transport HTTP normal. `page_navigate` navigue une requête
HTML racine et retourne son HTML stable. `page_click` navigue, clique un élément
et retourne le HTML rendu lorsqu'un sélecteur configuré apparaît. `page_fetch`
navigue puis exécute un `fetch()` JavaScript authentifié pour une sous-requête
HTML. `browser_context: origin` réutilise la même session navigateur lorsque
l'origine, le profil navigateur et la route proxy correspondent.
`browser_context: isolated` est réservé aux sous-requêtes d'entrée `page_click` :
il ouvre une page par valeur d'entrée et exécute jusqu'à quatre clics indépendants
en parallèle. Les templates de l'URL de
page, des en-têtes et du corps utilisent le contexte de paramètres de la
sous-requête ; les champs d'entrée sont aussi disponibles avec les caractères
non alphanumériques remplacés par `_`.

```yaml
http:
  execution: page_navigate
  browser_context: origin
```

`page_navigate` est réservé au HTML, ne prend en charge que `GET` et utilise
l'URL de la requête comme URL de navigation. Il exige
`browser_context: origin`.

```yaml
http:
  execution: page_click
  browser_context: origin
  page_url: "{episode_url}"
  browser_click:
    selector: '.player-list .lien[onclick*="''{player_field}''"]'
    wait_for_selector: "#videoIframe iframe[src]"
```

`page_click` est réservé aux sous-requêtes HTML. Il invoque l'élément de page
sélectionné : le JavaScript de la page garde alors la maîtrise du rendu CAPTCHA,
des callbacks et des requêtes de même origine. Il retourne ensuite le HTML de
la page lorsque `wait_for_selector` apparaît. `browser_click.selector` et
`wait_for_selector` sont obligatoires, utilisent des sélecteurs CSS et prennent
en charge les templates de sous-requête. Sur une sous-requête d'entrée, utiliser
`browser_context: isolated` lorsque chaque clic est indépendant et doit disposer
d'une page dédiée ; l'ordre des résultats reste celui des valeurs d'entrée. Les
variables de template dynamiques ne doivent pas servir à communiquer entre les
clics isolés concurrents.

```yaml
http:
  mode: auto
  execution: page_fetch
  browser_context: origin
  page_url: "{episode_url}"
  browser_token:
    source: turnstile_callback
    placeholder: "{browser_turnstile_token}"
    cache_scope: domain
    retry_on_rejection: once
    rejection_statuses: [403]
    rejection_body_markers: ["captcha invalid"]
```

`browser_context` doit valoir `origin`. `page_fetch` est réservé aux sous-requêtes
HTML. Le jeton reste en mémoire et n'est jamais exposé aux données extraites ni
aux diagnostics. `cache_scope: domain` réutilise explicitement le jeton seulement
pour la même origine, le même profil navigateur et la même route proxy ; sans ce
champ, le comportement reste à usage unique. `retry_on_rejection: once` réalise
au maximum une nouvelle navigation et soumission après un signal de rejet configuré.

`http.execution` est facultatif et utilise par défaut le chemin HTTP direct.
Avec `page_navigate`, `http.browser_context: origin` est obligatoire. Avec
`page_click`, `http.browser_context: origin` ou `isolated`, `http.page_url` et
`http.browser_click` sont obligatoires. Avec `page_fetch`,
`http.browser_context: origin` et `http.page_url` sont obligatoires. `page_url`,
les en-têtes et les valeurs du corps de requête sont résolus comme templates de
sous-requête avant l'envoi de la requête navigateur.

Scrapyfy ferme la page navigateur retenue pour l'origine après l'exécution de
toutes les sous-requêtes de chaque query racine. Les sessions de page navigateur
sont donc limitées à une query et ne demandent aucune option YAML de cycle de
vie.

`browser_token` est facultatif. S'il est présent, `source: turnstile_callback`
et un `placeholder` littéral dans le corps de la requête sont obligatoires ;
`turnstile_callback` est la seule source prise en charge. `cache_scope` peut
être absent ou valoir `domain`. `retry_on_rejection` vaut `never` par défaut et
accepte `never` ou `once`. `rejection_statuses` et
`rejection_body_markers` valent par défaut des listes vides ; ils classent une
réponse comme rejet de jeton uniquement lorsqu'un jeton a été soumis.

### Sous-requête HTML

```yaml
  - scraper_type: html
    row_selector: "div.episode"
    entries:
      - name: title
        type: string
        selector: "h3"
        actions:
          - type: get_text
    context_entries:                   # optionnel : extrait aussi du contexte parent
      - name: season_label
        type: string
        select: first
        actions:
          - type: format_text
            argument: "{season_label}"
```

Les `context_entries` HTML suivent le schéma d'entrée HTML (`selector`,
`actions`), pas la syntaxe JSON `pointer` ; elles sont ajoutées avant les entrées
HTML de la réponse. Utiliser les paramètres d'exécution pour les valeurs du
contexte de sous-requête. Les `context_entries` JSON suivent le schéma JSON et
lisent la ligne de contexte parente avant récupération.

### Sous-requête JSON

```yaml
  - scraper_type: json
    row_pointer: "/data/*"
    entries:
      - name: id
        type: string
        pointer: "/id"
    request_body_pointer: "/payload"   # optionnel : corps POST depuis le contexte
    request_body_select: first
    request_body_actions: []
    extract_next_data: false           # optionnel : parser Next.js
```

### Sous-requêtes d'entrée (`sub_queries` sur une `entry`)

Les entrées HTML et JSON peuvent aussi porter des sous-requêtes. La syntaxe est identique :

Par défaut, le résultat d'une sous-requête d'entrée remplace ou enrichit le
champ qui l'a déclenchée. Définir `target: parent` conserve ce champ et fusionne
les champs extraits dans son objet parent. C'est utile lorsqu'un lien déclenche
un fetch d'enrichissement qui doit produire des champs frères, par exemple
`web-link` et `players` à côté de `link`.

```yaml
entries:
  - name: url
    type: string
    selector: "a"
    actions:
      - type: get_attribut
        argument: href
    sub_queries:
      - scraper_type: html
        row_selector: "div.detail"
        entries:
          - name: description
            type: string
            selector: "p.desc"
            actions:
              - type: get_text
```

---

## 11. Actions (`actions`)

Les actions sont un pipeline de transformations appliquées aux valeurs extraites. Elles sont taggées par `type` en snake_case.

### Variables dynamiques `@...`
Certaines actions peuvent produire des variables dynamiques partagées pendant l'exécution d'une requête. Ces variables utilisent obligatoirement le préfixe réservé `@` et se référencent avec la syntaxe `{@nom}`.

Règles :

- Les paramètres standards de collection ne doivent pas commencer par `@`.
- Une variable dynamique ajoutée par action doit commencer par `@` et contenir un nom après ce préfixe.
- `extract_variables` refuse les doublons par défaut avec `on_duplicate: error`.
- `on_duplicate: replace` peut être utilisé explicitement quand le même bloc de variables request-scoped est relu plusieurs fois.
- Les variables dynamiques ne sont pas résolues automatiquement dans toutes les actions. Utiliser `replace_variables` à l'endroit exact où les placeholders dynamiques doivent être remplacés.
- `replace_variables` résout uniquement les variables dynamiques connues ; les placeholders standards et inconnus restent inchangés.

### `get_text`
Lit le contenu textuel concaténé de l'élément HTML sélectionné.

```yaml
- type: get_text
```

### `html_to_text`
Convertit le contenu HTML en texte brut via `quick_html2md`. Les valeurs déjà
en texte brut et sans balise HTML sont conservées après normalisation des espaces.

```yaml
- type: html_to_text
```

### `get_html`
Ajoute le HTML de l'élément sélectionné à la liste des valeurs.

```yaml
- type: get_html
```

### `get_attribut`
Lit un attribut HTML (ex: `href`, `src`, `content`).

```yaml
- type: get_attribut
  argument: "href"
```

### `split`
Divise chaque valeur par le séparateur et remplace la liste par les fragments.

```yaml
- type: split
  argument: ","
```

### `map`
Réécrit les valeurs via une table de correspondance.

| Champ | Type | Description |
|-------|------|-------------|
| `argument` | mapping | Table clé → valeur. Une clé `null` correspond à une entrée nulle |
| `default` | scalar (opt.) | Valeur de repli quand la clé est absente. `{}` = valeur originale |

```yaml
- type: map
  argument:
    movie: "film"
    series: "série"
  default: "autre"
```

Les valeurs mappées à `null` sont supprimées de la liste de sortie.

### `regex_find_all`
Applique une regex et remplace la liste par les groupes capturés.

| Champ | Type | Description |
|-------|------|-------------|
| `pattern` | string | Expression régulière |
| `format` | string | Template de sortie : `{1}` = groupe 1, `{request_url}`, `{base_url}`, etc. |

```yaml
- type: regex_find_all
  pattern: "id=(\\d+)"
  format: "{1}"
```

### `extract_variables`
Extrait des variables dynamiques depuis les valeurs courantes et les ajoute au contexte de la requête. La liste de valeurs courantes est conservée inchangée.

| Champ | Type | Description |
|-------|------|-------------|
| `pattern` ou `regex` | string | Expression régulière appliquée à chaque valeur |
| `name` | string | Template du nom de variable, avec captures `{1}`, `{2}`, etc. Le résultat doit commencer par `@` |
| `value` | string | Template de la valeur, avec captures `{1}`, `{2}`, etc. |
| `on_duplicate` | string | Politique de doublon : `error` par défaut ou `replace` |

Les variables extraites sont disponibles comme placeholders dynamiques `{@nom}` pour les actions qui résolvent explicitement les variables dynamiques. Un doublon `@...` est une erreur d'exécution avec `on_duplicate: error` ; `replace` met à jour la valeur dynamique existante tout en protégeant les paramètres standards. Les correspondances déjà traitées ne sont pas annulées si une correspondance suivante échoue.

```yaml
- type: extract_variables
  regex: "\\b([A-Za-z][A-Za-z0-9_]*)\\s*=\\s*(\\d+)\\s*;"
  name: "@{1}"
  value: "{2}"
  on_duplicate: replace
```

### `replace_variables`
Remplace explicitement les placeholders dynamiques dans les valeurs courantes. Cette action ne résout pas les paramètres standards et ne modifie pas les placeholders inconnus.

| Champ | Type | Description |
|-------|------|-------------|
| `variable_prefix` | string | Préfixe de variables à résoudre. Seul `@` est supporté actuellement. Défaut : `@` |

```yaml
- type: replace_variables
  variable_prefix: "@"
```

Exemple : si `@PortPart` vaut `80`, la valeur `"({@PortPart}+1)"` devient `"(80+1)"`. Les placeholders comme `{country}` ou `{@Missing}` restent inchangés.

### `get_request_url`
Ajoute l'URL actuelle de la requête à la liste des valeurs.

```yaml
- type: get_request_url
```

### `get_response_body`
Ajoute le corps brut de la réponse HTTP à la liste des valeurs.

```yaml
- type: get_response_body
```

### `suffix`
Ajoute un suffixe constant à chaque valeur.

```yaml
- type: suffix
  argument: ".jpg"
```

### `max`
Garde seulement la plus grande valeur entière positive.

```yaml
- type: max
```

### `resolve_url`
Résout chaque valeur comme une URL relative par rapport à l'URL de la page.

| Champ | Type | Description |
|-------|------|-------------|
| `proxy` | bool | Si true, enveloppe les URLs HTTP(S) via le proxy public |
| `proxy_headers` | object | En-têtes HTTP intégrés à l'URL proxy et envoyés en amont ; exige `proxy: true` ; les valeurs supportent les placeholders de requête, `{request_url}` et `{request_origin}` |
| `proxy_replace_all` | object[] | Règles `ReplaceAll` ordonnées jointes à l'URL proxy ; exige `proxy: true` |
| `proxy_follow_redirects` | bool ou entier positif | `true` suit les redirections amont avec la limite globale par défaut (16) ; un entier positif fixe le maximum ; exige `proxy: true` |

```yaml
- type: resolve_url
  proxy: true
  proxy_headers:
    Referer: "{request_origin}/"
  proxy_follow_redirects: true
  proxy_replace_all:
    - pattern: '(?m)^(https?://[^\r\n]+)'
      replacement: '{proxy}/$1'
      content_types: [text/vtt]
```

Les valeurs de `proxy_headers` supportent les placeholders nommés de requête, `{request_url}` et `{request_origin}`. Chaque règle contient `pattern` (regex obligatoire), `replacement` (template obligatoire avec
captures `$1`, `$2`, etc. et variables proxy comme `{proxy}` et `{proxy_inherited}`) et `content_types` (liste optionnelle
de MIME types). Le proxy applique les règles au corps textuel dans l'ordre déclaré.

`{proxy}` se résout en chemin proxy public (ex. `/api/proxy`). `{proxy_inherited}` se résout en ce même chemin mais
inclut le segment `opts_…` de l'URL de requête courante lorsqu'il est présent (ex. `/api/proxy/opts_ABCD`), ce qui
permet aux règles ReplaceAll de préserver les options proxy héritées dans les URLs réécrites.

`proxy_follow_redirects` est opt-in. Le proxy suit chaque hop après lecture de ses seuls headers, avant de relayer le corps du hop terminal. `false` (ou l’absence de la clé) conserve le comportement 3xx ; `0` est invalide.

L'encodage des options de redirection dans les URL proxy générées nécessite la
feature Cargo `arachnea-proxy`. Le helper interne de conversion JSON est compilé
avec cette feature ; le parsing et la validation YAML restent disponibles sans elle.

### `resolve_url_from_parent`
Résout les URLs relatives par rapport à un ancêtre de l'URL de la page.

| Champ | Type | Description |
|-------|------|-------------|
| `levels` | usize | Nombre de segments de chemin à remonter |
| `proxy` | bool | Envelopper via le proxy public |
| `proxy_headers` | object | En-têtes HTTP intégrés à l'URL proxy et envoyés en amont ; exige `proxy: true` ; les valeurs supportent les placeholders de requête, `{request_url}` et `{request_origin}` |
| `proxy_replace_all` | object[] | Règles `ReplaceAll` ordonnées jointes à l'URL proxy ; exige `proxy: true` |
| `proxy_follow_redirects` | bool ou entier positif | Suit les redirections amont avec la limite globale par défaut (16), ou avec un maximum positif explicite ; exige `proxy: true` |

```yaml
- type: resolve_url_from_parent
  levels: 2
  proxy: false
```

### `get_url_host`
Remplace chaque valeur par le hostname de l'URL parsée (supprime `www.`).

```yaml
- type: get_url_host
```

### `build_nextjs_data_url`
Convertit chaque valeur (page path) en URL `/_next/data/...json`.

| Champ | Type | Description |
|-------|------|-------------|
| `data_root` | string (opt.) | Préfixe avant `/_next/data` (ex: `/rtlplay`) |
| `route_prefix` | string (opt.) | Préfixe entre le build id et le path |
| `page_path_prefix_to_strip` | string (opt.) | Préfixe à retirer du path courant |

```yaml
- type: build_nextjs_data_url
  data_root: "/site"
  route_prefix: "detail"
```

### `ratio`
Multiplie les valeurs numériques par un facteur.

```yaml
- type: ratio
  argument: 0.5
```

### `math_formula`
Évalue chaque chaîne courante comme une expression mathématique. Cette action
n'a pas de champ de configuration `formula`. Construire d'abord l'expression
avec `format_text` ; `{}` désigne une valeur d'entrée courante. Les expressions
invalides sont supprimées.

```yaml
- type: format_text
  argument: "{} * 2 + 5"
- type: math_formula
```

Ne pas confondre cette action avec le calcul `post_build` d'un groupe, qui reçoit
`target` et `expression`, ni avec `compute_items_field`, qui agit sur des champs
objets.

### `replace_text`
Remplace toutes les occurrences d'une chaîne par une autre.

```yaml
- type: replace_text
  search: "&amp;"
  replace: "&"
```

### `format_text`
Formate chaque valeur via un template.

Placeholders disponibles :
- `{}` = valeur courante
- `{request_url}` = URL de la requête
- `{base_url}`, `{locale}`, etc. = paramètres d'exécution

```yaml
- type: format_text
  argument: "{base_url}/image/{}.jpg"
```

### `build_url`
Construit une URL à partir de plusieurs champs JSON de la réponse.

| Champ | Type | Description |
|-------|------|-------------|
| `base` | string | Template d'URL avec placeholders nommés |
| `fields` | mapping | Mapping : placeholder → pointeur JSON |

```yaml
- type: build_url
  base: "{base_url}/{program_code}-p_{program_id}/{clip_code}-c_{clip_id}"
  fields:
    program_code: "/program/code"
    program_id: "/program/id"
    clip_code: "/clip/code"
    clip_id: "/clip/id"
```

### `extract_field`
Extrait un champ du corps JSON de la réponse par pointeur JSON.

```yaml
- type: extract_field
  path: "/data/build_id"
```

### `get_date`
Normalise les dates au format `YYYY-MM-DD`.

| Champ | Type | Description |
|-------|------|-------------|
| `format` | string ou array | Format(s) d'entrée essayés dans l'ordre |
| `months` | mapping (opt.) | Table de correspondance des mois (nécessaire pour `dd_month_yyyy`) |

Formats supportés (`GetDateSource`) :

| Valeur YAML | Description |
|-------------|-------------|
| `yyyy_mm_dd` | `2026-03-25` ou `2026-03-25T20:15:00+01:00` |
| `dd_mm_yyyy` | `25/03/2026` ou `25.03.2026` |
| `dd_mm_yy` | `25/03/26` ou `25.03.26` (pivot 70 ans : 70-99→19xx, 0-69→20xx) |
| `dd_month_yyyy` | `mar. 25 mars 2026` (nécessite `months`) |
| `days_from_today` | `"3"` → aujourd'hui + 3 jours |
| `yyyy_mm_dd_hh_mm_ss` | `2026-03-25T20:15:00+01:00` ou `2026-03-25 20:15:00` |

```yaml
- type: get_date
  format: yyyy_mm_dd

# Plusieurs formats essayés dans l'ordre
- type: get_date
  format:
    - yyyy_mm_dd
    - dd_mm_yyyy
    - dd_month_yyyy
  months:
    janv: 1
    févr: 2
    mars: 3
    avril: 4
    mai: 5
    juin: 6
    juil: 7
    août: 8
    sept: 9
    oct: 10
    nov: 11
    déc: 12
```

### `normalize_duration`
Parse les durées lisibles (ex: `39 min`, `1 h 05 min`, `1 h 05`) en secondes. Un nombre seul suivant immédiatement une heure est interprété comme des minutes.

```yaml
- type: normalize_duration
```

### `base64_decode`
Décode chaque valeur en Base64 (alphabet standard). Les valeurs non décodables sont conservées telles quelles.

```yaml
- type: base64_decode
```

### `hex_decode`
Décode chaque valeur hexadécimale en texte UTF-8. Les valeurs comportant des caractères non
hexadécimaux, un nombre impair de chiffres ou des octets UTF-8 invalides sont supprimées. Les espaces
en début et fin de valeur sont retirés avant la validation des paires ASCII complètes ; une paire
incomplète est rejetée. Les chiffres hexadécimaux ASCII peuvent être en majuscules ou en minuscules.

```yaml
- type: hex_decode
```

### `aes_cbc_decrypt`
Déchiffre chaque valeur hexadécimale avec AES-128-CBC, une clé et un IV UTF-8 de 16 octets.
Les espaces en début et fin du texte chiffré sont retirés avant la validation des paires
hexadécimales ASCII complètes. Les longueurs impaires, les données hexadécimales ou le remplissage
PKCS#7 invalides et les textes déchiffrés non UTF-8 sont supprimés.

```yaml
- type: aes_cbc_decrypt
  key: "sixteen-byte-key"
  iv: "sixteen-byte-iv!"
```

### `request_url_actions`
Les actions de cette liste transforment l'URL obtenue après l'interpolation de `query_url` et avant
la requête HTTP. Elles sont utiles quand une URL d'entrée doit être convertie en endpoint API.

```yaml
query_url: "{url}"
request_url_actions:
  - type: regex_find_all
    pattern: '#([A-Za-z0-9]+)$'
    format: "https://example.invalid/api?id={1}"
```

### `caesar_shift`, `regex_replace_all`, `bytes_shift`, `reverse` et `json_extract_text`

Transformations génériques de valeurs texte.

```yaml
- type: caesar_shift
  shift: 13
- type: regex_replace_all
  pattern: '[^A-Za-z0-9]'
  replacement: ''
- type: bytes_shift
  value: -3
- type: reverse
- type: json_extract_text
  path: /source/url
```

`caesar_shift` fait pivoter les lettres ASCII d'un décalage signé. `regex_replace_all` effectue un remplacement regex global. `bytes_shift` applique un décalage signé avec retour à zéro sur chaque octet UTF-8 et conserve la valeur lorsque le résultat n'est pas un UTF-8 valide. `reverse` inverse les scalaires Unicode. `json_extract_text` parse chaque valeur courante comme JSON et retourne le scalaire ciblé par son pointeur JSON.

### `exec_js`
Exécute du JavaScript dans un sandbox isolé (moteur `boa_engine`) et retourne les
nouvelles variables globales numériques sous forme de lignes `Nom=Valeur`.

Tout contenu écrit avec `document.write(...)` / `document.writeln(...)` est retourné
sous forme d'une ligne unique `document_write=<contenu>`. C'est le seul canal qui
ramène une **chaîne** JavaScript dans la pipeline (la valeur de la dernière
expression évaluée n'est pas retournée) : par exemple `...;document.write(decoded);`
suivi d'une action `regex_find_all` avec `pattern: '^document_write=(.+)$'` et
`format: "{1}"`.

La valeur courante de la pipeline (`text`) est traitée comme du code JS à exécuter.

Trois sources JavaScript sont exécutées dans cet ordre :

1. **Scripts configurés** (`scripts`) : fonctions et objets simulés définis en YAML.
2. **Scripts HTML** (`inject_html_scripts: true`) : blocs `<script>` intégrés du
   `response_body`, dans l'ordre du document. Les URL de scripts externes ne sont
   pas récupérées.
3. **Texte du pipeline** (`text`) : chaque valeur courante comme code JavaScript.

Les scripts injectés depuis le HTML et la configuration sont limités aux 50 premières sources combinées ; le texte du pipeline est évalué ensuite. `timeout_ms` correspond à un budget de boucles de `max(timeout_ms × 20 000, 10 000 000)` ; ce n’est pas une échéance de temps réel.

| Champ | Type | Défaut | Description |
|---|---|---|---|
| `timeout_ms` | u64 | 500 | Budget max d'instructions via `loop_iteration_limit` |
| `inject_html_scripts` | bool | false | Si vrai, extrait les scripts du HTML de réponse et les exécute avant le code principal |
| `scripts` | string[] | [] | Liste de JS en dur à injecter avant le code principal |

```yaml
- type: exec_js
  timeout_ms: 500
  inject_html_scripts: false
  scripts:
    - "function helper(x) { return x ^ 42; }"
```

Utilisation typique avec Spys.one :
```yaml
- type: get_response_body
- type: exec_js
  timeout_ms: 500
  inject_html_scripts: true
- type: extract_variables
  pattern: "^([A-Za-z_][A-Za-z0-9_]*)=(\\d+)$"
  name: "@{1}"
  value: "{2}"
  on_duplicate: replace
```

### `unpack_packer`
Dépaquette le format déterministe Dean Edwards Packer sans exécuter de JavaScript. L'action
accepte uniquement les appels dont le payload, le radix, le compteur de symboles, le dictionnaire
et le séparateur de `split` sont littéraux. Une valeur non conforme ou malformée est supprimée.

Les radices de 2 à 62 sont supportés.

```yaml
- type: unpack_packer
```

---

## 12. Post-traitements (`post_process`)

Transformations appliquées après l'extraction et les sous-requêtes, dans l'ordre de déclaration. Le schéma commun HTTP les accepte pour `html`, `json` et `text` ; les sous-requêtes HTML et JSON les acceptent aussi.

Tagguées par `type` en snake_case.

### `remove_fields`

Supprime les champs temporaires du résultat courant après leur utilisation par
les traitements précédents. `fields`, obligatoire, est une liste de chemins
séparés par `>`. Un chemin absent est ignoré. Ce post-traitement de requête et
l'opération HTML `post_build` de même nom agissent à des niveaux différents.

```yaml
- type: remove_fields
  fields: [raw_html, internal > token]
```

### `sort_items`

Trie sur place un groupe d'objets. Les trois champs sont obligatoires :

| Champ | Type | Signification |
|---|---|---|
| `source` | string | Chemin du groupe d'objets, séparé par `>` |
| `field` | string | Chemin non vide du champ de l'élément, obligatoire même pour `reverse` |
| `method` | string | `asc`, `desc` ou `reverse` |

`reverse` inverse l'ordre d'extraction. `asc` et `desc` comparent la première
valeur scalaire de `field`, numériquement si les deux valeurs sont des entiers
signés, sinon lexicographiquement. Les valeurs absentes sont comparées comme des
chaînes vides. Une source absente ne produit aucun effet. Trier avant
`merge_items_by_field` si la fusion des doublons doit préserver cet ordre.

```yaml
- type: sort_items
  source: episodes
  field: episode-number
  method: asc
```

### `group_items_by_header`

Transforme un groupe ordonné contenant des lignes d'en-tête en sections. Il ne
trie pas la source. Les en-têtes définissent les libellés et ne sont pas copiés
comme entrées.

| Champ | Type | Obligatoire / défaut |
|---|---|---|
| `source` | string | Obligatoire : chemin du groupe d'objets à plat |
| `target` | string | Obligatoire : chemin du groupe de sections de sortie |
| `is_header_field` | string | Obligatoire : champ booléen signalant un en-tête |
| `label_field` | string | Obligatoire : champ contenant le libellé de l'en-tête |
| `entries_field` | string | Facultatif ; `entries` |
| `skip_fields` | string[] | Facultatif ; `[]` ; supprime un élément si l'un des indicateurs est vrai |
| `first_label` | string | Facultatif ; sans lui, les éléments avant le premier en-tête sont ignorés |
| `remove_item_fields` | string[] | Facultatif ; `[]` ; chemins retirés des entrées copiées |
| `remove_source` | boolean | Facultatif ; `false` |

La sortie est un tableau d'objets contenant `label` et le champ `entries_field`
configuré. Les sections vides sont omises. Si aucune section n'est produite, la
cible reste inchangée. `remove_source: true` retire la source même si aucune
section n'est produite.

```yaml
- type: group_items_by_header
  source: flat_episodes
  target: seasons
  is_header_field: is_header
  label_field: title
  first_label: "Episodes"
  skip_fields: [is_advertisement]
  remove_item_fields: [is_header, is_advertisement]
  remove_source: true
```

### `apply_actions_to_field`

Applique une pipeline normale d'actions aux valeurs scalaires d'un champ racine et écrit le résultat dans un autre champ racine.

| Champ | Type | Description |
|---|---|---|
| `source` | string | Champ racine existant à lire |
| `target` | string | Champ racine à créer ou remplacer |
| `output_type` | string (opt.) | Type de sortie assigné à la cible |
| `actions` | array | Pipeline d'actions non vide |

```yaml
- type: apply_actions_to_field
  source: encoded
  target: stream_url
  output_type: string[]
  actions:
    - type: base64_decode
    - type: json_extract_text
      path: /source
```

### `fetch_actions_to_field`

Récupère chaque URL d'un champ scalaire racine, puis applique une pipeline d'actions à chaque réponse et écrit les valeurs combinées dans un champ cible.

| Champ | Type | Description |
|---|---|---|
| `source` | string | Champ racine contenant les URL de requête |
| `target` | string | Champ racine recevant les valeurs extraites |
| `output_type` | string (opt.) | Type de sortie assigné à la cible |
| `request_actions` | array (opt.) | Actions appliquées aux URL avant la requête |
| `actions` | array | Pipeline non vide appliquée aux corps de réponse |

### `copy_item_fields`

Copie des champs dans chaque item d'un groupe racine. Les nœuds sources sont
clonés : leurs valeurs, types de sortie et structure imbriquée sont conservés.

| Champ | Type | Description |
|---|---|---|
| `source` | string | Groupe racine contenant les items à modifier |
| `fields` | array | Liste non vide de correspondances source/cible |

```yaml
- type: copy_item_fields
  source: players
  fields:
    - source: embed-link
      target: web-link
```


### `extract_regex_items`
Construit des items de groupe à partir de correspondances regex répétées dans un champ texte.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ source contenant le texte à analyser |
| `target` | string | Champ cible recevant les items extraits |
| `pattern` | string | Expression régulière avec groupes capturants |
| `entries` | array | Définition des champs extraits de chaque match |

```yaml
- type: extract_regex_items
  source: "raw_html"
  target: "episodes"
  pattern: '<a href="(.*?)">(.*?)</a>'
  entries:
    - name: url
      type: string
      capture_group: 1
      actions:
        - type: resolve_url
          proxy: true
    - name: title
      type: string
      capture_group: 2
```

Chaque `entry` dans `entries` :

| Champ | Type | Description |
|-------|------|-------------|
| `name` | string | Nom du champ de sortie (supporte `>`) |
| `type` | string (opt.) | Type JSON de sortie |
| `capture_group` | int (opt.) | Index du groupe de capture (1-based) |
| `actions` | array | Actions appliquées à la valeur extraite |

### `filter_items`
Filtre les items d'un groupe en testant un champ scalaire avec une regex.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source |
| `field` | string | Champ scalaire à tester dans chaque item |
| `pattern` | string | Regex : si `keep_matching: true`, garde les items qui matchent |
| `keep_matching` | bool | `true` (défaut) = garde les matchs, `false` = garde les non-matchs |

```yaml
- type: filter_items
  source: "videos"
  field: "format"
  pattern: "mp4|hls"
  keep_matching: true
```

### `filter_fields`
Supprime des champs spécifiques dans les items d'un groupe quand la valeur d'un champ conditionnel match (ou ne match pas) une regex.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source |
| `field` | string | Champ scalaire à tester dans chaque item |
| `pattern` | string | Regex appliquée à la valeur du champ conditionnel |
| `keep_matching` | bool | `true` (défaut) = supprime les champs des items qui matchent, `false` = supprime des items qui ne matchent pas |
| `remove` | string[] | Liste de chemins `>` des champs à supprimer |

```yaml
- type: filter_fields
  source: "entries"
  field: "resourceType"
  pattern: "^PROGRAM$"
  keep_matching: true
  remove:
    - img/landscape > link
    - _internal_field
```

### `fetch_regex_items_from_items`
Pour chaque item d'un groupe, fait une requête HTTP, applique une regex sur la réponse, et ajoute les résultats au chemin cible.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source |
| `request_field` | string | Champ de l'item contenant l'URL à fetch |
| `request_actions` | array | Actions appliquées à l'URL avant la requête |
| `request_headers` | array (opt.) | En-têtes attachés à chaque requête ; même structure que la section [`request_headers`](#request_headers) |
| `request_method` | `get` ou `post` (opt.) | Méthode HTTP ; `get` par défaut |
| `response_pointer` | string (opt.) | Pointeur JSON sélectionnant le texte de réponse avant l'extraction regex |
| `target` | string | Chemin cible pour les items extraits |
| `target_per_source` | boolean (opt.) | Ajoute les items sous chaque item source plutôt qu'à la racine |
| `pattern` | string | Regex appliquée sur le corps de la réponse |
| `entries` | array | Définition des champs extraits |
| `copy_item_fields` | array | Champs à copier de l'item source vers l'item cible |

```yaml
- type: fetch_regex_items_from_items
  source: "episodes"
  request_field: "detail_url"
  request_actions:
    - type: resolve_url
  request_headers:
    - name: "Referer"
      actions:
        - type: format_text
          argument: "{base_url}/"
  target: "episodes>sources"
  pattern: '<source src="(.*?)" type="(.*?)">'
  entries:
    - name: url
      type: string
      capture_group: 1
    - name: format
      type: string
      capture_group: 2
  copy_item_fields:
    - source: "id"
      target: "episode_id"
```

### `pivot_items_by_index`
Réorganise des listes de valeurs alignées en items indexés.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source |
| `target` | string | Champ cible |
| `values_field` | string | Champ contenant les listes de valeurs |
| `nested_field` | string | Champ dans values_field à utiliser comme pivot |
| `nested_value_field` | string | Sous-champ de la valeur à pivoter |
| `sort_by` | string (opt.) | Champ de tri des items résultants |
| `copy_item_fields` | array | Champs à copier de l'item source |
| `copy_root_fields` | array | Champs racine à copier |
| `promote_first_nested_fields` | array | Champs à promouvoir du premier item |
| `copy_target_fields` | array | Champs à copier dans l'item cible |
| `generated_fields` | array | Champs générés (index 1-based) |

Structure `ScraperFieldMapping` (pour `copy_item_fields`, `copy_root_fields`, `copy_target_fields`) :

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Chemin source |
| `target` | string | Chemin destination |

Structure `ScraperGeneratedField` :

| Champ | Type | Description |
|-------|------|-------------|
| `name` | string | Nom du champ |
| `type` | string (opt.) | Type JSON |
| `format` | string | Template où `{}` = index 1-based |

```yaml
- type: pivot_items_by_index
  source: "qualities"
  target: "sources"
  values_field: "values"
  nested_field: "format"
  nested_value_field: "url"
  sort_by: "height"
  generated_fields:
    - name: "label"
      type: string
      format: "Source {}"
  copy_item_fields:
    - source: "id"
      target: "item_id"
```

### `compute_items_field`
Calcule un champ scalaire via une expression mathématique par item.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source |
| `nested_source` | string (opt.) | Sous-champ du groupe |
| `target` | string | Champ cible pour le résultat |
| `expression` | string | Expression mathématique |
| `variables` | mapping | Variables disponibles dans l'expression |

Structure `ScraperComputedFieldVariable` :

| Champ | Type | Description |
|-------|------|-------------|
| `path` | string (opt.) | Chemin de résolution (défaut : nom de la variable) |
| `scope` | enum | `auto` (défaut), `current`, `parent`, `root`, `params` |

```yaml
- type: compute_items_field
  source: "qualities"
  target: "height"
  expression: "{height} * 2"
  variables:
    height:
      path: "height"
      scope: current
```

Portées des variables :

| Valeur | Description |
|--------|-------------|
| `auto` (défaut) | Cherche dans : item courant → item parent → racine → paramètres |
| `current` | Item en cours de traitement |
| `parent` | Item parent du `nested_source` |
| `root` | Nœud racine de la requête |
| `params` | Paramètres d'exécution |

### `derive_pagination`
Dérive les métadonnées de pagination à partir des champs extraits.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string (opt.) | Champ source (optionnel) |
| `entries_field` | string | Champ contenant les entrées (défaut: `"entries"`) |
| `current_page_field` | string | Champ contenant la page courante (défaut: `"current_page"`) |
| `total_pages_field` | string (opt.) | Champ contenant le nombre total de pages |
| `have_more_field` | string | Champ booléen "a plus" (défaut: `"have_more"`) |
| `next_value_field` | string (opt.) | Champ contenant la valeur de la prochaine page |
| `next_param` | string | Nom du paramètre pour la prochaine page (défaut: `"page"`) |
| `source_params_target` | string | Champ cible pour les paramètres source (défaut: `"source_params"`) |
| `page_size_field` | string (opt.) | Champ contenant la taille de page |
| `infer_from_full_page_field` | string (opt.) | Champ pour inférer depuis une page complète |
| `remove_fields` | array | Champs à supprimer après dérivation |

```yaml
- type: derive_pagination
  current_page_field: "page"
  have_more_field: "has_next"
  next_param: "page"
  remove_fields:
    - "page"
    - "has_next"
```

### `append_static_items`
Ajoute des items statiques à un groupe, avec déduplication optionnelle.

| Champ | Type | Description |
|-------|------|-------------|
| `target` | string | Champ groupe cible |
| `items` | array | Liste d'items (chaque item est une map string→string) |
| `unique_field` | string (opt.) | Champ utilisé pour dédupliquer |

```yaml
- type: append_static_items
  target: "links"
  items:
    - label: "Accueil"
      url: "{base_url}"
    - label: "Contact"
      url: "{base_url}/contact"
  unique_field: "url"
```

### `set_nested_fields`
Définit un ou plusieurs champs sur chaque item d'un sous-tableau, avec une
valeur statique, une copie depuis un champ voisin, ou une extraction de nom
d'hôte éventuellement traduite via un groupe de correspondance.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe source contenant les items |
| `nested_source` | string | Sous-champ tableau à l'intérieur de chaque item source |
| `fields` | array | Définitions de champs à appliquer |

Structure `ScraperNestedFieldDefinition` :

| Champ | Type | Description |
|-------|------|-------------|
| `target` | string | Chemin de destination (supporte `>` pour les chemins hiérarchiques) |
| `value` | string (opt.) | Valeur statique à écrire (mutuellement exclusif avec `copy_from` et `host_from`) |
| `copy_from` | string (opt.) | Champ source à copier depuis le même item (mutuellement exclusif avec `value` et `host_from`) |
| `host_from` | string (opt.) | Extrait le nom d'hôte de l'URL contenue dans ce champ voisin (mutuellement exclusif avec `value` et `copy_from`) |
| `lookup` | object (opt.) | Traduit la valeur de `copy_from` ou `host_from` via un groupe racine (interdit avec `value`) |

Structure `ScraperNestedFieldLookup` :

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe racine contenant les items de correspondance |
| `key` | string | Champ scalaire lu sur chaque item comme clé |
| `value` | string | Champ scalaire lu sur chaque item comme valeur |

Lorsque `lookup` est combiné avec `host_from`, le nom d'hôte exact est essayé
en premier, puis chaque domaine parent (`video.sibnet.ru` → `sibnet.ru`). Les
valeurs sans correspondance sont écrites telles quelles. Les lookups
`copy_from` utilisent uniquement une correspondance exacte.

```yaml
- type: set_nested_fields
  source: "episodes"
  nested_source: "players"
  fields:
    - target: resolver > kind
      value: "stream-resolver"
    - target: resolver > target_id
      copy_from: web-link
    - target: name
      host_from: web-link
      lookup:
        source: "hosters"
        key: "host"
        value: "label"
```

### `merge_items_by_field`
Fusionne les items voisins qui partagent la même valeur de champ scalaire. Utile
lorsqu'une même ligne logique est répétée dans le balisage une fois par variante
(par exemple un bloc d'épisode par langue) et que les variantes ne diffèrent que
par les champs listés dans `merge_fields`.

| Champ | Type | Description |
|-------|------|-------------|
| `source` | string | Champ groupe contenant les items à fusionner |
| `field` | string | Champ scalaire lu sur chaque item comme clé de fusion |
| `merge_fields` | array (opt.) | Chemins de champs ajoutés depuis les items en doublon vers le premier item de la même clé |

Les items sont parcourus dans l'ordre d'extraction. Le premier item d'une clé est
conservé ; les items suivants ayant la même clé ajoutent les nœuds nommés par
`merge_fields` : les valeurs scalaires sont ajoutées une seule fois (les valeurs
identiques sont ignorées), les nœuds enfants sont fusionnés récursivement et les
listes d'items imbriqués sont concaténées. Les items sans valeur pour `field`
sont conservés tels quels. Le groupe fusionné remplace le groupe source et
conserve son ordre d'extraction, ce qui suppose que `sort_items` soit exécuté
avant lui.

```yaml
- type: merge_items_by_field
  source: "episodes"
  field: episode-number
  merge_fields:
    - players
    - lang
```

---

## 13. Types de sortie (`type` / `ScraperOutputType`)

| Valeur YAML | Description |
|-------------|-------------|
| `string` | Chaîne de caractères |
| `number` | Nombre (entier ou flottant) |
| `boolean` | Booléen |
| `object` | Objet JSON |
| `string[]` | Tableau de chaînes |
| `number[]` | Tableau de nombres |
| `boolean[]` | Tableau de booléens |
| `object[]` | Tableau d'objets |

Une feuille sans valeur extraite est sérialisée en `null`, y compris lorsque son type est un tableau. Les types scalaires `string`, `number` et `boolean` refusent plusieurs valeurs extraites. Un nombre scalaire vide ou composé d'espaces produit `null` ; les autres nombres doivent être des entiers ou des flottants finis. Les booléens ignorent les espaces périphériques et la casse ASCII : `true`/`false`, `1`/`0`, `yes`/`no` et `y`/`n` sont acceptés, les autres valeurs sont des erreurs. Un type `object` refuse plusieurs items objet explicites. Les erreurs de conversion identifient le chemin de sortie concerné. Ces règles décrivent la sérialisation des résultats, sans ajouter de champ YAML.


---

## 14. Modèle de données de sortie (`ScraperDataNode`)

Le résultat d'une requête est un arbre de nœuds. Chaque nœud peut contenir :

- `values` — valeurs scalaires
- `children` — sous-nœuds nommés (chemins hiérarchiques)
- `items` — items explicites (tableaux d'objets)
- `output_type` — type JSON déclaré

Les chemins hiérarchiques sont exprimés avec `>` dans les noms :
```yaml
name: "media>images>poster"
```

Ceci crée la structure :
```json
{ "media": { "images": { "poster": "..." } } }
```

---

## 15. Exemple complet

```yaml
id: example-source
title: "Source d'exemple"
description:
  fr: "Une source d'exemple complète"
parameters:
  - name: base_url
    value: "https://www.example.com"
  - name: api_url
    value: "{base_url}/api"

http:
  mode: auto
  user_agent_profile: chrome

queries:
  # Requête statique pour les métadonnées
  - name: service_metadata
    scraper_type: static
    media_types: [metadata]
    entries:
      - name: name
        type: string
        value: "Example Source"
      - name: logo
        type: string
        value: "{base_url}/logo.png"
        actions:
          - type: resolve_url
            proxy: true

  # Requête JSON pour la recherche
  - name: search
    scraper_type: json
    base_url: "{base_url}"
    media_types: [movie, series]
    query_url: "{api_url}/search?q={query}&page={page}"
    row_pointer: "/results/*"
    entries:
      - name: id
        type: string
        pointer: "/id"
      - name: title
        type: string
        pointer: "/title"
      - name: type
        type: string
        pointer: "/type"
      - name: year
        type: number
        pointer: "/year"
      - name: poster
        type: string
        pointer: "/poster"
        actions:
          - type: resolve_url
            proxy: true
    sub_queries:
      - scraper_type: json
        context_pointer: "/results/*"
        target: "details"
        request_pointer: "/detail_url"
        request_actions:
          - type: resolve_url
        row_pointer: "/data/*"
        entries:
          - name: synopsis
            type: string
            pointer: "/synopsis"
          - name: duration
            type: number
            pointer: "/duration"
            actions:
              - type: normalize_duration
          - name: genres
            type: string[]
            pointer: "/genres/*"
    post_process:
      - type: derive_pagination
        current_page_field: "page"
        have_more_field: "has_next"
        next_param: "page"

  # Requête HTML pour les détails
  - name: details
    scraper_type: html
    base_url: "{base_url}"
    media_types: [movie, series]
    query_url: "/program/{id}"
    row_selector: "main.content"
    entries:
      - name: title
        type: string
        selector: "h1.title"
        actions:
          - type: get_text
      - name: description
        type: string
        selector: "div.description"
        actions:
          - type: html_to_text
      - name: duration
        type: number
        selector: "span.duration"
        actions:
          - type: get_text
          - type: normalize_duration
      - name: thumbnail
        type: string
        selector: "meta[property='og:image']"
        actions:
          - type: get_attribut
            argument: content
      - name: images
        type: object[]
        selector: "img.gallery"
        entries:
          - name: url
            type: string
            actions:
              - type: get_attribut
                argument: src
              - type: resolve_url
                proxy: true
          - name: alt
            type: string
            actions:
              - type: get_attribut
                argument: alt

  # Requête texte (CSV)
  - name: episodes_csv
    scraper_type: text
    base_url: "{base_url}"
    media_types: [episodes]
    query_url: "/episodes.csv"
    row_delimiter: "\n"
    field_delimiter: ","
    entries:
      - name: id
        type: string
        field: 1
      - name: season
        type: number
        field: 2
      - name: episode
        type: number
        field: 3
      - name: title
        type: string
        field: 4
      - name: duration_min
        type: number
        field: 5
        actions:
          - type: ratio
            argument: 60
```

## 16. Inscription des services (`services.json`)

Placer les YAML sous `server/services/<group>/`. Le `services.json` d'un groupe
est un tableau JSON. Chaque entrée déclare exactement un champ `path` ou `import`.

```json
[
  {"path": "example.yaml", "enabled": true,
   "parameters": [{"name": "base_url", "value": "https://example.org"}]},
  {"import": "other-sources/services.json"}
]
```

| Entrée | Champs et comportement |
|---|---|
| Source | `path` chaîne obligatoire ; `enabled` booléen facultatif, défaut `true` ; `parameters` tableau facultatif ayant la même structure que les paramètres YAML |
| Import | `import` chaîne obligatoire désignant un autre manifeste JSON |

Les chemins sont relatifs au manifeste qui les déclare. Les imports sont développés
récursivement en profondeur dans l'ordre de déclaration. Les imports répétés ou
cycliques sont ignorés avec un avertissement. Un manifeste JSON invalide ou absent
est une erreur. Un YAML absent est ignoré avec un avertissement ; les autres
erreurs d'accès font échouer le chargement. Maintenir tous les chemins valides
sans dépendre de cette tolérance.

Une surcharge d'activation administrateur est prioritaire sur `enabled`. Les YAML
désactivés sont tout de même lus et désérialisés dans le schéma brut avant le
contrôle d'activation ; ils doivent rester valides. La conversion sémantique
complète est réalisée pour les fichiers activés.

Les paramètres du manifeste remplacent les définitions YAML complètes par leur
`name`, ou en ajoutent. Les paramètres de requête à l'exécution remplacent ensuite
les valeurs par défaut ; les paramètres d'exécution propres à une source peuvent
remplacer les paramètres communs. **Limite actuelle :** la fusion des paramètres
du manifeste ne préserve pas l'ordre YAML. Éviter les pipelines d'actions de
paramètres dépendants lorsque des surcharges sont utilisées ; vérifier avec le
manifeste réel. Ne pas supposer qu'une surcharge de `base_url` recalcule une valeur
déjà substituée au chargement de la collection.

## 17. Liste de contrôle de l'exécution et de la sortie

- Les requêtes HTTP exigent `name`, `scraper_type`, `base_url`, `query_url`,
  `entries` et le localisateur de lignes propre à leur type. Les requêtes statiques
  exigent `name`, `scraper_type: static` et `entries`, sans URL HTTP.
- Résoudre les valeurs par défaut et entrées d'exécution, puis les correspondances
  de paramètres, URL, en-têtes et corps ; récupérer ; prétraiter ; analyser ;
  extraire les champs et groupes ; exécuter les sous-requêtes ; appliquer les
  post-traitements ordonnés ; fusionner les lignes. `result_item_field` promeut
  enfin les objets du groupe sélectionné en lignes de résultat distinctes.
- `pre_process`, `post_process`, actions et entrées sont des pipelines locaux
  ordonnés ; l'héritage HTTP ne les copie pas automatiquement dans les enfants.
- `select` vaut `all` par défaut. Un scalaire contenant plusieurs valeurs extraites
  échoue à la conversion JSON. Choisir `first` ou le type tableau approprié.
- `>` dans un nom crée des objets imbriqués. `/` est littéral dans un nom :
  `img/poster > link` produit `{"img/poster":{"link":"..."}}`.
  Les pointeurs JSON utilisent `/`, les chemins du résultat `>`, les sélecteurs CSS
  leur propre syntaxe.
- Une feuille vide devient `null`, même avec un type tableau. Ne pas confondre
  `null`, `[]`, un champ absent et une requête échouée.
- L'interpolation n'encode pas automatiquement une chaîne arbitraire pour une URL
  ou pour JSON. Utiliser les entrées du service, telles que `search_terms` encodé
  ou `search_query_json` échappé pour JSON.
- Les champs YAML inconnus peuvent être ignorés à la désérialisation. Une faute
  de frappe peut donc être sans effet malgré un chargement réussi. Utiliser les
  clés documentées.
- Un nom de requête absent de la collection ne produit aucune ligne à l'exécution.
  Ne pas annoncer une capacité uniquement parce qu'un appelant accepte son nom.
- Vérifier le JSON et le chargement. Une validation structurelle ne prouve ni la
  disponibilité du site distant, ni la conformité de sa réponse, ni la lecture vidéo.
