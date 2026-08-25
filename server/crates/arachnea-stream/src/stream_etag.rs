//! Global ETag construction and decoding for aggregated scraper responses.
//!
//! The implementation moved to the generic scraping engine
//! ([`arachnea_scrapyfy::global_etag`]); this module keeps the historical
//! re-exports for `arachnea-stream` consumers.

pub use arachnea_scrapyfy::global_etag::{
    build_global_etag, decode_client_fragments, normalize_client_etag, GLOBAL_ETAG_SEPARATOR,
};
