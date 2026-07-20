//! Thread-safe error code generator for Arachnea backend services.
//!
//! The generated codes are correlation identifiers that allow frontend error
//! notifications to be matched against server-side log entries. The format is
//! `ARACHNEA_E{unix_millis_utc}{sequence:02}`, which is monotonically
//! increasing within a single process.

use serde::Serialize;
use std::sync::Mutex;
use time::OffsetDateTime;

/// An Arachnea error correlation code.
///
/// Format: `ARACHNEA_E{timestamp_millis_utc}{sequence:02}`
#[derive(Debug, Clone, Serialize)]
pub struct ArachneaErrorCode(String);

impl std::fmt::Display for ArachneaErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

struct Inner {
    last_millis: i64,
    sequence: u8,
}

/// Thread-safe generator producing unique [`ArachneaErrorCode`] values.
///
/// The generator guarantees intra-process monotonicity: two calls to
/// [`next_code`](ErrorCodeGenerator::next_code) never return the same code.
/// Sequence numbers wrap from 00 to 99 per millisecond; if more than 100
/// codes are requested during the same clock millisecond the generator
/// advances to the next logical millisecond to avoid overflow.
///
/// # Example
///
/// ```
/// use arachnea_core::error_code::ErrorCodeGenerator;
///
/// let gen = ErrorCodeGenerator::new();
/// let code = gen.next_code();
/// assert!(code.to_string().starts_with("ARACHNEA_E"));
/// ```
pub struct ErrorCodeGenerator {
    inner: Mutex<Inner>,
}

impl ErrorCodeGenerator {
    /// Creates a new generator with reset internal state.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                last_millis: 0,
                sequence: 0,
            }),
        }
    }

    /// Produces the next unique error correlation code.
    ///
    /// The returned code is unique within this process for the lifetime of the
    /// generator.
    pub fn next_code(&self) -> ArachneaErrorCode {
        let mut guard = self.inner.lock().expect("ErrorCodeGenerator mutex poisoned");
        let now = OffsetDateTime::now_utc();
        let millis = now.unix_timestamp() * 1000 + now.millisecond() as i64;

        if millis == guard.last_millis {
            guard.sequence += 1;
            if guard.sequence > 99 {
                guard.last_millis += 1;
                guard.sequence = 0;
            }
        } else {
            guard.last_millis = millis;
            guard.sequence = 0;
        }

        ArachneaErrorCode(format!(
            "ARACHNEA_E{}{:02}",
            guard.last_millis, guard.sequence,
        ))
    }
}

impl Default for ErrorCodeGenerator {
    fn default() -> Self {
        Self::new()
    }
}
