//! Temporary persistence directories shared by existing unit-test suites.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Creates an empty, unique temporary-directory path for a persistence scenario.
///
/// # Arguments
/// - `namespace`: Existing suite prefix distinguishing SQLite and file stores.
/// - `label`: Existing scenario label.
pub(crate) fn temp_dir(namespace: &str, label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "{namespace}-{}-{}-{}",
        label,
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}
