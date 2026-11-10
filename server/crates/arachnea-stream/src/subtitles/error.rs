//! Subtitle document errors and explicit HTTP status mapping.

use arachnea_translation::TranslationError;
use http::StatusCode;

/// Error produced by subtitle document orchestration.
///
/// Stream builds file responses explicitly from [`Self::http_status`]; an
/// error is never serialized as a WebVTT body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SubtitleTranslationError {
    /// A descriptor, token or cache identity is structurally invalid.
    #[error("invalid subtitle translation descriptor: {detail}")]
    InvalidDescriptor {
        /// Human-readable detail.
        detail: String,
    },
    /// The caller does not own the requested access scope.
    #[error("subtitle translation access denied")]
    AccessDenied,
    /// The prepared token expired.
    #[error("subtitle translation token expired")]
    TokenExpired,
    /// The source format is unsupported.
    #[error("unsupported subtitle format {media_type}")]
    UnsupportedFormat {
        /// Rejected media type.
        media_type: String,
    },
    /// The source document is invalid or cannot be parsed.
    #[error("invalid subtitle document: {detail}")]
    InvalidDocument {
        /// Human-readable detail.
        detail: String,
    },
    /// The source document exceeds the local byte bound.
    #[error("subtitle document size {bytes} exceeds limit {limit}")]
    DocumentTooLarge {
        /// Measured size.
        bytes: u64,
        /// Applied limit.
        limit: u64,
    },
    /// The source document exceeds the local cue bound.
    #[error("subtitle document has {cues} cues, above limit {limit}")]
    TooManyCues {
        /// Measured cue count.
        cues: u64,
        /// Applied limit.
        limit: u64,
    },
    /// The translatable text exceeds the local unit bound.
    #[error("subtitle translatable text {units} exceeds limit {limit}")]
    TooManyUnits {
        /// Measured units.
        units: u64,
        /// Applied limit.
        limit: u64,
    },
    /// Authorized source retrieval failed.
    #[error("subtitle source retrieval failed: {detail}")]
    RetrievalFailed {
        /// Human-readable detail.
        detail: String,
    },
    /// The source is unavailable.
    #[error("subtitle source is unavailable")]
    SourceUnavailable,
    /// The document cache failed.
    #[error("subtitle translation cache failed: {detail}")]
    CacheUnavailable {
        /// Human-readable detail.
        detail: String,
    },
    /// Generic translation failed.
    #[error("subtitle translation failed: {0}")]
    Translation(#[from] TranslationError),
    /// A HEAD request has no existing cached result.
    ///
    /// HEAD must never trigger retrieval or translation. The documented
    /// response is `404 Not Found` until a GET has produced a result.
    #[error("no translated subtitle result is available for HEAD")]
    HeadUnavailable,
    /// An unexpected internal failure occurred.
    #[error("internal subtitle translation failure: {detail}")]
    Internal {
        /// Human-readable detail.
        detail: String,
    },
}

impl SubtitleTranslationError {
    /// Returns the HTTP status recommended for this error.
    pub fn http_status(&self) -> StatusCode {
        match self {
            Self::InvalidDescriptor { .. }
            | Self::UnsupportedFormat { .. }
            | Self::InvalidDocument { .. } => StatusCode::BAD_REQUEST,
            Self::AccessDenied => StatusCode::FORBIDDEN,
            Self::TokenExpired => StatusCode::GONE,
            Self::DocumentTooLarge { .. }
            | Self::TooManyCues { .. }
            | Self::TooManyUnits { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            Self::HeadUnavailable => StatusCode::NOT_FOUND,
            Self::RetrievalFailed { .. } | Self::SourceUnavailable => StatusCode::BAD_GATEWAY,
            Self::CacheUnavailable { .. } | Self::Internal { .. } => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::Translation(error) => error.http_status(),
        }
    }

    /// Returns whether a bounded retry may succeed without changing the request.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RetrievalFailed { .. } | Self::SourceUnavailable => true,
            Self::Translation(error) => error.is_retryable(),
            _ => false,
        }
    }
}
