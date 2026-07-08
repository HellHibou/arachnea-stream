# Spécification du format YAML arachnea-proxies

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
| `protocol` | `string` | oui | Protocole du proxy. Valeurs acceptées : `http`, `https`, `socks4`, `socks4a`, `socks5`. |
| `host` | `string` | oui | Adresse IPv4, IPv6 ou nom d'hôte du proxy. |
| `port` | `number` | oui | Port d'écoute du proxy. |
| `country` | `string` | non | Code pays ISO (alpha-2, ex: `FR`, `US`). Le provider normalise automatiquement en majuscules. |
| `supports_https` | `boolean` | non | Indique si le proxy supporte le tunnelling HTTPS. |
| `availability` | `string` | non | Indice de disponibilité annoncé par la source. Valeurs : `low`, `medium`, `high`. |
| `latency_ms` | `number` | non | Latence mesurée par la source, en millisecondes. |
| `failure_count` | `number` | non | Nombre d'échecs consécutifs rapportés par la source (défaut: `0`). |
| `authentication_required` | `boolean` | non | Indique si le proxy nécessite une authentification. |

Les champs `host` et `port` sont obligatoires. `protocol` est fortement
recommandé — sans lui, le proxy ne peut pas être converti en nœud de transport
et sera ignoré.

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
