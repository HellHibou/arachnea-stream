# arachnea-proxies YAML format specification

## 1. Overview

An arachnea-proxies collection declares one or more public (free, open) proxy
sources. Each query produces records of type `application/arachnea-proxy`
that the `ProxyDataProvider` module in `arachnea-scrapyfy` consumes and
transforms into `ProxyRecord`.

This document only describes fields specific to the proxy format. The general
structure of a YAML file (`id`, `parameters`, `http`, `queries`, `entries`,
`actions`, etc.) is documented in `arachnea-scrapyfy-yaml.md`.

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
| `protocol` | `string` | yes | Proxy protocol. Accepted values: `http`, `https`, `socks4`, `socks4a`, `socks5`. |
| `host` | `string` | yes | IPv4 address, IPv6 address, or hostname of the proxy. |
| `port` | `number` | yes | Listening port of the proxy. |
| `country` | `string` | no | ISO country code (alpha-2, e.g. `FR`, `US`). The provider normalises to uppercase. |
| `supports_https` | `boolean` | no | Whether the proxy supports HTTPS tunnelling. |
| `availability` | `string` | no | Availability hint from the source. Values: `low`, `medium`, `high`. |
| `latency_ms` | `number` | no | Latency measured by the source, in milliseconds. |
| `failure_count` | `number` | no | Consecutive failure count reported by the source (default: `0`). |
| `authentication_required` | `boolean` | no | Whether the proxy requires authentication. |

`host` and `port` are required. `protocol` is strongly recommended — without
it the proxy cannot be converted to a transport node and will be ignored.

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
in `proxy_provider.rs`.
