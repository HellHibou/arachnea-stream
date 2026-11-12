//! Subtitle revalidation and translation through the generic Scrapyfy bridge.

use arachnea_scrapyfy::ScraperAgregator;
use arachnea_translation::{
    selection::SelectionRoute, AccessScope, LocalTranslationLimits, SelectionPolicy,
    SelectionRequirement, TextTranslationRequest, TranslationError, TranslationOptions,
    TranslationService, UrlTranslationRequest,
};

use super::{
    revalidate_subtitle, LocalSubtitleLimits, SubtitleDocumentPlan, SubtitleFormat,
    SubtitleRetrievalOutcome, SubtitleRetrievalPort, SubtitleRevalidation,
    SubtitleRevalidationRequest, SubtitleSourceValidator, SubtitleTranslationDescriptor,
    SubtitleTranslationError, SubtitleTranslationValidator, TranslatedSubtitleDocument,
    WebVttBlock, WebVttDocument,
};
use crate::translation_catalog::TranslationCatalog;
use crate::translation_port::{
    load_translation_bridge, select_text_translation_bridge, shared_translation_budget,
};

/// Outcome of a complete subtitle translation request.
pub enum SubtitleTranslationOutcome {
    /// The browser can reuse its representation without a provider call.
    NotModified {
        /// Weak provider/source entity tag.
        etag: String,
    },
    /// Complete validated translated subtitle and its provider/source entity tag.
    Translated(TranslatedSubtitleDocument),
    /// The source is already in the requested language or has no visible text.
    Original {
        /// Complete original subtitle document.
        bytes: Vec<u8>,
    },
}

/// Inputs for one subtitle translation operation.
pub struct SubtitleTranslationRequest<'a> {
    /// Authorized translation descriptor.
    pub descriptor: &'a SubtitleTranslationDescriptor,
    /// Caller access scope.
    pub access_scope: &'a AccessScope,
    /// Previously issued provider/source entity tag, when the client has its body.
    pub client_etag: Option<&'a str>,
    /// Source URL independently verified by the caller as public and safe to
    /// disclose to the provider, with no required headers or credentials.
    /// It must equal the original source URL and have no query or fragment.
    pub provider_accessible_url: Option<&'a str>,
    /// Local document bounds.
    pub subtitle_limits: LocalSubtitleLimits,
    /// Local provider request bounds.
    pub translation_limits: LocalTranslationLimits,
}

/// Document route selected before the first provider request.
enum DocumentRoute {
    Text(SubtitleDocumentPlan),
    Url(String),
}

/// Revalidates or translates a complete WebVTT document.
///
/// Provider candidates are segmented and measured independently; the selected
/// bridge is held for the entire document and an execution failure never
/// triggers a provider fallback. No source or translated document is retained.
///
/// # Arguments
///
/// * `retrieval` - Authorized and destination-restricted retrieval port.
/// * `aggregator` - Loaded Scrapyfy services and credentials.
/// * `catalog` - Effective translation service catalog.
/// * `group_name` - Scrapyfy translation service group.
/// * `request` - Descriptor, caller, client validator and local bounds.
///
/// # Errors
///
/// Returns authorization, source, selection, translation or document errors.
pub async fn translate_subtitle(
    retrieval: &dyn SubtitleRetrievalPort,
    aggregator: &ScraperAgregator,
    catalog: &TranslationCatalog,
    group_name: &str,
    request: SubtitleTranslationRequest<'_>,
) -> Result<SubtitleTranslationOutcome, SubtitleTranslationError> {
    let descriptor = request.descriptor;
    if descriptor.source.format != SubtitleFormat::WebVtt {
        return Err(SubtitleTranslationError::UnsupportedFormat {
            media_type: descriptor.source.format.media_type().into(),
        });
    }
    let (source, source_validator) = match revalidate_subtitle(
        retrieval,
        SubtitleRevalidationRequest {
            descriptor,
            access_scope: request.access_scope,
            client_etag: request.client_etag,
            catalog,
            limits: request.subtitle_limits,
        },
    )
    .await?
    {
        SubtitleRevalidation::NotModified { etag } => {
            return Ok(SubtitleTranslationOutcome::NotModified { etag })
        }
        SubtitleRevalidation::Modified { source, validator } => (source, validator),
    };
    let document = WebVttDocument::parse(&source.bytes)?;
    if document.cue_count() > request.subtitle_limits.max_cues {
        return Err(SubtitleTranslationError::TooManyCues {
            cues: document.cue_count(),
            limit: request.subtitle_limits.max_cues,
        });
    }
    let preferred = match &descriptor.policy {
        SelectionPolicy::Auto { preferred_service } => preferred_service
            .as_deref()
            .or_else(|| catalog.preferred_service_id()),
        SelectionPolicy::Constrained { service_id } => Some(service_id.as_str()),
    };
    let provider_url = request.provider_accessible_url.and_then(|value| {
        let parsed = url::Url::parse(value).ok()?;
        (value == descriptor.source.url
            && source.final_url.as_deref() == Some(value)
            && matches!(parsed.scheme(), "http" | "https")
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.query().is_none()
            && parsed.fragment().is_none())
        .then_some(value)
    });
    let mut entries: Vec<_> = catalog
        .services()
        .iter()
        .filter(|entry| {
            (entry.supports_text_translation
                || (entry.supports_url_translation && provider_url.is_some()))
                && entry.service_yaml_hash.is_some()
                && match &descriptor.policy {
                    SelectionPolicy::Constrained { service_id } => entry.service_id == *service_id,
                    SelectionPolicy::Auto { .. } => true,
                }
        })
        .collect();
    entries.sort_by(|left, right| {
        (
            preferred != Some(left.service_id.as_str()),
            left.administrator_order,
            &left.service_id,
        )
            .cmp(&(
                preferred != Some(right.service_id.as_str()),
                right.administrator_order,
                &right.service_id,
            ))
    });
    let mut selected = None;
    for entry in entries {
        let Ok(bridge) =
            load_translation_bridge(aggregator, catalog, group_name, &entry.service_id).await
        else {
            continue;
        };
        if let (Some(url), Some(capability)) =
            (provider_url, bridge.metadata().url_translation.as_ref())
        {
            let bytes = source.bytes.len() as u64;
            let metadata = bridge.metadata();
            let supported = metadata.supports_pair(descriptor.source_language.as_ref(), &descriptor.target_language)
                && (descriptor.source_language.is_some() || (metadata.detection_available()
                    && (metadata.supported_pairs.is_none() || metadata.detection_can_establish_source())))
                && capability.accepts_format(descriptor.source.format.media_type())
                && bytes <= request.subtitle_limits.max_translatable_units
                && capability.max_document_bytes.is_none_or(|limit| bytes <= limit)
                // The URL capability has no declared unit for its unit limit.
                && capability.max_document_units.is_none()
                // URL capabilities do not currently declare a quota unit.
                // Never silently bypass a configured provider unit budget.
                && entry.max_units_per_window.is_none()
                && shared_translation_budget(entry).available(0);
            if supported {
                let rank = (
                    preferred != Some(entry.service_id.as_str()),
                    entry.administrator_order,
                    0,
                    entry.service_id.clone(),
                );
                if selected.as_ref().is_none_or(|(best, _, _, _)| rank < *best) {
                    selected = Some((rank, entry, bridge, DocumentRoute::Url(url.to_string())));
                }
                continue;
            }
        }
        let Some(capability) = bridge.metadata().text_translation.as_ref() else {
            continue;
        };
        let plan = match SubtitleDocumentPlan::plan(
            document.clone(),
            source.bytes.len() as u64,
            capability,
            &request.translation_limits,
            &request.subtitle_limits,
        ) {
            Ok(plan) => plan,
            Err(SubtitleTranslationError::Translation(_)) => continue,
            Err(error) => return Err(error),
        };
        if plan.blocks().is_empty() {
            return Ok(SubtitleTranslationOutcome::Original {
                bytes: source.bytes,
            });
        }
        let requirement = SelectionRequirement {
            source_language: descriptor.source_language.clone(),
            target_language: descriptor.target_language.clone(),
            blocks: plan.blocks().to_vec(),
            route: SelectionRoute::Text,
            media_type: None,
            document_bytes: Some(source.bytes.len() as u64),
            require_context: false,
            require_markup: false,
            local_limits: request.translation_limits,
        };
        let policy = SelectionPolicy::Constrained {
            service_id: entry.service_id.clone(),
        };
        let Ok((bridge, decision)) =
            select_text_translation_bridge(aggregator, catalog, group_name, &policy, &requirement)
                .await
        else {
            continue;
        };
        let batches = decision
            .evaluations
            .iter()
            .find(|evaluation| evaluation.service_id == entry.service_id)
            .and_then(|evaluation| evaluation.estimated_batches)
            .unwrap_or(u64::MAX);
        let rank = (
            preferred != Some(entry.service_id.as_str()),
            entry.administrator_order,
            batches,
            entry.service_id.clone(),
        );
        if selected.as_ref().is_none_or(|(best, _, _, _)| rank < *best) {
            selected = Some((rank, entry, bridge, DocumentRoute::Text(plan)));
        }
    }
    let Some((_, entry, bridge, plan)) = selected else {
        return Err(TranslationError::NoEligibleService {
            detail: "no eligible subtitle translation service".into(),
        }
        .into());
    };
    let bytes = match plan {
        DocumentRoute::Text(plan) => {
            let translated = bridge
                .translate_texts(TextTranslationRequest {
                    source_language: descriptor.source_language.clone(),
                    target_language: descriptor.target_language.clone(),
                    blocks: plan.blocks().to_vec(),
                    options: TranslationOptions::default(),
                })
                .await?;
            if translated.source_language.as_ref() == Some(&descriptor.target_language) {
                return Ok(SubtitleTranslationOutcome::Original {
                    bytes: source.bytes,
                });
            }
            plan.render(&translated)?
        }
        DocumentRoute::Url(url) => {
            let translated = bridge
                .translate_url(UrlTranslationRequest {
                    source_url: url,
                    source_language: descriptor.source_language.clone(),
                    target_language: descriptor.target_language.clone(),
                    media_type: Some(descriptor.source.format.media_type().into()),
                    options: TranslationOptions::default(),
                })
                .await?;
            if translated.detected_source_language.as_ref() == Some(&descriptor.target_language) {
                return Ok(SubtitleTranslationOutcome::Original {
                    bytes: source.bytes,
                });
            }
            if translated.bytes.len() as u64 > request.subtitle_limits.max_document_bytes {
                return Err(SubtitleTranslationError::DocumentTooLarge {
                    bytes: translated.bytes.len() as u64,
                    limit: request.subtitle_limits.max_document_bytes,
                });
            }
            if !translated
                .content_type
                .split(';')
                .next()
                .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/vtt"))
            {
                return Err(SubtitleTranslationError::UnsupportedFormat {
                    media_type: translated.content_type,
                });
            }
            let confirmed = retrieval
                .retrieve(
                    &descriptor.source,
                    request.access_scope,
                    Some(&source_validator),
                )
                .await?;
            let stable = match confirmed {
                SubtitleRetrievalOutcome::NotModified => matches!(
                    &source_validator,
                    SubtitleSourceValidator::Etag { .. }
                        | SubtitleSourceValidator::Content {
                            last_modified: Some(_),
                            ..
                        }
                ),
                SubtitleRetrievalOutcome::Modified(current) => current.bytes == source.bytes,
            };
            if !stable {
                return Err(SubtitleTranslationError::RetrievalFailed {
                    detail: "source changed during URL translation".into(),
                });
            }
            let output = WebVttDocument::parse(&translated.bytes)?;
            if !same_structure(&document, &output) {
                return Err(SubtitleTranslationError::InvalidDocument {
                    detail: "provider changed WebVTT structure or cue timing".into(),
                });
            }
            output.to_bytes()
        }
    };
    let validator = SubtitleTranslationValidator {
        service_id: entry.service_id.clone(),
        service_yaml_hash: entry.service_yaml_hash.clone().expect("filtered above"),
        source: source_validator,
    };
    Ok(SubtitleTranslationOutcome::Translated(
        TranslatedSubtitleDocument::new(bytes, validator.to_etag()?)?,
    ))
}

/// Checks that URL translation preserves metadata, block kinds and cue timing.
fn same_structure(source: &WebVttDocument, translated: &WebVttDocument) -> bool {
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
