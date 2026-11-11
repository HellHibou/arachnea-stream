#![warn(missing_docs)]
//! Generic translation contracts for the Arachnea backend.
//!
//! This crate owns language and block identifiers, provider capability and
//! limit metadata, normalized query inputs and outputs, deterministic service
//! selection, batch planning, translation revision identity and the generic bridge used
//! to execute translation queries through `arachnea-scrapyfy`.
//!
//! It deliberately has no knowledge of WebVTT, subtitle cues, players or
//! tracks. Document parsing, cue segmentation, subtitle retrieval and the
//! subtitle revalidation live in the `arachnea-stream` subtitle module; stream
//! services provide authorized retrieval through the injected port defined there.
//!
//! # Boundaries
//!
//! * Provider endpoints, authentication headers, request bodies and response
//!   mappings are described by Scrapyfy service YAML files. No provider is
//!   implemented in Rust here.
//! * [`bridge::ScrapyfyQueryPort`] is the only outbound Scrapyfy dependency.
//!   Stream supplies an implementation backed by its service catalog.
//! * The `arachnea-stream` subtitle module depends on this crate, never the
//!   opposite, and this crate never depends on `arachnea-stream`.
//!
//! Translation is not exposed publicly at this lot: the crate defines the
//! contracts needed by the catalog, orchestrator and transport layers.

pub mod block;
pub mod bridge;
pub mod budget;
pub mod cache;
pub mod capability;
pub mod error;
pub mod language;
pub mod metadata;
pub mod metrics;
pub mod planning;
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
