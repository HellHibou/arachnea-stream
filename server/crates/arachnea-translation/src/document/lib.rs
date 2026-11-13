//! Document translation contracts and compiled format registry.
pub mod error;
#[cfg(feature = "retrieval-http")]
pub mod http;
pub mod retrieval;
pub mod types;
#[cfg(feature = "document-vtt")]
pub mod vtt;

use crate::{
    LanguageCode, LocalTranslationLimits, TextBlock, TextTranslationCapability, TranslatedDocument,
    TranslationOptions, TranslationService,
};
use async_trait::async_trait;

/// Inputs for translation of retrieved document bytes by a selected service.
pub struct DocumentRequest<'a> {
    /// Retrieved source bytes.
    pub bytes: &'a [u8],
    /// Known source language, or none for detection.
    pub source: Option<LanguageCode>,
    /// Desired target language.
    pub target: LanguageCode,
    /// Provider options.
    pub options: TranslationOptions,
    /// Local provider request limits.
    pub translation_limits: LocalTranslationLimits,
    /// Local document limits.
    pub document_limits: LocalSubtitleLimits,
}

/// Result of document execution, including source reuse and provider metrics.
pub struct DocumentResult {
    /// Complete document and execution metrics.
    pub document: TranslatedDocument,
    /// Whether the source was returned without translation.
    pub original: bool,
}

/// Provider-specific document plan retained from selection through execution.
#[async_trait]
pub trait PreparedDocument: Send + Sync {
    /// Returns the exact blocks used for provider selection and measurement.
    fn blocks(&self) -> &[TextBlock];

    /// Executes the retained plan without changing provider or segmentation.
    ///
    /// # Arguments
    /// * `request` - Source bytes, languages and options.
    /// * `service` - Provider selected using this plan's blocks.
    ///
    /// # Errors
    /// Returns provider or reconstruction errors without fallback.
    async fn execute(
        &self,
        request: DocumentRequest<'_>,
        service: &dyn TranslationService,
    ) -> Result<DocumentResult, SubtitleTranslationError>;
}

/// Format-specific document translator, independent of application retrieval.
#[async_trait]
pub trait DocumentTranslator: Send + Sync {
    /// Returns the canonical document media type.
    fn content_type(&self) -> &'static str;
    /// Validates source bytes and format-specific limits before provider selection.
    ///
    /// # Arguments
    /// * `bytes` - Complete source document.
    /// * `limits` - Local document bounds.
    ///
    /// # Errors
    /// Returns invalid-document or size-limit errors.
    fn validate_source(
        &self,
        bytes: &[u8],
        limits: &LocalSubtitleLimits,
    ) -> Result<(), SubtitleTranslationError>;

    /// Validates and normalizes a provider URL result against the source structure.
    ///
    /// # Arguments
    /// * `source` - Validated original bytes.
    /// * `translated` - Complete provider result.
    ///
    /// # Errors
    /// Returns parsing or structure-preservation errors.
    fn validate_url_result(
        &self,
        source: &[u8],
        translated: &[u8],
    ) -> Result<Vec<u8>, SubtitleTranslationError>;

    /// Prepares blocks for candidate-specific selection without a provider call.
    ///
    /// # Arguments
    /// * `bytes` - Retrieved source document.
    /// * `capability` - Candidate text capability.
    /// * `translation_limits` - Local request limits.
    /// * `document_limits` - Local document bounds.
    ///
    /// # Errors
    /// Returns parsing, segmentation or local limit errors.
    fn prepare(
        &self,
        bytes: &[u8],
        capability: &TextTranslationCapability,
        translation_limits: &LocalTranslationLimits,
        document_limits: &LocalSubtitleLimits,
    ) -> Result<Box<dyn PreparedDocument>, SubtitleTranslationError>;

    /// Translates bytes with an already selected service.
    ///
    /// # Arguments
    /// * `request` - Bytes, languages, options and limits.
    /// * `service` - Selected text translation service.
    ///
    /// # Errors
    /// Returns document, planning or service errors without provider fallback.
    async fn translate(
        &self,
        request: DocumentRequest<'_>,
        service: &dyn TranslationService,
    ) -> Result<DocumentResult, SubtitleTranslationError>;
}

/// Resolves a compiled translator by media type, ignoring case and parameters.
///
/// # Arguments
/// * `content_type` - Requested MIME type, optionally with parameters.
///
/// # Errors
/// Returns an unsupported-format error if no compiled translator matches.
pub fn translator(
    content_type: &str,
) -> Result<&'static dyn DocumentTranslator, SubtitleTranslationError> {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim();
    #[cfg(feature = "document-vtt")]
    if media_type.eq_ignore_ascii_case(crate::WEBVTT_MEDIA_TYPE) {
        return Ok(&vtt::VttDocumentTranslator);
    }
    Err(SubtitleTranslationError::UnsupportedFormat {
        media_type: media_type.to_string(),
    })
}

/// Resolves a document translator and delegates to it before any provider call.
///
/// # Arguments
///
/// * `content_type` - Requested document MIME type.
/// * `request` - Retrieved bytes, languages, options and limits.
/// * `service` - Previously selected provider.
///
/// # Errors
///
/// Returns an unsupported-format error or an error from the selected translator.
pub async fn translate(
    content_type: &str,
    request: DocumentRequest<'_>,
    service: &dyn TranslationService,
) -> Result<DocumentResult, SubtitleTranslationError> {
    translator(content_type)?.translate(request, service).await
}

pub mod cache;
pub mod descriptor;
#[cfg(feature = "runtime")]
pub mod orchestration;
pub mod response;
#[cfg(feature = "runtime")]
pub mod revalidation;
pub use cache::{SubtitleSourceValidator, SubtitleTranslationValidator};
pub use descriptor::SubtitleTranslationDescriptor;
#[cfg(feature = "runtime")]
pub use orchestration::{
    translate_subtitle, SubtitleTranslationOutcome, SubtitleTranslationRequest,
};
pub use response::{
    LocalSubtitleLimits, RetrievedSubtitle, SubtitleDocumentStats, SubtitleFormat,
    SubtitleRetrievalOutcome, SubtitleSourceDescriptor, TranslatedSubtitleDocument,
};
pub use retrieval::SubtitleRetrievalPort;
#[cfg(feature = "runtime")]
pub use revalidation::service_supports_pair;
#[cfg(feature = "runtime")]
pub use revalidation::{revalidate_subtitle, SubtitleRevalidation, SubtitleRevalidationRequest};
#[cfg(feature = "document-vtt")]
pub use vtt::webvtt::{WebVttBlock, WebVttDocument};

pub use error::SubtitleTranslationError;
