use std::io;

/// Convenient result alias for proxy operations.
pub type Result<T> = std::result::Result<T, ProxyError>;

/// Typed errors returned by the proxy core and adapters.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    /// Business-request failure correlated with its actual route and delivery evidence.
    #[error("proxy request failed ({delivery:?}): {source}")]
    RequestAttempt {
        /// Actual selected endpoint, when route selection completed.
        selection: Option<super::ProxySelectionToken>,
        /// Delivery evidence at the failing I/O boundary.
        delivery: super::ProxyRequestDelivery,
        /// Original contextual transport error.
        #[source]
        source: Box<ProxyError>,
    },
    /// Transport error attributed at the boundary where it was observed.
    #[error("proxy phase {phase:?} failed: {source}")]
    Observation {
        /// Actual failing phase, not inferred from an opaque error string.
        phase: super::ProxyObservationPhase,
        /// Original typed transport failure.
        #[source]
        source: Box<ProxyError>,
    },
    /// The supplied configuration is invalid or unsafe.
    #[error("invalid proxy configuration: {0}")]
    Config(String),
    /// The destination cannot be parsed or represented by the requested transport.
    #[error("invalid destination: {0}")]
    InvalidDestination(String),
    /// Route selection did not produce a usable path.
    #[error("no usable proxy route: {0}")]
    RouteUnavailable(String),
    /// A requested protocol or feature is deliberately not implemented yet.
    #[error("unsupported proxy feature: {0}")]
    Unsupported(&'static str),
    /// The remote proxy rejected a request.
    #[error("upstream proxy rejected request: {0}")]
    UpstreamRejected(String),
    /// The peer sent invalid proxy protocol data.
    #[error("proxy protocol error: {0}")]
    Protocol(String),
    /// A security policy refused the request.
    #[error("proxy request denied by policy: {0}")]
    AccessDenied(String),
    /// DNS integration failed.
    #[error("dns failure: {0}")]
    Dns(String),
    /// TLS setup or validation failed.
    #[error("tls failure: {0}")]
    Tls(String),
    /// An operation timed out.
    #[error("proxy operation timed out: {0}")]
    Timeout(&'static str),
    /// An I/O operation failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl ProxyError {
    /// Attaches a known phase without replacing an already precise attribution.
    ///
    /// # Arguments
    /// - `phase`: Boundary that directly observed the failure.
    pub fn at_phase(self, phase: super::ProxyObservationPhase) -> Self {
        if matches!(self, Self::Observation { .. }) {
            self
        } else {
            Self::Observation {
                phase,
                source: Box::new(self),
            }
        }
    }
}
