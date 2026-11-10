//! Deterministic service filtering, exclusion reasons and ranking.

use crate::block::TextBlock;
use crate::error::{ExclusionReason, TranslationError};
use crate::language::LanguageCode;
use crate::metadata::TranslationMetadata;
use crate::planning::{plan_batches, LocalTranslationLimits};
use serde::{Deserialize, Serialize};

/// Caller-selected service policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SelectionPolicy {
    /// Select among compatible candidates, preferring one service when given.
    Auto {
        /// Optional preferred service used only as a ranking hint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preferred_service: Option<String>,
    },
    /// Restrict selection to one explicitly requested service.
    Constrained {
        /// Required service identifier.
        service_id: String,
    },
}

/// Requested translation route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionRoute {
    /// Text translation of the supplied blocks.
    Text,
    /// Provider-side URL document translation.
    Url,
    /// URL when declared and usable, otherwise text translation of the blocks.
    UrlWithTextFallback,
}

/// Requirements applied to each candidate without sending a translation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionRequirement {
    /// Known source language, or `None` when detection is requested.
    pub source_language: Option<LanguageCode>,
    /// Target language.
    pub target_language: LanguageCode,
    /// Abstract blocks already split by the document layer.
    pub blocks: Vec<TextBlock>,
    /// Requested route.
    pub route: SelectionRoute,
    /// Source document media type for URL translation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Retrieved source document size, when already known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_bytes: Option<u64>,
    /// Whether neighbouring context is required.
    #[serde(default)]
    pub require_context: bool,
    /// Whether markup preservation is required.
    #[serde(default)]
    pub require_markup: bool,
    /// Local bounds applied during batch simulation.
    #[serde(default)]
    pub local_limits: LocalTranslationLimits,
}

/// One candidate service with activation, freshness and budget state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCandidate {
    /// Stable service identifier.
    pub service_id: String,
    /// Validated metadata loaded for this service.
    pub metadata: TranslationMetadata,
    /// Whether metadata is still inside its freshness window.
    pub metadata_fresh: bool,
    /// Whether a budget or concurrency reservation is available.
    pub budget_available: bool,
    /// Administrator-defined ordering rank; lower values rank first.
    pub administrator_order: u32,
}

/// Result of evaluating one candidate without a provider call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateEvaluation {
    /// Service identifier.
    pub service_id: String,
    /// Whether the service is eligible.
    pub eligible: bool,
    /// Normalized exclusion reason when not eligible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusion: Option<ExclusionReason>,
    /// Contextual detail kept for administration diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Estimated number of requests; zero for a URL path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_batches: Option<u64>,
}

/// One normalized exclusion retained in a selection decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateExclusion {
    /// Excluded service identifier.
    pub service_id: String,
    /// Normalized exclusion reason.
    pub reason: ExclusionReason,
    /// Contextual detail for diagnostics.
    pub detail: String,
}

/// Selected service and the evaluations that produced the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionDecision {
    /// Selected service identifier.
    pub service_id: String,
    /// Evaluation of every considered candidate.
    pub evaluations: Vec<CandidateEvaluation>,
}

impl SelectionDecision {
    /// Returns the normalized exclusions from this decision.
    pub fn exclusions(&self) -> Vec<CandidateExclusion> {
        self.evaluations
            .iter()
            .filter_map(|evaluation| {
                evaluation.exclusion.map(|reason| CandidateExclusion {
                    service_id: evaluation.service_id.clone(),
                    reason,
                    detail: evaluation
                        .detail
                        .clone()
                        .unwrap_or_else(|| "service excluded".to_string()),
                })
            })
            .collect()
    }
}

/// Filters and ranks candidates deterministically without translating.
///
/// # Arguments
///
/// * `candidates` - Activated, configured candidates with validated metadata.
/// * `policy` - Automatic or constrained service policy.
/// * `requirement` - Pair, route, document and local bounds to satisfy.
///
/// # Errors
///
/// Returns [`TranslationError::NoEligibleService`] when no candidate remains.
/// The decision diagnostics are kept in the error detail; texts and secrets are
/// never included.
pub fn select_service(
    candidates: &[ServiceCandidate],
    policy: &SelectionPolicy,
    requirement: &SelectionRequirement,
) -> Result<SelectionDecision, TranslationError> {
    let mut evaluations = Vec::with_capacity(candidates.len());
    let mut eligible: Vec<(usize, CandidateEvaluation)> = Vec::new();

    for (index, candidate) in candidates.iter().enumerate() {
        if let SelectionPolicy::Constrained { service_id } = policy {
            if candidate.service_id != *service_id {
                continue;
            }
        }
        let evaluation = evaluate_candidate(candidate, requirement);
        if evaluation.eligible {
            eligible.push((index, evaluation.clone()));
        }
        evaluations.push(evaluation);
    }

    let selected = match policy {
        SelectionPolicy::Constrained { .. } => eligible.first(),
        SelectionPolicy::Auto { preferred_service } => {
            eligible.iter().min_by_key(|(index, evaluation)| {
                let candidate = &candidates[*index];
                let preferred = preferred_service
                    .as_deref()
                    .is_some_and(|service| service == candidate.service_id);
                (
                    !preferred,
                    candidate.administrator_order,
                    evaluation.estimated_batches.unwrap_or(0),
                    candidate.service_id.clone(),
                )
            })
        }
    };

    match selected {
        Some((_, evaluation)) => Ok(SelectionDecision {
            service_id: evaluation.service_id.clone(),
            evaluations,
        }),
        None => {
            let reasons = evaluations
                .iter()
                .filter_map(|evaluation| {
                    evaluation
                        .exclusion
                        .map(|exclusion| format!("{}={exclusion:?}", evaluation.service_id))
                })
                .collect::<Vec<_>>()
                .join(", ");
            Err(TranslationError::NoEligibleService {
                detail: if reasons.is_empty() {
                    "no candidate service was available".to_string()
                } else {
                    reasons
                },
            })
        }
    }
}

/// Evaluates one candidate against a requirement.
///
/// # Arguments
///
/// * `candidate` - Candidate state to evaluate.
/// * `requirement` - Requirements to apply.
fn evaluate_candidate(
    candidate: &ServiceCandidate,
    requirement: &SelectionRequirement,
) -> CandidateEvaluation {
    let excluded = |reason, detail: String| CandidateEvaluation {
        service_id: candidate.service_id.clone(),
        eligible: false,
        exclusion: Some(reason),
        detail: Some(detail),
        estimated_batches: None,
    };

    if !candidate.metadata_fresh {
        return excluded(
            ExclusionReason::MetadataUnavailable,
            "metadata is stale or unavailable".to_string(),
        );
    }
    if !candidate.budget_available {
        return excluded(
            ExclusionReason::BudgetExhausted,
            "no budget or concurrency reservation is available".to_string(),
        );
    }

    let metadata = &candidate.metadata;
    if requirement.source_language.is_none() {
        if !metadata.detection_available() {
            return excluded(
                ExclusionReason::SourceLanguageRequired,
                "source detection is not declared".to_string(),
            );
        }
        if metadata.supported_pairs.is_some() && !metadata.detection_can_establish_source() {
            return excluded(
                ExclusionReason::SourceLanguageRequired,
                "restricted pairs require a prior detectable source".to_string(),
            );
        }
    }
    if !metadata.supports_pair(
        requirement.source_language.as_ref(),
        &requirement.target_language,
    ) {
        return excluded(
            ExclusionReason::UnsupportedLanguagePair,
            format!(
                "pair {:?} -> {} is not declared",
                requirement
                    .source_language
                    .as_ref()
                    .map(LanguageCode::as_str),
                requirement.target_language
            ),
        );
    }

    match requirement.route {
        SelectionRoute::Text => evaluate_text(candidate, requirement)
            .unwrap_or_else(|(reason, detail)| excluded(reason, detail)),
        SelectionRoute::Url => evaluate_url(candidate, requirement)
            .unwrap_or_else(|(reason, detail)| excluded(reason, detail)),
        SelectionRoute::UrlWithTextFallback => {
            if let Ok(evaluation) = evaluate_url(candidate, requirement) {
                return evaluation;
            }
            evaluate_text(candidate, requirement)
                .unwrap_or_else(|(reason, detail)| excluded(reason, detail))
        }
    }
}

/// Evaluates the text route and estimates the number of batches.
///
/// # Arguments
///
/// * `candidate` - Candidate service.
/// * `requirement` - Requirements including blocks and local bounds.
///
/// # Errors
///
/// Returns a normalized exclusion reason and detail.
fn evaluate_text(
    candidate: &ServiceCandidate,
    requirement: &SelectionRequirement,
) -> Result<CandidateEvaluation, (ExclusionReason, String)> {
    let capability = candidate.metadata.text_translation.as_ref().ok_or((
        ExclusionReason::UnsupportedCapability,
        "text translation is not declared".to_string(),
    ))?;
    if requirement.require_context && !capability.context_support {
        return Err((
            ExclusionReason::UnsupportedCapability,
            "context is required but not supported".to_string(),
        ));
    }
    if requirement.require_markup && !capability.markup_support {
        return Err((
            ExclusionReason::UnsupportedCapability,
            "markup preservation is required but not supported".to_string(),
        ));
    }
    let plan = plan_batches(&requirement.blocks, capability, &requirement.local_limits).map_err(
        |error| {
            (
                ExclusionReason::SizeLimit,
                match error {
                    TranslationError::SizeLimit { detail } => detail,
                    other => other.to_string(),
                },
            )
        },
    )?;
    Ok(CandidateEvaluation {
        service_id: candidate.service_id.clone(),
        eligible: true,
        exclusion: None,
        detail: None,
        estimated_batches: Some(plan.batches.len() as u64),
    })
}

/// Evaluates the URL route.
///
/// # Arguments
///
/// * `candidate` - Candidate service.
/// * `requirement` - Requirements including the document format and size.
///
/// # Errors
///
/// Returns a normalized exclusion reason and detail.
fn evaluate_url(
    candidate: &ServiceCandidate,
    requirement: &SelectionRequirement,
) -> Result<CandidateEvaluation, (ExclusionReason, String)> {
    let capability = candidate.metadata.url_translation.as_ref().ok_or((
        ExclusionReason::UnsupportedCapability,
        "URL translation is not declared".to_string(),
    ))?;
    if let Some(media_type) = &requirement.media_type {
        if !capability.accepts_format(media_type) {
            return Err((
                ExclusionReason::UnsupportedCapability,
                format!("URL translation does not accept {media_type}"),
            ));
        }
    }
    if let (Some(bytes), Some(limit)) = (requirement.document_bytes, capability.max_document_bytes)
    {
        if bytes > limit {
            return Err((
                ExclusionReason::SizeLimit,
                format!("document size {bytes} exceeds URL limit {limit}"),
            ));
        }
    }
    Ok(CandidateEvaluation {
        service_id: candidate.service_id.clone(),
        eligible: true,
        exclusion: None,
        detail: None,
        estimated_batches: Some(0),
    })
}
