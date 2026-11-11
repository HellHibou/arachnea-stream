//! Subtitle document processing for the Stream backend.
//!
//! This module owns WebVTT and subtitle-specific types: source track
//! descriptors, document format and statistics, translated subtitle
//! descriptors, authorized conditional retrieval, composite validators, the WebVTT parser and serializer, cue segmentation and the
//! document-level translation plan. It depends on the generic
//! `arachnea-translation` contracts and never on provider YAML or a player.
//!
//! WebVTT parsing keeps cue identifiers, raw timing lines and non-cue blocks.
//! Only visible cue text is submitted to translation; inline markup and
//! internal timestamps remain protected in their cue. Provider execution,
//! document retrieval, revalidation and transport stay in later layers.

pub mod cache;
pub mod descriptor;
pub mod document;
pub mod error;
pub mod pipeline;
pub mod ports;
mod segments;
pub mod webvtt;

pub use cache::{SubtitleSourceValidator, SubtitleTranslationValidator};
pub use descriptor::{
    SubtitleTranslationDescriptor, SubtitleTranslationPreparation, SubtitleTranslationToken,
};
pub use document::{
    LocalSubtitleLimits, RetrievedSubtitle, SubtitleDocumentStats, SubtitleFormat,
    SubtitleRetrievalOutcome, SubtitleSourceDescriptor, TranslatedSubtitleDocument,
};
pub use error::SubtitleTranslationError;
pub use pipeline::SubtitleDocumentPlan;
pub use ports::SubtitleRetrievalPort;
pub use webvtt::{WebVttBlock, WebVttCue, WebVttDocument, WebVttHeader};
