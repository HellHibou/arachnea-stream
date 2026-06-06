use crate::core::ProxyError;

/// Result alias for connector operations.
pub type Result<T> = std::result::Result<T, ConnectorError>;

/// Errors produced by client connector adapters.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    /// URI-to-destination conversion failed.
    #[error("invalid connector uri: {0}")]
    InvalidUri(String),
    /// A rquest client could not be built.
    #[error("rquest integration error: {0}")]
    Rquest(String),
    /// The proxy core failed to open the stream.
    #[error(transparent)]
    Proxy(#[from] ProxyError),
    /// An I/O operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
