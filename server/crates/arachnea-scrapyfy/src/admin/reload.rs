//! Generic service-group reload validation shared by administration runtimes.

use serde::Serialize;
use std::sync::Arc;

use arachnea_core::persistence::TypedEntityStore;

use crate::scrapyfy::{
    load_service_catalog_detailed, PersistenceSourceEnabled, ScraperSourceEnabled,
    ServiceCatalogFailureReason, SourceServiceRecord,
};

use super::{AdminRuntimeAdapter, AdminServerSettingsReport, AdminServiceGroupConfig};

/// Outcome of one application runtime rebuild or generic group validation.
#[derive(Clone, Debug, Default, Serialize)]
pub struct RuntimeReloadReport {
    /// Whether the group accepted its validated state.
    pub applied: bool,
    /// Failure context when validation or runtime reconstruction failed.
    pub build_error: Option<String>,
}

/// A service group whose manifest and activation state were validated.
#[derive(Clone, Debug)]
pub struct ValidatedReloadGroup {
    /// Technical service-store identifier validated against the manifest.
    pub service_store_id: String,
    /// Manifest path that completed validation.
    pub manifest_path: String,
}

/// Outcome of one group inside a coordinated reload.
#[derive(Clone, Debug, Serialize)]
pub struct ReloadGroupReport {
    /// Technical identifier of the group that was processed.
    pub service_store_id: String,
    /// Whether the group accepted its validated state.
    pub applied: bool,
    /// Failure context when validation or runtime reconstruction failed.
    pub build_error: Option<String>,
}

/// Aggregate outcome of every group processed by one reload request.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ReloadSummary {
    /// Ordered reports for every declared administrable group.
    pub groups: Vec<ReloadGroupReport>,
}

impl ReloadSummary {
    /// Returns the first group outcome used by the legacy flat administration response.
    pub fn primary(&self) -> Option<RuntimeReloadReport> {
        self.groups.first().map(|group| RuntimeReloadReport {
            applied: group.applied,
            build_error: group.build_error.clone(),
        })
    }

    /// Returns a compact, log-friendly rendering of every group outcome.
    pub fn log_summary(&self) -> String {
        if self.groups.is_empty() {
            return "no administrable service groups".to_string();
        }
        self.groups
            .iter()
            .map(|group| {
                let mut summary = format!("{}: applied={}", group.service_store_id, group.applied);
                if let Some(build_error) = &group.build_error {
                    summary.push_str(&format!("; build_error={build_error}"));
                }
                summary
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// Combined result of a tray reload and optional REST settings application.
#[derive(Clone, Debug)]
pub struct TrayReloadSummary {
    /// Results of the coordinated service-group reload.
    pub reload: ReloadSummary,
    /// REST settings application result when a running REST server is available.
    pub server_settings: Option<AdminServerSettingsReport>,
}

impl TrayReloadSummary {
    /// Returns a compact log rendering of reload and server settings outcomes.
    pub fn log_summary(&self) -> String {
        let mut summary = self.reload.log_summary();
        if let Some(settings) = &self.server_settings {
            summary.push_str(&format!(
                " | server_settings: applied={}; port={}; network={}; root={:?}",
                settings.applied,
                settings.server_port,
                settings.network_mode,
                settings.entrypoint_root
            ));
            if let Some(apply_error) = &settings.apply_error {
                summary.push_str(&format!("; apply_error={apply_error}"));
            }
        }
        summary
    }
}

/// Coordinates generic validation and application-specific runtime rebuilds.
pub struct ReloadCoordinator {
    groups: Vec<AdminServiceGroupConfig>,
    source_enabled: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
    adapter: Arc<dyn AdminRuntimeAdapter>,
}

impl ReloadCoordinator {
    /// Creates a reload coordinator for declared groups and one application runtime adapter.
    pub fn new(
        groups: Vec<AdminServiceGroupConfig>,
        source_enabled: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
        adapter: Arc<dyn AdminRuntimeAdapter>,
    ) -> Self {
        Self {
            groups,
            source_enabled,
            adapter,
        }
    }

    /// Validates and reloads every declared group in display order.
    pub async fn reload_all(&self) -> ReloadSummary {
        let policy = PersistenceSourceEnabled::with_typed_store(Arc::clone(&self.source_enabled));
        let mut groups = Vec::with_capacity(self.groups.len());
        for group in &self.groups {
            let runtime = match prepare_group_reload(group, &policy).await {
                Err(report) => report,
                Ok(validated_group) => {
                    match self.adapter.rebuild_validated_group(&validated_group).await {
                        Ok(Some(report)) => report,
                        Ok(None) => RuntimeReloadReport {
                            applied: true,
                            build_error: None,
                        },
                        Err(error) => RuntimeReloadReport {
                            applied: false,
                            build_error: Some(format!("{error:#}")),
                        },
                    }
                }
            };
            groups.push(ReloadGroupReport {
                service_store_id: group.service_store_id.clone(),
                applied: runtime.applied,
                build_error: runtime.build_error,
            });
        }
        ReloadSummary { groups }
    }

    /// Reloads groups and then applies pending REST settings when available.
    ///
    /// The two steps are intentionally independent: a group failure does not
    /// prevent a server settings application, and a bind failure does not undo
    /// a successfully published group runtime.
    ///
    /// # Errors
    ///
    /// Returns an error when the worker runtime cannot be created or panics.
    pub fn reload_all_and_apply_server_settings_blocking(
        self: &Arc<Self>,
    ) -> anyhow::Result<TrayReloadSummary> {
        let coordinator = Arc::clone(self);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().map_err(anyhow::Error::from)?;
            let reload = runtime.block_on(coordinator.reload_all());
            let server_settings = coordinator
                .adapter
                .target_server_settings()
                .map(|_| coordinator.adapter.apply_server_settings());
            Ok(TrayReloadSummary {
                reload,
                server_settings,
            })
        })
        .join()
        .map_err(|_| anyhow::anyhow!("tray reload coordinator thread panicked"))?
    }
}

/// Validates one administrable group and synchronizes its activation defaults.
///
/// Runtimes with an application-specific rebuild receive the returned context
/// only after this validation succeeds. Groups without an application-specific
/// runtime use the successful context as their accepted reload outcome.
///
/// # Arguments
///
/// * `group` - Group manifest and persistence namespace to validate.
/// * `policy` - Persistent source activation policy shared by all groups.
///
/// # Returns
///
/// A validated group, or a reload report containing the validation failure.
pub async fn prepare_group_reload(
    group: &AdminServiceGroupConfig,
    policy: &PersistenceSourceEnabled,
) -> Result<ValidatedReloadGroup, RuntimeReloadReport> {
    let catalog = match load_service_catalog_detailed(&group.manifest_path, &group.service_store_id)
    {
        Ok(catalog) => catalog,
        Err(error) => {
            return Err(RuntimeReloadReport {
                applied: false,
                build_error: Some(format!("{error:#}")),
            });
        }
    };

    let defaults = catalog
        .entries
        .iter()
        .map(|entry| entry.source.clone())
        .collect::<Vec<_>>();
    let mut failures = Vec::new();

    if let Err(error) = policy.register_defaults(&defaults).await {
        failures.push(format!(
            "Failed to synchronize service activation defaults: {error:#}"
        ));
    }
    for entry in &catalog.entries {
        if let Err(error) = policy.is_enabled(&entry.source).await {
            failures.push(format!(
                "{}: failed to read activation state: {error:#}",
                entry.source.path.display()
            ));
        }
    }
    for failure in &catalog.failures {
        if matches!(
            failure.reason,
            ServiceCatalogFailureReason::Invalid | ServiceCatalogFailureReason::Duplicate
        ) {
            failures.push(format!("{}: {}", failure.path.display(), failure.message));
        }
    }

    if !failures.is_empty() {
        return Err(RuntimeReloadReport {
            applied: false,
            build_error: Some(failures.join("; ")),
        });
    }

    Ok(ValidatedReloadGroup {
        service_store_id: group.service_store_id.clone(),
        manifest_path: group.manifest_path.clone(),
    })
}
