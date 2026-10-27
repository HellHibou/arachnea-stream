//! Optional request-local search progress counters and logs.

use super::ProxySearchContext;

impl ProxySearchContext {
    /// Counts one completed technical operation without reserving new work.
    ///
    /// # Arguments
    /// - `validation`: Whether the operation was a capability validation.
    /// - `usable`: Whether the operation returned usable evidence.
    pub(crate) fn completed_work(&self, validation: bool, usable: bool) {
        let mut work = self.work.lock().unwrap();
        if validation {
            work.validations_completed += 1;
            work.validations_eligible += usize::from(usable);
        } else {
            work.prechecks_completed += 1;
            work.prechecks_reachable += usize::from(usable);
        }
    }
    /// Logs optional bounded DEBUG counters without URLs, cookies or bodies.
    /// Available only with `performance-diagnostics`; a disabled DEBUG filter
    /// avoids locking and counting reserved work.
    /// Reservations include queued and completed work; cancelled reservations are released.
    ///
    /// # Arguments
    /// - `stage`: Static lifecycle description supplied by the orchestration layer.
    pub fn log_progress(&self, stage: &'static str) {
        if !tracing::enabled!(target: "arachnea_proxy::core::proxy_search", tracing::Level::DEBUG) {
            return;
        }
        let work = self.work.lock().unwrap();
        let prechecks_reserved = work
            .technical
            .iter()
            .filter(|(validation, _)| !validation)
            .count();
        let validations_reserved = work.technical.len() - prechecks_reserved;
        tracing::debug!(target: "arachnea_proxy::core::proxy_search",
            search_id = self.id(),
            stage,
            elapsed_ms = self.started.elapsed().as_millis(),
            remaining_ms = self.remaining().as_millis(),
            prechecks_reserved,
            prechecks_completed = work.prechecks_completed,
            prechecks_reachable = work.prechecks_reachable,
            validations_reserved,
            validations_completed = work.validations_completed,
            validations_eligible = work.validations_eligible,
            business_endpoints = work.endpoints.len(),
            origin_active = work.active,
            reused_origin = work.reused_origin,
            accepted = work.accepted,
            "localized proxy search progress"
        );
    }
}
