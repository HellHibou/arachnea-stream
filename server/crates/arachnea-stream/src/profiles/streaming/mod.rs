//! Streaming data attached to authoritative common UUIDs.

/// Versioned streaming persistence entities.
pub mod models;
mod repository;
pub use repository::StreamingRepository;
