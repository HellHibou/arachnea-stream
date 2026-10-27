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
//! * [`query`] — runtime query and sub-query types with their execution logic.
//! JSON pointer selection is implemented in [`entry`]. Response row collection,
//! filtering and post-processing are handled by the unified executor in
//! [`crate::scrapyfy::scraper::query_executor`].

pub mod config;
/// JSON scraper entry configuration and runtime types.
pub mod entry;
pub mod query;

pub use config::EntrySubQueryRaw;
pub use entry::{JsonScraperEntry, JsonScraperEntryRaw};
pub use query::{JsonScraperQuery, JsonScraperQueryRaw};
