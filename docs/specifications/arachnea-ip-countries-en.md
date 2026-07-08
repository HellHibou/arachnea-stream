# arachnea-ip-countries YAML format specification

## 1. Overview

An arachnea-ip-countries collection declares one or more IP geolocation
sources. Each query produces records of type `application/arachnea-ip-country`
that the `ScrapyfyIpCountryDataProvider` module in `arachnea-scrapyfy`
consumes to resolve the country code of an IP address.

This document only describes fields specific to the IP-country format. The
general structure of a YAML file (`id`, `parameters`, `http`, `queries`,
`entries`, `actions`, etc.) is documented in `arachnea-scrapyfy-yaml.md`.

---

## 2. Media type

```yaml
media_types:
  - application/arachnea-ip-country
```

Every query in the collection must declare this media type. It enables the
engine to route results to `ScrapyfyIpCountryDataProvider`.

---

## 3. Output data structure

The `resolve_ip_country` query receives an `ip` parameter (the address to
resolve) and returns a single JSON object containing at least the country
code. The following fields are available:

| YAML field | YAML type | Required | Description |
|---|---|---|---|
| `country_code` | `string` | yes | ISO 3166-1 alpha-2 country code (e.g. `FR`, `US`). The provider normalises to uppercase. |
| `query_ip` | `string` | no | The IP address that was queried (useful for debugging or verification). |

The `country_code` field is required. Without it the record is ignored by
the provider.

The `row_pointer: "/"` setting indicates that the JSON response is a single
object (not an array). With this setting, the scraper treats the root object
as the single result row.

---

## 4. Query parameter

The `resolve_ip_country` query expects an `ip` parameter passed through
execution parameters. This parameter is injected into the `query_url`
template:

```yaml
query_url: "{base_url}/{ip}"
```

Example call from the Rust provider:

```rust
params.insert("ip".to_string(), ip_str.clone());
// → Resolves to: http://ip-api.com/json/8.8.8.8
```

---

## 5. Complete example

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

## 6. Mapping to `IpCountryRecord`

Fields extracted by the scraper map to Rust code as follows:

- `country_code` → `IpCountryRecord.country` (`String`, normalised to uppercase)
- The IP address comes from the call parameter, not from the YAML result
- `source` is set statically in code (e.g. `"ip-api.com"`)

```rust
IpCountryRecord {
    ip: *expected_ip,               // from the call parameter
    country: country.trim().to_ascii_uppercase(),  // normalised
    source: Some("ip-api.com"),     // set in the provider
}
```

---

## 7. Naming conventions

### 7.1 Collection group

The collection group used by `ScrapyfyIpCountryDataProvider` is
`arachnea-ip-countries` (constant `IP_COUNTRY_GROUP_NAME` in the code).

### 7.2 Query

The query must be named `resolve_ip_country`. This is the name expected by
both `ScrapyfyIpCountryDataProvider` and `ScrapyfyProxyDataProvider` when
they call the scraper engine.

### 7.3 HTTP mode

Geolocation APIs are typically simple services without Cloudflare protection.
The `direct` mode is recommended:

```yaml
http:
  mode: direct
```

---

## 8. Persistence

IP-country resolutions are persisted in a JSON file (default
`data/ip-countries.json`). The file is atomically rewritten via a temporary
file (`data/ip-countries.json.tmp`) to prevent corruption on crash. The
codec used is JSON pretty-print (`JsonIpCountryCodec`).

Persistent file format:

```json
[
  {
    "ip": "8.8.8.8",
    "country": "US",
    "source": "resolver"
  }
]
```

The `source` field is `"resolver"` for entries added during on-the-fly
resolution, and may be `"ip-api.com"` or other values for bulk refreshes.
