//! JSON query definitions and executors.
//!
//! A JSON query fetches a JSON payload from a remote endpoint (or a sibling
//! sub-query context) and extracts typed rows.
//!
//! See [`query`] for the runtime and raw configuration types of
//! [`JsonScraperQuery`] and [`JsonScraperSubQuery`], and [`entry`] for the
//! runtime and raw configuration types of [`JsonScraperEntry`].

pub mod entry;
pub mod query;

pub use entry::{JsonScraperEntry, JsonScraperEntryRaw};
pub use query::{JsonScraperQuery, JsonScraperQueryRaw};
