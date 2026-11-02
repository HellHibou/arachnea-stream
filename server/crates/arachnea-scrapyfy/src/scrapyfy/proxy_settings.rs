//! Shared proxy configuration parsing and command-line precedence.

use crate::{PROXY_ENABLED_KEY, PROXY_KEYS_KEY, PROXY_NETWORK_KEY, PROXY_PORT_KEY};
use anyhow::{bail, Context, Result};
use arachnea_core::controler::options::{CoreApplicationOptions, ServerNetworkMode};
use arachnea_proxy::server::{ProxyApiKey, ProxyListenerSettings};
use std::collections::BTreeMap;

/// Resolves shared persisted settings and explicit command-line overrides.
///
/// # Arguments
/// * `config` - Persisted option map.
/// * `cli` - Only values explicitly passed on the command line.
///
/// # Errors
/// Rejects malformed settings and invalid security verifiers.
pub fn configuration(
    config: &CoreApplicationOptions,
    cli: &BTreeMap<String, String>,
) -> Result<(ProxyListenerSettings, Vec<ProxyApiKey>)> {
    let value = |name| {
        cli.get(name)
            .map(String::as_str)
            .or_else(|| config.additional_option(name))
    };
    let enabled = value(PROXY_ENABLED_KEY)
        .unwrap_or("false")
        .parse()
        .context("Invalid proxy-enabled setting")?;
    let port = value(PROXY_PORT_KEY)
        .map(str::parse)
        .transpose()
        .context("Invalid proxy-server-port setting")?
        .unwrap_or(0);
    let network = value(PROXY_NETWORK_KEY)
        .unwrap_or("local")
        .parse::<ServerNetworkMode>()
        .map_err(anyhow::Error::msg)
        .context("Invalid proxy-network setting")?;
    let settings = ProxyListenerSettings {
        enabled,
        port,
        network,
    };
    settings.validate()?;
    let keys: Vec<ProxyApiKey> =
        serde_json::from_str(config.additional_option(PROXY_KEYS_KEY).unwrap_or("[]"))
            .map_err(|_| anyhow::anyhow!("Invalid proxy-api-keys configuration."))?;
    if keys.len() > crate::PROXY_MAX_USER_KEYS {
        bail!("Proxy API key limit exceeded.");
    }
    let mut ids = std::collections::HashSet::new();
    let mut digests = std::collections::HashSet::new();
    for key in &keys {
        if key.id.is_empty()
            || key.label.trim().is_empty()
            || key.label.len() > crate::PROXY_KEY_LABEL_MAX_BYTES
            || key.digest.len() != 64
            || !key
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || !ids.insert(&key.id)
            || !digests.insert(&key.digest)
        {
            bail!("Invalid or duplicate proxy API key metadata.");
        }
    }
    Ok((settings, keys))
}
