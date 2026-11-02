//! API-key authentication and network policy for the application proxy.

use arachnea_core::controler::options::ServerNetworkMode;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    net::IpAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
};

/// Persisted user-key metadata; the usable secret is never persisted.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyApiKey {
    /// Public identifier used for revocation.
    pub id: String,
    /// Administrator-supplied display label.
    pub label: String,
    /// Creation time as Unix seconds.
    pub created_at: u64,
    /// SHA-256 digest of the encoded secret.
    pub digest: String,
}

/// Resolved listener policy, supplied by the owning application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyListenerSettings {
    /// Enables user API keys; internal authentication remains available.
    pub enabled: bool,
    /// Requested port; zero is allowed only for internal-only operation.
    pub port: u16,
    /// Network policy; internal-only operation forces loopback.
    pub network: ServerNetworkMode,
}

impl Default for ProxyListenerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 0,
            network: ServerNetworkMode::Local,
        }
    }
}

impl ProxyListenerSettings {
    /// Validates the resolved policy.
    ///
    /// # Errors
    /// Rejects enabled user access without a fixed nonzero port.
    pub fn validate(&self) -> std::io::Result<()> {
        if self.enabled && self.port == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Enabled proxy requires a port between 1 and 65535.",
            ));
        }
        Ok(())
    }

    /// Returns the effective policy, forcing disabled access to local and automatic.
    pub fn effective(&self) -> Self {
        if self.enabled {
            self.clone()
        } else {
            Self::default()
        }
    }
}

/// Generates a 64-byte random secret encoded as unpadded Base64url.
pub fn generate_api_key() -> String {
    let mut bytes = [0; crate::PROXY_API_KEY_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Computes the persisted cryptographic verifier of an API key.
///
/// # Arguments
/// * `secret` - Encoded key received through proxy authentication.
pub fn api_key_digest(secret: &str) -> String {
    format!("{:x}", Sha256::digest(secret.as_bytes()))
}

/// Shared authentication state, intentionally without a Debug implementation.
pub(super) struct ProxyAccess {
    /// Terminal shutdown signal shared by every listener generation.
    pub(super) stopped: AtomicBool,
    /// Process-only secret, never serialized or formatted.
    pub(super) internal_key: String,
    /// Policy notifications used to bound revoked tunnel lifetimes.
    pub(super) changes: tokio::sync::watch::Sender<u64>,
    /// Current resolved settings and user-key verifiers.
    pub(super) policy: RwLock<(ProxyListenerSettings, Vec<ProxyApiKey>)>,
}

/// Admission state of a listener generation, including retired listeners.
pub(crate) struct ListenerAccess {
    /// Runtime-wide authentication and network policy.
    pub(super) shared: Arc<ProxyAccess>,
    /// False after retirement, immediately rejecting new user admissions.
    pub(super) external: AtomicBool,
}

impl ListenerAccess {
    /// Applies the current network policy before authentication.
    ///
    /// # Arguments
    /// * `peer` - Actual TCP peer address, never a forwarded header.
    pub(crate) fn allows(&self, peer: IpAddr) -> bool {
        if self.shared.stopped.load(Ordering::Acquire) {
            return false;
        }
        if peer.is_loopback() {
            return true;
        }
        if !self.external.load(Ordering::Acquire) {
            return false;
        }
        let policy = self
            .shared
            .policy
            .read()
            .expect("proxy policy lock poisoned");
        if !policy.0.enabled {
            return false;
        }
        match policy.0.network {
            ServerNetworkMode::Local => false,
            ServerNetworkMode::Private => arachnea_core::controler::rest::local_networks()
                .iter()
                .any(|net| net.contains(&peer)),
            ServerNetworkMode::Public => true,
        }
    }

    /// Waits until a user session is revoked, then permits bounded tunnel drainage.
    ///
    /// # Arguments
    /// * `secret` - Authenticated user token, never logged.
    /// * `peer` - Actual TCP peer address.
    pub(crate) async fn revoked(&self, secret: &str, peer: IpAddr) {
        let mut changes = self.shared.changes.subscribe();
        loop {
            if self.authenticate(secret, peer) != Some(false) {
                break;
            }
            if changes.changed().await.is_err() {
                break;
            }
        }
        tokio::time::sleep(crate::PROXY_LISTENER_DRAIN).await;
    }

    /// Verifies a secret and returns whether it is the internal key.
    ///
    /// # Arguments
    /// * `secret` - Password supplied by the client.
    /// * `peer` - Actual TCP peer address.
    pub(crate) fn authenticate(&self, secret: &str, peer: IpAddr) -> Option<bool> {
        if secret.len() != crate::PROXY_API_KEY_ENCODED_LENGTH || !self.allows(peer) {
            return None;
        }
        if equal(secret, &self.shared.internal_key) {
            return peer.is_loopback().then_some(true);
        }
        let policy = self
            .shared
            .policy
            .read()
            .expect("proxy policy lock poisoned");
        if !self.external.load(Ordering::Acquire) || !policy.0.enabled {
            return None;
        }
        let digest = api_key_digest(secret);
        policy
            .1
            .iter()
            .any(|key| equal(&digest, &key.digest))
            .then_some(false)
    }
}

fn equal(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}
