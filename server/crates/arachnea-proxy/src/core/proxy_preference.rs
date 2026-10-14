//! Accepted endpoint history for an exact destination, independent of countries.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::{ApplicationProtocol, Destination, ProxyKey};

/// Maximum number of distinct accepted endpoints retained per destination.
pub const PROXY_PREFERENCE_HISTORY_LIMIT: usize = 3;

/// An endpoint accepted by the destination, not merely by a complementary probe.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyAcceptedEndpoint {
    /// Inventory identity; country is resolved from the current inventory record.
    pub endpoint: ProxyKey,
    /// Completion date of the most recent accepted response.
    #[serde(with = "super::proxy_record::system_time_serde")]
    pub accepted_at: SystemTime,
}

/// One durable entity containing the bounded history of an exact destination.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyDestinationPreference {
    /// Canonical scheme/host/port identity, with no country component.
    pub destination: String,
    /// Accepted endpoints, newest first, without age-based expiration.
    pub endpoints: Vec<ProxyAcceptedEndpoint>,
}

impl ProxyDestinationPreference {
    /// Builds a canonical identity for a destination.
    ///
    /// # Arguments
    /// * `destination` - Exact origin whose accepted history is addressed.
    pub fn destination_key(destination: &Destination) -> String {
        let scheme = match &destination.protocol {
            ApplicationProtocol::Http => "http",
            ApplicationProtocol::Https => "https",
            ApplicationProtocol::Udp => "udp",
            ApplicationProtocol::WebSocket => "ws",
            ApplicationProtocol::Other(value) => value.as_str(),
            ApplicationProtocol::Tcp => "tcp",
        };
        let host = destination.host_for_protocol().to_ascii_lowercase();
        format!(
            "{}:{}|{}:{}|{}",
            scheme.len(),
            scheme,
            host.len(),
            host,
            destination.port
        )
    }

    /// Merges an accepted observation without allowing an older completion to win.
    ///
    /// # Arguments
    /// * `endpoint` - Inventory endpoint actually used by the request.
    /// * `accepted_at` - Date captured when acceptance was observed, before I/O.
    pub fn record_success(&mut self, endpoint: ProxyKey, accepted_at: SystemTime) {
        let accepted_at = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_millis(
                accepted_at
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
            );
        if let Some(existing) = self
            .endpoints
            .iter_mut()
            .find(|item| item.endpoint == endpoint)
        {
            existing.accepted_at = existing.accepted_at.max(accepted_at);
        } else {
            self.endpoints.push(ProxyAcceptedEndpoint {
                endpoint,
                accepted_at,
            });
        }
        self.normalize();
    }

    /// Deduplicates and bounds decoded or merged history with stable tie-breaking.
    pub fn normalize(&mut self) {
        self.endpoints.sort_by(|a, b| {
            b.accepted_at
                .cmp(&a.accepted_at)
                .then_with(|| a.endpoint.host.cmp(&b.endpoint.host))
                .then_with(|| a.endpoint.port.cmp(&b.endpoint.port))
        });
        let mut unique = Vec::new();
        self.endpoints.retain(|item| {
            if unique.contains(&item.endpoint) {
                false
            } else {
                unique.push(item.endpoint.clone());
                true
            }
        });
        self.endpoints.truncate(PROXY_PREFERENCE_HISTORY_LIMIT);
    }
}
