//! JSON query definitions and executors.
//!
//! A JSON query fetches a JSON payload from a remote endpoint (or a sibling
//! sub-query context) and extracts typed rows.
//!
//! See [`query`] for the runtime and raw configuration types of
//! [`JsonScraperQuery`] and [`JsonScraperSubQuery`], and [`entry`] for the
//! runtime and raw configuration types of [`JsonScraperEntry`].
//!
//! The module is organised as follows:
//!
//! * [`config`] — raw YAML deserialization targets and bidirectional conversions.
//! * [`entry`] — field and group extractors applied to JSON rows.
//! * [`pointer`] — JSON pointer utilities (placeholder, core functions live in [`entry`]).
//! * [`query`] — runtime query and sub-query types with their execution logic.
//! * [`response_parser`] — helpers for collecting and filtering JSON rows.
//! * [`row_extractor`] — entry application and row extraction (placeholder).

pub mod config;
pub mod entry;
pub(crate) mod pointer;
pub mod query;
pub(crate) mod response_parser;
pub(crate) mod row_extractor;

pub use entry::{JsonScraperEntry, JsonScraperEntryRaw};
pub use query::{JsonScraperQuery, JsonScraperQueryRaw};