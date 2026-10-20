//! Request-owned correlation for a concrete, pinned proxy route.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::SystemTime;

use super::{
    ArachneaProxyCore, Destination, ProxyDestinationFailureReason, ProxyInventory,
    ProxyObservation, ProxyObservationOutcome, ProxyObservationPhase, ProxyProbeContext,
};

static NEXT_SELECTION: AtomicU64 = AtomicU64::new(1);

/// Evidence used to select this exact endpoint and destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxySelectionProvenance {
    /// Candidate without a prior business success for this destination.
    Discovery,
    /// Previously accepted final response from this destination.
    AcceptedDestination,
    /// Previously followed redirect from this destination, without content acceptance.
    RedirectDestination,
}

/// Opaque association between a concrete route, endpoint and exact destination.
/// Never serialize this token or forward it to an origin server.
#[derive(Clone)]
pub struct ProxySelectionToken {
    id: u64,
    inventory: Arc<ProxyInventory>,
    endpoint: String,
    destination: Destination,
    provenance: ProxySelectionProvenance,
    discovery: Arc<Mutex<Option<tokio::sync::OwnedMutexGuard<()>>>>,
    accepted: Arc<tokio::sync::OnceCell<()>>,
    continuation: Option<Arc<super::proxy_inventory::preparation::ProbeContinuation>>,
}

impl std::fmt::Debug for ProxySelectionToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProxySelectionToken")
            .field("id", &self.id)
            .field("endpoint", &self.endpoint)
            .field("destination", &self.destination)
            .field("provenance", &self.provenance)
            .finish()
    }
}

impl PartialEq for ProxySelectionToken {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Arc::ptr_eq(&self.inventory, &other.inventory)
    }
}
impl Eq for ProxySelectionToken {}

impl ProxySelectionToken {
    pub(crate) fn new(
        inventory: Arc<ProxyInventory>,
        endpoint: String,
        destination: Destination,
    ) -> Self {
        let search = super::ProxySearchContext::current();
        let provenance = search
            .as_ref()
            .map(|search| search.selection_provenance(&endpoint, &destination))
            .unwrap_or(ProxySelectionProvenance::Discovery);
        Self {
            id: NEXT_SELECTION.fetch_add(1, Ordering::Relaxed),
            inventory,
            endpoint,
            destination,
            provenance,
            continuation: search
                .as_ref()
                .and_then(|search| search.continuation.clone()),
            discovery: Arc::new(Mutex::new(
                search.and_then(|search| search.take_discovery()),
            )),
            accepted: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Returns the exact destination selected for this route generation.
    pub fn destination(&self) -> &Destination {
        &self.destination
    }

    /// Returns the selected proxy authority, without credentials.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Evidence attached to this selection, independent of concurrent calls.
    pub fn provenance(&self) -> ProxySelectionProvenance {
        self.provenance
    }

    /// Whether a previous business exchange supports the longer connection profile.
    pub fn reuses_destination(&self) -> bool {
        self.provenance != ProxySelectionProvenance::Discovery
    }

    /// Releases cold-discovery ownership without changing health or acceptance.
    /// Adapters also call this on terminal body errors after response receipt.
    pub fn finish_discovery(&self) {
        self.discovery.lock().unwrap().take();
    }

    /// Records a followed redirect for this origin without accepting final content.
    ///
    /// # Arguments
    /// - `status`: Redirect status actually followed by the HTTP client.
    pub async fn record_redirect(&self, status: u16) {
        if !matches!(status, 301 | 302 | 303 | 307 | 308) {
            self.finish_discovery();
            return;
        }
        let now = SystemTime::now();
        self.inventory
            .record_observation(
                &self.endpoint,
                ProxyObservation {
                    context: ProxyProbeContext {
                        phase: ProxyObservationPhase::DestinationHttp,
                        protocol: None,
                        target: Some(self.destination.to_string()),
                        method: None,
                        configuration: "business-redirect-v1".into(),
                    },
                    outcome: ProxyObservationOutcome::Succeeded,
                    completed_at: now,
                    last_succeeded_at: Some(now),
                    http_status: Some(status),
                },
            )
            .await;
        self.inventory.notify_destination_success();
        tracing::info!(search_id = super::ProxySearchContext::current().map(|search| search.id()),
            proxy = self.endpoint(), destination = %self.destination, status,
            "recorded followed proxy redirect without final content acceptance");
        self.finish_discovery();
    }

    /// Records acceptance once for this logical selection, before body transfer.
    /// Concurrent clones share the awaited commit and release discovery ownership.
    pub async fn record_accepted(&self) {
        self.accepted
            .get_or_init(|| async {
                self.inventory
                    .record_validated_response(&self.endpoint, &self.destination)
                    .await;
                if let Some(continuation) = &self.continuation {
                    self.inventory.prepare_after_acceptance(
                        continuation,
                        &self.endpoint,
                        &self.destination,
                    );
                }
            })
            .await;
        self.finish_discovery();
    }

    /// Applies a classified response only to this exact selection.
    ///
    /// # Arguments
    /// - `decision`: Classifier result; inconclusive preserves durable history.
    pub async fn record_response_decision(
        &self,
        decision: &super::http::response_classifier::ProxyResponseDecision,
    ) {
        use super::http::response_classifier::ProxyResponseDecision;
        match decision {
            ProxyResponseDecision::Accepted => self.record_accepted().await,
            ProxyResponseDecision::RejectedForDestination { cooldown, reason } => {
                tracing::debug!(endpoint = %self.endpoint, %reason, "proxy rejected for selected destination");
                self.inventory
                    .record_destination_failure_with_cooldown(
                        &self.endpoint,
                        &self.destination,
                        ProxyDestinationFailureReason::BlockedByOrigin,
                        *cooldown,
                    )
                    .await;
            }
            ProxyResponseDecision::Inconclusive => {}
        }
        self.finish_discovery();
    }

    /// Excludes this association without degrading another route or endpoint.
    ///
    /// # Arguments
    /// - `reason`: Explicit or ambiguous destination failure classification.
    pub async fn record_failure(&self, reason: ProxyDestinationFailureReason) {
        self.inventory
            .record_destination_failure(&self.endpoint, &self.destination, reason)
            .await;
        self.finish_discovery();
    }

    /// Records the remaining destination failure without guessing its endpoint.
    /// Global failures are applied at the core's directly observed endpoint boundary.
    ///
    /// # Arguments
    /// - `error`: Failure returned by the pinned core before business delivery.
    pub async fn record_transport_failure(&self, error: &super::ProxyError) {
        let _ = error;
        self.record_failure(ProxyDestinationFailureReason::Other)
            .await;
    }

    /// Records response receipt independently of its application status.
    ///
    /// # Arguments
    /// - `status`: HTTP status actually received on this route.
    pub async fn record_http_response(&self, status: u16) {
        self.record_phase(ProxyObservationPhase::DestinationHttp, true, Some(status))
            .await;
    }

    /// Records a phase directly observed on this pinned route.
    ///
    /// # Arguments
    /// - `phase`: Boundary actually observed by the transport.
    /// - `succeeded`: Whether that phase completed successfully.
    /// - `status`: HTTP status when a response was received.
    pub async fn record_phase(
        &self,
        phase: ProxyObservationPhase,
        succeeded: bool,
        status: Option<u16>,
    ) {
        let now = SystemTime::now();
        self.inventory
            .record_observation(
                &self.endpoint,
                ProxyObservation {
                    context: ProxyProbeContext {
                        phase,
                        protocol: None,
                        target: Some(self.destination.to_string()),
                        method: None,
                        configuration: "business-response-v1".into(),
                    },
                    outcome: if succeeded {
                        ProxyObservationOutcome::Succeeded
                    } else {
                        ProxyObservationOutcome::Inconclusive
                    },
                    completed_at: now,
                    last_succeeded_at: succeeded.then_some(now),
                    http_status: status,
                },
            )
            .await;
    }
}

/// Concrete route prepared before an HTTP attempt; reconnects cannot rotate it.
#[derive(Clone, Debug)]
pub struct SelectedProxy {
    /// Opaque identity of the resolved route, used to isolate reusable clients.
    pub route_generation: u64,
    /// Core restricted to the resolved route and destination for this attempt.
    pub core: ArachneaProxyCore,
    /// Dynamic endpoint correlation, absent for static or direct routes.
    pub selection_token: Option<ProxySelectionToken>,
}
