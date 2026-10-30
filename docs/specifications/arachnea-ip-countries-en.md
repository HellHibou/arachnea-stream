# arachnea-ip-countries YAML format specification

## 1. Overview

An arachnea-ip-countries collection declares one or more IP geolocation
sources. Each query produces records of type `application/arachnea-ip-country`
that the `ScrapyfyIpCountryDataProvider` module in `arachnea-scrapyfy`
consumes to resolve the country code of an IP address.

This document only describes fields specific to the IP-country format. The
general structure of a YAML file (`id`, `parameters`, `http`, `queries`,
`entries`, `actions`, etc.) is documented in [the common YAML specification](arachnea-scrapyfy-en.md).

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
resolve) and returns a result row containing at least the country
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

For example, with `ip=8.8.8.8` and `base_url=http://ip-api.com/json`,
this requests `http://ip-api.com/json/8.8.8.8`.

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



---

## 7. Naming conventions

### 7.1 Collection group

The collection group used by `ScrapyfyIpCountryDataProvider` is
`arachnea-ip-countries` (constant `IP_COUNTRY_GROUP_NAME` in the code).

### 7.2 Query

The explicit-address query must be named `resolve_ip_country`. The optional
outbound-country query is `resolve_current_country` (section 9). These names are expected by
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

## 9. Current outbound-country query

`resolve_current_country` determines the country of the application's public
outbound connection. It takes **no `ip` input**. Query the provider's endpoint
for the calling IP rather than interpolating an absent `{ip}`. Return the same
`country_code` and optional `query_ip` fields as `resolve_ip_country`.

Add this second query to the complete example's `queries` list:

```yaml
  - name: resolve_current_country
    scraper_type: json
    base_url: "{base_url}"
    query_url: "{base_url}"
    media_types: [application/arachnea-ip-country]
    row_pointer: "/"
    entries:
      - name: country_code
        type: string
        pointer: /countryCode
      - name: query_ip
        type: string
        pointer: /query
```

The runtime trims and uppercases the extracted country string; it does not
validate membership in the ISO list here. Emit a nonblank alpha-2 code only.
Provider failure messages, arbitrary strings and whitespace-only responses are
not valid countries. `query_ip` is diagnostic: batch records use the requested
IP and do not validate it against `query_ip`. Do not use that field as a trust check.

## 10. Authoring and validation checklist

1. Place the YAML in `server/services/arachnea-ip-countries/` and register it in
   `services.json`, using the common specification's manifest format.
2. Implement `resolve_ip_country` for explicit addresses and
   `resolve_current_country` if outbound-country detection is needed.
3. Keep `http.mode: direct` for the reference geolocation provider; routing the
   detection request through a proxy would observe that proxy's outbound country.
4. Declare the provider media type on both queries. Use `row_pointer: "/"` for
   a root object, and a pointer to repeated objects only for an array response.
5. Confirm that unsupported IPs and provider errors cannot be mistaken for
   countries. The consumer uses the first row with a non-empty country value.
6. Declare only parameters actually interpolated by the query. In the reference
   service, `fields` is declared but is not appended to either URL; its presence
   alone does not alter the provider's HTTP response.
7. Check valid YAML, final JSON, manifest activation and live API output separately.
   Persistence is managed by the application; it is not a service YAML field.

### Existing service examples

These optional examples illustrate provider-specific patterns. The complete
authoring contract remains in this guide and the common specification; do not
copy a provider URL, identifier or selector without checking the target response.

- [ip-api.yaml](../../server/services/arachnea-ip-countries/ip-api.yaml) — Explicit-IP and outbound-country queries.
