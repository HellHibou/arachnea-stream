//! Text scraper — parses a text payload split by row and field delimiters.

pub mod entry;
pub mod query;

pub use entry::{TextScraperEntry, TextScraperEntryRaw};
pub use query::{TextScraperQuery, TextScraperQueryRaw};
