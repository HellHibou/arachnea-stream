//! Generic translation service interface.

use crate::block::{
    TextTranslationRequest, TranslatedDocument, TranslatedTexts, UrlTranslationRequest,
};
use crate::error::TranslationError;
use crate::metadata::TranslationMetadata;
use async_trait::async_trait;

/// Generic translation service implemented by the shared Scrapyfy bridge.
///
/// `translate_texts` is the base route and is mandatory. `translate_url` is
/// optional: an absent URL capability leaves text translation usable, and the
/// document layer falls back to retrieval plus text blocks.
#[async_trait]
pub trait TranslationService: Send + Sync {
    /// Returns the stable service identifier.
    fn service_id(&self) -> &str;

    /// Returns validated capabilities, languages and limits.
    fn metadata(&self) -> &TranslationMetadata;

    /// Translates identified abstract text blocks.
    ///
    /// # Arguments
    ///
    /// * `request` - Languages, blocks and optional language options.
    ///
    /// # Errors
    ///
    /// Returns a contextualized [`TranslationError`] when the pair, limits or
    /// normalized provider response are invalid.
    async fn translate_texts(
        &self,
        request: TextTranslationRequest,
    ) -> Result<TranslatedTexts, TranslationError>;

    /// Translates a document that the provider retrieves itself.
    ///
    /// # Arguments
    ///
    /// * `request` - Source URL, languages and declared media type.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::UnsupportedCapability`] when the service
    /// declares no URL capability, or another contextualized error.
    async fn translate_url(
        &self,
        request: UrlTranslationRequest,
    ) -> Result<TranslatedDocument, TranslationError> {
        let _ = request;
        Err(TranslationError::UnsupportedCapability {
            service_id: self.service_id().to_string(),
            detail: "URL translation is not declared".to_string(),
        })
    }
}
