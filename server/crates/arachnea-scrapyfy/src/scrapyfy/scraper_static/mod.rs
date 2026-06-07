//! Static query definitions and executors.
//!
//! A static query returns YAML-declared rows without issuing any HTTP request.
//! It is used to expose service-level metadata (id, title, logo, description)
//! or hard-coded lists that do not depend on remote data.
//!
//! See [`query`] for the runtime and raw configuration types.

pub mod query;

pub use query::{StaticScraperEntryRaw, StaticScraperQuery, StaticScraperQueryRaw};
