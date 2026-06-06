use http::StatusCode;
use thiserror::Error;

/// Error type returned by the Arachnea HTTP crate.
#[derive(Debug, Error)]
pub enum ArachneaHttpError {
    /// A URL could not be parsed or is missing required authority data.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    /// The client configuration is invalid.
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(String),
    /// An outbound network request failed.
    #[error("network error: {0}")]
    Network(String),
    /// The server returned an HTTP status that the caller asked to treat as an error.
    #[error("HTTP status error: {status}")]
    HttpStatus {
        /// HTTP status received from the server.
        status: StatusCode,
    },
    /// A Cloudflare challenge or block was detected.
    #[error("Cloudflare block detected for {origin}: {reason}")]
    CloudflareBlocked {
        /// Origin where the block was detected.
        origin: String,
        /// Human-readable block signal summary with secret values redacted.
        reason: String,
    },
    /// Ghostwire failed to solve or refresh a Cloudflare-protected origin.
    #[error("Ghostwire failure: {0}")]
    GhostwireFailure(String),
    /// chaser-cf failed to solve or refresh a Cloudflare-protected origin.
    #[error("chaser-cf failure: {0}")]
    ChaserCfFailure(String),
    /// The interactive Tauri/Wry Cloudflare solver failed.
    #[error("Tauri Cloudflare solver failure: {0}")]
    TauriCloudflareSolverFailure(String),
    /// No Cloudflare-capable solver is available for a refresh request.
    #[error("no Cloudflare solver is available; inject a solver engine or enable an automatic solver feature")]
    CloudflareSolverUnavailable,
    /// An engine cannot execute the requested normalized operation.
    #[error("engine {engine} does not support {operation}")]
    UnsupportedEngineOperation {
        /// Engine that rejected the operation.
        engine: &'static str,
        /// Operation that cannot be represented by the engine API.
        operation: &'static str,
    },
    /// A required cookie is not present in the shared cache.
    #[error("missing cookie {name} for {origin}")]
    CookieAbsent {
        /// Origin where the cookie was expected.
        origin: String,
        /// Cookie name that was missing.
        name: String,
    },
    /// A required cookie is expired or too close to expiry.
    #[error("cookie {name} for {origin} is expired")]
    CookieExpired {
        /// Origin where the cookie was inspected.
        origin: String,
        /// Cookie name that is expired.
        name: String,
    },
    /// Header construction failed.
    #[error("invalid header: {0}")]
    InvalidHeader(String),
    /// A redirect chain exceeded the configured limit.
    #[error("redirect chain exceeded the configured limit of {limit}")]
    RedirectLimitExceeded {
        /// Configured maximum redirect count.
        limit: usize,
    },
    /// JSON serialization failed before sending a request.
    #[error("JSON error: {0}")]
    Json(String),
    /// Response body could not be decoded as UTF-8 text.
    #[error("text decoding error: {0}")]
    Text(String),
}
