//! Browser-held subtitle validators and composite HTTP entity tags.

use super::error::SubtitleTranslationError;
use crate::{SUBTITLE_ETAG_MAX_LENGTH, SUBTITLE_ETAG_PREFIX};
use arachnea_translation::TranslationRevision;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

/// Validator used to check whether the upstream subtitle source changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubtitleSourceValidator {
    /// Original HTTP entity tag, including quotes and any `W/` prefix.
    Etag {
        /// Upstream entity tag to send in `If-None-Match`.
        etag: String,
    },
    /// Digest of the source bytes, optionally with an HTTP modification date.
    Content {
        /// Digest of the complete source bytes.
        hash: String,
        /// Original `Last-Modified` value for `If-Modified-Since`, if present.
        last_modified: Option<String>,
    },
}

/// Complete identity of a browser-held translated subtitle representation.
///
/// Fingerprints are opaque, non-secret digests bound to the authorized source
/// and access scope. This tag is client-controlled and must never authorize a
/// request: check access and compare its fields with the current request before
/// using its source validator. A tag does not prove source freshness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubtitleTranslationValidator {
    /// Opaque fingerprint of the authorized source track identity.
    pub source_fingerprint: String,
    /// Opaque fingerprint binding the tag to the authorized access scope.
    pub access_scope_fingerprint: String,
    /// Upstream source validator or content digest.
    pub source: SubtitleSourceValidator,
    /// Provider, language and translation revision identity.
    pub translation: TranslationRevision,
    /// Source document format and interpretation revision.
    pub document_format: String,
    /// Revision of the WebVTT parser and serializer.
    pub parser_revision: String,
    /// Revision of the subtitle orchestration pipeline.
    pub subtitle_pipeline_revision: String,
}

impl SubtitleTranslationValidator {
    /// Encodes the identity as a versioned, weak HTTP entity tag.
    ///
    /// # Errors
    ///
    /// Returns an invalid-descriptor error for malformed or oversized input.
    pub fn to_etag(&self) -> Result<String, SubtitleTranslationError> {
        self.validate()?;
        let json = serde_json::to_vec(self).map_err(|_| invalid_validator())?;
        let tag = format!(
            "W/\"{}{}\"",
            SUBTITLE_ETAG_PREFIX,
            URL_SAFE_NO_PAD.encode(json)
        );
        if tag.len() > SUBTITLE_ETAG_MAX_LENGTH {
            return Err(invalid_validator());
        }
        Ok(tag)
    }

    /// Decodes a single entity tag produced by [`Self::to_etag`].
    ///
    /// `If-None-Match` lists and `*` must be parsed by the transport layer;
    /// a caller must independently authorize and revalidate the decoded data.
    ///
    /// # Arguments
    ///
    /// * `etag` - One weak or strong entity tag, including its quotes.
    ///
    /// # Errors
    ///
    /// Returns an invalid-descriptor error for malformed, unsupported or oversized tags.
    pub fn from_etag(etag: &str) -> Result<Self, SubtitleTranslationError> {
        let etag = etag.trim();
        if etag.len() > SUBTITLE_ETAG_MAX_LENGTH {
            return Err(invalid_validator());
        }
        let quoted = etag.strip_prefix("W/").unwrap_or(etag);
        let encoded = quoted
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .and_then(|v| v.strip_prefix(SUBTITLE_ETAG_PREFIX))
            .ok_or_else(invalid_validator)?;
        let json = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| invalid_validator())?;
        let validator: Self = serde_json::from_slice(&json).map_err(|_| invalid_validator())?;
        validator.validate()?;
        Ok(validator)
    }

    /// Checks required fields and the syntax of the upstream validator.
    fn validate(&self) -> Result<(), SubtitleTranslationError> {
        let t = &self.translation;
        if [
            self.source_fingerprint.as_str(),
            self.access_scope_fingerprint.as_str(),
            self.document_format.as_str(),
            self.parser_revision.as_str(),
            self.subtitle_pipeline_revision.as_str(),
            t.service_id.as_str(),
            t.service_yaml_hash.as_str(),
            t.service_configuration_revision.as_str(),
            t.pipeline_revision.as_str(),
            t.context_fingerprint.as_str(),
        ]
        .iter()
        .any(|value| value.trim().is_empty())
        {
            return Err(invalid_validator());
        }
        match &self.source {
            SubtitleSourceValidator::Etag { etag } if valid_entity_tag(etag) => Ok(()),
            SubtitleSourceValidator::Content {
                hash,
                last_modified,
            } if !hash.trim().is_empty()
                && last_modified.as_ref().is_none_or(|value| {
                    !value.trim().is_empty() && http::HeaderValue::from_str(value).is_ok()
                }) =>
            {
                Ok(())
            }
            _ => Err(invalid_validator()),
        }
    }
}

/// Accepts one HTTP entity tag, preserving weak upstream validators.
fn valid_entity_tag(value: &str) -> bool {
    let value = value.strip_prefix("W/").unwrap_or(value);
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return false;
    };
    inner
        .bytes()
        .all(|byte| byte == 0x21 || (0x23..=0x7e).contains(&byte) || byte >= 0x80)
}

fn invalid_validator() -> SubtitleTranslationError {
    SubtitleTranslationError::InvalidDescriptor {
        detail: "invalid subtitle translation validator".to_string(),
    }
}
