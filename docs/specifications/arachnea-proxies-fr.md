# Spécification du format YAML arachnea-proxies

### Classification HTTP runtime — lot 3

Le schéma des listes fournisseur ne change pas. Les options HTTP du scraper et
les options JSON des URLs médias partagent `proxy_response_classifier` :
`{"type":"default"}`, `{"type":"http403"}`, ou
`{"type":"statuses","rejected_statuses":[403],"accepted_statuses":[200,206],"cooldown_seconds":86400}`.
Les statuts 100..599 doivent être disjoints entre acceptation et rejet ; un
cooldown explicite est positif et représentable. Cette politique remplace la
liste historique `proxy_rejection_statuses` ; un handler injecté par code reste
prioritaire. Aucun objet trait n’est sérialisé. La politique suit les URLs de
redirection réécrites, et ne change pas les activations 403 de FranceTV/TF1.

Par défaut, seuls les 2xx créent une préférence. Un 4xx/5xx ou une redirection
non classifiés restent inconclusifs ; un rejet explicite exclut seulement le
couple endpoint/destination du jeton. La classification précède les post-actions,
sans lecture du corps par défaut. Un handler de contenu peut demander un extrait
non destructif de 64 Kio maximum, pendant trois secondes au plus et dans le
timeout restant disponible. Un extrait incomplet ne prouve pas l’absence d’un
marqueur. Les moteurs sans observation précoce corrélée restent sans effet sur
les préférences/exclusions. Les budgets et retries communs restent au lot 5.

### Persistance runtime — lot 2

Sans changement du YAML fournisseur, l’application injecte les vues typées des
tables `proxy-inventory`, `proxy-preferences` et `proxy-failures` d’un seul store
SQLite physique, `<application-data>/data/proxy-inventory.sqlite3`, avec une
connexion partagée et des métadonnées propres à chaque table. Les fichiers
auxiliaires sont adjacents (`proxy-inventory.sqlite3-wal` et `-shm` si présents).
Les anciennes bases `<STORENAME>/records.sqlite3*` ne sont pas reprises ; les
nouvelles bases démarrent vides, puis persistent normalement entre redémarrages.
Les autres stores SQLite utilisent aussi `<STORENAME>.sqlite3*` sans sous-dossier ;
les sessions Cloudflare et les activations/credentials des anciens stores ne
sont donc pas repris. L’historique durable
conserve jusqu’à trois endpoints acceptés par schéma/hôte/port, sans pays dans la
clé ni expiration par âge. Il est restauré avant découverte ; le succès éligible
le plus récent gagne dans les pays autorisés, sans priorité liée à leur ordre.
Les exclusions par destination restent persistées et ne sont pas effacées par
une réussite technique ou une actualisation fournisseur.

L’éviction globale exige un échec de connexion à l’endpoint après 24 heures depuis
la dernière preuve positive, ou la découverte initiale sans preuve positive.
`last_checked` n’est pas une preuve. Avant ce seuil, le cooldown global vaut
15 minutes par défaut, configurable. La table distincte `proxy-failures` conserve au maximum
1 024 états d’éviction pendant 48 heures, nettoyés à la demande, pour préserver
les temporisations lors d’une redécouverte. Aucun worker n’est ajouté. L’ordre
du parcours des autres candidats reste à traiter au lot 4. Le regroupement
physique ne rend pas atomiques les commits d’éviction et de nettoyage des
préférences ; les références restaurées sont vérifiées avant utilisation.

## 1. Aperçu

Une collection arachnea-proxies déclare une ou plusieurs sources de proxys publics
(gratuits, open). Chaque requête produit des enregistrements de type
`application/arachnea-proxy` que le module `ProxyDataProvider` de
`arachnea-scrapyfy` consomme et transforme en `ProxyRecord`.

Ce document ne décrit que les champs propres au format proxy. La structure
générale d'un fichier YAML ( `id`, `parameters`, `http`, `queries`,
`entries`, `actions`, etc. ) est documentée dans
`spec-arachnea-scrapyfy-yaml.md`.

---

## 2. Type média

```yaml
media_types:
  - application/arachnea-proxy
```

Chaque requête de la collection doit déclarer ce type média. Il permet au
moteur de router les résultats vers `ScrapyfyProxyDataProvider`.

---

## 3. Structure des données de sortie

Chaque ligne (item) extraite par le scraper correspond à un proxy unique.
Les champs utilisables sont les suivants :

| Champ YAML | Type YAML | Obligatoire | Description |
|---|---|---|---|
| `protocol` | `string` | conditionnel | Protocole singulier du proxy. Valeurs acceptées : `http`, `https`, `socks4`, `socks4a`, `socks5`. Il est prioritaire lorsqu'il est valide. |
| `protocols` | `string[]` | conditionnel | Protocoles disponibles lorsque la source expose plusieurs valeurs. Le provider en sélectionne un de façon déterministe dans cet ordre : `socks5`, `socks4`, `https`, `socks4a`, puis `http` en priorité la plus basse. Le moteur de sondes utilise le même ordre lorsqu'il doit déterminer le protocole. |
| `host` | `string` | oui | Adresse IPv4, IPv6 ou nom d'hôte du proxy. |
| `port` | `number` | oui | Port d'écoute du proxy. |
| `country` | `string` | non | Code pays ISO (alpha-2, ex: `FR`, `US`). Le provider normalise automatiquement en majuscules. |
| `supports_https` | `boolean` | non | Indique si le proxy supporte le tunnelling HTTPS. |
| `availability` | `string` | non | Indice de disponibilité annoncé par la source. Valeurs : `low`, `medium`, `high`. |
| `latency_ms` | `number` | non | Latence mesurée par la source, en millisecondes. |
| `failure_count` | `number` | non | Nombre d'échecs consécutifs rapportés par la source (défaut: `0`). |
| `authentication_required` | `boolean` | non | Indique si le proxy nécessite une authentification. |

Les champs `host` et `port` sont obligatoires. Au moins l'un des champs
`protocol` ou `protocols` est fortement recommandé — sans protocole reconnu,
le proxy ne peut pas être converti en nœud de transport et sera ignoré lors de
la construction des nœuds de transport.

Les champs `status`, `destination_failures`, `last_checked` et
`cooldown_until` sont gérés par le moteur de sondes et ne doivent pas être
déclarés dans le YAML source.

### 3.1 Résultat groupé avec `result_item_field`

Les deux sources de référence (`iplocate`, `proxifly`) encapsulent les proxys
dans un groupe nommé `proxies` et utilisent `result_item_field: proxies`
pour aplatir ce groupe en lignes de résultat :

```yaml
result_item_field: proxies
entries:
  - name: proxies
    type: object[]
    # ... sous-entrées (protocol, host, port, etc.)
```

---

## 4. Exemple simple

### 4.1 Source texte (délimiteurs `:`)

```yaml
id: ma-source-proxy
title: "Ma liste de proxys"
http:
  mode: auto

parameters:
  - name: base_url
    value: https://example.com/proxies

queries:
  - name: list_proxies_for_country
    scraper_type: text
    base_url: "{base_url}"
    query_url: "{base_url}/{country}/proxies.txt"
    media_types:
      - application/arachnea-proxy
    result_item_field: proxies
    row_delimiter: "\n"
    field_delimiter: ":"
    entries:
      - name: proxies
        type: object[]
        entries:
          - name: protocol
            type: string
            field: 1
          - name: host
            type: string
            field: 2
            actions:
              - type: replace_text
                search: "//"
                replace: ""
          - name: port
            type: number
            field: 3
```

### 4.2 Source JSON

```yaml
id: ma-source-proxy
title: "Proxys depuis une API JSON"
http:
  mode: auto

parameters:
  - name: base_url
    value: https://api.example.com/proxies

queries:
  - name: list_proxies_for_country
    scraper_type: json
    base_url: "{base_url}"
    query_url: "{base_url}/countries/{country}/data.json"
    media_types:
      - application/arachnea-proxy
    row_pointer: "/*"
    result_item_field: proxies
    entries:
      - name: proxies
        type: object[]
        pointer: /
        select: all
        entries:
          - name: protocol
            type: string
            pointer: /protocol
          - name: host
            type: string
            pointer: /ip
          - name: port
            type: number
            pointer: /port
          - name: supports_https
            type: boolean
            pointer: /https
          - name: country
            type: string
            pointer: /geolocation/country
```

### 4.3 Source HTML

```yaml
id: ma-source-proxy
title: "Proxys depuis une page HTML"
http:
  mode: auto

parameters:
  - name: base_url
    value: https://example.com/proxies

queries:
  - name: list_proxies_for_country
    scraper_type: html
    base_url: "{base_url}"
    query_url: "/{country}"
    media_types:
      - application/arachnea-proxy
    result_item_field: proxies
    row_selector: "table.proxy-list tr"
    entries:
      - name: proxies
        type: object[]
        entries:
          - name: protocol
            type: string
            selector: "td.protocol"
            actions:
              - type: get_text
          - name: host
            type: string
            selector: "td.ip"
            actions:
              - type: get_text
          - name: port
            type: number
            selector: "td.port"
            actions:
              - type: get_text
          - name: supports_https
            type: boolean
            selector: "td.https"
            actions:
              - type: get_text
```

---

## 5. Correspondance avec `ProxyRecord`

Les champs extraits par le scraper sont mappés dans le code Rust comme suit :

- `protocol` → `ProxyProtocol` (`http`, `https`, `socks4`, `socks4a`, `socks5`)
- `protocols` → premier `ProxyProtocol` disponible selon la priorité déterministe `socks5`, `socks4`, `https`, `socks4a`, puis `http` ; le moteur de sondes utilise le même ordre pour déterminer un protocole inconnu et un champ singulier `protocol` valide reste prioritaire
- `host` → `ProxyRecord.host` (`String`)
- `port` → `ProxyRecord.port` (`u16`)
- `country` → normalisé en `ProxyRecord.country` (`Option<String>`, majuscules)
- `supports_https` → `ProxyRecord.supports_https` (`Option<bool>`)
- `availability` → `ProxyAvailabilityHint` (`"low"`/`"medium"`/`"high"`)
- `latency_ms` → `ProxyRecord.latency_ms` (`Option<u64>`)
- `failure_count` → `ProxyRecord.failure_count` (`u32`)
- `authentication_required` → `ProxyRecord.authentication_required` (`Option<bool>`)

Les champs `status`, `destination_failures`, `last_checked` et `cooldown_until`
sont réservés au moteur interne et ne proviennent pas du fichier YAML.

---

## 6. Convention de nommage des requêtes

Par convention, la requête principale d'une collection proxy s'appelle
`list_proxies_for_country` et accepte un paramètre `country` correspondant au
code pays ISO. C'est le nom attendu par `ScrapyfyProxyDataProvider` dans
`proxy_provider.rs`.
