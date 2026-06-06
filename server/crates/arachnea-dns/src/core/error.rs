use crate::core::protocol::RecordType;
use thiserror::Error;

/// Convenience result type used by the core crate.
pub type Result<T> = std::result::Result<T, DnsError>;

/// Typed errors returned by DNS resolution and configuration APIs.
#[derive(Debug, Error)]
pub enum DnsError {
    /// The query did not complete before its configured timeout.
    #[error("DNS request timed out")]
    Timeout,
    /// The resolver returned NXDOMAIN for the queried name.
    #[error("domain does not exist: {0}")]
    Nxdomain(String),
    /// The name exists, but no record of the requested type was available.
    #[error("no data for {name} {record_type:?}")]
    NoData {
        /// Normalized query name.
        name: String,
        /// Requested DNS record type.
        record_type: RecordType,
    },
    /// The upstream returned SERVFAIL.
    #[error("upstream returned SERVFAIL")]
    Servfail,
    /// DNSSEC validation failed.
    #[error("DNSSEC validation failed: {0}")]
    DnssecValidationFailed(String),
    /// The provided configuration is invalid or internally inconsistent.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    /// No usable upstream was available.
    #[error("upstream unavailable: {0}")]
    UpstreamUnavailable(String),
    /// A policy refused the query.
    #[error("query blocked by policy: {0}")]
    Blocked(String),
    /// The requested feature is not available in the current build or slice.
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),
    /// DNS message encoding or decoding failed.
    #[error("DNS protocol error: {0}")]
    Protocol(String),
    /// Underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
