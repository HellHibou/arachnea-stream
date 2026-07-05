# Implémentation du chargement dynamique de proxies via execute_query_async

## Contexte

Le fichier `server/crates/arachnea-scrapyfy/src/scrapyfy/proxy_provider.rs` contient
`ScrapyfyProxyDataProvider::load_proxies()` avec un TODO à la ligne 48. Actuellement,
la méthode retourne un seul proxy FR statique codé en dur.

`execute_query_async` dans `scraper_agregator.rs:365` permet d'exécuter une requête
YAML sur un groupe de sources et retourne `Vec<HashMap<String, ScraperDataNode>>`.
Chaque `HashMap` représente un item dont les clés sont les noms de champs extraits
par le YAML.

`ProxyRecord` (dans `proxy_record.rs:115`) contient des champs dont les noms
correspondent directement aux noms attendus dans la réponse YAML.

## Modifications nécessaires

### 1. Ajouter des helpers d'extraction sur `ScraperDataNode`

Dans `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_data_node.rs`, ajouter
dans l'impl block existant les méthodes suivantes :

```rust
/// Returns the first scalar value as a `&str`, or `None` if the node is empty.
pub fn value_as_string(&self) -> Option<&str> {
    self.values.first().map(String::as_str)
}

/// Returns the first scalar value parsed as `u16`, or `None`.
pub fn value_as_u16(&self) -> Option<u16> {
    self.values.first().and_then(|v| v.parse().ok())
}

/// Returns the first scalar value parsed as `u32`, or `None`.
pub fn value_as_u32(&self) -> Option<u32> {
    self.values.first().and_then(|v| v.parse().ok())
}

/// Returns the first scalar value parsed as `u64`, or `None`.
pub fn value_as_u64(&self) -> Option<u64> {
    self.values.first().and_then(|v| v.parse().ok())
}

/// Returns the first scalar value parsed as `bool`, or `None`.
pub fn value_as_bool(&self) -> Option<bool> {
    self.values.first().and_then(|v| v.parse().ok())
}
```

Tous les champs sont plats (pas de `>` dans les noms de champs YAML). Les helpers
ci-dessus suffisent — pas besoin de `get_*` par clé enfant.

### 2. Signature modifiée de `load_proxies`

```rust
async fn load_proxies(&self, request: ProxyLoadRequest) -> Result<Vec<ProxyRecord>> {
    // SAFETY: the raw pointer is valid for the lifetime of the provider
    // because the ScraperAgregator owns the core which owns the inventory
    // which owns this provider.
    let agregator = unsafe { &*self.scraper_agregator };

    let results = agregator
        .execute_query_async(
            "arachnea-proxies", // group_name
            "list_proxies",     // query_name
            &HashMap::new(),    // params — aucun paramètre de template YAML
            None,               // source_params
            None,               // scrapper_list
            None,               // query_media_type_filter
            None,               // fields_filters
            Some("source"),     // source_field_name
        )
        .await?;

    let records: Vec<ProxyRecord> = results
        .into_iter()
        .filter_map(|entry| entry_to_proxy_record(&entry))
        .collect();

    // Filtre Rust-side par pays si demandé
    let country = request.country;
    let filtered = if let Some(ref country) = country {
        records
            .into_iter()
            .filter(|r| r.country.as_deref() == Some(country.as_str()))
            .collect()
    } else {
        records
    };

    // Déduplication par authority normalisée (host:port)
    let mut seen = HashSet::new();
    let deduped = filtered
        .into_iter()
        .filter(|r| {
            let authority = format!("{}:{}", r.host, r.port);
            seen.insert(authority)
        })
        .collect();

    Ok(deduped)
}
```

### 3. Conversion `HashMap<String, ScraperDataNode>` → `ProxyRecord`

Chaque entrée est une `HashMap<String, ScraperDataNode>` où les clés sont les
noms de champs extraits par le YAML (`protocol`, `host`, `port`, etc.) et les
valeurs sont des `ScraperDataNode` scalaires (un seul élément dans `values`).

```rust
fn entry_to_proxy_record(entry: &HashMap<String, ScraperDataNode>) -> Option<ProxyRecord> {
    let host = entry.get("host")?.value_as_string()?;
    if host.is_empty() {
        return None;
    }

    let port = entry.get("port")?.value_as_u16()?;
    let protocol = entry
        .get("protocol")
        .and_then(|node| node.value_as_string())
        .and_then(|s| match s.to_lowercase().as_str() {
            "http" => Some(ProxyProtocol::Http),
            "https" => Some(ProxyProtocol::Https),
            "socks4" => Some(ProxyProtocol::Socks4),
            "socks4a" => Some(ProxyProtocol::Socks4a),
            "socks5" => Some(ProxyProtocol::Socks5),
            _ => None,
        });

    let supports_https = entry.get("supports_https").and_then(|n| n.value_as_bool());
    let availability = entry
        .get("availability")
        .and_then(|node| node.value_as_string())
        .map(|s| match s.to_lowercase().as_str() {
            "low" => ProxyAvailabilityHint::Low,
            "medium" => ProxyAvailabilityHint::Medium,
            "high" => ProxyAvailabilityHint::High,
            _ => ProxyAvailabilityHint::Unknown,
        })
        .unwrap_or(ProxyAvailabilityHint::Unknown);

    Some(ProxyRecord {
        protocol,
        host: host.to_string(),
        port,
        country: entry.get("country").and_then(|n| n.value_as_string()).map(|s| s.to_string()),
        supports_https,
        status: ProxyRuntimeStatus::Unknown,
        latency_ms: entry.get("latency_ms").and_then(|n| n.value_as_u64()),
        failure_count: entry.get("failure_count").and_then(|n| n.value_as_u32()).unwrap_or(0),
        authentication_required: entry.get("authentication_required").and_then(|n| n.value_as_bool()),
        availability,
        destination_failures: Vec::new(),
        source: entry.get("source").and_then(|n| n.value_as_string()).map(|s| s.to_string()),
        last_checked: None,
        cooldown_until: None,
    })
}
```

Note : le champ `country` dans la réponse YAML est optionnel. Le filtre Rust-side
par pays compare `country.as_deref() == Some(country)`. Un record sans `country`
ne matchera jamais un filtre pays et sera simplement exclu si un pays est demandé.

### Points d'attention

1. **Nom du groupe** : `"arachnea-proxies"` passé comme `group_name` doit
   correspondre à un groupe défini dans le registre de sources
   (`server/services/proxies/services.json`).

2. **Nom de la query** : `"list_proxies"` passé comme `query_name` doit
   correspondre à une query définie dans les YAML de sources proxy.

3. **Champs plats** : Tous les champs sont des noms simples sans `>`.
   Les helpers `value_as_*` sur le `ScraperDataNode` suffisent ; pas besoin
   de `get_*` par clé enfant.

4. **Filtre pays Rust-side** : `country` n'est pas passé comme paramètre de
   template YAML. Le filtre est appliqué après conversion en `ProxyRecord`,
   en comparant `r.country` avec `request.country`. Un record sans `country`
   est exclu quand un pays est demandé.

5. **Déduplication** : Un `HashSet<String>` sur l'authority `host:port` élimine
   les doublons entre sources. `use std::collections::HashSet;` est nécessaire.

6. **Helpers `ScraperDataNode`** : `value_as_string()`, `value_as_u16()`,
   `value_as_u32()`, `value_as_u64()`, `value_as_bool()` sont ajoutés pour
   standardiser l'extraction. `latency_ms` utilise `value_as_u64()` pour
   couvrir les valeurs >65535ms.

7. **Parsing de `protocol`** : Valeurs YAML attendues : `http`, `https`,
   `socks4`, `socks4a`, `socks5`.

8. **Champ `source`** : Taggé automatiquement via `source_field_name:
   Some("source")`. Le `HashMap` ne contient donc pas de clé `source` venant
   du YAML — elle est ajoutée par `execute_query_async`.

9. **Déréférencement du raw pointer** : `unsafe { &*self.scraper_agregator }`.
   La documentation existante garantit la sécurité.

10. **Nouvelles dépendances** : `use std::collections::{HashMap, HashSet};`
    dans `proxy_provider.rs`.

## Étapes d'implémentation

1. Ajouter les helpers `value_as_string()`, `value_as_u16()`, `value_as_u32()`,
   `value_as_u64()`, `value_as_bool()` à `ScraperDataNode` dans
   `scraper_data_node.rs`.

2. Ajouter `use std::collections::{HashMap, HashSet};` et la fonction
   `entry_to_proxy_record` dans `proxy_provider.rs`.

3. Modifier `load_proxies` pour déréférencer le raw pointer, appeler
   `execute_query_async`, convertir, filtrer par pays et dédupliquer.

4. Créer le registre `server/services/proxies/services.json` définissant le
   groupe `arachnea-proxies` et les sources YAML associées.
