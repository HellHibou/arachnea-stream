//! Operational profile defaults.

use serde::{Deserialize, Serialize};

/// Operational profile used to derive inspectable defaults.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyProfile {
    /// Direct networking with basic safety defaults.
    #[serde(rename = "system_relay", alias = "direct")]
    Direct,
    /// All traffic is expected to use a single configured upstream.
    #[serde(rename = "single_forwarder", alias = "single_proxy")]
    SingleProxy,
    /// All traffic is expected to use one fixed multi-hop chain.
    MultiForwarder,
    /// Privacy-oriented defaults such as minimized logs and no direct fallback.
    Privacy,
    /// Fallback-friendly profile for censorship-resistance experiments.
    CensorshipResistance,
    /// Explicit configuration without strong implicit behavior.
    Advanced,
}

impl Default for ProxyProfile {
    /// Returns the direct profile as the default.
    fn default() -> Self {
        Self::Direct
    }
}
