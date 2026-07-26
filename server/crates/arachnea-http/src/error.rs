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
    /// A configured proxy transport or proxy route failed.
    #[error("proxy error: {0}")]
    Proxy(String),
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
    /// A persistent browser session is required but unavailable.
    #[error("browser session unavailable for {origin}: {reason}")]
    BrowserSessionUnavailable {
        /// Origin for which the session was requested.
        origin: String,
        /// Reason the session could not be obtained.
        reason: String,
    },
    /// A Turnstile callback token is required but was not provided by the page.
    #[error("no Turnstile token available for {origin}")]
    TokenAbsent {
        /// Origin where the token was expected.
        origin: String,
    },
    /// A Turnstile token was rejected by the server on submission.
    #[error("Turnstile token rejected for {origin}")]
    TokenRejected {
        /// Origin where the token was rejected.
        origin: String,
    },
    /// An in-page JavaScript fetch/XHR execution failed.
    #[error("page fetch failed: {0}")]
    PageFetchFailed(String),
    /// A browser-page click or DOM wait failed.
    #[error("page interaction failed: {0}")]
    PageInteractionFailed(String),
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
