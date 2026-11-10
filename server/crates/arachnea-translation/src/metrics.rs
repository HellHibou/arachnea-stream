//! Non-sensitive translation execution metrics.

use serde::{Deserialize, Serialize};

/// Counters describing one translation execution.
///
/// Metrics never contain translated text, source text, credentials or provider
/// secrets. They are suitable for logs and cache diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationMetrics {
    /// Successfully completed translation batches.
    pub batches: u64,
    /// Provider query attempts, including retries and detection calls.
    pub attempts: u64,
    /// Retried provider query attempts.
    pub retries: u64,
    /// Dedicated source-detection attempts.
    pub detection_attempts: u64,
    /// Translated blocks included in completed batches.
    pub blocks: u64,
    /// Provider units included in completed batches.
    pub units: u64,
    /// Wall-clock execution time in milliseconds.
    pub elapsed_ms: u64,
}
