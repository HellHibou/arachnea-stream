//! Hostname resolution mode.

use serde::{Deserialize, Serialize};

/// How the proxy treats hostnames before a transport requires an IP address.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameResolutionMode {
    /// Preserve hostnames and pass them to capable upstreams.
    PreserveHostname,
    /// Resolve locally as soon as a socket endpoint is needed.
    ResolveWhenRequired,
}

impl Default for NameResolutionMode {
    /// Returns hostname-preserving resolution mode.
    fn default() -> Self {
        Self::PreserveHostname
    }
}
