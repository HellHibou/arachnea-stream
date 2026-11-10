//! Stream-backed adapter for the generic translation bridge.
//!
//! The adapter borrows the already loaded Scrapyfy aggregator, so credentials
//! and editable parameters stay resolved by the same service configuration that
//! built the effective translation catalog.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use arachnea_scrapyfy::ScraperAgregator;
use arachnea_translation::{
    bridge::{ScrapyfyQueryPort, ScrapyfyTranslationBridge},
    BudgetLimit, InMemoryTranslationBudget, TranslationBudgetPort, TranslationError,
};
use async_trait::async_trait;

use crate::translation_catalog::{TranslationCatalog, TranslationCatalogEntry};

/// Process-local account budgets, keyed by stable service identifier.
///
/// A budget is created from the catalog on first use and then retained so
/// concurrent calls share the same quota window and concurrency slots.
static TRANSLATION_BUDGETS: OnceLock<Mutex<HashMap<String, Arc<InMemoryTranslationBudget>>>> =
    OnceLock::new();

/// Translation query port backed by one loaded Stream service group.
pub struct StreamTranslationPort<'a> {
    aggregator: &'a ScraperAgregator,
    group_name: &'a str,
    service_id: String,
}

impl<'a> StreamTranslationPort<'a> {
    /// Creates a port for one loaded translation service.
    ///
    /// # Arguments
    ///
    /// * `aggregator` - Aggregator containing the translation service group.
    /// * `group_name` - Service store containing the translation providers.
    /// * `service_id` - Stable service identifier used in diagnostics.
    pub fn new(
        aggregator: &'a ScraperAgregator,
        group_name: &'a str,
        service_id: impl Into<String>,
    ) -> Self {
        Self {
            aggregator,
            group_name,
            service_id: service_id.into(),
        }
    }
}

#[async_trait]
impl ScrapyfyQueryPort for StreamTranslationPort<'_> {
    fn measure_translation_query(
        &self,
        query: &str,
        params: &BTreeMap<String, String>,
    ) -> Result<u64> {
        self.aggregator.measure_query_body_on_source(
            self.group_name,
            &self.service_id,
            query,
            &params.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        )
    }

    async fn execute_translation_query(
        &self,
        query: &str,
        params: &BTreeMap<String, String>,
        media_types: &[String],
    ) -> Result<Vec<HashMap<String, arachnea_scrapyfy::ScraperDataNode>>> {
        let params = params
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<HashMap<_, _>>();
        let media_types = media_types.to_vec();
        self.aggregator
            .execute_query_on_source(
                self.group_name,
                &self.service_id,
                query,
                &params,
                Some(&media_types),
            )
            .await
    }
}

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
/// published, or another bridge error when the metadata query or validation
/// fails.
pub async fn load_translation_bridge<'a>(
    aggregator: &'a ScraperAgregator,
    catalog: &TranslationCatalog,
    group_name: &'a str,
    service_id: &str,
) -> Result<ScrapyfyTranslationBridge<StreamTranslationPort<'a>>, TranslationError> {
    let entry =
        catalog
            .service(service_id)
            .ok_or_else(|| TranslationError::MetadataUnavailable {
                detail: format!("translation service `{service_id}` is not published"),
            })?;
    let port = StreamTranslationPort::new(aggregator, group_name, service_id);
    ScrapyfyTranslationBridge::load(port, service_id, entry.configuration_revision.clone())
        .await
        .map(|bridge| bridge.with_budget(shared_translation_budget(entry)))
}

/// Returns the shared process-local budget for one catalog entry.
///
/// # Arguments
///
/// * `entry` - Published catalog entry carrying account limits.
fn shared_translation_budget(entry: &TranslationCatalogEntry) -> Arc<dyn TranslationBudgetPort> {
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

/// Selects a text service with actual request planning and shared account state.
///
/// Selection performs only metadata queries and offline body construction.
/// The returned bridge keeps the selected provider for the entire execution;
/// callers must propagate an execution failure without selecting a fallback.
///
/// # Arguments
///
/// * `aggregator` - Loaded services and resolved credentials.
/// * `catalog` - Effective translation catalog.
/// * `group_name` - Translation service group.
/// * `policy` - Automatic or constrained provider policy.
/// * `requirement` - Text direction, blocks and local limits.
///
/// # Errors
///
/// Returns an invalid route or no eligible provider before any translation.
pub async fn select_text_translation_bridge<'a>(
    aggregator: &'a ScraperAgregator,
    catalog: &TranslationCatalog,
    group_name: &'a str,
    policy: &arachnea_translation::SelectionPolicy,
    requirement: &arachnea_translation::SelectionRequirement,
) -> Result<
    (
        ScrapyfyTranslationBridge<StreamTranslationPort<'a>>,
        arachnea_translation::SelectionDecision,
    ),
    TranslationError,
> {
    use arachnea_translation::selection::SelectionRoute;
    use arachnea_translation::{
        SelectionPolicy, ServiceCandidate, TextTranslationRequest, TranslationOptions,
        TranslationService,
    };
    if requirement.route != SelectionRoute::Text {
        return Err(TranslationError::InvalidRequest {
            detail: "text service selection requires the text route".into(),
        });
    }
    let request = TextTranslationRequest {
        source_language: requirement.source_language.clone(),
        target_language: requirement.target_language.clone(),
        blocks: requirement.blocks.clone(),
        options: TranslationOptions::default(),
    };
    let policy = match policy {
        SelectionPolicy::Auto {
            preferred_service: None,
        } => SelectionPolicy::Auto {
            preferred_service: catalog.preferred_service_id().map(str::to_string),
        },
        policy => policy.clone(),
    };
    let mut candidates = Vec::new();
    let mut bridges = HashMap::new();
    let mut plans = HashMap::new();
    for entry in catalog.services() {
        if let SelectionPolicy::Constrained { service_id } = &policy {
            if service_id != &entry.service_id {
                continue;
            }
        }
        let Ok(bridge) =
            load_translation_bridge(aggregator, catalog, group_name, &entry.service_id).await
        else {
            continue;
        };
        let plan = bridge.plan_texts(&request, &requirement.local_limits);
        let units = plan
            .as_ref()
            .ok()
            .and_then(|plan| bridge.planned_budget_units(&request, plan).ok())
            .unwrap_or(u64::MAX);
        candidates.push(ServiceCandidate {
            service_id: entry.service_id.clone(),
            metadata: bridge.metadata().clone(),
            metadata_fresh: true,
            budget_available: shared_translation_budget(entry).available(units),
            administrator_order: entry.administrator_order.min(u32::MAX as u64) as u32,
        });
        plans.insert(entry.service_id.clone(), plan);
        bridges.insert(
            entry.service_id.clone(),
            bridge.with_local_limits(requirement.local_limits),
        );
    }
    let decision = arachnea_translation::selection::select_service_with_planner(
        &candidates,
        &policy,
        requirement,
        |candidate, _| plans[&candidate.service_id].clone(),
    )?;
    let bridge = bridges.remove(&decision.service_id).ok_or_else(|| {
        TranslationError::MetadataUnavailable {
            detail: "selected service is no longer available".into(),
        }
    })?;
    Ok((bridge, decision))
}
