# Spécification du format YAML arachnea-ip-countries

## 1. Aperçu

Une collection arachnea-ip-countries déclare une ou plusieurs sources de
géo-localisation d'adresses IP. Chaque requête produit des enregistrements de
type `application/arachnea-ip-country` que le module
`ScrapyfyIpCountryDataProvider` de `arachnea-scrapyfy` consomme pour résoudre
le code pays d'une adresse IP.

Ce document ne décrit que les champs propres au format IP-country. La structure
générale d'un fichier YAML ( `id`, `parameters`, `http`, `queries`,
`entries`, `actions`, etc. ) est documentée dans
`spec-arachnea-scrapyfy-yaml.md`.

---

## 2. Type média

```yaml
media_types:
  - application/arachnea-ip-country
```

Chaque requête de la collection doit déclarer ce type média. Il permet au
moteur de router les résultats vers `ScrapyfyIpCountryDataProvider`.

---

## 3. Structure des données de sortie

La requête `resolve_ip_country` reçoit un paramètre `ip` (l'adresse à
résoudre) et retourne un objet JSON unique contenant au moins le code pays.
Les champs utilisables sont les suivants :

| Champ YAML | Type YAML | Obligatoire | Description |
|---|---|---|---|
| `country_code` | `string` | oui | Code pays ISO 3166-1 alpha-2 (ex: `FR`, `US`). Le provider normalise automatiquement en majuscules. |
| `query_ip` | `string` | non | Adresse IP qui a été interrogée (utile pour débogage ou vérification). |

Le champ `country_code` est obligatoire. Sans lui, l'enregistrement est
ignoré par le provider.

La structure `row_pointer: "/"` indique que la réponse JSON est un objet
unique (pas un tableau). Avec ce réglage, le scraper traite l'objet racine
comme l'unique ligne de résultat.

---

## 4. Paramètre de requête

La requête `resolve_ip_country` attend un paramètre `ip` passé via les
paramètres d'exécution. Ce paramètre est injecté dans le template
`query_url` :

```yaml
query_url: "{base_url}/{ip}"
```

Exemple d'appel depuis le provider Rust :

```rust
params.insert("ip".to_string(), ip_str.clone());
// → Résout en : http://ip-api.com/json/8.8.8.8
```

---

## 5. Exemple complet

```yaml
id: ip-api
title: "ip-api.com IP geolocation"
description:
  en: "Free IP geolocation API that returns country code for any public IP address."
http:
  mode: direct

parameters:
  - name: base_url
    value: http://ip-api.com/json
    description: Base URL for the ip-api.com JSON endpoint.
  - name: fields
    value: query,countryCode
    description: Response fields to request from the API.

queries:
  - name: resolve_ip_country
    scraper_type: json
    base_url: "{base_url}"
    query_url: "{base_url}/{ip}"
    media_types:
      - application/arachnea-ip-country
    row_pointer: "/"
    entries:
      - name: country_code
        type: string
        pointer: /countryCode
      - name: query_ip
        type: string
        pointer: /query
```

---

## 6. Correspondance avec `IpCountryRecord`

Les champs extraits par le scraper sont mappés dans le code Rust comme suit :

- `country_code` → `IpCountryRecord.country` (`String`, normalisé en majuscules)
- L'adresse IP provient du paramètre d'appel, pas du résultat YAML
- `source` est défini statiquement dans le code (ex: `"ip-api.com"`)

```rust
IpCountryRecord {
    ip: *expected_ip,               // issu du paramètre d'appel
    country: country.trim().to_ascii_uppercase(),  // normalisé
    source: Some("ip-api.com"),     // défini dans le provider
}
```

---

## 7. Convention de nommage

### 7.1 Groupe de collection

Le groupe de collection utilisé par `ScrapyfyIpCountryDataProvider` est
`arachnea-ip-countries` (constante `IP_COUNTRY_GROUP_NAME` dans le code).

### 7.2 Requête

La requête doit s'appeler `resolve_ip_country`. C'est le nom attendu par
`ScrapyfyIpCountryDataProvider` et `ScrapyfyProxyDataProvider` lorsqu'ils
appellent le moteur de scraping.

### 7.3 Mode HTTP

Les API de géo-localisation sont généralement des services simples sans
protection Cloudflare. Le mode `direct` est recommandé :

```yaml
http:
  mode: direct
```

---

## 8. Persistance

Les résolutions IP-pays sont persistées dans un fichier JSON (par défaut
`data/ip-countries.json`). Le fichier est atomiquement réécrit via un
temporaire (`data/ip-countries.json.tmp`) pour éviter la corruption en cas
de crash. Le codec utilisé est le JSON pretty-print (`JsonIpCountryCodec`).

Format du fichier persistant :

```json
[
  {
    "ip": "8.8.8.8",
    "country": "US",
    "source": "resolver"
  }
]
```

Le champ `source` vaut `"resolver"` pour les entrées ajoutées lors d'une
résolution à la volée, et peut valoir `"ip-api.com"` ou autre pour les
rafraîchissements en masse.
