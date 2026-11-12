//! Generic batch planning and request-size measurement.

use crate::block::TextBlock;
use crate::capability::TextTranslationCapability;
use crate::error::TranslationError;
use crate::language::LanguageCode;
use crate::{
    DEFAULT_MAX_BLOCKS_PER_REQUEST, DEFAULT_MAX_REQUEST_BYTES, DEFAULT_MAX_UNITS_PER_BLOCK,
    DEFAULT_MAX_UNITS_PER_REQUEST,
};
use serde::{Deserialize, Serialize};

/// Local bounds applied even when a provider declares no limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalTranslationLimits {
    /// Maximum blocks per request.
    pub max_blocks_per_request: u64,
    /// Maximum units in one block.
    pub max_units_per_block: u64,
    /// Maximum cumulative units in one request.
    pub max_units_per_request: u64,
    /// Maximum serialized request bytes.
    pub max_request_bytes: u64,
}

impl Default for LocalTranslationLimits {
    /// Returns prudent local defaults from the crate constants.
    fn default() -> Self {
        Self {
            max_blocks_per_request: DEFAULT_MAX_BLOCKS_PER_REQUEST,
            max_units_per_block: DEFAULT_MAX_UNITS_PER_BLOCK,
            max_units_per_request: DEFAULT_MAX_UNITS_PER_REQUEST,
            max_request_bytes: DEFAULT_MAX_REQUEST_BYTES,
        }
    }
}

/// Measured size of one request or batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestMeasurement {
    /// Number of blocks in the request.
    pub blocks: u64,
    /// Cumulative provider units in the request.
    pub units: u64,
    /// Serialized request bytes.
    pub serialized_bytes: u64,
}

/// One planned request containing one or more abstract blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationBatch {
    /// Blocks assigned to this request.
    pub blocks: Vec<TextBlock>,
    /// Measured size of the request.
    pub measurement: RequestMeasurement,
}

/// Complete deterministic batching plan for one translation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationPlan {
    /// Ordered batches.
    pub batches: Vec<TranslationBatch>,
    /// Total provider units across all batches.
    pub total_units: u64,
}

/// Plans batches while respecting block count, per-block units, cumulative
/// units and serialized request bytes.
///
/// When `accepts_multiple_blocks` is false, each batch contains exactly one
/// block. Splitting an oversized block is the document layer's responsibility;
/// this function rejects it instead of cutting text or markup.
///
/// # Arguments
///
/// * `blocks` - Abstract blocks already split by the document layer.
/// * `capability` - Declared text capability of the selected service.
/// * `local` - Local bounds always applied.
///
/// # Errors
///
/// Returns [`TranslationError::InvalidRequest`] for an empty block list and
/// [`TranslationError::SizeLimit`] when a block or request cannot fit.
pub fn plan_batches(
    blocks: &[TextBlock],
    capability: &TextTranslationCapability,
    local: &LocalTranslationLimits,
) -> Result<TranslationPlan, TranslationError> {
    plan_batches_with_measurement(blocks, capability, local, |blocks| {
        Ok(measure_request(Some(blocks), capability, None, None).serialized_bytes)
    })
}

/// Plans batches using the actual serialized provider body for byte limits.
///
/// # Arguments
///
/// * `blocks` - Document-layer blocks in input order.
/// * `capability` - Provider unit and count limits.
/// * `local` - Additional local limits.
/// * `measure_body` - Offline request constructor returning the body byte count.
///
/// # Errors
///
/// Returns an invalid request, size violation or body-construction error.
pub fn plan_batches_with_measurement(
    blocks: &[TextBlock],
    capability: &TextTranslationCapability,
    local: &LocalTranslationLimits,
    mut measure_body: impl FnMut(&[TextBlock]) -> Result<u64, TranslationError>,
) -> Result<TranslationPlan, TranslationError> {
    let mut measure = |blocks: &[TextBlock]| -> Result<RequestMeasurement, TranslationError> {
        Ok(RequestMeasurement {
            blocks: blocks.len() as u64,
            units: blocks
                .iter()
                .map(|block| capability.size_unit.measure(&block.text))
                .sum(),
            serialized_bytes: measure_body(blocks)?,
        })
    };
    if blocks.is_empty() {
        return Err(TranslationError::InvalidRequest {
            detail: "text translation requires at least one block".to_string(),
        });
    }

    let provider_max_blocks = capability
        .max_blocks_per_request
        .unwrap_or(u64::MAX)
        .min(local.max_blocks_per_request);
    let max_blocks = if capability.accepts_multiple_blocks {
        provider_max_blocks.max(1)
    } else {
        1
    };
    let max_units_per_block = capability
        .max_units_per_block
        .unwrap_or(u64::MAX)
        .min(local.max_units_per_block);
    let max_units_per_request = capability
        .max_units_per_request
        .unwrap_or(u64::MAX)
        .min(local.max_units_per_request);
    let max_request_bytes = capability
        .max_request_bytes
        .unwrap_or(u64::MAX)
        .min(local.max_request_bytes);

    let mut ids = std::collections::HashSet::new();
    for block in blocks {
        if block.id.as_str().trim().is_empty()
            || block.text.trim().is_empty()
            || !ids.insert(&block.id)
        {
            return Err(TranslationError::InvalidRequest {
                detail: "blocks require unique nonblank identifiers and nonblank text".into(),
            });
        }
    }
    let mut batches = Vec::new();
    let mut current: Vec<TextBlock> = Vec::new();
    let mut total_units = 0u64;

    for block in blocks {
        let units = capability.size_unit.measure(&block.text);
        if units > max_units_per_block {
            return Err(TranslationError::SizeLimit {
                detail: format!(
                    "block {} measures {units} {}, above the per-block limit {max_units_per_block}",
                    block.id.as_str(),
                    capability.size_unit.as_str()
                ),
            });
        }

        current.push(block.clone());
        let measurement = measure(&current)?;
        let fits = current.len() as u64 <= max_blocks
            && measurement.units <= max_units_per_request
            && measurement.serialized_bytes <= max_request_bytes;

        if fits {
            continue;
        }

        let _ = current.pop();
        if current.is_empty() {
            return Err(TranslationError::SizeLimit {
                detail: format!(
                    "block {} cannot fit in one request with the current limits",
                    block.id.as_str()
                ),
            });
        }

        let measurement = measure(&current)?;
        total_units += measurement.units;
        batches.push(TranslationBatch {
            blocks: current,
            measurement,
        });
        current = vec![block.clone()];

        let measurement = measure(&current)?;
        if measurement.serialized_bytes > max_request_bytes
            || measurement.units > max_units_per_request
        {
            return Err(TranslationError::SizeLimit {
                detail: format!(
                    "block {} cannot fit in one request with the current limits",
                    block.id.as_str()
                ),
            });
        }
    }

    if !current.is_empty() {
        let measurement = measure(&current)?;
        total_units += measurement.units;
        batches.push(TranslationBatch {
            blocks: current,
            measurement,
        });
    }

    Ok(TranslationPlan {
        batches,
        total_units,
    })
}

/// Measures blocks with the same normalized JSON shape used by the generic
/// bridge request parameters.
///
/// This standalone estimate is used by document segmentation before a service
/// is bound. The executable bridge and Stream selection instead use
/// [`plan_batches_with_measurement`] with the actual Scrapyfy body builder.
///
/// # Arguments
///
/// * `blocks` - Optional blocks to measure.
/// * `capability` - Capability supplying the size unit.
/// * `source` - Optional source language included in the body.
/// * `target` - Optional target language included in the body.
pub fn measure_request(
    blocks: Option<&[TextBlock]>,
    capability: &TextTranslationCapability,
    source: Option<&LanguageCode>,
    target: Option<&LanguageCode>,
) -> RequestMeasurement {
    let blocks = blocks.unwrap_or(&[]);
    let units = blocks
        .iter()
        .map(|block| capability.size_unit.measure(&block.text))
        .sum();
    let body = serde_json::json!({
        "source": source.map(LanguageCode::as_str),
        "target": target.map(LanguageCode::as_str),
        "blocks": blocks,
    });
    let serialized_bytes = serde_json::to_vec(&body)
        .map(|bytes| bytes.len() as u64)
        .unwrap_or(u64::MAX);
    RequestMeasurement {
        blocks: blocks.len() as u64,
        units,
        serialized_bytes,
    }
}
