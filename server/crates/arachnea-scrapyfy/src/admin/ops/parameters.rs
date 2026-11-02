//! Namespaced, validated, atomic service parameter editing operations.

use super::*;
use crate::scrapyfy::service_parameters::{apply_overrides, resolve_values, validate_definitions};
use crate::scrapyfy::service_parameters_store::{load_parameter_values, replace_parameter_values};

/// Resolves YAML constants and editable defaults after applying saved overrides.
///
/// Unknown saved keys are ignored so removed parameters cannot become implicit definitions.
///
/// # Arguments
/// * `entry` - Catalog entry containing constants and editable parameter definitions.
/// * `overrides` - Optional raw administrator values, before parameter actions.
///
/// # Returns
/// The action-processed configuration values indexed by their declared names.
///
/// # Errors
/// Returns an error for invalid definitions, incompatible overrides or dependency cycles.
pub(super) fn effective_values(
    entry: &ScraperServiceCatalogEntry,
    overrides: Option<&HashMap<String, String>>,
) -> Result<HashMap<String, String>> {
    validate_definitions(&entry.constants, &entry.parameters)?;
    let mut parameters = entry.parameters.clone();
    if let Some(overrides) = overrides {
        apply_overrides(&mut parameters, overrides)?;
    }
    let mut values = entry.constants.clone();
    values.extend(parameters);
    resolve_values(&values)
}

/// Reads editable definitions and saved values without executing source queries.
///
/// Invalid saved values remain readable so administrators can correct or reset them.
///
/// # Arguments
/// * `state` - Shared administration state, catalog and parameter override store.
/// * `context` - Incoming controller context used to authorize the read.
/// * `input` - Namespaced service identity whose parameters are requested.
///
/// # Errors
/// Returns an administration error for unauthorized access, an unknown service,
/// invalid YAML definitions or a failed catalog/store read.
pub(super) async fn op_parameters(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: ServiceParametersRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    let source = resolve_source(&state, Some(&input.service_store_id), &input.service_id).await?;
    validate_definitions(&source.entry.constants, &source.entry.parameters).map_err(admin_err)?;
    let key = state.source_key(&input.service_store_id, &input.service_id);
    let values = load_parameter_values(state.service_parameters.as_ref(), &key)
        .await
        .map_err(admin_err)?;
    let parameters = source
        .entry
        .parameters
        .into_iter()
        .map(|parameter| {
            let value = values
                .get(&parameter.name)
                .cloned()
                .unwrap_or_else(|| parameter.value.clone());
            AdminServiceParameter {
                name: parameter.name,
                label: parameter.label.expect("validated editor label"),
                description: parameter.description,
                has_override: value != parameter.value,
                default_value: parameter.value,
                value,
                editor: parameter.editor.expect("validated editor"),
            }
        })
        .collect();
    Ok(AdminReply::ok(&ServiceParametersResponse { parameters }))
}

/// Validates a complete parameter draft and atomically replaces its saved overrides.
///
/// Values equal to YAML defaults are omitted; an empty override set removes the
/// source’s parameter rows. The active runtime changes only after an explicit reload.
///
/// # Arguments
/// * `state` - Shared administration state, catalog and parameter override store.
/// * `context` - Incoming controller context used for authorization and write checks.
/// * `input` - Namespaced service identity and complete raw parameter draft.
///
/// # Returns
/// A successful reply marking the runtime as requiring a reload.
///
/// # Errors
/// Returns an administration error for unauthorized or disallowed writes, an
/// unknown service, unknown/non-editable names, invalid values, failed query
/// conversion or catalog/store failures. Validation completes before persistence.
pub(super) async fn op_save_parameters(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: SaveServiceParametersRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;
    let source = resolve_source(&state, Some(&input.service_store_id), &input.service_id).await?;
    for name in input.values.keys() {
        if !source
            .entry
            .parameters
            .iter()
            .any(|parameter| parameter.name == *name)
        {
            return Err(AdminError::bad_request(format!(
                "Unknown or non-editable parameter {name}"
            )));
        }
    }
    effective_values(&source.entry, Some(&input.values))
        .map_err(|error| AdminError::bad_request(format!("{error:#}")))?;
    // Validate complete query conversion before touching persistence.
    let mut raw: crate::ScraperQueryCollectionRaw = serde_yaml::from_reader(
        std::fs::File::open(&source.entry.source.path).map_err(|error| admin_err(error.into()))?,
    )
    .map_err(|error| admin_err(error.into()))?;
    apply_overrides(&mut raw.parameters, &input.values)
        .map_err(|error| AdminError::bad_request(format!("{error:#}")))?;
    let _: crate::ScraperQueryCollection = raw
        .try_into()
        .map_err(|error| AdminError::bad_request(format!("{error:#}")))?;
    let values = input
        .values
        .into_iter()
        .filter(|(name, value)| {
            source
                .entry
                .parameters
                .iter()
                .any(|parameter| parameter.name == *name && parameter.value != *value)
        })
        .collect::<HashMap<_, _>>();
    let key = state.source_key(&input.service_store_id, &input.service_id);
    replace_parameter_values(state.service_parameters.as_ref(), &key, values)
        .await
        .map_err(admin_err)?;
    Ok(AdminReply::ok(&ServiceParametersSavedResponse {
        reload_required: true,
    }))
}

/// Restores the current YAML defaults by removing every override for one service.
///
/// Uses the same validation and atomic persistence path as saving an empty draft.
///
/// # Arguments
/// * `state` - Shared administration state, catalog and parameter override store.
/// * `context` - Incoming controller context used for authorization and write checks.
/// * `input` - Namespaced identity of the service to reset.
///
/// # Returns
/// A successful reply requiring an explicit runtime reload to apply the defaults.
///
/// # Errors
/// Returns the authorization, validation, query-conversion or persistence errors
/// of [`op_save_parameters`]; invalid YAML defaults leave existing overrides intact.
pub(super) async fn op_reset_parameters(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: ServiceParametersRequest,
) -> Result<AdminReply, AdminError> {
    op_save_parameters(
        state,
        context,
        SaveServiceParametersRequest {
            service_store_id: input.service_store_id,
            service_id: input.service_id,
            values: HashMap::new(),
        },
    )
    .await
}
