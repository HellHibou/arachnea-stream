//! Streaming data attached to authoritative common UUIDs.

/// Versioned streaming persistence entities.
pub mod models;
mod repository;
pub use repository::StreamingRepository;

mod operations;
/// Transactional collection revisions and tombstones.
pub(crate) mod state;
mod validation;
