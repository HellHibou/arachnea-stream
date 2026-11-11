//! Application assembly for subtitle source retrieval and translation.

use arachnea_scrapyfy::scrapyfy::conditional::hash62;
use std::collections::HashMap;

use super::StreamScraper;
use crate::subtitles::{
    translate_subtitle, SubtitleHttpRetrieval, SubtitleTranslationError,
    SubtitleTranslationOutcome, SubtitleTranslationRequest,
};

impl StreamScraper {
    /// Revalidates or translates an authorized subtitle using this application snapshot.
    ///
    /// The borrowed scraper holds one catalog and provider configuration for
    /// the whole operation. Headers, route options and limits are fingerprinted
    /// into the validator without exposing their values. Sources requiring
    /// headers cannot be sent to a provider URL capability. Source and result
    /// bytes are not persisted.
    ///
    /// # Arguments
    /// * `request` - Descriptor, caller, conditional validator and document bounds.
    /// * `headers` - Trusted upstream headers from the source resolver.
    /// * `proxy_public_path` - Exact public application proxy prefix, when configured.
    ///
    /// # Errors
    /// Returns access, expiry, source, translation or document validation errors.
    pub async fn translate_subtitle(
        &self,
        request: SubtitleTranslationRequest<'_>,
        headers: HashMap<String, String>,
        proxy_public_path: Option<&str>,
    ) -> Result<SubtitleTranslationOutcome, SubtitleTranslationError> {
        let mut canonical_headers = headers
            .iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value))
            .collect::<Vec<_>>();
        canonical_headers.sort_unstable();
        let context = serde_json::to_vec(&(
            request.context_fingerprint,
            canonical_headers,
            request.provider_accessible_url,
            request.subtitle_limits,
            request.translation_limits,
        ))
        .map_err(|_| SubtitleTranslationError::InvalidDescriptor {
            detail: "unable to encode subtitle request context".into(),
        })?;
        let fingerprint = hash62(&context);
        let provider_accessible_url = request
            .provider_accessible_url
            .filter(|_| headers.is_empty());
        let request = SubtitleTranslationRequest {
            context_fingerprint: &fingerprint,
            provider_accessible_url,
            ..request
        };
        let mut retrieval = SubtitleHttpRetrieval::new(headers, request.subtitle_limits);
        if let Some(path) = proxy_public_path {
            retrieval = retrieval.with_proxy_public_path(path);
        }
        if let Some(core) = self
            .proxy_http_core
            .as_ref()
            .or_else(|| self.scraper_agregator.proxy_core())
        {
            retrieval = retrieval.with_proxy_core(core.clone());
        }
        translate_subtitle(
            &retrieval,
            &self.scraper_agregator,
            &self.translation_catalog,
            crate::TRANSLATION_SERVICE_GROUP_NAME,
            request,
        )
        .await
    }
}
