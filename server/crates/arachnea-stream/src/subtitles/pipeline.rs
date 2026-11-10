//! Document-level planning independent from providers, transport and players.
//!
//! The pipeline parses or accepts a WebVTT document, applies local subtitle
//! bounds, extracts translatable blocks with their cue/segment mapping, and
//! rebuilds a complete document from a normalized translation result. Provider
//! selection and cache orchestration remain outside this module.

use super::document::{LocalSubtitleLimits, SubtitleDocumentStats, SubtitleFormat};
use super::error::SubtitleTranslationError;
use super::segments::{segment, SegmentedDocument};
use super::webvtt::WebVttDocument;
use arachnea_translation::{
    plan_batches, LocalTranslationLimits, TextBlock, TextTranslationCapability, TranslatedTexts,
    TranslationPlan,
};

/// Planned translation of one WebVTT document.
#[derive(Debug, Clone)]
pub struct SubtitleDocumentPlan {
    /// Segmented source document and block mapping.
    segmented: SegmentedDocument,
    /// Measured source document statistics.
    stats: SubtitleDocumentStats,
}

impl SubtitleDocumentPlan {
    /// Plans a document that has already been parsed.
    ///
    /// # Arguments
    ///
    /// * `document` - Parsed WebVTT document.
    /// * `document_bytes` - Decompressed source size in bytes.
    /// * `capability` - Selected text translation capability.
    /// * `translation_limits` - Local translation bounds used while splitting.
    /// * `subtitle_limits` - Local document bounds applied before translation.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::InvalidDocument`] for malformed cue
    /// markup, a local bound error, or a propagated translation planning error.
    pub fn plan(
        document: WebVttDocument,
        document_bytes: u64,
        capability: &TextTranslationCapability,
        translation_limits: &LocalTranslationLimits,
        subtitle_limits: &LocalSubtitleLimits,
    ) -> Result<Self, SubtitleTranslationError> {
        let cue_count = document.cue_count();
        if cue_count > subtitle_limits.max_cues {
            return Err(SubtitleTranslationError::TooManyCues {
                cues: cue_count,
                limit: subtitle_limits.max_cues,
            });
        }
        let segmented = segment(document, capability, translation_limits)?;
        let translatable_units = segmented
            .blocks()
            .iter()
            .map(|block| capability.size_unit.measure(&block.text))
            .fold(0u64, u64::saturating_add);
        let stats = SubtitleDocumentStats {
            document_bytes,
            cue_count,
            translatable_units,
            format: SubtitleFormat::WebVtt,
        };
        subtitle_limits.validate(&stats)?;
        Ok(Self { segmented, stats })
    }

    /// Parses and plans a source document, enforcing the byte bound first.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Complete source document bytes.
    /// * `capability` - Selected text translation capability.
    /// * `translation_limits` - Local translation bounds used while splitting.
    /// * `subtitle_limits` - Local document bounds applied before translation.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::DocumentTooLarge`] before parsing
    /// when the source exceeds the byte bound, or any error from
    /// [`Self::plan`].
    pub fn parse_and_plan(
        bytes: &[u8],
        capability: &TextTranslationCapability,
        translation_limits: &LocalTranslationLimits,
        subtitle_limits: &LocalSubtitleLimits,
    ) -> Result<Self, SubtitleTranslationError> {
        let document_bytes = bytes.len() as u64;
        if document_bytes > subtitle_limits.max_document_bytes {
            return Err(SubtitleTranslationError::DocumentTooLarge {
                bytes: document_bytes,
                limit: subtitle_limits.max_document_bytes,
            });
        }
        let document = WebVttDocument::parse(bytes)?;
        Self::plan(
            document,
            document_bytes,
            capability,
            translation_limits,
            subtitle_limits,
        )
    }

    /// Returns the parsed source document.
    pub fn source_document(&self) -> &WebVttDocument {
        self.segmented.document()
    }

    /// Returns the abstract blocks submitted to translation.
    pub fn blocks(&self) -> &[TextBlock] {
        self.segmented.blocks()
    }

    /// Returns the measured source document statistics.
    pub fn stats(&self) -> SubtitleDocumentStats {
        self.stats
    }

    /// Plans translation batches without any network call.
    ///
    /// A document with no translatable text yields an empty plan.
    ///
    /// # Arguments
    ///
    /// * `capability` - Selected text translation capability.
    /// * `local` - Local translation bounds.
    ///
    /// # Errors
    ///
    /// Propagates [`arachnea_translation::TranslationError::SizeLimit`] when a
    /// planned block no longer fits the supplied limits.
    pub fn batches(
        &self,
        capability: &TextTranslationCapability,
        local: &LocalTranslationLimits,
    ) -> Result<TranslationPlan, SubtitleTranslationError> {
        if self.segmented.blocks().is_empty() {
            return Ok(TranslationPlan {
                batches: Vec::new(),
                total_units: 0,
            });
        }
        plan_batches(self.segmented.blocks(), capability, local).map_err(Into::into)
    }

    /// Rebuilds a complete WebVTT document from translated blocks.
    ///
    /// # Arguments
    ///
    /// * `translated` - Normalized translation result covering every block.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::Translation`] when the result has
    /// missing, extra, reordered or empty blocks.
    pub fn apply(
        &self,
        translated: &TranslatedTexts,
    ) -> Result<WebVttDocument, SubtitleTranslationError> {
        self.segmented.apply(translated)
    }

    /// Rebuilds and serializes a complete WebVTT document.
    ///
    /// # Arguments
    ///
    /// * `translated` - Normalized translation result covering every block.
    ///
    /// # Errors
    ///
    /// Returns any error from [`Self::apply`].
    pub fn render(
        &self,
        translated: &TranslatedTexts,
    ) -> Result<Vec<u8>, SubtitleTranslationError> {
        Ok(self.apply(translated)?.to_bytes())
    }
}
