//! Core transport timeout values and duration helpers.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Timeout values used by core transports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeoutConfig {
    /// TCP or upstream connect timeout in milliseconds.
    pub connect_ms: u64,
    /// Proxy handshake timeout in milliseconds.
    pub handshake_ms: u64,
    /// Idle timeout in milliseconds for adapters that enforce one.
    pub idle_ms: u64,
}

impl Default for TimeoutConfig {
    /// Returns conservative core transport timeouts.
    fn default() -> Self {
        Self {
            connect_ms: crate::PROXY_DEFAULT_CONNECT_TIMEOUT_MS,
            handshake_ms: crate::PROXY_DEFAULT_HANDSHAKE_TIMEOUT_MS,
            idle_ms: crate::PROXY_DEFAULT_IDLE_TIMEOUT_MS,
        }
    }
}

impl TimeoutConfig {
    /// Returns the connect timeout as a duration.
    ///
    /// # Returns
    ///
    /// TCP connect timeout.
    pub fn connect_duration(&self) -> Duration {
        Duration::from_millis(self.connect_ms)
    }

    /// Returns the handshake timeout as a duration.
    ///
    /// # Returns
    ///
    /// Proxy handshake timeout.
    pub fn handshake_duration(&self) -> Duration {
        Duration::from_millis(self.handshake_ms)
    }
}
