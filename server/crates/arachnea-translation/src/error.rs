//! Contextualized translation errors and normalized exclusion reasons.

use crate::language::LanguageCode;
use http::StatusCode;
use serde::{Deserialize, Serialize};

/// Normalized reason explaining why a service was excluded from selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// The requested source/target direction is not declared by the service.
    UnsupportedLanguagePair,
    /// The required text or URL capability or format is not declared.
    UnsupportedCapability,
    /// The document cannot be split within the service and local limits.
    SizeLimit,
    /// The account budget or quota is unavailable before sending.
    BudgetExhausted,
    /// The service metadata is absent, invalid or stale.
    MetadataUnavailable,
    /// The source language is unknown and the service cannot establish it.
    SourceLanguageRequired,
}

/// Category attached to a normalized provider failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorCategory {
    /// Credentials were rejected.
    Authentication,
    /// The provider refused the language direction.
    Language,
    /// The provider refused the request size.
    Size,
    /// The provider quota or rate limit was exhausted.
    Quota,
    /// A temporary provider or transport incident occurred.
    Temporary,
    /// The provider response did not match the normalized contract.
    InvalidResponse,
}

/// Error produced by the generic translation layer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranslationError {
    /// A raw language code violates the two-letter contract.
    #[error("invalid language code {value:?}")]
    InvalidLanguageCode {
        /// Rejected raw value.
        value: String,
    },
    /// A block identifier is absent or blank.
    #[error("invalid translation block identifier {value:?}")]
    InvalidBlockId {
        /// Rejected raw identifier.
        value: String,
    },
    /// A translation request is structurally invalid.
    #[error("invalid translation request: {detail}")]
    InvalidRequest {
        /// Human-readable detail.
        detail: String,
    },
    /// Declared service metadata is absent or inconsistent.
    #[error("translation metadata unavailable for service {service_id}: {detail}")]
    InvalidMetadata {
        /// Service identifier owning the invalid metadata.
        service_id: String,
        /// Human-readable detail.
        detail: String,
    },
    /// Metadata is unavailable, stale or was not returned by the query.
    #[error("translation metadata unavailable: {detail}")]
    MetadataUnavailable {
        /// Human-readable detail.
        detail: String,
    },
    /// The requested source/target direction is unsupported.
    #[error("unsupported translation pair {:?} -> {target}", source_language.as_ref().map(LanguageCode::as_str))]
    UnsupportedLanguagePair {
        /// Requested source, absent for automatic detection.
        source_language: Option<LanguageCode>,
        /// Requested target.
        target: LanguageCode,
    },
    /// The required capability is not declared.
    #[error("unsupported translation capability for service {service_id}: {detail}")]
    UnsupportedCapability {
        /// Service identifier.
        service_id: String,
        /// Human-readable detail.
        detail: String,
    },
    /// A block or request exceeds a declared or local size limit.
    #[error("translation size limit exceeded: {detail}")]
    SizeLimit {
        /// Human-readable detail.
        detail: String,
    },
    /// No source language is known and the service cannot detect it.
    #[error("source language is required for target {target}")]
    SourceLanguageRequired {
        /// Requested target.
        target: LanguageCode,
    },
    /// No budget or concurrency reservation is available.
    #[error("translation budget exhausted: {detail}")]
    BudgetExhausted {
        /// Human-readable detail.
        detail: String,
    },
    /// A normalized provider response is invalid.
    #[error("invalid provider translation response: {detail}")]
    InvalidResponse {
        /// Human-readable detail.
        detail: String,
    },
    /// The provider rejected the request.
    #[error("provider translation request failed ({category:?}): {detail}")]
    ProviderRejected {
        /// Normalized failure category.
        category: ProviderErrorCategory,
        /// Human-readable detail.
        detail: String,
    },
    /// A Scrapyfy query could not be executed.
    #[error("translation query {query} failed: {detail}")]
    QueryFailed {
        /// Query name.
        query: String,
        /// Human-readable detail.
        detail: String,
    },
    /// No eligible service remains after filtering.
    #[error("no eligible translation service: {detail}")]
    NoEligibleService {
        /// Human-readable detail.
        detail: String,
    },
}

impl TranslationError {
    /// Returns the HTTP status recommended for this backend error.
    ///
    /// The file callback in Stream builds its response explicitly from this
    /// mapping; a translation failure is never served as a WebVTT body.
    pub fn http_status(&self) -> StatusCode {
        match self {
            Self::InvalidLanguageCode { .. }
            | Self::InvalidBlockId { .. }
            | Self::InvalidRequest { .. } => StatusCode::BAD_REQUEST,
            Self::UnsupportedLanguagePair { .. }
            | Self::UnsupportedCapability { .. }
            | Self::SizeLimit { .. }
            | Self::SourceLanguageRequired { .. }
            | Self::NoEligibleService { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::BudgetExhausted { .. }
            | Self::ProviderRejected {
                category: ProviderErrorCategory::Quota,
                ..
            } => StatusCode::TOO_MANY_REQUESTS,
            Self::MetadataUnavailable { .. }
            | Self::ProviderRejected {
                category: ProviderErrorCategory::Temporary,
                ..
            } => StatusCode::SERVICE_UNAVAILABLE,
            Self::ProviderRejected {
                category: ProviderErrorCategory::Authentication,
                ..
            }
            | Self::QueryFailed { .. } => StatusCode::BAD_GATEWAY,
            Self::InvalidMetadata { .. }
            | Self::InvalidResponse { .. }
            | Self::ProviderRejected { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Returns whether a bounded retry may succeed without changing the request.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::ProviderRejected {
                category: ProviderErrorCategory::Temporary,
                ..
            } | Self::QueryFailed { .. }
        )
    }
}
