# Intégration de `ArachneaProxyCore` dans `ScraperManager`

## Résumé

Propager `ArachneaProxyCore` (owned) depuis le trait `ScraperManager` jusque dans les résolveurs (m6play, rtbf-auvio) via `ScraperAgregator`, pour que les `HttpClient` wrappers scraper créés par les résolveurs utilisent le proxy core quand il est disponible.

---

## Architecture actuelle

### Définitions clés

| Symbole | Crate | Rôle |
|---|---|---|
| `ArachneaProxyCore` | `arachnea-proxy` | Cœur proxy : routing, transport, DNS, privacy, possède `ArachneaHttpClient` |
| `HttpClient` (arachnea-scrapyfy) | `arachnea-scrapyfy` | Wrapper scraper autour d'`ArachneaHttpClient`, avec `SharedProxyConfigHandle` |
| `ArachneaHttpClient` | `arachnea-http` | Client HTTP bas niveau (rquest, Cloudflare engines) |
| `ScraperManager` trait | `arachnea-scrapyfy` | Interface pour le moteur de scraping |
| `ScraperAgregator` | `arachnea-scrapyfy` | Agrégateur de `ScraperQueryCollection`, chaque query possède son `HttpClient` |
| `StreamScraper` | `arachnea-stream` | Implémente `ScraperManager`, construit `ScraperAgregator` |

### Problème

1. `ArachneaProxyCore` créé dans `main.rs` n'est pas accessible via `ScraperManager`.
2. Les résolveurs (m6play, rtbf-auvio) créent leur propre `HttpClient` sans passer par le proxy core.
3. Aucun moyen pour `ScraperManager` d'exposer le proxy core.

---

## Décisions confirmées

| Point | Décision |
|---|---|
| Type stocké | `Option<ArachneaProxyCore>` (owned, pas de référence) — évite les lifetimes complexes |
| Passage à StreamScraper | `set_proxy_core()` après construction |
| Signature `create_http_client` | `fn create_http_client(&self, ScraperHttpConfig) -> HttpClient` (retourne owned) |
| Tests sans proxy core | Inchangés, doivent compiler |
| Migration des résolveurs | Oui, utiliser `manager.create_http_client()` |
| Feature gate | `#[cfg(feature = "arachnea-proxy")]` sur toutes les méthodes manipulant `ArachneaProxyCore` |

---

## Changements proposés

### 1. Ajouter `proxy_core: Option<ArachneaProxyCore>` dans `ScraperAgregator`

```rust
// arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs
#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::core::ArachneaProxyCore;

pub struct ScraperAgregator {
    queries_collection: Vec<ScraperQueryCollection>,
    proxy_handle: SharedProxyConfigHandle,
    #[cfg(feature = "arachnea-proxy")]
    proxy_core: Option<ArachneaProxyCore>,
}
```

Avec les accesseurs :

```rust
#[cfg(feature = "arachnea-proxy")]
pub fn set_proxy_core(&mut self, proxy_core: ArachneaProxyCore) {
    self.proxy_core = Some(proxy_core);
}

#[cfg(feature = "arachnea-proxy")]
pub fn proxy_core(&self) -> Option<&ArachneaProxyCore> {
    self.proxy_core.as_ref()
}
```

### 2. Ajouter `create_http_client` sur `ScraperAgregator`

```rust
// arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs
impl ScraperAgregator {
    /// Creates a scraper `HttpClient` configured to use the proxy core when available.
    ///
    /// When `proxy_core` is `Some`, the returned `HttpClient` uses it as the HTTP
    /// proxy transport via `SharedProxyConfigHandle`. Otherwise a standalone client
    /// is created with the given config.
    ///
    /// # Arguments
    ///
    /// * `http_config` - Scraper HTTP configuration (mode, user agent, etc.).
    pub fn create_http_client(&self, http_config: ScraperHttpConfig) -> HttpClient {
        #[cfg(feature = "arachnea-proxy")]
        if let Some(proxy_core) = &self.proxy_core {
            let handle = SharedProxyConfigHandle::new();
            handle.set_proxy(HttpProxyConfig::Arachnea(proxy_core.clone()));
            return HttpClient::with_http_config_and_proxy_handle(http_config, handle);
        }

        HttpClient::with_http_config(http_config)
    }
}
```

### 3. Ajouter `create_http_client` dans le trait `ScraperManager`

```rust
// arachnea-scrapyfy/src/scrapyfy/scraper_manager.rs
pub trait ScraperManager {
    fn get_scraper_agregator_mut(&mut self) -> &mut ScraperAgregator;

    /// Creates a scraper `HttpClient` configured with the available proxy core.
    ///
    /// Default implementation delegates to `self.get_scraper_agregator_mut()`
    /// (via `&self` emulation — voir note ci-dessous).
    fn create_http_client(&self, http_config: ScraperHttpConfig) -> HttpClient {
        // Par défaut : client simple sans proxy core.
        // Les implémentations concrètes peuvent surcharger.
        HttpClient::with_http_config(http_config)
    }

    fn register_service(self, controler: &mut dyn ControlerService)
    where
        Self: Sized;

    fn init_sub_logger_levels() { ... }
    fn init_sub_logger_level(level: Level) { ... }
}
```

**Note** : Le trait `ScraperManager` n'a pas actuellement de méthode `&self` vers l'aggregateur (seulement `get_scraper_agregator_mut(&mut self)`). L'implémentation par défaut de `create_http_client` ne peut donc pas déléguer à `self.get_scraper_agregator_mut()` car elle prend `&self`. Les implémentations concrètes (`StreamScraper`) devront surcharger la méthode pour accéder à l'aggregateur. L'implémentation par défaut crée un `HttpClient::with_http_config()` simple.

### 4. Mettre à jour `StreamScraper`

Dans `stream_scraper.rs` :

```rust
pub struct StreamScraper {
    scraper_agregator: ScraperAgregator,
    credentials_store: Arc<dyn CredentialsStore>,
    proxy_handle: SharedProxyConfigHandle,
}

impl ScraperManager for StreamScraper {
    fn get_scraper_agregator_mut(&mut self) -> &mut ScraperAgregator {
        &mut self.scraper_agregator
    }

    fn create_http_client(&self, http_config: ScraperHttpConfig) -> HttpClient {
        self.scraper_agregator.create_http_client(http_config)
    }

    // ...
}
```

### 5. Mettre à jour `main.rs`

```rust
// arachnea-stream/src/main.rs
let proxy_core = match ArachneaProxyCore::new(pool_config) {
    Ok(pc) => {
        manager.set_proxy(HttpProxyConfig::Arachnea(pc.clone()));
        manager.get_scraper_agregator_mut().set_proxy_core(pc);
        Some(pc)
    }
    Err(e) => {
        tracing::warn!(
            "Failed to create proxy core: {}; proxy_http will be unavailable", e
        );
        None
    }
};
```

### 6. Migrer les résolveurs (m6play, rtbf-auvio)

Chaque résolveur reçoit `manager: &impl ScraperManager` et utilise `manager.create_http_client()`.

#### `m6play_resolver.rs`

```rust
async fn resolve_replay_stream(
    manager: &impl ScraperManager,
    credentials_store: &dyn CredentialsStore,
    video_id: &str,
    stream_kind: Option<String>,
) -> Result<ResolvedPlayerStream> {
    let http_client = manager.create_http_client(ScraperHttpConfig::default());
    // ... reste inchangé
}

async fn resolve_live_stream(
    manager: &impl ScraperManager,
    credentials_store: &dyn CredentialsStore,
    channel_id: &str,
    stream_kind: Option<String>,
) -> Result<ResolvedPlayerStream> {
    let http_client = manager.create_http_client(ScraperHttpConfig::default());
    // ... reste inchangé
}
```

Et la méthode du trait `PlayerStreamResolver` peut être adaptée pour recevoir le manager ou simplement le `HttpClient` directement.

#### `rtbf_auvio_resolver.rs`

Même principe : remplacer `HttpClient::new(RTBF_AUVIO_BASE_URL)` par `manager.create_http_client(ScraperHttpConfig::default())`.

---

## Fichiers impactés

| Fichier | Changement |
|---|---|
| `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs` | Ajout champ `proxy_core`, méthodes `set_proxy_core()`, `proxy_core()`, `create_http_client()` |
| `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_manager.rs` | Ajout méthode `create_http_client()` au trait (implémentation par défaut simple) |
| `server/crates/arachnea-stream/src/stream_scraper.rs` | Surcharge `create_http_client()` vers aggregator |
| `server/crates/arachnea-stream/src/main.rs` | Appel `set_proxy_core()` après création du proxy core |
| `server/crates/arachnea-stream/src/services/m6play_resolver.rs` | Migration vers `manager.create_http_client()` |
| `server/crates/arachnea-stream/src/services/rtbf_auvio_resolver.rs` | Migration vers `manager.create_http_client()` |

---

## Détails d'implémentation

### Feature gate

Tout le code manipulant `ArachneaProxyCore` est protégé par `#[cfg(feature = "arachnea-proxy")]` :

```rust
// scraper_agregator.rs
#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::core::ArachneaProxyCore;

#[cfg(feature = "arachnea-proxy")]
pub fn set_proxy_core(&mut self, proxy_core: ArachneaProxyCore) { ... }

#[cfg(feature = "arachnea-proxy")]
pub fn proxy_core(&self) -> Option<&ArachneaProxyCore> { ... }

pub fn create_http_client(&self, http_config: ScraperHttpConfig) -> HttpClient {
    #[cfg(feature = "arachnea-proxy")]
    if let Some(pc) = &self.proxy_core { ... }
    HttpClient::with_http_config(http_config)
}
```

### Tests

Les tests existants ne créent pas de `ArachneaProxyCore` et n'appellent pas `set_proxy_core()`. Le comportement par défaut de `create_http_client()` (sans proxy core) est identique à l'ancien `HttpClient::with_http_config(config)`. Aucun changement de comportement pour les tests.

---

## Notes sur le trait `ScraperManager`

Actuellement le trait expose `get_scraper_agregator_mut(&mut self)` mais pas d'accesseur `&self` vers l'aggregateur. La méthode `create_http_client(&self, ...)` ne peut donc pas déléguer à l'aggregateur dans l'implémentation par défaut. Ce n'est pas un problème car :

1. L'implémentation par défaut crée un `HttpClient` standard sans proxy core
2. `StreamScraper` surcharge la méthode et délègue à son aggregateur
3. Les tests et autres implémentations qui n'ont pas de proxy core utilisent l'implémentation par défaut

Ajouter une méthode `get_scraper_agregator(&self) -> &ScraperAgregator` au trait serait une option future si nécessaire, mais n'est pas requis pour cette tâche.