use serde::{Deserialize, Serialize};

use crate::core::{Destination, ProxyError, Result};

/// Security controls applied before opening outbound connections.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityPolicy {
    /// Forbid literal private, loopback and link-local target IPs.
    pub forbid_private_ip_targets: bool,
    /// Forbid loopback targets when a non-local client is involved.
    pub forbid_loopback_targets_from_remote_clients: bool,
    /// Require TLS verification for TLS-based upstream proxies.
    pub verify_upstream_tls: bool,
    /// Allow upstreams that explicitly disable TLS verification.
    pub allow_insecure_upstreams: bool,
    /// Optional allowlist of target ports.
    pub allow_ports: Vec<u16>,
    /// Optional denylist of target ports.
    pub deny_ports: Vec<u16>,
}

impl Default for SecurityPolicy {
    /// Returns baseline security controls for local proxy use.
    fn default() -> Self {
        Self {
            forbid_private_ip_targets: false,
            forbid_loopback_targets_from_remote_clients: true,
            verify_upstream_tls: true,
            allow_insecure_upstreams: false,
            allow_ports: Vec::new(),
            deny_ports: Vec::new(),
        }
    }
}

impl SecurityPolicy {
    /// Validates a destination before route selection opens sockets.
    ///
    /// # Parameters
    ///
    /// - `destination`: Destination requested by a caller or inbound protocol.
    ///
    /// # Errors
    ///
    /// Returns an access-denied error when the destination violates private IP
    /// or port policy.
    pub fn validate_destination(&self, destination: &Destination) -> Result<()> {
        if self.forbid_private_ip_targets && destination.is_private_or_loopback_ip() {
            return Err(ProxyError::AccessDenied(
                "literal private, loopback or link-local destination refused".to_string(),
            ));
        }

        if !self.allow_ports.is_empty() && !self.allow_ports.contains(&destination.port) {
            return Err(ProxyError::AccessDenied(format!(
                "target port {} is not allowed",
                destination.port
            )));
        }

        if self.deny_ports.contains(&destination.port) {
            return Err(ProxyError::AccessDenied(format!(
                "target port {} is denied",
                destination.port
            )));
        }

        Ok(())
    }
}
