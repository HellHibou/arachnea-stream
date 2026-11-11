//! Authorized, conditional source revalidation before provider execution.

use arachnea_scrapyfy::scrapyfy::conditional::hash62;
use arachnea_translation::{AccessScope, SelectionPolicy, TRANSLATION_PIPELINE_REVISION};

use super::{
    LocalSubtitleLimits, RetrievedSubtitle, SubtitleRetrievalOutcome, SubtitleRetrievalPort,
    SubtitleSourceValidator, SubtitleTranslationDescriptor, SubtitleTranslationError,
    SubtitleTranslationValidator,
};
use crate::translation_catalog::{TranslationCatalog, TranslationCatalogEntry};
use crate::{SUBTITLE_TRANSLATION_PIPELINE_REVISION, WEBVTT_PARSER_REVISION};

/// Result of checking a browser-held representation against its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleRevalidation {
    /// The client representation remains usable and needs no provider call.
    NotModified {
        /// Previously issued composite ETag, including its weak prefix.
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
    /// Current Unix time, used to reject expired descriptors.
    pub now_unix_seconds: u64,
    /// Single composite validator supplied by the client, if any.
    pub client_etag: Option<&'a str>,
    /// Current provider catalog, already filtered for activation and credentials.
    pub catalog: &'a TranslationCatalog,
    /// Opaque fingerprint of output-affecting options and source headers.
    pub context_fingerprint: &'a str,
    /// Maximum source document size and other local limits.
    pub limits: LocalSubtitleLimits,
}

/// Revalidates a subtitle source without invoking a translation provider.
///
/// The retrieval implementation must enforce destination, redirect, proxy and
/// header policies. A caller may only supply a fingerprint derived from trusted
/// options. The descriptor is checked before retrieval and no result body is
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
/// Returns access, expiry, retrieval, size or invalid-source errors.
pub async fn revalidate_subtitle(
    retrieval: &dyn SubtitleRetrievalPort,
    request: SubtitleRevalidationRequest<'_>,
) -> Result<SubtitleRevalidation, SubtitleTranslationError> {
    let descriptor = request.descriptor;
    descriptor.validate()?;
    descriptor.authorize(request.access_scope)?;
    if descriptor.is_expired(request.now_unix_seconds) {
        return Err(SubtitleTranslationError::TokenExpired);
    }
    if request.context_fingerprint.trim().is_empty() {
        return Err(SubtitleTranslationError::InvalidDescriptor {
            detail: "translation context fingerprint must not be blank".into(),
        });
    }
    let source_fingerprint = source_identity(descriptor)?;
    let scope_fingerprint = hash62(request.access_scope.as_str().as_bytes());
    let prior = request.client_etag.and_then(|etag| {
        let validator = SubtitleTranslationValidator::from_etag(etag).ok()?;
        let service = request.catalog.service(&validator.translation.service_id)?;
        if matches_context(
            &validator,
            descriptor,
            service,
            &source_fingerprint,
            &scope_fingerprint,
            request.context_fingerprint,
        ) {
            Some(validator)
        } else {
            None
        }
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

/// Fingerprints the complete source descriptor, including URL and format.
fn source_identity(
    descriptor: &SubtitleTranslationDescriptor,
) -> Result<String, SubtitleTranslationError> {
    let bytes = serde_json::to_vec(&descriptor.source).map_err(|_| {
        SubtitleTranslationError::InvalidDescriptor {
            detail: "unable to encode source identity".into(),
        }
    })?;
    Ok(hash62(&bytes))
}

/// Checks the client tag against the active, authorized request and service.
fn matches_context(
    validator: &SubtitleTranslationValidator,
    descriptor: &SubtitleTranslationDescriptor,
    service: &TranslationCatalogEntry,
    source_fingerprint: &str,
    scope_fingerprint: &str,
    context_fingerprint: &str,
) -> bool {
    let translation = &validator.translation;
    let allowed = match &descriptor.policy {
        SelectionPolicy::Constrained { service_id } => service_id == &service.service_id,
        SelectionPolicy::Auto { .. } => true,
    };
    let pair = service
        .target_languages
        .contains(&descriptor.target_language)
        && match &descriptor.source_language {
            Some(source) => {
                service.source_languages.contains(source)
                    && service.supported_pairs.as_ref().is_none_or(|pairs| {
                        pairs.iter().any(|pair| {
                            pair.source == *source && pair.target == descriptor.target_language
                        })
                    })
            }
            None => {
                service.source_detection
                    && service.supported_pairs.as_ref().is_none_or(|pairs| {
                        pairs
                            .iter()
                            .any(|pair| pair.target == descriptor.target_language)
                    })
            }
        };
    allowed
        && pair
        && (service.supports_text_translation || service.supports_url_translation)
        && validator.source_fingerprint == source_fingerprint
        && validator.access_scope_fingerprint == scope_fingerprint
        && validator.document_format == descriptor.source.format.media_type()
        && validator.parser_revision == WEBVTT_PARSER_REVISION
        && validator.subtitle_pipeline_revision == SUBTITLE_TRANSLATION_PIPELINE_REVISION
        && translation.source_language == descriptor.source_language
        && translation.target_language == descriptor.target_language
        && translation.service_id == service.service_id
        && service.service_yaml_hash.as_deref() == Some(translation.service_yaml_hash.as_str())
        && translation.service_configuration_revision == service.configuration_revision
        && translation.pipeline_revision == TRANSLATION_PIPELINE_REVISION
        && translation.context_fingerprint == context_fingerprint
}
