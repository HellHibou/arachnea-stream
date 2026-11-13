//! In-memory descriptors of authorized subtitle translation requests.

use super::error::SubtitleTranslationError;
use super::retrieval::SubtitleSourceDescriptor;
use crate::{AccessScope, LanguageCode, SelectionPolicy};
use serde::{Deserialize, Serialize};

/// In-memory descriptor of one authorized subtitle translation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleTranslationDescriptor {
    /// Source track descriptor.
    pub source: SubtitleSourceDescriptor,
    /// Known source language, or `None` when detection is requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_language: Option<LanguageCode>,
    /// Target language.
    pub target_language: LanguageCode,
    /// Automatic, preferred or constrained service policy.
    pub policy: SelectionPolicy,
    /// Access scope required to download the result.
    pub access_scope: AccessScope,
}

impl SubtitleTranslationDescriptor {
    /// Validates the descriptor structure.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::InvalidDescriptor`] for an invalid
    /// source or identical source and target languages.
    pub fn validate(&self) -> Result<(), SubtitleTranslationError> {
        self.source.validate()?;
        if self.source_language.as_ref() == Some(&self.target_language) {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "source and target languages are identical".to_string(),
            });
        }
        Ok(())
    }

    /// Verifies that the caller owns the descriptor access scope.
    ///
    /// # Arguments
    ///
    /// * `access_scope` - Access scope resolved for the current caller.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::AccessDenied`] when the scopes differ.
    pub fn authorize(&self, access_scope: &AccessScope) -> Result<(), SubtitleTranslationError> {
        if &self.access_scope == access_scope {
            Ok(())
        } else {
            Err(SubtitleTranslationError::AccessDenied)
        }
    }
}

#[cfg(feature = "runtime")]
impl SubtitleTranslationDescriptor {
    /// Computes the document-specific provider revision carried by validators.
    ///
    /// # Arguments
    /// * `service` - Effective provider metadata and configuration revision.
    pub fn validator_hash(&self, service: &crate::catalog::TranslationCatalogEntry) -> String {
        let identity = serde_json::json!({
            "pipeline": crate::DOCUMENT_PIPELINE_REVISION,
            "descriptor": self,
            "configuration": service.configuration_revision,
            "yaml": service.service_yaml_hash,
        });
        arachnea_scrapyfy::scrapyfy::conditional::hash62(identity.to_string().as_bytes())
    }
}
