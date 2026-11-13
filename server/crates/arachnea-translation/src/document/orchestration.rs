//! Subtitle revalidation and translation through the generic Scrapyfy bridge.

use crate::{
    selection::SelectionRoute, AccessScope, LocalTranslationLimits, SelectionPolicy,
    SelectionRequirement, TranslationError, TranslationOptions, TranslationService,
    UrlTranslationRequest,
};
use arachnea_scrapyfy::ScraperAgregator;

use super::{
    revalidate_subtitle, LocalSubtitleLimits, SubtitleRetrievalOutcome, SubtitleRetrievalPort,
    SubtitleRevalidation, SubtitleRevalidationRequest, SubtitleSourceValidator,
    SubtitleTranslationDescriptor, SubtitleTranslationError, SubtitleTranslationValidator,
    TranslatedSubtitleDocument,
};
use crate::catalog::TranslationCatalog;
use crate::runtime::{load_translation_bridge, shared_translation_budget};

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
    Text(Box<dyn crate::document::PreparedDocument>),
    Url(String),
}

/// Revalidates or translates a complete document using its registered translator.
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
    let translator = crate::document::translator(descriptor.source.format.media_type())?;
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
    translator.validate_source(&source.bytes, &request.subtitle_limits)?;
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
        let plan = match translator.prepare(
            &source.bytes,
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
        let text_request = crate::TextTranslationRequest {
            source_language: requirement.source_language.clone(),
            target_language: requirement.target_language.clone(),
            blocks: requirement.blocks.clone(),
            options: TranslationOptions::default(),
        };
        let measured = bridge.plan_texts(&text_request, &request.translation_limits);
        let units = measured
            .as_ref()
            .ok()
            .and_then(|plan| bridge.planned_budget_units(&text_request, plan).ok())
            .unwrap_or(u64::MAX);
        let candidate = crate::ServiceCandidate {
            service_id: entry.service_id.clone(),
            metadata: bridge.metadata().clone(),
            metadata_fresh: true,
            budget_available: shared_translation_budget(entry).available(units),
            administrator_order: entry.administrator_order.min(u32::MAX as u64) as u32,
        };
        let Ok(decision) = crate::selection::select_service_with_planner(
            &[candidate],
            &policy,
            &requirement,
            |_, _| measured.clone(),
        ) else {
            continue;
        };
        let bridge = bridge.with_local_limits(request.translation_limits);
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
            let translated = plan
                .execute(
                    crate::document::DocumentRequest {
                        bytes: &source.bytes,
                        source: descriptor.source_language.clone(),
                        target: descriptor.target_language.clone(),
                        options: TranslationOptions::default(),
                        translation_limits: request.translation_limits,
                        document_limits: request.subtitle_limits,
                    },
                    &bridge,
                )
                .await?;
            if translated.original {
                return Ok(SubtitleTranslationOutcome::Original {
                    bytes: source.bytes,
                });
            }
            translated.document.bytes
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
                .is_some_and(|media_type| {
                    media_type
                        .trim()
                        .eq_ignore_ascii_case(translator.content_type())
                })
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
            translator.validate_url_result(&source.bytes, &translated.bytes)?
        }
    };
    let validator = SubtitleTranslationValidator {
        service_id: entry.service_id.clone(),
        service_yaml_hash: descriptor.validator_hash(entry),
        source: source_validator,
    };
    let mut document = TranslatedSubtitleDocument::new(bytes, validator.to_etag()?)?;
    document.media_type = translator.content_type().into();
    Ok(SubtitleTranslationOutcome::Translated(document))
}
