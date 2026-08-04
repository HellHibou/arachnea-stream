//! Cooperative shutdown signal shared between the HTTP server and the tray.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;

/// A cooperative shutdown flag shared across the server and the tray.
///
/// The HTTP server waits on it to begin its graceful shutdown, while the main
/// thread loop polls it to unblock once shutdown has been requested.
#[derive(Clone)]
pub struct ShutdownSignal {
    /// Whether shutdown has been requested.
    requested: Arc<AtomicBool>,
    /// Notifier waking up `wait` futures.
    notify: Arc<Notify>,
}

impl ShutdownSignal {
    /// Creates a new, non-signalled shutdown signal.
    ///
    /// # Returns
    /// A new [`ShutdownSignal`].
    pub fn new() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Requests shutdown, unblocking any [`Self::wait`] waiter.
    pub fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    /// Returns whether shutdown has been requested.
    ///
    /// # Returns
    /// `true` when shutdown was requested, `false` otherwise.
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    /// Resolves once shutdown has been requested.
    ///
    /// Used by `warp::serve(...).bind_with_graceful_shutdown` to start the
    /// graceful shutdown of the HTTP server.
    pub async fn wait(&self) {
        if self.is_requested() {
            return;
        }
        let notified = self.notify.notified();
        if self.is_requested() {
            return;
        }
        notified.await;
    }
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}
