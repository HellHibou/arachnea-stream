//! Browser-held subtitle validators using Scrapyfy-compatible ETag fragments.

use std::collections::HashMap;

use arachnea_scrapyfy::scrapyfy::conditional::{
    conditional_from_fragment, fragment_from_response, fragment_yaml_hash,
};
use arachnea_scrapyfy::scrapyfy::global_etag::{build_global_etag, decode_client_fragments};
use serde::{Deserialize, Serialize};

use super::error::SubtitleTranslationError;
use crate::{SUBTITLE_ETAG_MAX_DATE_UNIX_SECONDS, SUBTITLE_ETAG_MAX_LENGTH};

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

/// One provider and source validator carried by a translated subtitle ETag.
///
/// The request URL distinguishes languages and options. Client-supplied tags
/// never authorize access or establish source freshness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleTranslationValidator {
    /// Translation service identifier used as the source name.
    pub service_id: String,
    /// Translation service YAML hash.
    pub service_yaml_hash: String,
    /// Upstream source validator or content digest.
    pub source: SubtitleSourceValidator,
}

impl SubtitleTranslationValidator {
    /// Encodes one Scrapyfy-compatible source fragment as a weak HTTP ETag.
    ///
    /// # Errors
    /// Returns an invalid-descriptor error for malformed or oversized input.
    pub fn to_etag(&self) -> Result<String, SubtitleTranslationError> {
        self.validate()?;
        let yaml = &self.service_yaml_hash;
        let fragment = match &self.source {
            SubtitleSourceValidator::Etag { etag } => {
                fragment_from_response(yaml, Some(etag), None, "")
            }
            SubtitleSourceValidator::Content {
                hash,
                last_modified,
            } => {
                let empty = fragment_from_response(yaml, None, last_modified.as_deref(), "");
                let (prefix, _) = empty.rsplit_once('-').ok_or_else(invalid_validator)?;
                format!("{prefix}-{hash}")
            }
        };
        let fragments = HashMap::from([(self.service_id.clone(), fragment)]);
        let value = build_global_etag(&fragments).ok_or_else(invalid_validator)?;
        let etag = format!("W/\"{value}\"");
        if etag.len() > SUBTITLE_ETAG_MAX_LENGTH {
            return Err(invalid_validator());
        }
        Ok(etag)
    }

    /// Decodes one canonical weak or strong Scrapyfy-compatible entity tag.
    ///
    /// # Arguments
    /// * `etag` - Single HTTP entity tag, including its quotes.
    ///
    /// # Errors
    /// Returns an error for malformed, unsupported or oversized tags.
    pub fn from_etag(etag: &str) -> Result<Self, SubtitleTranslationError> {
        let etag = etag.trim();
        if etag.len() > SUBTITLE_ETAG_MAX_LENGTH {
            return Err(invalid_validator());
        }
        let quoted = etag.strip_prefix("W/").unwrap_or(etag);
        if !valid_entity_tag(quoted) {
            return Err(invalid_validator());
        }
        let opaque = quoted
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .ok_or_else(invalid_validator)?;
        if opaque.contains(';') {
            return Err(invalid_validator());
        }
        let fragments = decode_client_fragments(opaque);
        if fragments.len() != 1 || build_global_etag(&fragments).as_deref() != Some(opaque) {
            return Err(invalid_validator());
        }
        let (service_id, fragment) = fragments.into_iter().next().ok_or_else(invalid_validator)?;
        let yaml_hash = fragment_yaml_hash(&fragment)
            .filter(|hash| is_base62(hash))
            .ok_or_else(invalid_validator)?
            .to_string();
        if let Some(rest) = fragment.strip_prefix(&format!("C:{yaml_hash}-")) {
            let (timestamp, hash) = rest.split_once('-').ok_or_else(invalid_validator)?;
            let timestamp: u64 = timestamp.parse().map_err(|_| invalid_validator())?;
            if timestamp > SUBTITLE_ETAG_MAX_DATE_UNIX_SECONDS || !is_base62(hash) {
                return Err(invalid_validator());
            }
        } else if !fragment.starts_with(&format!("E:{yaml_hash}-")) {
            return Err(invalid_validator());
        }
        let conditional = conditional_from_fragment(&fragment).ok_or_else(invalid_validator)?;
        let source = if let Some(etag) = conditional.if_none_match {
            if !valid_entity_tag(&etag) {
                return Err(invalid_validator());
            }
            SubtitleSourceValidator::Etag { etag }
        } else if let Some(hash) = conditional.content_hash {
            SubtitleSourceValidator::Content {
                hash,
                last_modified: conditional.if_modified_since,
            }
        } else {
            return Err(invalid_validator());
        };
        let validator = Self {
            service_id,
            service_yaml_hash: yaml_hash,
            source,
        };
        if validator.to_etag()?.strip_prefix("W/") != Some(quoted) {
            return Err(invalid_validator());
        }
        Ok(validator)
    }

    /// Checks the service identifier, YAML hash and source validator.
    fn validate(&self) -> Result<(), SubtitleTranslationError> {
        if self.service_id.trim().is_empty() || !is_base62(&self.service_yaml_hash) {
            return Err(invalid_validator());
        }
        match &self.source {
            SubtitleSourceValidator::Etag { etag } if valid_entity_tag(etag) => Ok(()),
            SubtitleSourceValidator::Content {
                hash,
                last_modified,
            } if is_base62(hash)
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

/// Checks one nonempty base62 hash segment.
fn is_base62(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// Accepts one HTTP entity tag, preserving weak upstream validators.
pub(super) fn valid_entity_tag(value: &str) -> bool {
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
