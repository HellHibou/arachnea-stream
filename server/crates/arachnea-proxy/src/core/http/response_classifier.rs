//! Shared, side-effect-free HTTP response policies and bounded body inspection.

use std::{pin::Pin, sync::Arc, time::Duration};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use http::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::core::{ProxyError, ProxySelectionToken, Result};

/// Maximum prefix exposed to response classifiers.
///
/// Both inspection limits are defined in the crate root and re-exported here
/// for path compatibility.
pub use crate::{MAX_PROXY_RESPONSE_PREFIX, PROXY_RESPONSE_INSPECTION_TIMEOUT};

/// Application acceptance is distinct from receiving an HTTP response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProxyResponseDecision {
    /// The observed response satisfies the configured acceptance policy.
    Accepted,
    /// Only this endpoint/destination association must be excluded.
    RejectedForDestination {
        /// Optional policy override; `None` uses the inventory default.
        cooldown: Option<Duration>,
        /// Non-sensitive diagnostic supplied by the policy.
        reason: String,
    },
    /// Insufficient evidence, an application error, or a classifier failure.
    Inconclusive,
}

/// Optional body access requested before response transformations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxyBodyRequirement {
    /// Classify headers without reading body bytes.
    None,
    /// Inspect at most this many bytes, capped at 64 KiB by the adapter.
    Prefix(usize),
}

/// Completeness of the inspected prefix; a reached limit never proves EOF.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxyBodyState {
    /// EOF was observed (or the HTTP method/status forbids a body).
    Complete,
    /// The prefix limit was reached without observing EOF.
    Limited,
    /// Inspection was not requested, timed out, or encountered a read error.
    Unavailable,
}

/// Borrowed original response metadata, independent of any HTTP client crate.
pub struct ProxyResponseView<'a> {
    /// Actual method used for this response, including redirect rewrites.
    pub method: &'a str,
    /// URL originally requested by the caller.
    pub requested_url: &'a str,
    /// URL to which the observed response belongs.
    pub effective_url: &'a str,
    /// Redirect targets known before this response.
    pub redirect_urls: &'a [String],
    /// Numeric HTTP status.
    pub status: u16,
    /// Original response headers, before any post-action.
    pub headers: &'a HeaderMap,
    /// Exact route, absent for engines without reliable correlation.
    pub selection: Option<&'a ProxySelectionToken>,
    /// Optional prefix; never infer absence of content from an incomplete prefix.
    pub body: &'a [u8],
    /// Whether the prefix represents the complete body.
    pub body_state: ProxyBodyState,
}

/// Synchronous, object-safe policy. Implementations must not block, perform I/O,
/// mutate inventory, or start retries. Incomplete evidence must remain inconclusive
/// unless the observed facts alone suffice for a decision.
pub trait ProxyResponseClassifier: Send + Sync {
    /// Returns optional body needs; no body is inspected by default.
    fn body_requirement(&self) -> ProxyBodyRequirement {
        ProxyBodyRequirement::None
    }

    /// Selects body inspection from original response metadata.
    /// Existing callbacks retain their historical requirement by default.
    ///
    /// # Arguments
    /// - `_response`: Original metadata before body inspection or transformations.
    fn body_requirement_for_response(
        &self,
        _response: &ProxyResponseView<'_>,
    ) -> ProxyBodyRequirement {
        self.body_requirement()
    }

    /// Opts into technical transport retention after an inconclusive redirect is followed.
    /// This never accepts content or overrides a rejection. Custom policies opt out by default.
    fn allows_redirect_transport_reuse(&self) -> bool {
        false
    }

    /// Classifies original metadata and an optional bounded prefix.
    ///
    /// # Arguments
    /// - `response`: Correlated response observations, before transformation.
    ///
    /// # Errors
    /// Policy errors are logged by the adapter and treated as inconclusive.
    fn classify(&self, response: &ProxyResponseView<'_>) -> Result<ProxyResponseDecision>;
}

/// Cloneable code-injected policy, deliberately not serializable.
#[derive(Clone)]
pub struct ProxyResponsePolicy(pub Arc<dyn ProxyResponseClassifier>);

impl std::fmt::Debug for ProxyResponsePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProxyResponsePolicy(custom)")
    }
}

/// Validated serializable policies shared by YAML and media URL options.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProxyResponseClassifierConfig {
    /// Accept 2xx; leave all other responses inconclusive.
    Default,
    /// Explicitly reject 403, otherwise use conservative acceptance.
    Http403,
    /// Reject configured statuses, otherwise accept only configured successes.
    Statuses {
        /// Explicit endpoint/destination rejection statuses.
        #[serde(default)]
        rejected_statuses: Vec<u16>,
        /// Expected acceptance statuses; an omitted list means all 2xx.
        #[serde(default = "default_accepted_statuses")]
        accepted_statuses: Vec<u16>,
        /// Optional positive exclusion duration in seconds.
        #[serde(default)]
        cooldown_seconds: Option<u64>,
    },
}

/// Returns the default accepted HTTP statuses from 200 through 299 inclusive.
fn default_accepted_statuses() -> Vec<u16> {
    (200..300).collect()
}

impl ProxyResponseClassifierConfig {
    /// Validates statuses, contradictions and exclusion duration.
    ///
    /// # Errors
    /// Rejects non-HTTP statuses, overlapping decisions and zero/overflowing durations.
    pub fn validate(&self) -> Result<()> {
        if let Self::Statuses {
            rejected_statuses,
            accepted_statuses,
            cooldown_seconds,
        } = self
        {
            if rejected_statuses
                .iter()
                .chain(accepted_statuses)
                .any(|s| !(100..=599).contains(s))
                || rejected_statuses
                    .iter()
                    .any(|s| accepted_statuses.contains(s))
                || cooldown_seconds.is_some_and(|s| {
                    s == 0
                        || std::time::SystemTime::now()
                            .checked_add(Duration::from_secs(s))
                            .is_none()
                })
            {
                return Err(ProxyError::Protocol(
                    "invalid proxy response classifier statuses or cooldown".into(),
                ));
            }
        }
        Ok(())
    }

    /// Builds a shared policy from validated configuration.
    ///
    /// # Errors
    /// Returns a configuration error for invalid statuses or durations.
    pub fn build(&self) -> Result<ProxyResponsePolicy> {
        self.validate()?;
        let classifier = match self {
            Self::Default => HttpStatusProxyResponseClassifier::default(),
            Self::Http403 => HttpStatusProxyResponseClassifier::http_403(),
            Self::Statuses {
                rejected_statuses,
                accepted_statuses,
                cooldown_seconds,
            } => HttpStatusProxyResponseClassifier {
                rejected_statuses: rejected_statuses.clone(),
                accepted_statuses: accepted_statuses.clone(),
                cooldown: cooldown_seconds.map(Duration::from_secs),
            },
        };
        Ok(ProxyResponsePolicy(Arc::new(classifier)))
    }
}

/// Conservative reusable status classifier; 403 rejection is opt-in.
#[derive(Clone, Debug)]
pub struct HttpStatusProxyResponseClassifier {
    rejected_statuses: Vec<u16>,
    accepted_statuses: Vec<u16>,
    cooldown: Option<Duration>,
}

impl Default for HttpStatusProxyResponseClassifier {
    fn default() -> Self {
        Self {
            rejected_statuses: Vec::new(),
            accepted_statuses: default_accepted_statuses(),
            cooldown: None,
        }
    }
}

impl HttpStatusProxyResponseClassifier {
    /// Returns the explicit 403 rejection policy, without accepting other errors.
    pub fn http_403() -> Self {
        Self {
            rejected_statuses: vec![403],
            ..Self::default()
        }
    }
}

impl ProxyResponseClassifier for HttpStatusProxyResponseClassifier {
    fn allows_redirect_transport_reuse(&self) -> bool {
        true
    }

    fn classify(&self, response: &ProxyResponseView<'_>) -> Result<ProxyResponseDecision> {
        Ok(if self.rejected_statuses.contains(&response.status) {
            ProxyResponseDecision::RejectedForDestination {
                cooldown: self.cooldown,
                reason: format!("configured HTTP status {}", response.status),
            }
        } else if self.accepted_statuses.contains(&response.status) {
            ProxyResponseDecision::Accepted
        } else {
            ProxyResponseDecision::Inconclusive
        })
    }
}

/// Resolves explicit configuration before legacy statuses; code injection takes
/// precedence at the caller. Legacy rejection overrides default 2xx acceptance.
///
/// # Arguments
/// - `config`: Optional known policy configuration.
/// - `legacy_statuses`: Existing explicit rejection list.
///
/// # Errors
/// Returns invalid configuration errors, never silently ignoring a bad policy.
pub fn resolve_response_policy(
    config: Option<&ProxyResponseClassifierConfig>,
    legacy_statuses: &[u16],
) -> Result<ProxyResponsePolicy> {
    match config {
        Some(config) => config.build(),
        None => ProxyResponseClassifierConfig::Statuses {
            rejected_statuses: legacy_statuses.to_vec(),
            accepted_statuses: default_accepted_statuses()
                .into_iter()
                .filter(|s| !legacy_statuses.contains(s))
                .collect(),
            cooldown_seconds: None,
        }
        .build(),
    }
}

/// Owned body stream used only at the observation boundary.
pub type ProxyResponseStream = Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>;

/// Inspects a bounded prefix and returns every consumed byte/error to the caller.
/// Classification itself has no inventory effects. Cancellation drops only this
/// request's stream; no detached task is created.
///
/// # Arguments
/// - `policy`: Selected policy, or conservative defaults.
/// - `view`: Original response metadata; body fields are filled by this function.
/// - `stream`: Decoded response body before transformations.
/// - `timeout`: Remaining inspection allowance, capped at three seconds.
pub async fn inspect_proxy_response(
    policy: Option<&ProxyResponsePolicy>,
    view: ProxyResponseView<'_>,
    stream: ProxyResponseStream,
    timeout: Duration,
) -> (ProxyResponseDecision, ProxyResponseStream) {
    let (decision, stream, _) =
        inspect_proxy_response_with_transport_policy(policy, view, stream, timeout).await;
    (decision, stream)
}

/// Inspects a response while retaining permission for technical redirect transport reuse.
/// The third result is true only for an explicitly permitted, inconclusive redirect
/// without inspection or classifier failure. The caller must still consume the complete
/// body successfully and confirm redirect following before admitting its transport.
///
/// # Arguments
/// - `policy`: Selected policy, or conservative status defaults.
/// - `view`: Original response metadata, before transformations.
/// - `stream`: Decoded response body, replayed without losing consumed bytes or errors.
/// - `timeout`: Inspection allowance, capped at three seconds.
pub async fn inspect_proxy_response_with_transport_policy(
    policy: Option<&ProxyResponsePolicy>,
    view: ProxyResponseView<'_>,
    stream: ProxyResponseStream,
    timeout: Duration,
) -> (ProxyResponseDecision, ProxyResponseStream, bool) {
    let default = ProxyResponsePolicy(Arc::new(HttpStatusProxyResponseClassifier::default()));
    let policy = policy.unwrap_or(&default);
    let requirement = policy.0.body_requirement_for_response(&view);
    inspect_proxy_response_with_requirement(Some(policy), view, stream, timeout, requirement).await
}

/// Inspects using a requirement already selected from original response metadata.
/// This avoids evaluating the response-specific hook twice when the caller also
/// needs its result to preserve transfer timing. Business classification remains authoritative.
///
/// # Arguments
/// - `policy`: Selected policy, or conservative status defaults.
/// - `view`: Original response metadata before inspection.
/// - `stream`: Decoded body; consumed bytes and errors are replayed unchanged.
/// - `timeout`: Inspection allowance, capped at three seconds.
/// - `requirement`: Body requirement selected for this response.
///
/// # Returns
/// Business decision, replayable stream and technical redirect-retention permission.
pub async fn inspect_proxy_response_with_requirement(
    policy: Option<&ProxyResponsePolicy>,
    view: ProxyResponseView<'_>,
    mut stream: ProxyResponseStream,
    timeout: Duration,
    requirement: ProxyBodyRequirement,
) -> (ProxyResponseDecision, ProxyResponseStream, bool) {
    let default = ProxyResponsePolicy(Arc::new(HttpStatusProxyResponseClassifier::default()));
    let policy = policy.unwrap_or(&default);
    let limit = match requirement {
        ProxyBodyRequirement::None => 0,
        ProxyBodyRequirement::Prefix(limit) => limit.min(MAX_PROXY_RESPONSE_PREFIX),
    };
    let mut prefix = Vec::new();
    let mut replay = Vec::new();
    let mut state = if view.method.eq_ignore_ascii_case("HEAD")
        || matches!(view.status, 204 | 304)
        || view.status < 200
    {
        ProxyBodyState::Complete
    } else {
        ProxyBodyState::Unavailable
    };
    if limit > 0 && state != ProxyBodyState::Complete {
        let deadline = tokio::time::Instant::now() + timeout.min(PROXY_RESPONSE_INSPECTION_TIMEOUT);
        while prefix.len() < limit {
            if tokio::time::Instant::now() >= deadline {
                state = ProxyBodyState::Unavailable;
                break;
            }
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(Some(Ok(bytes))) => {
                    if bytes.is_empty() {
                        continue;
                    }
                    let count = (limit - prefix.len()).min(bytes.len());
                    prefix.extend_from_slice(&bytes[..count]);
                    replay.push(Ok(bytes));
                    state = ProxyBodyState::Limited;
                }
                Ok(None) => {
                    state = ProxyBodyState::Complete;
                    break;
                }
                Ok(Some(Err(error))) => {
                    replay.push(Err(error));
                    state = ProxyBodyState::Unavailable;
                    break;
                }
                Err(_) => {
                    state = ProxyBodyState::Unavailable;
                    break;
                }
            }
        }
    }
    let view = ProxyResponseView {
        body: &prefix,
        body_state: state,
        ..view
    };
    let (decision, classified) = match policy.0.classify(&view) {
        Ok(decision) => (decision, true),
        Err(error) => {
            tracing::warn!(%error, "proxy response classifier failed; result is inconclusive");
            (ProxyResponseDecision::Inconclusive, false)
        }
    };
    let allow_transport = classified
        && policy.0.allows_redirect_transport_reuse()
        && (limit == 0 || state != ProxyBodyState::Unavailable)
        && matches!(view.status, 301 | 302 | 303 | 307 | 308)
        && matches!(decision, ProxyResponseDecision::Inconclusive);
    (
        decision,
        Box::pin(futures::stream::iter(replay).chain(stream)),
        allow_transport,
    )
}
