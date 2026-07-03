use std::sync::Arc;

use async_trait::async_trait;

use arachnea_proxy::core::{
    InventoryConfig, ProbeConfig, ProxyDataProvider, ProxyInventory, ProxyLoadRequest, ProxyProbe,
    ProxyRecord, ProxyProtocol, ProxyRuntimeStatus, Result,
};

/// `arachnea-scrapyfy` provider placeholder for dynamic proxy data.
///
/// Data loading is intentionally left unimplemented so source-specific loading
/// can be added separately without coupling this module to one public list
/// format.
pub struct ScrapyfyProxyDataProvider;

impl ScrapyfyProxyDataProvider {
    /// Creates an empty proxy data provider.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ScrapyfyProxyDataProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProxyDataProvider for ScrapyfyProxyDataProvider {
    async fn load_proxies(&self, _request: ProxyLoadRequest) -> Result<Vec<ProxyRecord>> {
        // TODO: A implementer.
        Ok(vec![ProxyRecord {
            protocol: Some(ProxyProtocol::Socks5),
            host: "51.210.5.144".to_string(),
            port: 1088,
            country: Some("FR".to_string()),
            supports_https: None,
            status: ProxyRuntimeStatus::Unknown,
            latency_ms: None,
            failure_count: 0,
            authentication_required: None,
            availability: Default::default(),
            destination_failures: Vec::new(),
            source: Some("static-fr-socks5".to_string()),
            last_checked: None,
            cooldown_until: None,
        }])
    }
}

/// Builds a default dynamic proxy inventory backed by scrapyfy proxy sources.
///
/// The returned inventory uses [`ScrapyfyProxyDataProvider`] and the default
/// [`ProxyProbe`] configuration. Persistence remains a caller concern so
/// applications can choose their store path and codec.
pub fn default_scrapyfy_proxy_inventory() -> ProxyInventory {
    ProxyInventory::new(
        InventoryConfig::default(),
        Some(Arc::new(ScrapyfyProxyDataProvider::new())),
        Some(Arc::new(ProxyProbe::new(ProbeConfig::default()))),
    )
}
