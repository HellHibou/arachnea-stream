//! Offline provider request planning shared by selection and execution.

use super::*;
use crate::planning::{plan_batches_with_measurement, TranslationPlan};

impl<P: ScrapyfyQueryPort> ScrapyfyTranslationBridge<P> {
    /// Applies caller-owned limits to subsequent execution.
    ///
    /// # Arguments
    ///
    /// * `limits` - Local bounds used for the selected document.
    pub fn with_local_limits(mut self, limits: LocalTranslationLimits) -> Self {
        self.local_limits = limits;
        self
    }

    /// Computes conservative account units including dedicated source detection.
    ///
    /// # Arguments
    ///
    /// * `request` - Planned text request.
    /// * `plan` - Offline batch plan for that request.
    ///
    /// # Errors
    ///
    /// Returns an invalid sample or an overflow error before sending.
    pub fn planned_budget_units(
        &self,
        request: &TextTranslationRequest,
        plan: &TranslationPlan,
    ) -> Result<u64, TranslationError> {
        let detection = self.metadata.source_detection.as_ref();
        let needs_detection = request.source_language.is_none()
            && detection.is_some_and(|d| d.query.is_some())
            && (plan.batches.len() > 1
                || self.metadata.supported_pairs.is_some()
                || !detection.is_some_and(|d| d.returns_detected_language));
        let extra = if needs_detection {
            let capability = self.metadata.text_translation.as_ref().ok_or_else(|| {
                TranslationError::InvalidRequest {
                    detail: "text capability is required for detection planning".into(),
                }
            })?;
            capability
                .size_unit
                .measure(&detection_sample(&request.blocks, &capability.size_unit)?.text)
        } else {
            0
        };
        plan.total_units
            .checked_add(extra)
            .ok_or_else(|| TranslationError::SizeLimit {
                detail: "planned account units overflow".into(),
            })
    }

    /// Plans the request using Scrapyfy's actual body construction, without HTTP.
    ///
    /// An unknown source measures both automatic and explicit-source queries so
    /// stabilizing detection cannot invalidate the byte limit of later batches.
    ///
    /// # Arguments
    ///
    /// * `request` - Texts and requested language direction.
    /// * `local` - Caller-owned safety bounds.
    ///
    /// # Errors
    ///
    /// Returns a capability, size or body-construction error before sending.
    pub fn plan_texts(
        &self,
        request: &TextTranslationRequest,
        local: &LocalTranslationLimits,
    ) -> Result<TranslationPlan, TranslationError> {
        let capability = self.metadata.text_translation.as_ref().ok_or_else(|| {
            TranslationError::UnsupportedCapability {
                service_id: self.service_id.clone(),
                detail: "text translation is not declared".into(),
            }
        })?;
        plan_batches_with_measurement(&request.blocks, capability, local, |blocks| {
            let mut params = BTreeMap::new();
            params.insert(
                PARAM_BLOCKS_JSON.into(),
                serde_json::to_string(blocks).map_err(|_| TranslationError::InvalidRequest {
                    detail: "cannot serialize blocks".into(),
                })?,
            );
            params.insert(
                PARAM_TARGET_LANGUAGE.into(),
                request.target_language.to_string(),
            );
            let sources: Vec<_> = request
                .source_language
                .clone()
                .map(|source| vec![source])
                .unwrap_or_else(|| self.metadata.effective_source_languages().to_vec());
            let mut bytes = 0;
            for source in sources {
                params.insert(PARAM_SOURCE_LANGUAGE.into(), source.to_string());
                bytes = bytes.max(
                    self.port
                        .measure_translation_query(&capability.query, &params)
                        .map_err(|error| query_error(&capability.query, error))?,
                );
            }
            if request.source_language.is_none() {
                params.insert(PARAM_SOURCE_LANGUAGE.into(), AUTO_SOURCE_LANGUAGE.into());
                let query = capability
                    .auto_query
                    .as_deref()
                    .unwrap_or(&capability.query);
                if capability.auto_query.is_some() {
                    params.remove(PARAM_SOURCE_LANGUAGE);
                }
                bytes = bytes.max(
                    self.port
                        .measure_translation_query(query, &params)
                        .map_err(|error| query_error(query, error))?,
                );
            }
            Ok(bytes)
        })
    }
}
