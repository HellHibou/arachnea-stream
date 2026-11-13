//! Authorized, conditional source revalidation before provider execution.

use crate::{AccessScope, LanguageCode, SelectionPolicy};
use arachnea_scrapyfy::scrapyfy::conditional::hash62;

use super::{
    LocalSubtitleLimits, RetrievedSubtitle, SubtitleRetrievalOutcome, SubtitleRetrievalPort,
    SubtitleSourceValidator, SubtitleTranslationDescriptor, SubtitleTranslationError,
    SubtitleTranslationValidator,
};
use crate::catalog::{TranslationCatalog, TranslationCatalogEntry};

/// Result of checking a browser-held representation against its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleRevalidation {
    /// The client representation remains usable and needs no provider call.
    NotModified {
        /// Previously issued provider/source ETag, including its weak prefix.
        etag: String,
    },
    /// A complete authorized source is available for translation.
    Modified {
        /// Complete source document and upstream metadata.
        source: RetrievedSubtitle,
        /// Current validator computed from the source response.
        validator: SubtitleSourceValidator,
    },
}

/// Inputs for authorized subtitle revalidation.
pub struct SubtitleRevalidationRequest<'a> {
    /// Prepared and authorized translation request.
    pub descriptor: &'a SubtitleTranslationDescriptor,
    /// Access scope belonging to the current caller.
    pub access_scope: &'a AccessScope,
    /// Single provider/source validator supplied by the client, if any.
    pub client_etag: Option<&'a str>,
    /// Current provider catalog, already filtered for activation and credentials.
    pub catalog: &'a TranslationCatalog,
    /// Maximum source document size and other local limits.
    pub limits: LocalSubtitleLimits,
}

/// Revalidates a subtitle source without invoking a translation provider.
///
/// The retrieval implementation must enforce destination, redirect, proxy and
/// header policies. The descriptor is checked before retrieval and no result body is
/// stored by this function. HTTP list and wildcard handling belongs to the
/// transport layer.
///
/// # Arguments
///
/// * `retrieval` - Authorized source retrieval implementation.
/// * `request` - Caller, descriptor, configuration and optional client ETag.
///
/// # Errors
///
/// Returns access, retrieval, size or invalid-source errors.
pub async fn revalidate_subtitle(
    retrieval: &dyn SubtitleRetrievalPort,
    request: SubtitleRevalidationRequest<'_>,
) -> Result<SubtitleRevalidation, SubtitleTranslationError> {
    let descriptor = request.descriptor;
    descriptor.validate()?;
    descriptor.authorize(request.access_scope)?;
    let prior = request.client_etag.and_then(|etag| {
        let validator = SubtitleTranslationValidator::from_etag(etag).ok()?;
        let service = request.catalog.service(&validator.service_id)?;
        let allowed = match &descriptor.policy {
            SelectionPolicy::Constrained { service_id } => service_id == &service.service_id,
            SelectionPolicy::Auto { .. } => true,
        };
        (allowed
            && (service.supports_text_translation || service.supports_url_translation)
            && service_supports_pair(
                service,
                descriptor.source_language.as_ref(),
                &descriptor.target_language,
            )
            && descriptor.validator_hash(service) == validator.service_yaml_hash)
            .then_some(validator)
    });
    let outcome = retrieval
        .retrieve(
            &descriptor.source,
            request.access_scope,
            prior.as_ref().map(|value| &value.source),
        )
        .await?;
    match outcome {
        SubtitleRetrievalOutcome::NotModified => {
            let validator = prior.ok_or_else(|| SubtitleTranslationError::RetrievalFailed {
                detail: "source returned not modified without a usable conditional validator"
                    .into(),
            })?;
            if matches!(
                &validator.source,
                SubtitleSourceValidator::Content {
                    last_modified: None,
                    ..
                }
            ) {
                return Err(SubtitleTranslationError::RetrievalFailed {
                    detail: "source returned not modified without an upstream conditional header"
                        .into(),
                });
            }
            Ok(SubtitleRevalidation::NotModified {
                etag: validator.to_etag()?,
            })
        }
        SubtitleRetrievalOutcome::Modified(source) => {
            if source.bytes.len() as u64 > request.limits.max_document_bytes {
                return Err(SubtitleTranslationError::DocumentTooLarge {
                    bytes: source.bytes.len() as u64,
                    limit: request.limits.max_document_bytes,
                });
            }
            let unchanged = prior
                .as_ref()
                .is_some_and(|previous| match &previous.source {
                    SubtitleSourceValidator::Etag { etag } => {
                        source.remote_etag.as_deref() == Some(etag)
                    }
                    SubtitleSourceValidator::Content { hash, .. } => hash62(&source.bytes) == *hash,
                });
            if unchanged {
                return Ok(SubtitleRevalidation::NotModified {
                    etag: prior.expect("checked above").to_etag()?,
                });
            }
            let validator = match source
                .remote_etag
                .as_deref()
                .filter(|etag| !etag.trim().is_empty())
            {
                Some(etag) if super::cache::valid_entity_tag(etag) => {
                    SubtitleSourceValidator::Etag {
                        etag: etag.to_string(),
                    }
                }
                _ => SubtitleSourceValidator::Content {
                    hash: hash62(&source.bytes),
                    last_modified: source.last_modified.clone(),
                },
            };
            Ok(SubtitleRevalidation::Modified { source, validator })
        }
    }
}

/// Returns whether one service can translate the requested language pair.
///
/// # Arguments
///
/// * `service` - Published service capability.
/// * `source_language` - Known source language, or `None` for detection.
/// * `target_language` - Requested target language.
pub fn service_supports_pair(
    service: &TranslationCatalogEntry,
    source_language: Option<&LanguageCode>,
    target_language: &LanguageCode,
) -> bool {
    service.target_languages.contains(target_language)
        && match source_language {
            Some(source) => {
                service.source_languages.contains(source)
                    && service.supported_pairs.as_ref().is_none_or(|pairs| {
                        pairs
                            .iter()
                            .any(|pair| pair.source == *source && pair.target == *target_language)
                    })
            }
            None => {
                service.source_detection
                    && service.supported_pairs.as_ref().is_none_or(|pairs| {
                        pairs.iter().any(|pair| pair.target == *target_language)
                    })
            }
        }
}
