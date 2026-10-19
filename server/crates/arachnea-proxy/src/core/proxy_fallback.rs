//! Source-scoped diagnostics for successful direct fallbacks.

use std::{
    future::Future,
    sync::{Arc, Mutex},
};

tokio::task_local! {
    static DIAGNOSTICS: ProxyFallbackDiagnostics;
}

/// Collects non-fatal proxy failures during one logical source execution.
/// Clones share the collector; concurrent source scopes remain independent.
#[derive(Clone, Debug, Default)]
pub struct ProxyFallbackDiagnostics(Arc<Mutex<Vec<String>>>);

impl ProxyFallbackDiagnostics {
    /// Collects diagnostics emitted while polling a source's execution future.
    ///
    /// # Arguments
    /// - `future`: Source execution, including nested HTTP calls.
    pub async fn scope<F: Future>(&self, future: F) -> F::Output {
        DIAGNOSTICS.scope(self.clone(), future).await
    }

    /// Returns the collected, deduplicated fallback messages.
    pub fn messages(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    pub(crate) fn record(message: String) {
        let _ = DIAGNOSTICS.try_with(|diagnostics| {
            let mut messages = diagnostics.0.lock().unwrap();
            if !messages.contains(&message) {
                messages.push(message);
            }
        });
    }
}
