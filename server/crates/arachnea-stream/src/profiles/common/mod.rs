//! Shared account and profile domain, independent of streaming tables.

/// Common typed persistence entities.
pub mod models;
/// Durable opaque operation records, independent of service tables.
pub mod operations;
mod repository;
pub use repository::CommonRepository;
