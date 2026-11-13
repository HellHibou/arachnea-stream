#![warn(missing_docs)]
//! Generic translation contracts for the Arachnea backend.
//!
//! This crate owns language and block identifiers, provider capability and
//! limit metadata, normalized query inputs and outputs, deterministic service
//! selection, batch planning, translation revision identity and the generic bridge used
//! to execute translation queries through `arachnea-scrapyfy`.
//!
//! Optional document translators provide format-specific parsing and reconstruction.
//! Optional HTTP and proxy adapters retrieve source documents; application configuration
//! remains the caller’s responsibility.
//!
//! # Boundaries
//!
//! * Provider endpoints, authentication headers, request bodies and response
//!   mappings are described by Scrapyfy service YAML files. No provider is
//!   implemented in Rust here.
//! * [`bridge::ScrapyfyQueryPort`] abstracts query execution. The optional
//!   aggregator adapter accepts an already loaded service group from the caller.
//! * Stream imports document and runtime contracts from this crate, which never
//!   depends on `arachnea-stream`.
//!
//! The API feature provides shared transport handlers; callers supply a runtime
//! snapshot and authorized retrieval adapters for document orchestration.

#[path = "document/lib.rs"]
pub mod document;

pub mod aggregator_port;
#[cfg(feature = "api")]
pub mod api;
pub mod block;
pub mod bridge;
pub mod budget;
pub mod cache;
pub mod capability;
#[cfg(feature = "runtime")]
pub mod catalog;
pub mod error;
pub mod language;
pub mod metadata;
pub mod metrics;
pub mod planning;
#[cfg(feature = "runtime")]
pub mod runtime;
pub mod selection;
pub mod service;

pub use block::{
    BlockId, TextBlock, TextTranslationRequest, TranslatedBlock, TranslatedDocument,
    TranslatedTexts, TranslationOptions, UrlTranslationRequest,
};
pub use budget::{
    BudgetLimit, InMemoryTranslationBudget, TranslationBudgetPort, TranslationBudgetReservation,
    UnlimitedTranslationBudget,
};
pub use cache::{AccessScope, TranslationRevision};
pub use capability::{
    SizeUnit, SourceDetection, TextTranslationCapability, UrlResultMode, UrlTranslationCapability,
};
pub use error::{ExclusionReason, ProviderErrorCategory, TranslationError};
pub use language::{LanguageCode, LanguageCodeError, SupportedPair, TranslationLanguagePair};
pub use metadata::{MetadataFreshness, TranslationMetadata};
pub use metrics::TranslationMetrics;
pub use planning::{
    plan_batches, plan_batches_with_measurement, LocalTranslationLimits, RequestMeasurement,
    TranslationBatch, TranslationPlan,
};
pub use selection::{
    select_service, CandidateEvaluation, CandidateExclusion, SelectionDecision, SelectionPolicy,
    SelectionRequirement, ServiceCandidate,
};
pub use service::TranslationService;

// Translation metadata and normalized query contract identifiers.

/// Version of the normalized translation metadata schema.
///
/// A service declaring another version is rejected until this crate supports
/// it explicitly; it is never interpreted with best-effort assumptions.
pub const TRANSLATION_METADATA_SCHEMA_VERSION: u32 = 1;

/// Media type identifying the normalized metadata query result.
pub const TRANSLATION_METADATA_MEDIA_TYPE: &str = "application/arachnea-translation-metadata";

/// Media type identifying normalized translated text rows.
pub const TRANSLATION_TEXT_MEDIA_TYPE: &str = "application/arachnea-translation-result";

/// Media type identifying a normalized translated document.
pub const TRANSLATION_DOCUMENT_MEDIA_TYPE: &str = "application/arachnea-translation-document";

/// Media type identifying normalized source-detection rows.
pub const TRANSLATION_DETECTION_MEDIA_TYPE: &str = "application/arachnea-translation-detection";

/// Revision of the generic translation pipeline used to revalidate browser-held results.
///
/// Changing this value invalidates previously cached translations that were
/// produced by an incompatible contract or validation pipeline.
pub const TRANSLATION_PIPELINE_REVISION: &str = "translation-1";

/// Name of the mandatory Scrapyfy query exposing service capabilities.
pub const GET_TRANSLATION_METADATA_QUERY: &str = "get_translation_metadata";

/// Default Scrapyfy query name used for normalized text translation.
pub const DEFAULT_TRANSLATE_TEXTS_QUERY: &str = "translate_texts";

/// Default Scrapyfy query name used for optional URL translation.
pub const DEFAULT_TRANSLATE_URL_QUERY: &str = "translate_url";

/// Runtime parameter carrying the normalized or automatic source language.
pub const PARAM_SOURCE_LANGUAGE: &str = "translation_source_language";

/// Runtime parameter carrying the normalized target language.
pub const PARAM_TARGET_LANGUAGE: &str = "translation_target_language";

/// Runtime parameter carrying the normalized block array as serialized JSON.
pub const PARAM_BLOCKS_JSON: &str = "translation_blocks_json";

/// Runtime parameter carrying an optional URL for provider-side retrieval.
pub const PARAM_DOCUMENT_URL: &str = "translation_document_url";

/// Value passed to a YAML query when the source language must be detected.
///
/// Each service YAML maps this value to its own API representation, such as
/// omitting the field or sending `auto`. It is not a language code and never
/// appears in [`TranslationMetadata::languages`].
pub const AUTO_SOURCE_LANGUAGE: &str = "auto";

/// Default local cap on serialized request bytes.
///
/// This bound applies even when a provider declares no request-size limit.
pub const DEFAULT_MAX_REQUEST_BYTES: u64 = 65_536;

/// Default local cap on the number of abstract blocks sent per request.
pub const DEFAULT_MAX_BLOCKS_PER_REQUEST: u64 = 50;

/// Default local cap on the size of one abstract block.
pub const DEFAULT_MAX_UNITS_PER_BLOCK: u64 = 10_000;

/// Default local cap on the cumulative size of one request.
pub const DEFAULT_MAX_UNITS_PER_REQUEST: u64 = 50_000;

/// Default freshness bound applied to loaded translation metadata.
pub const DEFAULT_METADATA_FRESHNESS_SECONDS: u64 = 86_400;

/// Default maximum number of attempts for one retryable translation query.
pub const DEFAULT_RETRY_MAX_ATTEMPTS: u32 = 3;

/// Default initial retry delay in milliseconds.
pub const DEFAULT_RETRY_INITIAL_DELAY_MS: u64 = 500;

/// Default upper bound for one retry delay in milliseconds.
pub const DEFAULT_RETRY_MAX_DELAY_MS: u64 = 8_000;

/// Maximum sample size, in provider units, sent to a dedicated detection query.
pub const DEFAULT_DETECTION_SAMPLE_UNITS: u64 = 2_000;

/// Maximum duration in seconds of one translation or detection query attempt.
pub const DEFAULT_QUERY_TIMEOUT_SECONDS: u64 = 60;

/// Runtime parameter carrying the plain text for dedicated detection queries.
pub const PARAM_DETECTION_TEXT: &str = "translation_detection_text";

/// Canonical WebVTT media type.
pub const WEBVTT_MEDIA_TYPE: &str = "text/vtt";
/// Canonical SubRip media type, reserved for a future translator.
pub const SRT_MEDIA_TYPE: &str = "application/x-subrip";
/// Default maximum decompressed document size in bytes.
pub const DEFAULT_MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;
/// Default maximum cue count per document.
pub const DEFAULT_MAX_CUES: u64 = 10_000;
/// Default maximum translatable units per document.
pub const DEFAULT_MAX_TRANSLATABLE_UNITS: u64 = 500_000;

/// Runtime parameter controlling the administrator selection order.
pub const TRANSLATION_ORDER_PARAMETER: &str = "translation_selection_order";

/// Runtime parameter marking the preferred translation provider.
pub const TRANSLATION_PREFERRED_PARAMETER: &str = "translation_preferred";

/// Runtime parameter bounding concurrent provider requests.
pub const TRANSLATION_MAX_CONCURRENT_PARAMETER: &str = "translation_max_concurrent_requests";

/// Runtime parameter bounding provider units per quota window.
pub const TRANSLATION_MAX_UNITS_PER_WINDOW_PARAMETER: &str = "translation_max_units_per_window";

/// Runtime parameter declaring the quota window in seconds.
pub const TRANSLATION_BUDGET_WINDOW_SECONDS_PARAMETER: &str = "translation_budget_window_seconds";

/// Default selection order when a service does not declare one.
pub const DEFAULT_TRANSLATION_ORDER: u64 = 1_000;

/// Default maximum concurrent provider requests.
pub const DEFAULT_TRANSLATION_MAX_CONCURRENT_REQUESTS: usize = 1;

/// Default account quota window: 30 days.
pub const DEFAULT_TRANSLATION_BUDGET_WINDOW_SECONDS: u64 = 2_592_000;

/// Maximum representable modification date in subtitle validators (Unix seconds).
pub const SUBTITLE_ETAG_MAX_DATE_UNIX_SECONDS: u64 = 253_402_300_799;
/// Maximum encoded subtitle entity-tag length in bytes.
pub const SUBTITLE_ETAG_MAX_LENGTH: usize = 4096;
/// Browser-only translation cache policy requiring revalidation.
pub const DEFAULT_SUBTITLE_CACHE_CONTROL: &str = "private, no-cache";

/// Revision of document-specific validator identity; changing it invalidates browser results.
pub const DOCUMENT_PIPELINE_REVISION: &str = "document-2";

/// Maximum encoded proxy options length accepted for a subtitle source, in bytes.
pub const SUBTITLE_PROXY_OPTIONS_MAX_LENGTH: usize = 4096;

/// Maximum redirects followed when retrieving a subtitle source.
pub const SUBTITLE_SOURCE_MAX_REDIRECTS: usize = 5;

/// Timeout for each subtitle source request, in seconds.
pub const SUBTITLE_SOURCE_TIMEOUT_SECONDS: u64 = 30;

/// Maximum number of segments assembled for one finite HLS subtitle track.
#[cfg(feature = "document-vtt")]
pub(crate) const SUBTITLE_HLS_MAX_SEGMENTS: usize = 4096;

/// Maximum total retrieval and assembly time for an HLS subtitle rendition, in seconds.
#[cfg(all(feature = "document-vtt", feature = "retrieval-http"))]
pub(crate) const SUBTITLE_HLS_TIMEOUT_SECONDS: u64 = 120;
