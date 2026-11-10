//! Generic bridge executing normalized translation queries through Scrapyfy.

use crate::block::{
    BlockId, TextBlock, TextTranslationRequest, TranslatedBlock, TranslatedDocument,
    TranslatedTexts, UrlTranslationRequest,
};
use crate::budget::{TranslationBudgetPort, UnlimitedTranslationBudget};
use crate::capability::{SizeUnit, TextTranslationCapability, UrlResultMode};
use crate::error::TranslationError;
use crate::language::LanguageCode;
use crate::metadata::TranslationMetadata;
use crate::metrics::TranslationMetrics;
use crate::planning::LocalTranslationLimits;
use crate::service::TranslationService;
use crate::ProviderErrorCategory;
use crate::{
    AUTO_SOURCE_LANGUAGE, DEFAULT_DETECTION_SAMPLE_UNITS, DEFAULT_RETRY_INITIAL_DELAY_MS,
    DEFAULT_RETRY_MAX_ATTEMPTS, DEFAULT_RETRY_MAX_DELAY_MS, GET_TRANSLATION_METADATA_QUERY,
    PARAM_BLOCKS_JSON, PARAM_DOCUMENT_URL, PARAM_SOURCE_LANGUAGE, PARAM_TARGET_LANGUAGE,
    TRANSLATION_DETECTION_MEDIA_TYPE, TRANSLATION_DOCUMENT_MEDIA_TYPE,
    TRANSLATION_METADATA_MEDIA_TYPE, TRANSLATION_TEXT_MEDIA_TYPE,
};
use arachnea_http::ArachneaHttpError;
use arachnea_scrapyfy::{ScraperDataNode, ScraperQueryCollection};
use async_trait::async_trait;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// One normalized Scrapyfy query row.
pub type QueryRow = HashMap<String, ScraperDataNode>;

/// Outbound port used to execute Scrapyfy translation queries.
///
/// Stream implements this port over its service catalog. Keeping the port
/// generic avoids a dependency from this crate to Stream and makes request
/// measurement and test execution replaceable.
#[async_trait]
pub trait ScrapyfyQueryPort: Send + Sync {
    /// Measures a serialized provider request body without sending it.
    ///
    /// # Arguments
    ///
    /// * `query` - Query whose body construction is reused.
    /// * `params` - Same runtime parameters as execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the query cannot be measured offline.
    fn measure_translation_query(
        &self,
        query: &str,
        params: &BTreeMap<String, String>,
    ) -> anyhow::Result<u64>;

    /// Executes one named query with normalized runtime parameters.
    ///
    /// # Arguments
    ///
    /// * `query` - Query name declared by the service YAML.
    /// * `params` - Normalized runtime parameters.
    /// * `media_types` - Optional media-type filter applied by Scrapyfy.
    ///
    /// # Errors
    ///
    /// Returns an error when the query cannot be executed or its response
    /// cannot be parsed.
    async fn execute_translation_query(
        &self,
        query: &str,
        params: &BTreeMap<String, String>,
        media_types: &[String],
    ) -> anyhow::Result<Vec<QueryRow>>;
}

/// Scrapyfy query port backed by an already loaded query collection.
pub struct ScrapyfyCollectionPort {
    collection: Arc<ScraperQueryCollection>,
    service_id: String,
}

impl ScrapyfyCollectionPort {
    /// Creates a port for one activated service collection.
    ///
    /// # Arguments
    ///
    /// * `collection` - Loaded and validated Scrapyfy query collection.
    /// * `service_id` - Stable service identifier used in diagnostics.
    pub fn new(collection: Arc<ScraperQueryCollection>, service_id: impl Into<String>) -> Self {
        Self {
            collection,
            service_id: service_id.into(),
        }
    }

    /// Returns the service identifier.
    pub fn service_id(&self) -> &str {
        &self.service_id
    }
}

#[async_trait]
impl ScrapyfyQueryPort for ScrapyfyCollectionPort {
    fn measure_translation_query(
        &self,
        query: &str,
        params: &BTreeMap<String, String>,
    ) -> anyhow::Result<u64> {
        self.collection.measure_query_body(
            query,
            &params.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        )
    }

    /// Executes the query through the wrapped Scrapyfy collection.
    async fn execute_translation_query(
        &self,
        query: &str,
        params: &BTreeMap<String, String>,
        media_types: &[String],
    ) -> anyhow::Result<Vec<QueryRow>> {
        let params = params
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<HashMap<_, _>>();
        let media_types = media_types.to_vec();
        self.collection
            .execute_query(query, &params, Some(&media_types), None)
            .await
    }
}

/// Bounded retry policy applied to retryable provider failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranslationRetryPolicy {
    /// Total attempts, including the initial one. Values below one are treated as one.
    pub max_attempts: u32,
    /// Initial exponential backoff delay.
    pub initial_delay: Duration,
    /// Upper bound for one retry delay, including provider `Retry-After` hints.
    pub max_delay: Duration,
}

impl Default for TranslationRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_RETRY_MAX_ATTEMPTS,
            initial_delay: Duration::from_millis(DEFAULT_RETRY_INITIAL_DELAY_MS),
            max_delay: Duration::from_millis(DEFAULT_RETRY_MAX_DELAY_MS),
        }
    }
}

impl TranslationRetryPolicy {
    /// Returns the delay before one retry.
    ///
    /// The exponential base is jittered within a bounded half-window to avoid
    /// synchronized retries. A provider `Retry-After` hint is honored but never
    /// exceeds [`Self::max_delay`].
    fn delay(&self, attempt: u32, retry_after_seconds: Option<u64>) -> Duration {
        let exponent = attempt.saturating_sub(1).min(16);
        let base = self
            .initial_delay
            .saturating_mul(1u32 << exponent)
            .min(self.max_delay);
        let jitter_bound = (base.as_millis() as u64) / 2;
        let jitter = if jitter_bound == 0 {
            0
        } else {
            rand::random::<u64>() % (jitter_bound + 1)
        };
        let backoff = base + Duration::from_millis(jitter);
        let retry_after = retry_after_seconds.map(Duration::from_secs);
        backoff
            .max(retry_after.unwrap_or_default())
            .min(self.max_delay)
    }
}

/// Generic translation service executing YAML queries through Scrapyfy.
pub struct ScrapyfyTranslationBridge<P> {
    port: P,
    service_id: String,
    metadata: TranslationMetadata,
    configuration_revision: String,
    budget: Arc<dyn TranslationBudgetPort>,
    retry: TranslationRetryPolicy,
    local_limits: LocalTranslationLimits,
}

impl<P: ScrapyfyQueryPort> ScrapyfyTranslationBridge<P> {
    /// Loads and validates service metadata through the mandatory query.
    ///
    /// # Arguments
    ///
    /// * `port` - Scrapyfy query port for this service.
    /// * `service_id` - Stable service identifier.
    /// * `configuration_revision` - Revision of YAML, actions and parameters
    ///   influencing translation output.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the metadata query fails, returns no
    /// row or violates the normalized contract.
    pub async fn load(
        port: P,
        service_id: impl Into<String>,
        configuration_revision: impl Into<String>,
    ) -> Result<Self, TranslationError> {
        let service_id = service_id.into();
        let params = BTreeMap::new();
        let media_types = [TRANSLATION_METADATA_MEDIA_TYPE.to_string()];
        let rows = port
            .execute_translation_query(GET_TRANSLATION_METADATA_QUERY, &params, &media_types)
            .await
            .map_err(|error| {
                query_error(
                    &format!("{GET_TRANSLATION_METADATA_QUERY} ({service_id})"),
                    error,
                )
            })?;
        let row = rows
            .first()
            .ok_or_else(|| TranslationError::MetadataUnavailable {
                detail: format!(
                    "{GET_TRANSLATION_METADATA_QUERY} returned no row for {service_id}"
                ),
            })?;
        let value = metadata_value(row)?;
        let metadata = TranslationMetadata::from_value(&value)?;
        Ok(Self {
            port,
            service_id,
            metadata,
            configuration_revision: configuration_revision.into(),
            budget: Arc::new(UnlimitedTranslationBudget),
            retry: TranslationRetryPolicy::default(),
            local_limits: LocalTranslationLimits::default(),
        })
    }

    /// Replaces the account budget and concurrency port.
    ///
    /// # Arguments
    ///
    /// * `budget` - Shared budget implementation used before every provider call.
    pub fn with_budget(mut self, budget: Arc<dyn TranslationBudgetPort>) -> Self {
        self.budget = budget;
        self
    }

    /// Replaces the bounded retry policy.
    ///
    /// # Arguments
    ///
    /// * `retry` - Retry bounds applied to retryable provider failures.
    pub fn with_retry_policy(mut self, retry: TranslationRetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Returns the provider configuration revision recorded in cache identities.
    pub fn configuration_revision(&self) -> &str {
        &self.configuration_revision
    }

    /// Executes one normalized text query batch without retrying.
    ///
    /// # Arguments
    ///
    /// * `query` - Query name from the text capability.
    /// * `source` - Optional normalized source parameter. `None` omits it.
    /// * `target` - Normalized target language.
    /// * `blocks` - Blocks in this batch.
    /// * `units` - Provider units reserved for this attempt.
    /// * `preserves_request_order` - Whether positional response matching is allowed.
    /// * `metrics` - Counters updated for this attempt.
    ///
    /// # Errors
    ///
    /// Returns a normalized error when the query fails or its rows violate the
    /// input/output correspondence.
    #[allow(clippy::too_many_arguments)]
    async fn execute_text_batch(
        &self,
        query: &str,
        source: Option<&str>,
        target: &str,
        blocks: &[TextBlock],
        units: u64,
        preserves_request_order: bool,
        metrics: &mut TranslationMetrics,
    ) -> Result<(Vec<TranslatedBlock>, Option<LanguageCode>), TranslationError> {
        let blocks_json =
            serde_json::to_string(blocks).map_err(|error| TranslationError::InvalidRequest {
                detail: format!("failed to serialize translation blocks: {error}"),
            })?;
        let mut params = BTreeMap::new();
        if let Some(source) = source {
            params.insert(PARAM_SOURCE_LANGUAGE.to_string(), source.to_string());
        }
        params.insert(PARAM_TARGET_LANGUAGE.to_string(), target.to_string());
        params.insert(PARAM_BLOCKS_JSON.to_string(), blocks_json);
        let media_types = [TRANSLATION_TEXT_MEDIA_TYPE.to_string()];
        let rows = self
            .execute_query_with_retry(query, &params, &media_types, units, metrics, false)
            .await?;
        normalize_translated_rows(blocks, rows, preserves_request_order)
    }

    /// Executes one query with bounded retries, budget reservation and metrics.
    ///
    /// # Arguments
    ///
    /// * `query` - Query name.
    /// * `params` - Normalized runtime parameters.
    /// * `media_types` - Media-type filter.
    /// * `units` - Provider units reserved for one attempt.
    /// * `metrics` - Counters updated for every attempt.
    /// * `detection` - Whether this query is a dedicated source-detection call.
    ///
    /// # Errors
    ///
    /// Returns the last normalized error after retries are exhausted, or
    /// immediately for non-retryable failures.
    async fn execute_query_with_retry(
        &self,
        query: &str,
        params: &BTreeMap<String, String>,
        media_types: &[String],
        units: u64,
        metrics: &mut TranslationMetrics,
        detection: bool,
    ) -> Result<Vec<QueryRow>, TranslationError> {
        let max_attempts = self.retry.max_attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            let reservation = self.budget.reserve(units).await?;
            let started = Instant::now();
            let result = tokio::time::timeout(
                Duration::from_secs(crate::DEFAULT_QUERY_TIMEOUT_SECONDS),
                self.port
                    .execute_translation_query(query, params, media_types),
            )
            .await
            .unwrap_or_else(|_| {
                Err(anyhow::Error::new(TranslationError::ProviderRejected {
                    category: crate::ProviderErrorCategory::Temporary,
                    detail: "provider query timed out".into(),
                    retry_after_seconds: None,
                }))
            });
            tracing::debug!("translation attempt service={} attempt={} detection={} units={} elapsed_ms={} success={}",
                self.service_id, attempt, detection, units, elapsed_ms(started), result.is_ok());
            reservation.release();
            metrics.attempts += 1;
            if detection {
                metrics.detection_attempts += 1;
            }
            match result {
                Ok(rows) => return Ok(rows),
                Err(error) => {
                    let error = query_error(&format!("{query} ({})", self.service_id), error);
                    tracing::warn!(
                        "translation query failed service={} attempt={} status={}",
                        self.service_id,
                        attempt,
                        error.http_status()
                    );
                    if attempt >= max_attempts || !error.is_retryable() {
                        return Err(error);
                    }
                    if error
                        .retry_after_seconds()
                        .is_some_and(|seconds| Duration::from_secs(seconds) > self.retry.max_delay)
                    {
                        return Err(error);
                    }
                    metrics.retries += 1;
                    let delay = self.retry.delay(attempt, error.retry_after_seconds());
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    /// Detects a source language through the dedicated metadata query.
    ///
    /// # Arguments
    ///
    /// * `query` - Dedicated detection query name.
    /// * `capability` - Text capability used to measure the sample.
    /// * `blocks` - Complete blocks from which a representative sample is built.
    /// * `target` - Requested target language.
    /// * `metrics` - Counters updated for the detection call.
    ///
    /// # Errors
    ///
    /// Returns an error when no sample can be built, the query fails, or the
    /// returned language is invalid or unsupported.
    async fn detect_source(
        &self,
        query: &str,
        capability: &TextTranslationCapability,
        blocks: &[TextBlock],
        target: &LanguageCode,
        metrics: &mut TranslationMetrics,
    ) -> Result<LanguageCode, TranslationError> {
        let sample = detection_sample(blocks, &capability.size_unit)?;
        let blocks_json =
            serde_json::to_string(std::slice::from_ref(&sample)).map_err(|error| {
                TranslationError::InvalidRequest {
                    detail: format!("failed to serialize detection sample: {error}"),
                }
            })?;
        let mut params = BTreeMap::new();
        params.insert(PARAM_BLOCKS_JSON.to_string(), blocks_json);
        params.insert(crate::PARAM_DETECTION_TEXT.into(), sample.text.clone());
        let media_types = [TRANSLATION_DETECTION_MEDIA_TYPE.to_string()];
        let units = capability.size_unit.measure(&sample.text);
        let rows = self
            .execute_query_with_retry(query, &params, &media_types, units, metrics, true)
            .await?;
        let row = rows
            .first()
            .ok_or_else(|| TranslationError::InvalidResponse {
                detail: "source detection returned no row".to_string(),
            })?;
        let value =
            serde_json::to_value(row).map_err(|error| TranslationError::InvalidResponse {
                detail: format!("failed to read source detection row: {error}"),
            })?;
        let raw = value
            .get("language")
            .or_else(|| value.get("detected_source_language"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| TranslationError::InvalidResponse {
                detail: "source detection row is missing a language field".to_string(),
            })?;
        let language =
            LanguageCode::parse(raw).map_err(|error| TranslationError::InvalidResponse {
                detail: format!("invalid detected source language: {error}"),
            })?;
        if &language != target {
            validate_detected_source(&self.metadata, &language, target)?;
        }
        Ok(language)
    }
}

#[async_trait]
impl<P: ScrapyfyQueryPort> TranslationService for ScrapyfyTranslationBridge<P> {
    /// Returns the stable service identifier.
    fn service_id(&self) -> &str {
        &self.service_id
    }

    /// Returns the validated metadata.
    fn metadata(&self) -> &TranslationMetadata {
        &self.metadata
    }

    /// Translates blocks by planning batches and executing the configured query.
    async fn translate_texts(
        &self,
        request: TextTranslationRequest,
    ) -> Result<TranslatedTexts, TranslationError> {
        let started = Instant::now();
        let mut metrics = TranslationMetrics::default();
        let capability = self.metadata.text_translation.as_ref().ok_or_else(|| {
            TranslationError::UnsupportedCapability {
                service_id: self.service_id.clone(),
                detail: "text translation is not declared".to_string(),
            }
        })?;
        if !self
            .metadata
            .supports_pair(request.source_language.as_ref(), &request.target_language)
        {
            return Err(TranslationError::UnsupportedLanguagePair {
                source_language: request.source_language,
                target: request.target_language,
            });
        }
        if request.source_language.is_none()
            && (!self.metadata.detection_available()
                || (self.metadata.supported_pairs.is_some()
                    && !self.metadata.detection_can_establish_source()))
        {
            return Err(TranslationError::SourceLanguageRequired {
                target: request.target_language,
            });
        }
        if (request.options.context.is_some() && !capability.context_support)
            || (request.options.preserve_markup && !capability.markup_support)
        {
            return Err(TranslationError::UnsupportedCapability {
                service_id: self.service_id.clone(),
                detail: "requested context or markup is not supported".into(),
            });
        }
        let target = request.target_language.clone();
        if request.source_language.as_ref() == Some(&target) {
            metrics.elapsed_ms = elapsed_ms(started);
            return Ok(TranslatedTexts {
                blocks: original_blocks(request.blocks),
                source_language: request.source_language,
                detected_source_language: None,
                metrics,
            });
        }

        let plan = self.plan_texts(&request, &self.local_limits)?;
        if !self
            .budget
            .available(self.planned_budget_units(&request, &plan)?)
        {
            return Err(TranslationError::BudgetExhausted {
                detail: "planned document exceeds available account budget".into(),
            });
        }
        let mut blocks = Vec::with_capacity(request.blocks.len());
        let mut detected = None;
        let mut effective_source = request.source_language.clone();
        let needs_stabilization = effective_source.is_none() && plan.batches.len() > 1;

        if effective_source.is_none()
            && (needs_stabilization
                || self.metadata.supported_pairs.is_some()
                || !self
                    .metadata
                    .source_detection
                    .as_ref()
                    .is_some_and(|d| d.returns_detected_language))
        {
            if let Some(query) = self
                .metadata
                .source_detection
                .as_ref()
                .and_then(|detection| detection.query.as_deref())
            {
                let language = self
                    .detect_source(query, capability, &request.blocks, &target, &mut metrics)
                    .await?;
                if language == target {
                    metrics.elapsed_ms = elapsed_ms(started);
                    return Ok(TranslatedTexts {
                        blocks: original_blocks(request.blocks),
                        source_language: Some(language.clone()),
                        detected_source_language: Some(language),
                        metrics,
                    });
                }
                detected = Some(language.clone());
                effective_source = Some(language);
            }
        }

        let mut first_auto_batch = needs_stabilization && effective_source.is_none();
        let target_str = target.as_str();
        for batch in plan.batches {
            let source = effective_source
                .as_ref()
                .map(LanguageCode::as_str)
                .or_else(|| (capability.auto_query.is_none()).then_some(AUTO_SOURCE_LANGUAGE));
            let query = if effective_source.is_some() {
                capability.query.as_str()
            } else {
                capability
                    .auto_query
                    .as_deref()
                    .unwrap_or(capability.query.as_str())
            };
            let (mut translated, batch_detected) = self
                .execute_text_batch(
                    query,
                    source,
                    target_str,
                    &batch.blocks,
                    batch.measurement.units,
                    capability.preserves_request_order,
                    &mut metrics,
                )
                .await?;
            metrics.batches += 1;
            metrics.blocks += batch.measurement.blocks;
            metrics.units += batch.measurement.units;

            if effective_source.is_none() {
                if let Some(batch_detected) = batch_detected {
                    if batch_detected == target {
                        metrics.elapsed_ms = elapsed_ms(started);
                        return Ok(TranslatedTexts {
                            blocks: original_blocks(request.blocks),
                            source_language: Some(batch_detected.clone()),
                            detected_source_language: Some(batch_detected),
                            metrics,
                        });
                    }
                    validate_detected_source(&self.metadata, &batch_detected, &target)?;
                    detected = Some(batch_detected.clone());
                    effective_source = Some(batch_detected);
                } else if first_auto_batch {
                    return Err(TranslationError::SourceLanguageRequired { target });
                }
            }
            first_auto_batch = false;
            blocks.append(&mut translated);
        }

        let source_language = request.source_language.or(effective_source);
        metrics.elapsed_ms = elapsed_ms(started);
        Ok(TranslatedTexts {
            blocks,
            source_language,
            detected_source_language: detected,
            metrics,
        })
    }

    /// Translates a URL document when the service declares that capability.
    async fn translate_url(
        &self,
        request: UrlTranslationRequest,
    ) -> Result<TranslatedDocument, TranslationError> {
        let started = Instant::now();
        let mut metrics = TranslationMetrics::default();
        let capability = self.metadata.url_translation.as_ref().ok_or_else(|| {
            TranslationError::UnsupportedCapability {
                service_id: self.service_id.clone(),
                detail: "URL translation is not declared".to_string(),
            }
        })?;
        if capability.result_mode != UrlResultMode::InlineDocument {
            return Err(TranslationError::UnsupportedCapability {
                service_id: self.service_id.clone(),
                detail: "URL result mode is not inline_document".to_string(),
            });
        }
        if let Some(media_type) = &request.media_type {
            if !capability.accepts_format(media_type) {
                return Err(TranslationError::UnsupportedCapability {
                    service_id: self.service_id.clone(),
                    detail: format!("URL translation does not accept {media_type}"),
                });
            }
        }
        if request.source_language.as_ref() == Some(&request.target_language) {
            return Err(TranslationError::InvalidRequest {
                detail: "source and target languages are identical".to_string(),
            });
        }
        if !self
            .metadata
            .supports_pair(request.source_language.as_ref(), &request.target_language)
        {
            return Err(TranslationError::UnsupportedLanguagePair {
                source_language: request.source_language,
                target: request.target_language,
            });
        }
        if request.source_language.is_none()
            && (!self.metadata.detection_available()
                || (self.metadata.supported_pairs.is_some()
                    && !self.metadata.detection_can_establish_source()))
        {
            return Err(TranslationError::SourceLanguageRequired {
                target: request.target_language,
            });
        }

        let source = request
            .source_language
            .as_ref()
            .map(LanguageCode::as_str)
            .unwrap_or(AUTO_SOURCE_LANGUAGE)
            .to_string();
        let mut params = BTreeMap::new();
        params.insert(PARAM_SOURCE_LANGUAGE.to_string(), source);
        params.insert(
            PARAM_TARGET_LANGUAGE.to_string(),
            request.target_language.to_string(),
        );
        params.insert(PARAM_DOCUMENT_URL.to_string(), request.source_url.clone());
        let media_types = [TRANSLATION_DOCUMENT_MEDIA_TYPE.to_string()];
        let rows = self
            .execute_query_with_retry(
                &capability.query,
                &params,
                &media_types,
                0,
                &mut metrics,
                false,
            )
            .await?;
        metrics.batches = 1;
        let row = rows
            .first()
            .ok_or_else(|| TranslationError::InvalidResponse {
                detail: "URL translation returned no document row".to_string(),
            })?;
        let value =
            serde_json::to_value(row).map_err(|error| TranslationError::InvalidResponse {
                detail: format!("failed to read URL translation row: {error}"),
            })?;
        let content = string_field(&value, "content")?;
        let content_type = string_field(&value, "content_type")?;
        let detected_source_language = value
            .get("detected_source_language")
            .and_then(serde_json::Value::as_str)
            .map(LanguageCode::parse)
            .transpose()
            .map_err(|error| TranslationError::InvalidResponse {
                detail: format!("invalid detected source language: {error}"),
            })?;
        metrics.elapsed_ms = elapsed_ms(started);
        Ok(TranslatedDocument {
            bytes: content.as_bytes().to_vec(),
            content_type,
            detected_source_language,
            metrics,
        })
    }
}

/// Converts one anyhow query failure while preserving an already normalized error.
///
/// # Arguments
///
/// * `query` - Query name used in the contextual fallback.
/// * `error` - Raw Scrapyfy port error.
fn query_error(query: &str, error: anyhow::Error) -> TranslationError {
    let error = classify_provider_error(error);
    match error.downcast::<TranslationError>() {
        Ok(error) => error,
        Err(_error) => TranslationError::QueryFailed {
            query: query.to_string(),
            detail: "query execution failed; provider payload omitted".to_string(),
        },
    }
}

/// Returns bounded elapsed milliseconds.
///
/// # Arguments
///
/// * `started` - Execution start instant.
fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Converts submitted blocks into the normalized result without translation.
///
/// # Arguments
///
/// * `blocks` - Original blocks, consumed in input order.
fn original_blocks(blocks: Vec<TextBlock>) -> Vec<TranslatedBlock> {
    blocks
        .into_iter()
        .map(|block| TranslatedBlock {
            id: block.id,
            text: block.text,
        })
        .collect()
}

/// Builds a bounded representative sample for a dedicated detection query.
///
/// # Arguments
///
/// * `blocks` - Complete request blocks.
///
/// # Errors
///
/// Returns [`TranslationError::InvalidRequest`] when no nonblank sample exists.
fn detection_sample(
    blocks: &[TextBlock],
    size_unit: &SizeUnit,
) -> Result<TextBlock, TranslationError> {
    let limit = DEFAULT_DETECTION_SAMPLE_UNITS;
    let mut sample = String::new();
    'blocks: for block in blocks {
        for character in block.text.chars() {
            sample.push(character);
            if size_unit.measure(&sample) > limit {
                sample.pop();
                break 'blocks;
            }
        }
        if !sample.is_empty() && !sample.ends_with('\n') {
            sample.push('\n');
            if size_unit.measure(&sample) > limit {
                sample.pop();
                break;
            }
        }
    }
    let sample = sample.trim_end().to_string();
    if sample.is_empty() {
        return Err(TranslationError::InvalidRequest {
            detail: "source detection requires a nonblank sample".to_string(),
        });
    }
    TextBlock::new(BlockId::new("detection-sample")?, sample)
}

/// Validates a provider-detected source against declared metadata.
///
/// # Arguments
///
/// * `metadata` - Service metadata.
/// * `language` - Detected source language.
/// * `target` - Requested target language.
///
/// # Errors
///
/// Returns [`TranslationError::UnsupportedLanguagePair`] when the detected
/// source is outside the declared source list or directional pairs.
fn validate_detected_source(
    metadata: &TranslationMetadata,
    language: &LanguageCode,
    target: &LanguageCode,
) -> Result<(), TranslationError> {
    if !metadata.supports_pair(Some(language), target) {
        return Err(TranslationError::UnsupportedLanguagePair {
            source_language: Some(language.clone()),
            target: target.clone(),
        });
    }
    Ok(())
}

/// Extracts the normalized metadata value from one query row.
///
/// # Arguments
///
/// * `row` - Row returned by `get_translation_metadata`.
///
/// # Errors
///
/// Returns an error when the row cannot be serialized or contains no value.
fn metadata_value(row: &QueryRow) -> Result<serde_json::Value, TranslationError> {
    let node = row
        .get("translation")
        .or_else(|| (row.len() == 1).then(|| row.values().next()).flatten())
        .ok_or_else(|| TranslationError::MetadataUnavailable {
            detail: "metadata row does not contain a translation object".to_string(),
        })?;
    serde_json::to_value(node).map_err(|error| TranslationError::MetadataUnavailable {
        detail: format!("failed to read translation metadata: {error}"),
    })
}

mod planning;
mod response;
use response::{normalize_translated_rows, string_field};

/// Maps a Scrapyfy HTTP status failure to a normalized provider error.
///
/// The chain is inspected because Scrapyfy adds URL context around transport
/// errors. Non-HTTP failures are returned unchanged.
///
/// # Arguments
///
/// * `error` - Raw Scrapyfy query failure.
fn classify_provider_error(error: anyhow::Error) -> anyhow::Error {
    for cause in error.chain() {
        if cause.downcast_ref::<serde_json::Error>().is_some() {
            return anyhow::Error::new(TranslationError::InvalidResponse {
                detail: "provider response is not valid JSON".into(),
            });
        }
        let Some(http_error) = cause.downcast_ref::<ArachneaHttpError>() else {
            continue;
        };
        let ArachneaHttpError::HttpStatus {
            status,
            retry_after_seconds,
        } = http_error
        else {
            continue;
        };
        let category = match status.as_u16() {
            401 | 403 => ProviderErrorCategory::Authentication,
            408 | 425 | 500 | 502 | 503 | 504 => ProviderErrorCategory::Temporary,
            413 => ProviderErrorCategory::Size,
            429 => ProviderErrorCategory::Quota,
            400 | 404 | 415 | 422 => ProviderErrorCategory::Request,
            _ => ProviderErrorCategory::Temporary,
        };
        return anyhow::Error::new(TranslationError::ProviderRejected {
            category,
            detail: format!("provider returned HTTP status {status}"),
            retry_after_seconds: *retry_after_seconds,
        });
    }
    error
}
