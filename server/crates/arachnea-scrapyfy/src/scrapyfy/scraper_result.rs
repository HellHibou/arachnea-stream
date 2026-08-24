//! Serializable envelope types for `StreamScraper` JSON commands.
//!
//! These types wrap the response of every backend JSON command so that
//! per-source errors can be returned alongside partial data without failing
//! the entire request. The frontend [`call_api`] adapter unpacks the envelope
//! and routes errors to a global notification stack.

use arachnea_core::error_code::ArachneaErrorCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Freshness status of one source after conditional validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperSourceStatus {
    /// Remote content changed (or first request) — the source was parsed.
    Fresh,
    /// Remote content unchanged (`304` or identical hash) — not parsed.
    Stale,
}

/// Per-source conditional-validation outcome attached to an aggregation result.
///
/// The `etag` fragment is always up to date, whether the source is fresh or
/// stale, so callers can rebuild a coherent global ETag from phase-1 results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperSourceValidation {
    /// Whether the remote content was parsed for this request.
    pub status: ScraperSourceStatus,
    /// Up-to-date validator fragment (`E:...` or `C:...`) for this source.
    pub etag: String,
}

/// Origin of a scraper execution error.
///
/// Used to distinguish errors produced by the server from errors synthesized
/// on the client side (e.g. transport failure, invalid JSON).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperErrorOrigin {
    /// Error produced by the backend server (logged server-side).
    Backend,
    /// Error produced locally on the frontend.
    Frontend,
}

/// A structured error payload for a single source within an aggregate request.
///
/// Each instance corresponds to a single source that failed during execution.
/// The `code` field links the frontend notification to a server-side log
/// entry for diagnostics.
#[derive(Debug, Clone, Serialize)]
pub struct ScraperExecutionError {
    /// Correlation code matching the server log entry.
    pub code: ArachneaErrorCode,

    /// The operation that was being executed (e.g. `"load_home"`, `"search"`).
    pub operation: String,

    /// The source identifier, when the error can be attributed to one.
    ///
    /// `None` when the error occurred before source dispatch (e.g. validation).
    pub source: Option<String>,

    /// Origin of the error — backend or frontend.
    pub origin: ScraperErrorOrigin,

    /// Human-readable technical message for diagnostics.
    ///
    /// This is the full error text. The UI may show a localized summary by
    /// default and reveal this detail on user request.
    pub message: String,
}

/// Envelope returned by all JSON commands of [`StreamScraper`].
///
/// # Contract
///
/// * `data` is always present — its shape is the same as the previous bare
///   response for that command.
/// * `errors` is always present and empty on full success.
/// * Per-source errors never cause a HTTP 400 or a Tauri rejection; the
///   envelope is always serialized as a successful response.
///
/// # Example (serialized)
///
/// ```json
/// {
///   "data": [/* command-specific payload */],
///   "errors": [
///     {
///       "code": "ARACHNEA_E172147680012300",
///       "operation": "load_home",
///       "source": "example-source",
///       "origin": "backend",
///       "message": "Request failed: …"
///     }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct ScraperAggregationResult<T> {
    /// Successful response data, in the command-specific shape.
    pub data: T,

    /// Per-source error collection. Empty on full success.
    pub errors: Vec<ScraperExecutionError>,

    /// Per-source conditional-validation outcomes, keyed by source name.
    ///
    /// Empty when ETag validation is disabled or unsupported by the operation.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub validations: HashMap<String, ScraperSourceValidation>,
}

impl<T> ScraperAggregationResult<T> {
    /// Creates a fully successful result with no errors.
    pub fn ok(data: T) -> Self {
        Self {
            data,
            errors: Vec::new(),
            validations: HashMap::new(),
        }
    }

    /// Creates a result from data and a list of errors.
    pub fn new(data: T, errors: Vec<ScraperExecutionError>) -> Self {
        Self {
            data,
            errors,
            validations: HashMap::new(),
        }
    }

    /// Attaches per-source validation outcomes to this result.
    pub fn with_validations(
        mut self,
        validations: HashMap<String, ScraperSourceValidation>,
    ) -> Self {
        self.validations = validations;
        self
    }

    /// Returns `true` if there are no errors.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}
