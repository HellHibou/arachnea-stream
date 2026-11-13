//! Shared runtime for snapshot-backed translation bridges and account budgets.
//!
//! The adapter borrows the already loaded Scrapyfy aggregator, so credentials
//! and editable parameters stay resolved by the service configuration that
//! built the effective translation catalog.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::{
    bridge::ScrapyfyTranslationBridge, BudgetLimit, InMemoryTranslationBudget,
    TranslationBudgetPort, TranslationError,
};
use anyhow::Result;
use arachnea_scrapyfy::ScraperAgregator;

use crate::aggregator_port::AggregatorTranslationPort;
use crate::catalog::{TranslationCatalog, TranslationCatalogEntry};

/// Process-local account budgets, keyed by stable service identifier.
///
/// A budget is created from the catalog on first use and then retained so
/// concurrent calls share the same quota window and concurrency slots.
static TRANSLATION_BUDGETS: OnceLock<Mutex<HashMap<String, Arc<InMemoryTranslationBudget>>>> =
    OnceLock::new();

/// Loads the generic translation bridge for one published service.
///
/// # Arguments
///
/// * `aggregator` - Aggregator containing the translation service group.
/// * `catalog` - Effective catalog providing the configuration revision.
/// * `group_name` - Service store containing the translation providers.
/// * `service_id` - Published service identifier.
///
/// # Errors
///
/// Returns [`TranslationError::MetadataUnavailable`] when the service is not
/// published, or another bridge error when snapshot construction fails.
pub async fn load_translation_bridge<'a>(
    aggregator: &'a ScraperAgregator,
    catalog: &TranslationCatalog,
    group_name: &'a str,
    service_id: &str,
) -> Result<ScrapyfyTranslationBridge<AggregatorTranslationPort<'a>>, TranslationError> {
    let entry =
        catalog
            .service(service_id)
            .ok_or_else(|| TranslationError::MetadataUnavailable {
                detail: format!("translation service `{service_id}` is not published"),
            })?;
    let port = AggregatorTranslationPort::new(aggregator, group_name, service_id);
    Ok(ScrapyfyTranslationBridge::from_metadata(
        port,
        service_id,
        entry.configuration_revision.clone(),
        entry.metadata.clone(),
    )
    .with_budget(shared_translation_budget(entry)))
}

/// Returns the shared process-local budget for one catalog entry.
///
/// # Arguments
///
/// * `entry` - Published catalog entry carrying account limits.
pub fn shared_translation_budget(
    entry: &TranslationCatalogEntry,
) -> Arc<dyn TranslationBudgetPort> {
    let budgets = TRANSLATION_BUDGETS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut budgets = budgets
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let budget = budgets
        .entry(entry.service_id.clone())
        .or_insert_with(|| {
            let limit = BudgetLimit::new(
                entry.max_concurrent_requests,
                entry.max_units_per_window,
                entry.budget_window_seconds,
            )
            .unwrap_or_default();
            Arc::new(InMemoryTranslationBudget::new(limit))
        })
        .clone();
    budget.update_limit(
        BudgetLimit::new(
            entry.max_concurrent_requests,
            entry.max_units_per_window,
            entry.budget_window_seconds,
        )
        .unwrap_or_default(),
    );
    budget
}
