use std::task::{Context, Poll};

use http::Uri;
use tower_service::Service;

use crate::connectors::{ArachneaTowerService, ConnectorError, ConnectorStream};

/// Layer that can replace an inner connector with an Arachnea connector.
#[derive(Clone)]
pub struct ArachneaProxyLayer {
    service: ArachneaTowerService,
}

impl ArachneaProxyLayer {
    /// Creates a layer from an Arachnea tower service.
    ///
    /// # Parameters
    ///
    /// - `service`: Tower service that should replace an inner connector.
    ///
    /// # Returns
    ///
    /// Connector layer backed by `service`.
    pub fn new(service: ArachneaTowerService) -> Self {
        Self { service }
    }

    /// Returns a service backed by the proxy core.
    ///
    /// # Parameters
    ///
    /// - `_inner`: Inner connector being replaced.
    ///
    /// # Returns
    ///
    /// Cloned Arachnea tower service.
    pub fn layer<S>(&self, _inner: S) -> ArachneaTowerService {
        self.service.clone()
    }
}

impl Service<Uri> for ArachneaProxyLayer {
    /// Stream type returned after a successful connection.
    type Response = ConnectorStream;
    /// Error type returned by connector operations.
    type Error = ConnectorError;
    /// Future type delegated to the inner tower service.
    type Future = <ArachneaTowerService as Service<Uri>>::Future;

    /// Delegates readiness checks to the backing connector service.
    ///
    /// # Parameters
    ///
    /// - `cx`: Task context supplied by the caller.
    ///
    /// # Returns
    ///
    /// Poll state from the backing service.
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ConnectorError>> {
        self.service.poll_ready(cx)
    }

    /// Delegates connection requests to the backing connector service.
    ///
    /// # Parameters
    ///
    /// - `req`: URI requested by the caller.
    ///
    /// # Returns
    ///
    /// Future returned by the backing service.
    fn call(&mut self, req: Uri) -> Self::Future {
        self.service.call(req)
    }
}
