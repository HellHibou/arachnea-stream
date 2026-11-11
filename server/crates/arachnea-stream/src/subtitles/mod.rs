//! Subtitle document processing for the Stream backend.
//!
//! This module owns WebVTT and subtitle-specific types: source track
//! descriptors, document format and statistics, translated subtitle
//! descriptors, authorized conditional retrieval, composite validators, the WebVTT parser and serializer, cue segmentation and the
//! document-level translation plan. It uses the generic Scrapyfy bridge and
//! never contains provider-specific mapping or player logic.
//!
//! WebVTT parsing keeps cue identifiers, raw timing lines and non-cue blocks.
//! Only visible cue text is submitted to translation; inline markup and
//! internal timestamps remain protected in their cue. Direct HTTP retrieval,
//! conditional revalidation, internal proxy retrieval and text/URL execution
//! are available here. Public REST/Tauri transport remains a separate layer.

pub mod cache;
pub mod descriptor;
pub mod document;
pub mod error;
pub mod orchestration;
pub mod pipeline;
pub mod ports;
pub mod retrieval;
mod retrieval_proxy;
pub mod revalidation;
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
pub use orchestration::{
    translate_subtitle, SubtitleTranslationOutcome, SubtitleTranslationRequest,
};
pub use pipeline::SubtitleDocumentPlan;
pub use ports::SubtitleRetrievalPort;
pub use retrieval::SubtitleHttpRetrieval;
pub(crate) use revalidation::service_supports_pair;
pub use revalidation::{revalidate_subtitle, SubtitleRevalidation, SubtitleRevalidationRequest};
pub use webvtt::{WebVttBlock, WebVttCue, WebVttDocument, WebVttHeader};
