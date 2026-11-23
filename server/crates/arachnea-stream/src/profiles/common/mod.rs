//! Shared account and profile domain, independent of streaming tables.

/// Common typed persistence entities.
pub mod models;
mod repository;
pub use repository::CommonRepository;
