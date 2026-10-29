use std::io;

/// Convenient result alias for proxy operations.
pub type Result<T> = std::result::Result<T, ProxyError>;

/// Typed errors returned by the proxy core and adapters.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    /// Business-request failure correlated with its actual route and delivery evidence.
    #[error("proxy request failed ({delivery:?}): {source}")]
    RequestAttempt {
        /// Actual selected endpoint, boxed only on failure when route selection completed.
        selection: Option<Box<super::ProxySelectionToken>>,
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
    /// An HTTP proxy rejected CONNECT before business-request delivery.
    #[error("upstream proxy rejected request: http connect returned status {status}")]
    HttpConnectRejected {
        /// Status received from the upstream proxy, not the destination server.
        status: u16,
    },
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
    /// Extracts bounded diagnostics without inspecting remote text or changing retry policy.
    ///
    /// Unknown phases remain absent; CONNECT statuses never describe origin responses.
    pub(crate) fn transport_failure_details(
        &self,
    ) -> (
        Option<super::ProxyObservationPhase>,
        &'static str,
        Option<u16>,
    ) {
        match self {
            Self::Observation { phase, source } => {
                let (inner_phase, cause, status) = source.transport_failure_details();
                (inner_phase.or(Some(*phase)), cause, status)
            }
            Self::RequestAttempt { source, .. } => source.transport_failure_details(),
            Self::HttpConnectRejected { status } => (
                Some(super::ProxyObservationPhase::Tunnel),
                "http_connect_rejected",
                Some(*status),
            ),
            Self::Timeout(_) => (None, "timeout", None),
            Self::Tls(_) => (None, "tls", None),
            Self::Io(error) => (
                None,
                match error.kind() {
                    io::ErrorKind::ConnectionReset => "connection_reset",
                    io::ErrorKind::ConnectionRefused => "connection_refused",
                    io::ErrorKind::TimedOut => "timeout",
                    io::ErrorKind::UnexpectedEof => "unexpected_eof",
                    _ => "io",
                },
                None,
            ),
            Self::UpstreamRejected(_) => (None, "upstream_rejected", None),
            Self::Protocol(_) => (None, "protocol", None),
            Self::Dns(_) => (None, "dns", None),
            Self::AccessDenied(_) => (None, "access_denied", None),
            Self::Config(_) => (None, "configuration", None),
            Self::InvalidDestination(_) => (None, "invalid_destination", None),
            Self::RouteUnavailable(_) => (None, "route_unavailable", None),
            Self::Unsupported(_) => (None, "unsupported", None),
        }
    }

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
