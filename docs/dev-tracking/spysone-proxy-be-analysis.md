# Analyse : intégration proxy Spys.one Belgique

> Généré le 2026-07-24  
> Source cible : `https://spys.one/free-proxy-list/BE/`  
> Données POST demandées : `xx00=&xpp=5&xf1=0&xf2=0&xf4=0&xf5=0`  
> Cible Arachnea : `server/services/arachnea-proxies/spysone-be.yaml`

---

## 1. Objectif

Ajouter une source proxy Arachnea pour les proxys publics belges publiés par Spys.one. La collection doit produire des lignes `application/arachnea-proxy` consommables par `ScrapyfyProxyDataProvider` via la requête conventionnelle `list_proxies_for_country`.

La page doit être appelée en `POST` sur `/free-proxy-list/BE/` avec le corps formulaire suivant :

```text
xx00=&xpp=5&xf1=0&xf2=0&xf4=0&xf5=0
```

Sens observé des paramètres :

| Paramètre | Valeur | Sens |
|---|---:|---|
| `xx00` | vide | Champ caché du formulaire. |
| `xpp` | `5` | Afficher 500 lignes. |
| `xf1` | `0` | Tous les niveaux d'anonymat. |
| `xf2` | `0` | SSL+ et SSL-. |
| `xf4` | `0` | Tous les ports. |
| `xf5` | `0` | HTTP et SOCKS. |

---

## 2. État actuel

Les sources proxy existantes sont dans `server/services/arachnea-proxies/` :

| Fichier | Type | Particularité |
|---|---|---|
| `proxifly.yaml` | JSON | Source structurée, groupée sous `proxies`. |
| `iplocate-free-proxy-list.yaml` | Texte | Lignes `protocol://host:port`, groupées sous `proxies`. |

Le registre `server/services/arachnea-proxies/services.json` active actuellement ces deux sources.

Le format proxy attendu est décrit dans `docs/specifications/arachnea-proxies-fr.md`. Pour Spys.one, le minimum utile est :

| Champ Arachnea | Source Spys.one | Remarque |
|---|---|---|
| `protocol` | Colonne `Proxy type` | À normaliser vers `http`, `https`, `socks4` ou `socks5`. |
| `host` | Colonne `Proxy address:port` | L'IP est visible dans le HTML. |
| `port` | `document.write(...)` dans la colonne adresse | Le port est obfusqué par expressions JavaScript simples. |
| `country` | Paramètre `{country}` | Pour cette source : `BE`. |
| `supports_https` | Type affiché `HTTPS` | Ajoutable ensuite ; pas requis pour la première intégration. |
| `latency_ms` | Colonne `Latency**` | Source en secondes décimales ; conversion vers millisecondes à prévoir si utilisé. |
| `availability` | Colonne `Uptime` | Source en pourcentage ; conversion vers `low`/`medium`/`high` à prévoir si utilisé. |

Le schéma `arachnea-scrapyfy` supporte déjà `request_method: post`, `request_body_pointer`, `request_body_actions` et `request_headers`.

---

## 3. Structure HTML observée

Les lignes proxy utilisent les classes alternées `spy1x` et `spy1xx`.

Exemple simplifié :

```html
<tr class="spy1x">
  <td><font class="spy14">34.140.137.151<script>document.write(":" + (...))</script></font></td>
  <td><a href="/en/http-proxy-list/"><font class="spy1">HTTP</font></a></td>
  <td><font class="spy1">HIA</font></td>
  <td><font class="spy14">BE ...</font></td>
  <td>...</td>
  <td><font class="spy1">1.29</font></td>
  <td>...</td>
  <td><acronym title="277 of 484 - last check status=OK">57%</acronym></td>
  <td>...</td>
</tr>
```

L'hôte est directement extractible. Le port est écrit par des scripts de forme :

```html
<script>document.write(":"+(Four3SevenNine^Five1Five)+(Zero2SixFour^TwoFourSix))</script>
```

Les variables comme `Four3SevenNine` et `Five1Five` sont initialisées dans un script inline packé de type Dean Edwards Packer au début de la page.

---

## 4. Décision retenue

La solution retenue est une **action générique** utilisable dans le pipeline `actions` existant. Cette action extrait des variables depuis les valeurs courantes et les ajoute au système de variables déjà utilisé par les placeholders.

Les variables ajoutées par cette action doivent obligatoirement utiliser le préfixe `@` afin de les distinguer clairement des paramètres standards du YAML.

Exemples de variables injectées :

```text
{@Four3SevenNine}=80
{@Five1Five}=0
{@Zero2SixFour}=8
```

Cette convention garde le YAML lisible et évite les collisions avec `{country}`, `{base_url}`, `{request_url}` ou les paramètres déclarés dans `parameters`.

---

## 5. Action proposée : `extract_variables`

### 5.1 Contrat

`extract_variables` est une action de transformation avec effet de bord contrôlé : elle lit les valeurs courantes, extrait des paires nom/valeur, puis les injecte dans le contexte de variables de la requête.

Exemple conceptuel :

```yaml
- type: extract_variables
  regex: "\\b([A-Za-z][A-Za-z0-9_]*)\\s*=\\s*(\\d+)\\s*;"
  name: "@{1}"
  value: "{2}"
```

Règles proposées :

- `name` doit produire un nom commençant par `@` ; sinon la configuration est invalide.
- Les variables extraites sont accessibles avec la syntaxe placeholder existante : `{@NomVariable}`.
- Les variables `@...` ne peuvent pas écraser une variable standard.
- Les variables standards ne peuvent pas écraser une variable `@...`.
- En cas de doublon entre deux variables `@...`, le comportement par défaut doit être `error` pour éviter les ambiguïtés.
- Une option explicite `on_duplicate: replace` pourra être ajoutée si un site en a besoin.
- Les valeurs sont stockées sous forme texte ; les actions consommatrices réalisent la conversion numérique si nécessaire.
- L'action n'exécute pas JavaScript ; elle applique uniquement une extraction regex sur les valeurs courantes.

### 5.2 Usage avec Spys.one

Pour Spys.one, l'action serait placée dans un champ préparatoire extrait avant le champ `port`.

Le champ préparatoire lit le corps complet de la réponse, dépaquette le script, puis injecte les affectations numériques en variables `@...` :

```yaml
- name: _script_variables
  type: string
  actions:
    - type: get_response_body
    - type: unpack_dean_edwards
    - type: extract_variables
      regex: "\\b([A-Za-z][A-Za-z0-9_]*)\\s*=\\s*(\\d+)\\s*;"
      name: "@{1}"
      value: "{2}"
```

Le champ est technique et doit être supprimé du résultat final avec `post_build: remove_fields`.

### 5.3 Calcul du port

Après injection des variables, le champ `port` peut extraire l'expression `document.write`, résoudre les placeholders `{@...}`, puis utiliser une action de calcul.

Exemple conceptuel :

```yaml
- name: port
  type: number
  selector: "td:nth-child(1)"
  actions:
    - type: get_html
    - type: regex_find_all
      regex: "document\\.write\\(\":\"\\+(.+?)\\)</script>"
      format: "{1}"
    - type: replace_variables
```

Point à sécuriser : en JavaScript, `^` signifie XOR bitwise. Si le moteur mathématique existant interprète `^` comme puissance, l'action de calcul doit soit exposer une fonction explicite `xor(a,b)`, soit accepter un mode où `^` est traité comme XOR.

---

## 6. Proposition de service YAML

Cette proposition documente la structure cible. Elle ne doit être activée dans `services.json` qu'après validation de l'extraction du port.

```yaml
id: spysone-be-free-proxy-list
title: Spys.one free Belgium proxy list
description:
  en: "Free public proxies for Belgium from Spys.one."
http:
  mode: auto

parameters:
  - name: base_url
    value: https://spys.one
    description: Base URL used to resolve requests.
  - name: post_body
    value: "xx00=&xpp=5&xf1=0&xf2=0&xf4=0&xf5=0"
    description: Form body requesting up to 500 Belgian proxies with all filters enabled.

queries:
  - name: list_proxies_for_country
    scraper_type: html
    base_url: "{base_url}"
    query_url: "{base_url}/free-proxy-list/{country}/"
    request_method: post
    request_headers:
      - name: Content-Type
        actions:
          - type: format_text
            argument: application/x-www-form-urlencoded
      - name: User-Agent
        actions:
          - type: format_text
            argument: Mozilla/5.0
    request_body_pointer: /post_body
    request_body_select: first
    media_types:
      - application/arachnea-proxy
    result_item_field: proxies
    row_selector: "tr.spy1x, tr.spy1xx"
    entries:
      - name: proxies
        type: object[]
        entries:
          - name: _script_variables
            type: string
            actions:
              - type: get_response_body
              - type: unpack_dean_edwards
              - type: extract_variables
                regex: "\\b([A-Za-z][A-Za-z0-9_]*)\\s*=\\s*(\\d+)\\s*;"
                name: "@{1}"
                value: "{2}"
          - name: host
            type: string
            selector: "td:nth-child(1)"
            actions:
              - type: get_text
              - type: regex_find_all
                regex: "([0-9]{1,3}(?:\\.[0-9]{1,3}){3})"
                format: "{1}"
          - name: port
            type: number
            selector: "td:nth-child(1)"
            actions:
              - type: get_html
              - type: regex_find_all
                regex: "document\\.write\\(\":\"\\+(.+?)\\)</script>"
                format: "{1}"
              - type: replace_variables
              - type: eval_math
                operators: [xor, add]
          - name: protocol
            type: string
            selector: "td:nth-child(2)"
            actions:
              - type: get_text
              - type: regex_find_all
                regex: "(?i)(https|http|socks4|socks5)"
                format: "{1}"
              - type: lowercase
          - name: country
            type: string
            actions:
              - type: format_text
                argument: "{country}"
        post_build:
          - type: remove_fields
            fields: [_script_variables]
```

Notes :

- `request_body_pointer: /post_body` suppose que les paramètres YAML sont disponibles pour construire le body ; ce point doit être validé côté Rust.
- `_script_variables` est un champ technique utilisé uniquement pour exécuter l'action `extract_variables` dans le pipeline existant.
- `extract_variables`, `replace_variables`, `eval_math` et `lowercase` sont des intentions fonctionnelles si ces actions n'existent pas encore sous ces noms.
- `supports_https`, `latency_ms` et `availability` peuvent être ajoutés dans un second temps.

---

## 7. Impact attendu

L'intégration finale aura un impact localisé :

- Nouveau fichier `server/services/arachnea-proxies/spysone-be.yaml`.
- Ajout dans `server/services/arachnea-proxies/services.json` après validation.
- Pas de changement attendu dans `ProxyRecord` ni dans `ScrapyfyProxyDataProvider` si la sortie respecte `application/arachnea-proxy`.
- Extension générique du moteur `arachnea-scrapyfy` pour permettre à une action d'ajouter des variables `@...` au contexte existant.
- Mise à jour de `docs/specifications/arachnea-scrapyfy-fr.md` et `docs/specifications/arachnea-scrapyfy-en.md` si l'action est implémentée.

---

## 8. Plan recommandé

1. Vérifier si le body POST statique peut être construit depuis `parameters` avec le mécanisme actuel. **Fait : oui, via `request_body_pointer: /post_body`.**
2. Ajouter l'action générique `extract_variables` dans le système d'actions existant.
3. Faire respecter le préfixe obligatoire `@` pour toutes les variables ajoutées par cette action.
4. Ajouter ou réutiliser une action de calcul qui résout `{@...}` et traite le XOR sans ambiguïté.
5. Créer `server/services/arachnea-proxies/spysone-be.yaml` avec `result_item_field: proxies`.
6. Activer la source dans `services.json` seulement après validation que chaque ligne produit `protocol`, `host` et `port`.

---

## 9. Plan d'implémentation par étapes

### Étape 1 : Valider le chemin POST existant

Objectif : confirmer que le moteur peut envoyer le corps demandé sans nouvelle sémantique YAML.

Statut : **terminé**.

Résultat : le chemin POST existant couvre le besoin Spys.one. Le body statique peut être porté par un paramètre YAML `post_body`, puis sélectionné avec `request_body_pointer: /post_body`.

Constats dans le code :

- `ScraperRequestMethod::Post` existe et est converti en `http::Method::POST` dans `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper/config.rs`.
- L'exécuteur unifié résout le body avec `resolve_request_body(...)` avant l'appel HTTP dans `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper/query_executor.rs`.
- Pour une requête racine, `resolve_request_body(...)` construit un contexte JSON depuis les paramètres via `build_params_json_value(params)`, donc `/post_body` peut sélectionner un paramètre de collection.
- `HttpClient::send_for_request(...)` envoie le body fourni tel quel pour les requêtes POST dans `server/crates/arachnea-scrapyfy/src/scrapyfy/http_client.rs`.
- Si aucun body explicite n'est résolu, le client POST retombe sur la query string de l'URL ; ce fallback n'est pas nécessaire pour Spys.one et ne doit pas être utilisé ici.

Exemples existants confirmant le support POST :

- `server/services/arachnea-stream-hoster/vidara.yaml` utilise `request_method: post`, `request_body_pointer: /url` et `request_body_actions`.
- `server/services/arachnea-stream/legal-stream/m6play-fr.yaml` utilise `request_method: post` et un body JSON construit par `request_body_actions`.
- `server/services/arachnea-stream/legal-stream/rtbf-auvio-be.yaml` utilise `request_method: post` et un body JSON statique construit par `format_text`.

Configuration Spys.one retenue pour le POST :

```yaml
parameters:
  - name: post_body
    value: "xx00=&xpp=5&xf1=0&xf2=0&xf4=0&xf5=0"

queries:
  - name: list_proxies_for_country
    request_method: post
    request_body_pointer: /post_body
    request_body_select: first
    request_headers:
      - name: Content-Type
        actions:
          - type: format_text
            argument: application/x-www-form-urlencoded
```

Points validés :

- `request_method: post` envoie bien une requête POST pour les scrapers HTML.
- `request_body_pointer: /post_body` peut lire une valeur issue de `parameters`.
- `request_headers` permet de poser `Content-Type: application/x-www-form-urlencoded`.
- Le corps envoyé est exactement `xx00=&xpp=5&xf1=0&xf2=0&xf4=0&xf5=0`.

Décision : aucune nouvelle capacité de body statique n'est nécessaire pour Spys.one.

### Étape 2 : Ajouter le stockage des variables dynamiques `@...`

Objectif : permettre au contexte de requête d'accepter des variables ajoutées par une action.

Statut : **terminé**.

Résultat : le moteur dispose maintenant d'un stockage validé `DynamicTemplateVariables` pour les variables dynamiques préfixées `@`, d'une fusion explicite avec les paramètres standards, d'un stockage partagé dans `QueryContext`, et le résolveur de placeholders accepte la syntaxe `{@Nom}` lorsque la variable est présente dans le contexte de résolution.

Implémenté :

- Un espace de variables dynamique validé pendant l'extraction d'une requête.
- La résolution des placeholders `{@Nom}` via le même mécanisme que `{country}` ou `{base_url}`.
- Une validation empêchant les variables dynamiques sans préfixe `@`.
- Une séparation claire entre variables standards et variables dynamiques.
- Un rejet des paramètres standards utilisant le préfixe réservé `@` lors de la fusion.

Règles minimales :

- `@` est obligatoire pour toute variable ajoutée par action.
- Une variable `@...` ne peut pas remplacer une variable standard.
- Un doublon `@...` déclenche une erreur par défaut.

Constats dans le code :

- `DynamicTemplateVariables` et `build_template_params_with_dynamic_variables(...)` sont définis dans `server/crates/arachnea-scrapyfy/src/scrapyfy/query_helpers.rs`.
- `QueryContext` porte un `dynamic_template_variables` partagé avec les sous-requêtes pendant l'exécution.
- `replace_template_placeholders(...)` reconnaît désormais les placeholders de forme `{@Nom}` en plus des placeholders standards.
- Les tests unitaires couvrent la résolution, le préfixe obligatoire, les doublons et la séparation des namespaces.

### Étape 3 : Implémenter l'action `extract_variables`

Objectif : ajouter une action générique capable d'extraire des paires nom/valeur depuis les valeurs courantes.

Statut : **terminé**.

Résultat : l'action `extract_variables` est disponible dans les pipelines d'extraction. Elle accepte `pattern` ou son alias YAML `regex`, rend `name` et `value` avec les groupes capturés, injecte les variables dynamiques dans le `QueryContext`, et renvoie les valeurs courantes inchangées.

Contrat YAML cible :

```yaml
- type: extract_variables
  regex: "\\b([A-Za-z][A-Za-z0-9_]*)\\s*=\\s*(\\d+)\\s*;"
  name: "@{1}"
  value: "{2}"
```

Comportement :

- Appliquer `regex` sur chaque valeur courante.
- Construire `name` et `value` avec les groupes capturés.
- Valider que `name` commence par `@`.
- Ajouter la paire au contexte dynamique de la requête.
- Conserver les valeurs courantes inchangées pour que l'action reste chaînable.

Validation :

- Regex invalide refusée au chargement.
- `name` vide refusé.
- `name` sans préfixe `@` refusé.
- Doublon refusé par défaut.

Constats dans le code :

- `server/crates/arachnea-scrapyfy/src/scrapyfy/actions/extract_variables.rs` contient l'action et ses tests unitaires.
- `ScraperAction::ExtractVariables` est exposée via `type: extract_variables`.
- Les pipelines HTML, JSON, texte et statiques propagent les erreurs d'action afin qu'un doublon `@...` échoue proprement.

### Étape 4 : Ajouter ou adapter la résolution de variables dans les actions

Objectif : permettre à une action de champ de remplacer les références `{@...}` après extraction d'une expression.

Statut : **terminé**.

Résultat : l'action explicite `replace_variables` est disponible. Elle résout les variables dynamiques du contexte partagé uniquement lorsque le YAML la demande, avec `variable_prefix: "@"` par défaut.

Deux options acceptables :

| Option | Description | Choix recommandé |
|---|---|---|
| Résolution automatique avant chaque action | Tous les placeholders sont remplacés dans les valeurs courantes à chaque étape. | Non, risque de changer des comportements existants. |
| Action explicite `replace_variables` | Le YAML indique où résoudre les placeholders. | Oui, plus lisible et moins risqué. |

Contrat YAML cible :

```yaml
- type: replace_variables
  variable_prefix: "@"
```

Comportement :

- Remplacer uniquement les placeholders dynamiques `{@...}` si `variable_prefix: "@"` est fourni.
- Laisser les placeholders inconnus inchangés ou produire une erreur selon la politique existante du moteur.
- Ne pas modifier la résolution standard dans les URL, headers ou bodies.

Comportement implémenté :

- `variable_prefix` accepte uniquement `@` pour l'instant.
- Les placeholders dynamiques connus sont remplacés depuis `DynamicTemplateVariables`.
- Les placeholders standards comme `{country}` et les placeholders inconnus restent inchangés.
- Les paramètres standards, URLs, headers et bodies ne reçoivent pas de résolution automatique supplémentaire.

Constats dans le code :

- `server/crates/arachnea-scrapyfy/src/scrapyfy/actions/replace_variables.rs` contient l'action et ses tests unitaires.
- `ScraperAction::ReplaceVariables` est exposée via `type: replace_variables`.
- Le branchement utilise le même `dynamic_template_variables` partagé que `extract_variables`.

### Étape 5 : Ajouter ou adapter l'évaluation mathématique

Objectif : calculer les fragments de port après résolution des variables.

Statut : **terminé**.

Résultat : l'action `eval_math` est disponible pour les pipelines de champs. Elle évalue des expressions entières limitées aux nombres positifs, espaces, parenthèses, addition `+` et XOR bitwise `^`. Le moteur ne réutilise pas `mathexpr` pour éviter toute ambiguïté sur `^`.

À vérifier :

- Le moteur mathématique actuel supporte-t-il une action de champ équivalente à `eval_math` ? **Fait : non, ajout d'une action dédiée.**
- Le symbole `^` est-il interprété comme XOR ou puissance ? **Fait : `eval_math` l'interprète explicitement comme XOR bitwise.**
- Les concaténations Spys.one comme `(A^B)+(C^D)` produisent-elles le port attendu sous forme de texte ou de nombre ? **Fait : `js_string_concat: true` concatène les termes de niveau racine après évaluation.**

Décision recommandée :

- Éviter toute ambiguïté sur `^`.
- Si `^` signifie puissance dans le moteur actuel, ajouter un mode explicite `operators: [xor, add]` ou une fonction `xor(a,b)`.
- Garder l'évaluation limitée aux nombres, parenthèses, addition et XOR pour cette intégration.

Comportement implémenté :

- `operators` accepte uniquement `xor` et `add` comme garde déclarative.
- Par défaut, `+` est une addition numérique.
- Avec `js_string_concat: true`, les termes séparés par `+` au niveau racine sont évalués puis concaténés sous forme décimale. Exemple : `(80^0)+(8^0)` produit `808` au lieu de `88`.
- Les opérateurs non prévus, les parenthèses invalides et les entiers invalides produisent une erreur.

Constats dans le code :

- `server/crates/arachnea-scrapyfy/src/scrapyfy/actions/eval_math.rs` contient l'action et ses tests unitaires.
- `ScraperAction::EvalMath` est exposée via `type: eval_math`.

### Étape 6 : Créer le service YAML Spys.one

Objectif : ajouter `server/services/arachnea-proxies/spysone-be.yaml` sans l'activer immédiatement si l'extraction n'est pas validée.

Statut : **terminé**.

Résultat : le fichier `server/services/arachnea-proxies/spysone-be.yaml` a été ajouté et n'est pas encore référencé dans `services.json`.

Contenu minimal :

- `id: spysone-be-free-proxy-list`.
- Requête `list_proxies_for_country`.
- `request_method: post`.
- Body formulaire demandé.
- `row_selector: "tr.spy1x, tr.spy1xx"`.
- Champs `host`, `port`, `protocol`, `country`.
- Champ technique `_script_variables` retiré par `post_build: remove_fields`.

Validation attendue : chaque item retourné doit contenir au minimum `protocol`, `host` et `port`.

Notes d'implémentation :

- Le service utilise `unpack_packer`, `extract_variables`, `replace_variables` et `eval_math`.
- L'action `get_html` a été ajoutée pour lire l'expression `document.write(...)` depuis la cellule adresse.
- `_script_variables` utilise `on_duplicate: replace`, car les variables du script sont request-scoped et la même extraction est rejouée pour chaque ligne HTML.
- `services.json` n'est volontairement pas modifié à cette étape.

### Étape 7 : Mettre à jour la documentation du schéma

Objectif : documenter les nouvelles actions génériques et la convention `@`.

Statut : **terminé**.

Résultat : les spécifications FR et EN documentent les variables dynamiques `@...`, `get_html`, `extract_variables`, `replace_variables`, `eval_math`, `on_duplicate` et le mode `js_string_concat`.

Fichiers à mettre à jour si l'implémentation est réalisée :

- `docs/specifications/arachnea-scrapyfy-fr.md`.
- `docs/specifications/arachnea-scrapyfy-en.md`.

À documenter :

- `extract_variables`.
- `replace_variables` si ajoutée.
- `eval_math` ou l'extension retenue pour le calcul.
- Règles de nommage des variables dynamiques `@...`.
- Politique de doublons.

Fichiers mis à jour :

- `docs/specifications/arachnea-scrapyfy-fr.md`.
- `docs/specifications/arachnea-scrapyfy-en.md`.

### Étape 8 : Activer et valider la source

Objectif : activer Spys.one uniquement après extraction fiable.

À faire :

- Ajouter `spysone-be.yaml` dans `server/services/arachnea-proxies/services.json`.
- Exécuter les validations existantes pertinentes du scraper.
- Vérifier manuellement un échantillon de lignes produites.
- Confirmer que les ports correspondent au rendu HTML attendu.
- Vérifier que les lignes sans port ou protocole valide sont ignorées ou filtrées proprement.

Conclusion : la source Spys.one est intégrable sans exécuteur JavaScript complet. L'approche retenue est une action générique `extract_variables` qui injecte des variables dynamiques `@...` dans le contexte existant, puis une évaluation mathématique déterministe pour calculer les ports.
