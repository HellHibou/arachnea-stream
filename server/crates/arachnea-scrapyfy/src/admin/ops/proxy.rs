//! Authenticated proxy key management operations.
use super::*;
use crate::admin::proxy::{CreateProxyKeyRequest, DeleteProxyKeyRequest};

/// Creates a key after the ordinary admin authentication and write checks.
///
/// # Arguments
/// * `state` - Shared admin state and mutation lock.
/// * `context` - Transport context for access checks.
/// * `input` - Display label, never a caller-supplied secret.
pub(super) async fn op_create_proxy_key(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: CreateProxyKeyRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;
    let _write = state.settings_write.lock().await;
    let created = state
        .adapter()
        .create_proxy_key(&input.label)
        .await
        .map_err(admin_err)?;
    Ok(AdminReply::ok(&created))
}

/// Revokes a key under the shared configuration mutation lock.
///
/// # Arguments
/// * `state` - Shared admin state and mutation lock.
/// * `context` - Transport context for access checks.
/// * `input` - Public identifier of the key to revoke.
pub(super) async fn op_delete_proxy_key(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: DeleteProxyKeyRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;
    let _write = state.settings_write.lock().await;
    state
        .adapter()
        .delete_proxy_key(&input.id)
        .await
        .map_err(admin_err)?;
    Ok(AdminReply::ok(&AdminEmptyRequest {}))
}
