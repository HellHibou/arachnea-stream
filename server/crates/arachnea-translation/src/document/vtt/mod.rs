//! WebVTT parsing, segmentation and translation.
pub mod hls;
pub mod pipeline;
mod segments;
pub mod webvtt;

use super::error::SubtitleTranslationError;
use super::{DocumentRequest, DocumentResult, DocumentTranslator, PreparedDocument};
use crate::{
    TextTranslationRequest, TranslatedDocument, TranslationError, TranslationMetrics,
    TranslationService, WEBVTT_MEDIA_TYPE,
};
use async_trait::async_trait;
use pipeline::SubtitleDocumentPlan;

/// Translator preserving WebVTT structure while translating visible cue text.
pub struct VttDocumentTranslator;

#[async_trait]
impl DocumentTranslator for VttDocumentTranslator {
    fn content_type(&self) -> &'static str {
        WEBVTT_MEDIA_TYPE
    }
    fn validate_source(
        &self,
        bytes: &[u8],
        limits: &super::LocalSubtitleLimits,
    ) -> Result<(), SubtitleTranslationError> {
        let document = WebVttDocument::parse(bytes)?;
        if document.cue_count() > limits.max_cues {
            return Err(SubtitleTranslationError::TooManyCues {
                cues: document.cue_count(),
                limit: limits.max_cues,
            });
        }
        Ok(())
    }
    fn validate_url_result(
        &self,
        source: &[u8],
        translated: &[u8],
    ) -> Result<Vec<u8>, SubtitleTranslationError> {
        let source = WebVttDocument::parse(source)?;
        let output = WebVttDocument::parse(translated)?;
        if !same_structure(&source, &output) {
            return Err(SubtitleTranslationError::InvalidDocument {
                detail: "provider changed WebVTT structure or cue timing".into(),
            });
        }
        Ok(output.to_bytes())
    }
    fn prepare(
        &self,
        bytes: &[u8],
        capability: &crate::TextTranslationCapability,
        translation_limits: &crate::LocalTranslationLimits,
        document_limits: &super::types::LocalSubtitleLimits,
    ) -> Result<Box<dyn PreparedDocument>, SubtitleTranslationError> {
        Ok(Box::new(SubtitleDocumentPlan::parse_and_plan(
            bytes,
            capability,
            translation_limits,
            document_limits,
        )?))
    }
    async fn translate(
        &self,
        request: DocumentRequest<'_>,
        service: &dyn TranslationService,
    ) -> Result<DocumentResult, SubtitleTranslationError> {
        let capability = service
            .metadata()
            .text_translation
            .as_ref()
            .ok_or_else(|| TranslationError::UnsupportedCapability {
                service_id: service.service_id().into(),
                detail: "text translation is not declared".into(),
            })?;
        let plan = self.prepare(
            request.bytes,
            capability,
            &request.translation_limits,
            &request.document_limits,
        )?;
        plan.execute(request, service).await
    }
}

#[async_trait]
impl PreparedDocument for SubtitleDocumentPlan {
    fn blocks(&self) -> &[crate::TextBlock] {
        self.blocks()
    }
    async fn execute(
        &self,
        request: DocumentRequest<'_>,
        service: &dyn TranslationService,
    ) -> Result<DocumentResult, SubtitleTranslationError> {
        if self.blocks().is_empty() {
            return Ok(DocumentResult {
                original: true,
                document: TranslatedDocument {
                    bytes: request.bytes.to_vec(),
                    content_type: WEBVTT_MEDIA_TYPE.into(),
                    detected_source_language: None,
                    metrics: TranslationMetrics::default(),
                },
            });
        }
        let result = service
            .translate_texts(TextTranslationRequest {
                source_language: request.source,
                target_language: request.target.clone(),
                blocks: self.blocks().to_vec(),
                options: request.options,
            })
            .await?;
        let original = result.source_language.as_ref() == Some(&request.target);
        let bytes = if original {
            request.bytes.to_vec()
        } else {
            self.render(&result)?
        };
        Ok(DocumentResult {
            original,
            document: TranslatedDocument {
                bytes,
                content_type: WEBVTT_MEDIA_TYPE.into(),
                detected_source_language: result.detected_source_language,
                metrics: result.metrics,
            },
        })
    }
}

use webvtt::{WebVttBlock, WebVttDocument};

/// Checks that URL translation preserves metadata, block kinds and cue timing.
///
/// # Arguments
/// * `source` - Parsed source document.
/// * `translated` - Provider document to validate.
pub fn same_structure(source: &WebVttDocument, translated: &WebVttDocument) -> bool {
    source.header == translated.header
        && source.blocks.len() == translated.blocks.len()
        && source
            .blocks
            .iter()
            .zip(&translated.blocks)
            .all(|(left, right)| match (left, right) {
                (WebVttBlock::Cue(left), WebVttBlock::Cue(right)) => {
                    left.identifier == right.identifier
                        && left.timing == right.timing
                        && markup_tokens(&left.payload).is_some_and(|left_tokens| {
                            markup_tokens(&right.payload)
                                .is_some_and(|right_tokens| left_tokens == right_tokens)
                        })
                }
                (WebVttBlock::Note(left), WebVttBlock::Note(right))
                | (WebVttBlock::Style(left), WebVttBlock::Style(right))
                | (WebVttBlock::Region(left), WebVttBlock::Region(right)) => left == right,
                _ => false,
            })
}

/// Extracts angle-bracket markup and inline timestamps in their original order.
fn markup_tokens(lines: &[String]) -> Option<Vec<&str>> {
    let mut tokens = Vec::new();
    for line in lines {
        let mut remainder = line.as_str();
        while let Some(start) = remainder.find('<') {
            remainder = &remainder[start..];
            let end = remainder.find('>')?;
            let token = &remainder[..=end];
            if token[1..token.len() - 1].contains('<') {
                return None;
            }
            tokens.push(token);
            remainder = &remainder[end + 1..];
        }
    }
    Some(tokens)
}
