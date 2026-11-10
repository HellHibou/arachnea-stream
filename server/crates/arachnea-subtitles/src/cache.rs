//! Subtitle document cache identity and persisted entries.

use crate::document::{DocumentFingerprint, SubtitleFormat};
use crate::{
    SUBTITLE_CACHE_KEY_PAIR_SEPARATOR, SUBTITLE_CACHE_KEY_SEPARATOR, SUBTITLE_CACHE_KEY_VERSION,
};
use arachnea_translation::TranslationCacheIdentity;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Complete identity of one translated subtitle document.
///
/// The generic translation identity already covers languages, service,
/// endpoint, options, configuration and access scope. This identity adds the
/// source document fingerprint, format, parser revision and subtitle pipeline
/// revision so a parser change cannot reuse an incompatible document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleTranslationCacheIdentity {
    /// Generic translation identity that produced the document.
    pub translation: TranslationCacheIdentity,
    /// Content fingerprint of the source subtitle document.
    pub document_fingerprint: DocumentFingerprint,
    /// Source document format.
    pub document_format: SubtitleFormat,
    /// WebVTT parser and serializer revision.
    pub parser_revision: String,
    /// Subtitle orchestration pipeline revision.
    pub subtitle_pipeline_revision: String,
}

impl SubtitleTranslationCacheIdentity {
    /// Creates a cache identity from a generic translation identity.
    ///
    /// # Arguments
    ///
    /// * `translation` - Generic translation result identity.
    /// * `document_fingerprint` - Source document content fingerprint.
    /// * `document_format` - Source document format.
    /// * `parser_revision` - WebVTT parser and serializer revision.
    /// * `subtitle_pipeline_revision` - Subtitle orchestration revision.
    pub fn new(
        translation: TranslationCacheIdentity,
        document_fingerprint: DocumentFingerprint,
        document_format: SubtitleFormat,
        parser_revision: impl Into<String>,
        subtitle_pipeline_revision: impl Into<String>,
    ) -> Self {
        Self {
            translation,
            document_fingerprint,
            document_format,
            parser_revision: parser_revision.into(),
            subtitle_pipeline_revision: subtitle_pipeline_revision.into(),
        }
    }

    /// Builds the persisted subtitle document cache key.
    pub fn cache_key(&self) -> SubtitleTranslationCacheKey {
        let mut key = String::from(SUBTITLE_CACHE_KEY_VERSION);
        let mut push = |name: &str, value: &str| {
            key.push(SUBTITLE_CACHE_KEY_SEPARATOR);
            key.push_str(&escape(name));
            key.push(SUBTITLE_CACHE_KEY_PAIR_SEPARATOR);
            key.push_str(&escape(value));
        };
        push("translation", self.translation.cache_key().as_str());
        push("document", self.document_fingerprint.as_str());
        push("format", format_name(self.document_format));
        push("parser", &self.parser_revision);
        push("pipeline", &self.subtitle_pipeline_revision);
        SubtitleTranslationCacheKey(key)
    }
}

/// Persisted subtitle document cache key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubtitleTranslationCacheKey(String);

impl SubtitleTranslationCacheKey {
    /// Returns the key as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the key and returns its string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for SubtitleTranslationCacheKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Persisted translated subtitle document and its response metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedSubtitleTranslation {
    /// Complete translated WebVTT bytes.
    pub bytes: Vec<u8>,
    /// Strong ETag of the final bytes, including surrounding quotes.
    pub etag: String,
    /// Response media type.
    pub media_type: String,
    /// Response cache policy.
    pub cache_control: String,
    /// Publication time as Unix seconds.
    pub created_at_unix_seconds: u64,
    /// Retention expiration as Unix seconds.
    pub expires_at_unix_seconds: u64,
}

impl CachedSubtitleTranslation {
    /// Returns whether the entry is still retained.
    ///
    /// # Arguments
    ///
    /// * `now_unix_seconds` - Current Unix time in seconds.
    pub fn is_retained(&self, now_unix_seconds: u64) -> bool {
        now_unix_seconds < self.expires_at_unix_seconds
    }
}

/// Returns the serialized name of one subtitle format.
///
/// # Arguments
///
/// * `format` - Format to name.
fn format_name(format: SubtitleFormat) -> &'static str {
    match format {
        SubtitleFormat::WebVtt => "webvtt",
        SubtitleFormat::Srt => "srt",
    }
}

/// Escapes key separators so distinct identities cannot collide.
///
/// # Arguments
///
/// * `value` - Raw component value.
fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character == '\\' {
            escaped.push_str("\\\\");
        } else if character == SUBTITLE_CACHE_KEY_SEPARATOR
            || character == SUBTITLE_CACHE_KEY_PAIR_SEPARATOR
        {
            escaped.push_str(&format!("\\u{{{:x}}}", character as u32));
        } else {
            escaped.push(character);
        }
    }
    escaped
}
