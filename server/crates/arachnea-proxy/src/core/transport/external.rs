use crate::core::{ProxyError, Result};

/// Placeholder for externally managed tunnel orchestration.
///
/// # Errors
///
/// Always returns an unsupported-feature error until external tunnel
/// orchestration is implemented.
pub async fn connect_external_tunnel() -> Result<()> {
    Err(ProxyError::Unsupported("external tunnel orchestration"))
}
