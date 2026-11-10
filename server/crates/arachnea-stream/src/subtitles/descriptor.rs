//! Persistent subtitle translation descriptors and opaque tokens.

use super::document::SubtitleSourceDescriptor;
use super::error::SubtitleTranslationError;
use crate::DEFAULT_TOKEN_TTL_SECONDS;
use arachnea_translation::{AccessScope, LanguageCode, SelectionPolicy};
use serde::{Deserialize, Serialize};

/// Opaque token identifying a prepared subtitle translation.
///
/// The token is not a credential and must not contain a source URL. It has to
/// remain stable for one authorized request so the browser HTTP cache stays
/// useful, while backend renewal must resolve to the same result.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubtitleTranslationToken(String);

impl SubtitleTranslationToken {
    /// Creates a validated opaque token.
    ///
    /// # Arguments
    ///
    /// * `value` - Nonblank opaque token without whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::InvalidDescriptor`] when the token
    /// is blank or contains whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, SubtitleTranslationError> {
        let value = value.into();
        if value.trim().is_empty() || value.chars().any(char::is_whitespace) {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "subtitle translation token must be nonblank and contain no whitespace"
                    .to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the token as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Inputs used to prepare a persistent subtitle translation descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleTranslationPreparation {
    /// Opaque stable token.
    pub token: SubtitleTranslationToken,
    /// Source track descriptor.
    pub source: SubtitleSourceDescriptor,
    /// Known source language, or `None` when detection is requested.
    pub source_language: Option<LanguageCode>,
    /// Target language.
    pub target_language: LanguageCode,
    /// Automatic, preferred or constrained service policy.
    pub policy: SelectionPolicy,
    /// Access scope required to download the result.
    pub access_scope: AccessScope,
    /// Preparation time as Unix seconds.
    pub now_unix_seconds: u64,
    /// Optional lifetime; `None` uses [`DEFAULT_TOKEN_TTL_SECONDS`].
    pub ttl_seconds: Option<u64>,
}

/// Persisted descriptor of one prepared subtitle translation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleTranslationDescriptor {
    /// Opaque stable token.
    pub token: SubtitleTranslationToken,
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
    /// Creation time as Unix seconds.
    pub created_at_unix_seconds: u64,
    /// Expiration time as Unix seconds.
    pub expires_at_unix_seconds: u64,
}

impl SubtitleTranslationDescriptor {
    /// Prepares a descriptor with a bounded lifetime.
    ///
    /// # Arguments
    ///
    /// * `preparation` - Token, source, languages, policy, access scope and
    ///   lifetime inputs.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::InvalidDescriptor`] for an invalid
    /// source, a same-language request or a zero lifetime.
    pub fn prepare(
        preparation: SubtitleTranslationPreparation,
    ) -> Result<Self, SubtitleTranslationError> {
        let SubtitleTranslationPreparation {
            token,
            source,
            source_language,
            target_language,
            policy,
            access_scope,
            now_unix_seconds,
            ttl_seconds,
        } = preparation;
        source.validate()?;
        if source_language.as_ref() == Some(&target_language) {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "source and target languages are identical".to_string(),
            });
        }
        let ttl_seconds = ttl_seconds.unwrap_or(DEFAULT_TOKEN_TTL_SECONDS);
        if ttl_seconds == 0 {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "subtitle translation token lifetime must be greater than zero".to_string(),
            });
        }
        Ok(Self {
            token,
            source,
            source_language,
            target_language,
            policy,
            access_scope,
            created_at_unix_seconds: now_unix_seconds,
            expires_at_unix_seconds: now_unix_seconds.saturating_add(ttl_seconds),
        })
    }

    /// Validates the descriptor structure.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::InvalidDescriptor`] for an invalid
    /// source or an inconsistent expiration.
    pub fn validate(&self) -> Result<(), SubtitleTranslationError> {
        self.source.validate()?;
        if self.expires_at_unix_seconds <= self.created_at_unix_seconds {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "descriptor expiration must be after its creation".to_string(),
            });
        }
        if self.source_language.as_ref() == Some(&self.target_language) {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "source and target languages are identical".to_string(),
            });
        }
        Ok(())
    }

    /// Returns whether the token has expired.
    ///
    /// # Arguments
    ///
    /// * `now_unix_seconds` - Current Unix time in seconds.
    pub fn is_expired(&self, now_unix_seconds: u64) -> bool {
        now_unix_seconds >= self.expires_at_unix_seconds
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
