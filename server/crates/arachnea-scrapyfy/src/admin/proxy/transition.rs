//! Shared proxy transition and rollback, independent of application facades.

use crate::admin::RuntimeReloadReport;
use anyhow::Result;
use arachnea_proxy::server::{LoopbackProxyRuntime, ProxyApiKey, ProxyListenerSettings};

/// Applies a policy and rebuilds application transports only when the endpoint changes.
///
/// The caller holds its application's reconstruction lock throughout this call.
/// Retired internal listeners are retained until the replacement finishes building.
///
/// # Arguments
/// * `runtime` - Stable application-owned proxy listener runtime.
/// * `settings` - Resolved target policy.
/// * `keys` - Verifiers to install for user access.
/// * `previous_keys` - Verifiers restored if reconstruction fails.
/// * `rebuild` - Concrete application rebuild, called without reacquiring its lock.
///
/// # Errors
/// Returns bind, reconstruction or rollback failures, preserving the previous policy when possible.
pub async fn apply_proxy_runtime<F, Fut>(
    runtime: &LoopbackProxyRuntime,
    settings: ProxyListenerSettings,
    keys: Vec<ProxyApiKey>,
    previous_keys: Vec<ProxyApiKey>,
    rebuild: F,
) -> Result<()>
where
    F: FnOnce() -> Fut + Send,
    Fut: std::future::Future<Output = Result<RuntimeReloadReport>> + Send,
{
    let _listeners = runtime.retain_listeners().await;
    let previous = runtime.settings();
    let address = runtime.local_addr();
    runtime.configure(settings, keys).await?;
    if address != runtime.local_addr() {
        let failure = match rebuild().await {
            Ok(report) if report.applied => None,
            Ok(report) => Some(anyhow::anyhow!(report
                .build_error
                .unwrap_or_else(|| "Proxy transport reconstruction failed.".into()))),
            Err(error) => Some(error),
        };
        if let Some(error) = failure {
            runtime.configure(previous, previous_keys).await?;
            return Err(error);
        }
    }
    Ok(())
}
