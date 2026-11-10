//! Normalized provider response validation.

use super::*;

/// Validates that translated rows match the submitted block identifiers.
///
/// # Arguments
///
/// * `expected` - Blocks submitted in one batch.
/// * `rows` - Normalized rows returned by the provider query.
/// * `preserves_request_order` - Whether positional matching is declared.
///
/// # Errors
///
/// Returns [`TranslationError::InvalidResponse`] for a cardinality mismatch,
/// a missing or duplicated identifier, blank text, or inconsistent detection.
pub(super) fn normalize_translated_rows(
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
        if let Some(raw) = value
            .get("detected_source_language")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
        {
            let language =
                LanguageCode::parse(raw).map_err(|_| TranslationError::InvalidResponse {
                    detail: "invalid detected source language".into(),
                })?;
            if detected
                .as_ref()
                .is_some_and(|previous| previous != &language)
            {
                return Err(TranslationError::InvalidResponse {
                    detail: "inconsistent detected source languages in one batch".into(),
                });
            }
            detected = Some(language);
        }
        let text = string_field(&value, "text")?;
        if text.trim().is_empty() {
            return Err(TranslationError::InvalidResponse {
                detail: "translated block text is blank".into(),
            });
        }
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
pub(super) fn string_field(
    value: &serde_json::Value,
    field: &str,
) -> Result<String, TranslationError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| TranslationError::InvalidResponse {
            detail: format!("normalized response is missing string field {field}"),
        })
}
