# Spécification du format YAML arachnea-proxies

## 1. Aperçu

Une collection arachnea-proxies déclare une ou plusieurs sources de proxys publics
(gratuits, open). Chaque requête produit des enregistrements de type
`application/arachnea-proxy` que le module `ProxyDataProvider` de
`arachnea-scrapyfy` consomme et transforme en `ProxyRecord`.

Ce document ne décrit que les champs propres au format proxy. La structure
générale d'un fichier YAML ( `id`, `parameters`, `http`, `queries`,
`entries`, `actions`, etc. ) est documentée dans
[la spécification YAML commune](arachnea-scrapyfy-fr.md).

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
| `proxy_source` | `string` | non | Provenance conservée avec la déclaration. La découverte normale injecte l’ID de collection ; les autres appelants utilisent `unknown` si le champ est absent. |
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
code pays ISO. C'est le nom attendu par `ScrapyfyProxyDataProvider` par ce fournisseur.

## 7. Procédure de rédaction et de validation

1. Créer un YAML dans `server/services/arachnea-proxies/` et inscrire son chemin
   relatif dans le `services.json` du dossier, selon la spécification commune.
   L'ID de collection est indépendant du nom de fichier.
2. Implémenter `list_proxies_for_country` ; cette requête reçoit `{country}`,
   normalisé en code pays de deux lettres majuscules. Déclarer
   `media_types: [application/arachnea-proxy]`.
3. Extraire `host`, `port` numérique et `protocol` ou `protocols`. Les noms de
   protocoles sont nettoyés et comparés sans tenir compte de la casse. Un
   `protocol` valide est prioritaire ; sinon le fournisseur examine `protocols`
   dans l'ordre de priorité documenté.
4. Émettre un proxy par ligne de résultat. Pour un groupe `proxies` répété,
   définir `result_item_field: proxies` ; sinon le fournisseur ne peut pas lire
   `host` et `port` à la racine de la ligne. Vérifier le JSON final aplati.
5. Extraire `country` à partir des données de la source. Ne pas le remplir avec
   `{country}` uniquement parce que ce pays était demandé. Le fournisseur publie
   seulement les enregistrements du pays demandé. Une indication absente ou
   invalide exige une géolocalisation IP ; les noms d'hôtes sans pays ne peuvent
   pas utiliser ce repli limité aux IP.
6. La découverte normale injecte `proxy_source` depuis l'ID de collection.
   `supports_https` est une déclaration de la source, pas la preuve d'un tunnel
   fonctionnel ; les sondes déterminent les capacités.
7. Vérifier une réponse remplie et une réponse vide pour un pays. Maintenir les
   fournisseurs désactivés analysables. L'activation ne prouve ni disponibilité
   ni fiabilité.

Un hôte vide ou un port non convertible en entier non signé de 16 bits fait
ignorer la ligne. Utiliser des ports d'écoute réels dans `1..65535`.
Une indication de pays invalide est supprimée. Au plus 20 pays d'IP inconnues
distinctes sont recherchés par chargement d'un pays ; c'est une limite d'exécution,
pas un champ YAML. Un pays inconnu n'est jamais déduit de la demande.
Les options HTTP et de routage proxy appartiennent à la spécification commune ;
le YAML fournisseur ne configure ni la persistance de l'inventaire ni les sondes.

### Exemples de services existants

Ces exemples facultatifs illustrent des modèles propres aux fournisseurs. Le
contrat complet reste dans ce guide et la spécification commune ; ne pas copier
une URL, un identifiant ou un sélecteur sans vérifier la réponse de la cible.

- [iplocate-free-proxy-list.yaml](../../server/services/arachnea-proxies/iplocate-free-proxy-list.yaml) — Groupes de proxys JSON aplatis en lignes de résultat.
- [spysone.yaml](../../server/services/arachnea-proxies/spysone.yaml) — Extraction HTML et valeurs calculées par JavaScript.
