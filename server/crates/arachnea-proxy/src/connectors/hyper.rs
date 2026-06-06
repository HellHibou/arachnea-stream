use crate::core::ArachneaProxyCore;

use crate::connectors::ArachneaTowerService;

/// Hyper-oriented connector adapter.
///
/// In this initial implementation the type is a thin wrapper around the Tower
/// service shape used by Hyper ecosystem clients. A future feature can add
/// concrete `hyper-util` trait integration without changing core routing.
#[derive(Clone)]
pub struct ArachneaHyperConnector {
    service: ArachneaTowerService,
}

impl ArachneaHyperConnector {
    /// Creates a hyper connector wrapper.
    ///
    /// # Parameters
    ///
    /// - `core`: Proxy core used by the wrapped tower service.
    ///
    /// # Returns
    ///
    /// Hyper-oriented connector wrapper.
    pub fn new(core: ArachneaProxyCore) -> Self {
        Self {
            service: ArachneaTowerService::new(core),
        }
    }

    /// Returns the underlying tower service.
    pub fn into_service(self) -> ArachneaTowerService {
        self.service
    }
}
