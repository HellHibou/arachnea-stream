# arachnea-proxies YAML format specification

## 1. Overview

An arachnea-proxies collection declares one or more public (free, open) proxy
sources. Each query produces records of type `application/arachnea-proxy`
that the `ProxyDataProvider` module in `arachnea-scrapyfy` consumes and
transforms into `ProxyRecord`.

This document only describes fields specific to the proxy format. The general
structure of a YAML file (`id`, `parameters`, `http`, `queries`, `entries`,
`actions`, etc.) is documented in [the common YAML specification](arachnea-scrapyfy-en.md).

---

## 2. Media type

```yaml
media_types:
  - application/arachnea-proxy
```

Every query in the collection must declare this media type. It enables the
engine to route results to `ScrapyfyProxyDataProvider`.

---

## 3. Output data structure

Each line (item) extracted by the scraper corresponds to a single proxy.
The following fields are available:

| YAML field | YAML type | Required | Description |
|---|---|---|---|
| `protocol` | `string` | conditionally | Singular proxy protocol. Accepted values: `http`, `https`, `socks4`, `socks4a`, `socks5`. Takes precedence when valid. |
| `protocols` | `string[]` | conditionally | Available proxy protocols when the source exposes several values. The provider selects one deterministically in this order: `socks5`, `socks4`, `https`, `socks4a`, then lowest-priority `http`. The probe engine uses the same order when detecting a protocol. |
| `host` | `string` | yes | IPv4 address, IPv6 address, or hostname of the proxy. |
| `port` | `number` | yes | Listening port of the proxy. |
| `country` | `string` | no | ISO country code (alpha-2, e.g. `FR`, `US`). The provider normalises to uppercase. |
| `supports_https` | `boolean` | no | Whether the proxy supports HTTPS tunnelling. |
| `availability` | `string` | no | Availability hint from the source. Values: `low`, `medium`, `high`. |
| `latency_ms` | `number` | no | Latency measured by the source, in milliseconds. |
| `failure_count` | `number` | no | Consecutive failure count reported by the source (default: `0`). |
| `proxy_source` | `string` | no | Provenance label stored with the declaration. Normal proxy discovery injects the collection ID; other callers default to `unknown` when absent. |
| `authentication_required` | `boolean` | no | Whether the proxy requires authentication. |

`host` and `port` are required. At least one of `protocol` or `protocols` is
strongly recommended — without a recognised protocol the proxy cannot be
converted to a transport node and will be ignored when transport nodes are built.

The fields `status`, `destination_failures`, `last_checked` and
`cooldown_until` are managed by the probe engine and must not appear in
YAML source files.

### 3.1 Grouped result with `result_item_field`

Both reference sources (`iplocate`, `proxifly`) wrap proxies in a group named
`proxies` and use `result_item_field: proxies` to flatten this group into
result rows:

```yaml
result_item_field: proxies
entries:
  - name: proxies
    type: object[]
    # ... sub-entries (protocol, host, port, etc.)
```

---

## 4. Simple examples

### 4.1 Text source (`:` delimiter)

```yaml
id: my-proxy-source
title: "My Proxy List"
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

### 4.2 JSON source

```yaml
id: my-proxy-source
title: "Proxies from a JSON API"
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

### 4.3 HTML source

```yaml
id: my-proxy-source
title: "Proxies from an HTML page"
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

## 5. Mapping to `ProxyRecord`

Fields extracted by the scraper map to Rust code as follows:

- `protocol` → `ProxyProtocol` (`http`, `https`, `socks4`, `socks4a`, `socks5`)
- `protocols` → the first available `ProxyProtocol` according to the deterministic priority `socks5`, `socks4`, `https`, `socks4a`, then `http`; the probe engine uses the same order to detect an unknown protocol, while a valid singular `protocol` takes precedence
- `host` → `ProxyRecord.host` (`String`)
- `port` → `ProxyRecord.port` (`u16`)
- `country` → normalised to `ProxyRecord.country` (`Option<String>`, uppercase)
- `supports_https` → `ProxyRecord.supports_https` (`Option<bool>`)
- `availability` → `ProxyAvailabilityHint` (`"low"`/`"medium"`/`"high"`)
- `latency_ms` → `ProxyRecord.latency_ms` (`Option<u64>`)
- `failure_count` → `ProxyRecord.failure_count` (`u32`)
- `authentication_required` → `ProxyRecord.authentication_required` (`Option<bool>`)

The fields `status`, `destination_failures`, `last_checked` and
`cooldown_until` are reserved for the internal probe engine and do not
come from YAML files.

---

## 6. Query naming convention

By convention, the main query of a proxy collection is named
`list_proxies_for_country` and accepts a `country` parameter matching an
ISO country code. This is the name expected by `ScrapyfyProxyDataProvider`
by the proxy provider.

## 7. Authoring and validation procedure

1. Create a YAML file in `server/services/arachnea-proxies/` and register its
   relative path in that directory's `services.json` as described in the common
   specification. The collection ID is independent of the file name.
2. Implement `list_proxies_for_country`; it receives `{country}`, normalized to
   an uppercase two-letter country code. Declare
   `media_types: [application/arachnea-proxy]`.
3. Extract `host`, numeric `port` and `protocol` or `protocols`. Protocol names are
   trimmed and compared case-insensitively. A valid `protocol` takes precedence;
   otherwise the provider considers `protocols` in its documented priority order.
4. Emit one proxy per result row. For a repeated nested `proxies` group, set
   `result_item_field: proxies`; otherwise the provider cannot read `host` and
   `port` at row level. Verify the final flattened JSON.
5. Extract `country` from source evidence. Do not fill it with `{country}` merely
   because that country was requested. The provider only publishes records
   matching the requested country. Missing/invalid country hints require IP
   geolocation; hostnames without a country cannot use this IP-only fallback.
6. Normal proxy discovery injects `proxy_source` from the collection ID. `supports_https` is a source
   declaration, not proof of a successful tunnel; probing owns capabilities.
7. Validate both a populated response and an empty-country response. Keep disabled
   providers parseable. Activation does not establish availability or reliability.

A blank host or a port that cannot be converted to an unsigned 16-bit integer
causes the row to be discarded. Use actual listening ports in `1..65535`.
An invalid country hint is discarded. At most 20 distinct unknown IP countries
are scheduled for geolocation per country load; this is a runtime bound, not a
YAML configuration field. Unknown countries are never inferred from the request.
HTTP engine and proxy routing options belong to the common specification;
provider YAML does not configure inventory persistence or probe scheduling.

### Existing service examples

These optional examples illustrate provider-specific patterns. The complete
authoring contract remains in this guide and the common specification; do not
copy a provider URL, identifier or selector without checking the target response.

- [iplocate-free-proxy-list.yaml](../../server/services/arachnea-proxies/iplocate-free-proxy-list.yaml) — JSON proxy groups flattened into result rows.
- [spysone.yaml](../../server/services/arachnea-proxies/spysone.yaml) — HTML extraction and JavaScript-derived values.
