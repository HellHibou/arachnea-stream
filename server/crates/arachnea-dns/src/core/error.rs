use crate::core::protocol::RecordType;
use thiserror::Error;

/// Convenience result type used by the core crate.
/// 
/// This type alias simplifies error handling throughout the DNS resolution
/// and configuration APIs by providing a standard result type that uses
/// `DnsError` as the error variant.
pub type Result<T> = std::result::Result<T, DnsError>;

/// Typed errors returned by DNS resolution and configuration APIs.
/// 
/// This enum represents all possible error conditions that can occur during
/// DNS resolution, configuration validation, and upstream communication.
#[derive(Debug, Error)]
pub enum DnsError {
    /// The query did not complete before its configured timeout.
    /// 
    /// This error occurs when a DNS query takes longer than the configured
    /// timeout period to complete, typically due to slow or unresponsive
    /// upstream servers.
    #[error("DNS request timed out")]
    Timeout,
    /// The resolver returned NXDOMAIN for the queried name.
    /// 
    /// NXDOMAIN indicates that the domain name does not exist in the DNS.
    /// The string parameter contains the normalized domain name that
    /// could not be resolved.
    #[error("domain does not exist: {0}")]
    Nxdomain(String),
    /// The name exists, but no record of the requested type was available.
    /// 
    /// This error occurs when a domain name exists but has no records
    /// of the specific type requested (e.g., requesting AAAA records
    /// for a domain that only has A records).
    #[error("no data for {name} {record_type:?}")]
    NoData {
        /// Normalized query name.
        name: String,
        /// Requested DNS record type.
        record_type: RecordType,
    },
    /// The upstream returned SERVFAIL.
    /// 
    /// SERVFAIL indicates that the upstream DNS server encountered an
    /// error while processing the query and could not provide a definitive
    /// answer.
    #[error("upstream returned SERVFAIL")]
    Servfail,
    /// DNSSEC validation failed.
    /// 
    /// This error occurs when DNSSEC validation is enabled and the
    /// cryptographic signatures in the DNS response cannot be verified.
    /// The string parameter provides details about the validation failure.
    #[error("DNSSEC validation failed: {0}")]
    DnssecValidationFailed(String),
    /// The provided configuration is invalid or internally inconsistent.
    /// 
    /// This error is returned when configuration validation fails,
    /// such as when required fields are missing, values are out of range,
    /// or there are logical inconsistencies in the configuration.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    /// No usable upstream was available.
    /// 
    /// This error occurs when no upstream DNS servers are configured
    /// or all configured upstreams are unavailable or failed to respond.
    #[error("upstream unavailable: {0}")]
    UpstreamUnavailable(String),
    /// A policy refused the query.
    /// 
    /// This error is returned when a configured policy (such as a blocklist
    /// or access control rule) explicitly refuses to allow the DNS query.
    #[error("query blocked by policy: {0}")]
    Blocked(String),
    /// The requested feature is not available in the current build or slice.
    /// 
    /// This error occurs when attempting to use a feature that requires
    /// a compile-time feature flag that was not enabled, or when using
    /// a feature that is not implemented in the current version.
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),
    /// DNS message encoding or decoding failed.
    /// 
    /// This error indicates a problem with parsing or serializing DNS
    /// messages, typically due to malformed data or protocol violations.
    #[error("DNS protocol error: {0}")]
    Protocol(String),
    /// Underlying I/O error.
    /// 
    /// This error wraps standard I/O errors that occur during network
    /// operations, file operations, or other system-level I/O.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
