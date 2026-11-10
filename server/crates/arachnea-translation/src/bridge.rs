//! Generic bridge executing normalized translation queries through Scrapyfy.

use crate::block::{
    TextBlock, TextTranslationRequest, TranslatedBlock, TranslatedDocument, TranslatedTexts,
    UrlTranslationRequest,
};
use crate::capability::UrlResultMode;
use crate::error::TranslationError;
use crate::language::LanguageCode;
use crate::metadata::TranslationMetadata;
use crate::planning::{plan_batches, LocalTranslationLimits};
use crate::service::TranslationService;
use crate::{
    AUTO_SOURCE_LANGUAGE, GET_TRANSLATION_METADATA_QUERY, PARAM_BLOCKS_JSON, PARAM_DOCUMENT_URL,
    PARAM_SOURCE_LANGUAGE, PARAM_TARGET_LANGUAGE, TRANSLATION_DOCUMENT_MEDIA_TYPE,
    TRANSLATION_METADATA_MEDIA_TYPE, TRANSLATION_TEXT_MEDIA_TYPE,
};
use arachnea_scrapyfy::{ScraperDataNode, ScraperQueryCollection};
use async_trait::async_trait;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// One normalized Scrapyfy query row.
pub type QueryRow = HashMap<String, ScraperDataNode>;

/// Outbound port used to execute Scrapyfy translation queries.
///
/// Stream implements this port over its service catalog. Keeping the port
/// generic avoids a dependency from this crate to Stream and makes request
/// measurement and test execution replaceable.
#[async_trait]
pub trait ScrapyfyQueryPort: Send + Sync {
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

/// Generic translation service executing YAML queries through Scrapyfy.
pub struct ScrapyfyTranslationBridge<P> {
    port: P,
    service_id: String,
    metadata: TranslationMetadata,
    configuration_revision: String,
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
            .map_err(|error| TranslationError::QueryFailed {
                query: format!("{GET_TRANSLATION_METADATA_QUERY} ({service_id})"),
                detail: error.to_string(),
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
        })
    }

    /// Returns the provider configuration revision recorded in cache identities.
    pub fn configuration_revision(&self) -> &str {
        &self.configuration_revision
    }

    /// Executes one normalized text query batch.
    ///
    /// # Arguments
    ///
    /// * `query` - Query name from the text capability.
    /// * `source` - Normalized source parameter, including the automatic marker.
    /// * `target` - Normalized target language.
    /// * `blocks` - Blocks in this batch.
    ///
    /// # Errors
    ///
    /// Returns a normalized error when the query fails or its rows violate the
    /// input/output correspondence.
    async fn execute_text_batch(
        &self,
        query: &str,
        source: &str,
        target: &str,
        blocks: &[TextBlock],
        preserves_request_order: bool,
    ) -> Result<(Vec<TranslatedBlock>, Option<LanguageCode>), TranslationError> {
        let blocks_json =
            serde_json::to_string(blocks).map_err(|error| TranslationError::InvalidRequest {
                detail: format!("failed to serialize translation blocks: {error}"),
            })?;
        let mut params = BTreeMap::new();
        params.insert(PARAM_SOURCE_LANGUAGE.to_string(), source.to_string());
        params.insert(PARAM_TARGET_LANGUAGE.to_string(), target.to_string());
        params.insert(PARAM_BLOCKS_JSON.to_string(), blocks_json);
        let media_types = [TRANSLATION_TEXT_MEDIA_TYPE.to_string()];
        let rows = self
            .port
            .execute_translation_query(query, &params, &media_types)
            .await
            .map_err(|error| TranslationError::QueryFailed {
                query: format!("{query} ({})", self.service_id),
                detail: error.to_string(),
            })?;
        normalize_translated_rows(blocks, rows, preserves_request_order)
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
        if request.source_language.as_ref() == Some(&request.target_language) {
            return Ok(TranslatedTexts {
                blocks: request
                    .blocks
                    .into_iter()
                    .map(|block| TranslatedBlock {
                        id: block.id,
                        text: block.text,
                    })
                    .collect(),
                source_language: request.source_language,
                detected_source_language: None,
            });
        }

        let source = request
            .source_language
            .as_ref()
            .map(LanguageCode::as_str)
            .unwrap_or(AUTO_SOURCE_LANGUAGE)
            .to_string();
        let target = request.target_language.to_string();
        let plan = plan_batches(
            &request.blocks,
            capability,
            &LocalTranslationLimits::default(),
        )?;
        let mut blocks = Vec::with_capacity(request.blocks.len());
        let mut detected = None;
        for batch in plan.batches {
            let (mut translated, batch_detected) = self
                .execute_text_batch(
                    &capability.query,
                    &source,
                    &target,
                    &batch.blocks,
                    capability.preserves_request_order,
                )
                .await?;
            if detected.is_none() {
                detected = batch_detected;
            }
            blocks.append(&mut translated);
        }
        Ok(TranslatedTexts {
            blocks,
            source_language: request.source_language,
            detected_source_language: detected,
        })
    }

    /// Translates a URL document when the service declares that capability.
    async fn translate_url(
        &self,
        request: UrlTranslationRequest,
    ) -> Result<TranslatedDocument, TranslationError> {
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
            .port
            .execute_translation_query(&capability.query, &params, &media_types)
            .await
            .map_err(|error| TranslationError::QueryFailed {
                query: format!("{} ({})", capability.query, self.service_id),
                detail: error.to_string(),
            })?;
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
        Ok(TranslatedDocument {
            bytes: content.as_bytes().to_vec(),
            content_type,
            detected_source_language,
        })
    }
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

/// Validates that translated rows match the submitted block identifiers.
///
/// # Arguments
///
/// * `expected` - Blocks submitted in one batch.
/// * `rows` - Normalized rows returned by the provider query.
///
/// # Errors
///
/// Returns [`TranslationError::InvalidResponse`] for a cardinality mismatch,
/// a missing or duplicated identifier, or a missing text field.
fn normalize_translated_rows(
    expected: &[TextBlock],
    rows: Vec<QueryRow>,
    preserves_request_order: bool,
) -> Result<(Vec<TranslatedBlock>, Option<LanguageCode>), TranslationError> {
    if rows.len() != expected.len() {
        return Err(TranslationError::InvalidResponse {
            detail: format!(
                "expected {} translated blocks, received {}",
                expected.len(),
                rows.len()
            ),
        });
    }
    let positional = preserves_request_order && rows.iter().all(|row| !row.contains_key("id"));
    let mut by_id = HashMap::with_capacity(rows.len());
    let mut positional_blocks = Vec::with_capacity(rows.len());
    let mut detected = None;
    for (index, row) in rows.into_iter().enumerate() {
        let value =
            serde_json::to_value(&row).map_err(|error| TranslationError::InvalidResponse {
                detail: format!("failed to read translated block: {error}"),
            })?;
        if detected.is_none() {
            detected = value
                .get("detected_source_language")
                .and_then(serde_json::Value::as_str)
                .map(LanguageCode::parse)
                .transpose()
                .map_err(|error| TranslationError::InvalidResponse {
                    detail: format!("invalid detected source language: {error}"),
                })?;
        }
        let text = string_field(&value, "text")?;
        if positional {
            positional_blocks.push(TranslatedBlock {
                id: expected[index].id.clone(),
                text,
            });
        } else {
            let id = string_field(&value, "id")?;
            if by_id.insert(id.clone(), text).is_some() {
                return Err(TranslationError::InvalidResponse {
                    detail: format!("duplicate translated block identifier {id}"),
                });
            }
        }
    }
    if positional {
        return Ok((positional_blocks, detected));
    }

    let mut translated = Vec::with_capacity(expected.len());
    for block in expected {
        let text =
            by_id
                .remove(block.id.as_str())
                .ok_or_else(|| TranslationError::InvalidResponse {
                    detail: format!("missing translated block {}", block.id.as_str()),
                })?;
        translated.push(TranslatedBlock {
            id: block.id.clone(),
            text,
        });
    }
    if !by_id.is_empty() {
        let mut extra = by_id.into_keys().collect::<Vec<_>>();
        extra.sort();
        return Err(TranslationError::InvalidResponse {
            detail: format!(
                "unexpected translated block identifiers: {}",
                extra.join(", ")
            ),
        });
    }
    Ok((translated, detected))
}

/// Reads one required string field from a serialized query row.
///
/// # Arguments
///
/// * `value` - Serialized row object.
/// * `field` - Required field name.
///
/// # Errors
///
/// Returns [`TranslationError::InvalidResponse`] when the object or string is
/// missing.
fn string_field(value: &serde_json::Value, field: &str) -> Result<String, TranslationError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| TranslationError::InvalidResponse {
            detail: format!("normalized response is missing string field {field}"),
        })
}
