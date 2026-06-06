use std::sync::atomic::{AtomicU64, Ordering};

/// Thread-safe proxy counters.
#[derive(Debug, Default)]
pub struct ProxyStats {
    connections_opened: AtomicU64,
    connections_failed: AtomicU64,
}

impl ProxyStats {
    /// Records a successfully opened connection.
    pub fn record_opened(&self) {
        self.connections_opened.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a failed connection attempt.
    pub fn record_failed(&self) {
        self.connections_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns a snapshot of current counters.
    pub fn snapshot(&self) -> ProxyStatsSnapshot {
        ProxyStatsSnapshot {
            connections_opened: self.connections_opened.load(Ordering::Relaxed),
            connections_failed: self.connections_failed.load(Ordering::Relaxed),
        }
    }
}

/// Immutable proxy statistics snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProxyStatsSnapshot {
    /// Number of connections successfully opened.
    pub connections_opened: u64,
    /// Number of failed connection attempts.
    pub connections_failed: u64,
}
