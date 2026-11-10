#![warn(missing_docs)]
//! Subtitle document contracts for the Arachnea backend.
//!
//! This crate owns WebVTT and subtitle-specific types: source track
//! descriptors, document format and statistics, translated subtitle
//! descriptors, authorized retrieval and document cache ports, and the
//! document cache identity. It depends on the generic
//! `arachnea-translation` contracts and never on `arachnea-stream`.
//!
//! The parser and serializer are implemented at lot 3. Lot 1 defines the
//! boundaries, descriptors, access rules and cache-key contract so later layers
//! cannot leak WebVTT knowledge into the generic translation engine.

pub mod cache;
pub mod descriptor;
pub mod document;
pub mod error;
pub mod ports;

pub use cache::{
    CachedSubtitleTranslation, SubtitleTranslationCacheIdentity, SubtitleTranslationCacheKey,
};
pub use descriptor::{
    SubtitleTranslationDescriptor, SubtitleTranslationPreparation, SubtitleTranslationToken,
};
pub use document::{
    DocumentFingerprint, RetrievedSubtitle, SubtitleDocumentStats, SubtitleFormat,
    SubtitleSourceDescriptor, TranslatedSubtitleDocument,
};
pub use error::SubtitleTranslationError;
pub use ports::{SubtitleRetrievalPort, SubtitleTranslationCachePort};

// Subtitle document and cache contract identifiers.

/// Media type of a complete WebVTT document.
pub const WEBVTT_MEDIA_TYPE: &str = "text/vtt";

/// Media type of a SubRip document.
pub const SRT_MEDIA_TYPE: &str = "application/x-subrip";

/// Revision of the subtitle translation pipeline recorded in cache keys.
pub const SUBTITLE_TRANSLATION_PIPELINE_REVISION: &str = "subtitles-1";

/// Initial revision of the WebVTT parser and serializer.
///
/// Replace this value when parsing or serialization behavior changes in a way
/// that can alter the cached document.
pub const WEBVTT_PARSER_REVISION: &str = "webvtt-parser-1";

/// Version prefix of persisted subtitle document cache keys.
pub const SUBTITLE_CACHE_KEY_VERSION: &str = "st1";

/// Separator between the fields of a persisted subtitle cache key.
pub const SUBTITLE_CACHE_KEY_SEPARATOR: char = '\u{1d}';

/// Separator between a subtitle cache-key field name and its value.
pub const SUBTITLE_CACHE_KEY_PAIR_SEPARATOR: char = '\u{1c}';

/// Default maximum decompressed source document size, in bytes.
pub const DEFAULT_MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;

/// Default maximum number of cues accepted in one document.
pub const DEFAULT_MAX_CUES: u64 = 10_000;

/// Default maximum translatable units accepted in one document.
pub const DEFAULT_MAX_TRANSLATABLE_UNITS: u64 = 500_000;

/// Default source freshness window, in seconds.
pub const DEFAULT_SOURCE_FRESHNESS_SECONDS: u64 = 86_400;

/// Default retention of published translated documents, in seconds.
pub const DEFAULT_RESULT_RETENTION_SECONDS: u64 = 30 * 86_400;

/// Default lifetime of one prepared translation token, in seconds.
pub const DEFAULT_TOKEN_TTL_SECONDS: u64 = 86_400;

/// Default cache policy shared by translated subtitle file responses.
pub const DEFAULT_SUBTITLE_CACHE_CONTROL: &str = "private, no-cache";
