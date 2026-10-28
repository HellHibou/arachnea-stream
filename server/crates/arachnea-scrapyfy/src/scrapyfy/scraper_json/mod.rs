//! JSON query definitions and executors.
//!
//! A JSON query fetches a JSON payload from a remote endpoint (or a sibling
//! sub-query context) and extracts typed rows.
//!
//! See [`crate::scrapyfy::scraper_json::query`] for the runtime and raw configuration types of
//! [`JsonScraperQuery`] and [`crate::scrapyfy::scraper_json::query::JsonScraperSubQuery`], and [`crate::scrapyfy::scraper_json::entry`] for the
//! runtime and raw configuration types of [`JsonScraperEntry`].
//!
//! The module is organised as follows:
//!
//! * [`crate::scrapyfy::scraper_json::config`] — raw YAML deserialization targets and bidirectional conversions.
//! * [`crate::scrapyfy::scraper_json::entry`] — field and group extractors applied to JSON rows.
//! * [`crate::scrapyfy::scraper_json::query`] — runtime query and sub-query types with their execution logic.
//! JSON pointer selection is implemented in [`crate::scrapyfy::scraper_json::entry`]. Response row collection,
//! filtering and post-processing are handled by the unified executor in
//! `crate::scrapyfy::scraper::query_executor`.

pub mod config;
/// JSON scraper entry configuration and runtime types.
pub mod entry;
pub mod query;

pub use config::EntrySubQueryRaw;
pub use entry::{JsonScraperEntry, JsonScraperEntryRaw};
pub use query::{JsonScraperQuery, JsonScraperQueryRaw};
