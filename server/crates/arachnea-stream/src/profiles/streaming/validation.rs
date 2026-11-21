//! Version-one streaming document validation matching the public frontend contracts.

use crate::profiles::accounts::{for_field, reject};
use anyhow::Result;
use serde_json::Value;
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

/// Builds a field-specific safe rejection without exposing storage diagnostics.
fn invalid(field: &'static str) -> anyhow::Error {
    for_field(
        field,
        reject("invalid_data", 422, "Invalid streaming document."),
    )
}

/// Checks exact object fields, including required nullable fields.
fn fields(v: &Value, required: &[&str], optional: &[&str]) -> bool {
    v.as_object().is_some_and(|m| {
        required.iter().all(|k| m.contains_key(*k))
            && m.keys()
                .all(|k| required.contains(&k.as_str()) || optional.contains(&k.as_str()))
    })
}
fn text(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.trim().is_empty())
}
fn identifier(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && s.trim() == s && !s.chars().any(char::is_control))
}
fn nullable(v: &Value) -> bool {
    v.is_null() || text(v)
}
fn choice(v: &Value, choices: &[&str]) -> bool {
    v.as_str().is_some_and(|s| choices.contains(&s))
}

/// Validates a non-negative JSON-safe integer.
///
/// # Arguments
/// * `value` - Untrusted document number.
pub(super) fn integer(value: &Value) -> bool {
    value
        .as_i64()
        .is_some_and(|n| (0..=crate::PROFILE_MAX_REVISION).contains(&n))
}

/// Validates explicit section display options and order, refusing unknown fields.
///
/// # Arguments
/// * `value` - SectionRecord content without server metadata.
pub(super) fn section(value: &Value) -> Result<()> {
    if !fields(
        value,
        &[
            "section_key",
            "order",
            "collectionMode",
            "thumbnailOrientation",
            "thumbnailImageFit",
        ],
        &[],
    ) || !identifier(&value["section_key"])
        || !integer(&value["order"])
        || !display(value)
    {
        return Err(invalid("value"));
    }
    Ok(())
}
fn display(v: &Value) -> bool {
    [
        ("collectionMode", &["grid", "single-row", "list"][..]),
        ("thumbnailOrientation", &["portrait", "landscape"][..]),
        ("thumbnailImageFit", &["contain", "cover"][..]),
    ]
    .iter()
    .all(|(key, choices)| v.get(*key).is_none_or(|v| choice(v, choices)))
}
/// Checks a source-owned category reference, including its optional display title override.
fn category(v: &Value) -> bool {
    if !fields(
        v,
        &["sourceId", "categoryKey", "descriptor", "labelSnapshot"],
        &["ancestorKeys", "titleOverride"],
    ) || !identifier(&v["sourceId"])
        || !identifier(&v["categoryKey"])
        || !text(&v["labelSnapshot"])
        || !v.get("titleOverride").is_none_or(nullable)
        || v.get("ancestorKeys").is_some_and(|keys| {
            keys.as_array().is_none_or(|keys| {
                keys.len() > crate::CUSTOM_HOME_MAX_CATEGORY_ANCESTORS
                    || !keys.iter().all(identifier)
            })
        })
    {
        return false;
    }
    let Some(descriptor) = v["descriptor"].as_object() else {
        return false;
    };
    let forbidden =
        regex::Regex::new(r"(?i)authorization|cookie|password|secret|token|api[_-]?key").unwrap();
    let credentials = regex::Regex::new(
        r"(?i)(?:[?&](?:access_token|token|api_key|password|secret)=)|(?:https?://[^/]*@)",
    )
    .unwrap();
    descriptor.get("name") == Some(&v["sourceId"])
        && descriptor.get("source").is_none_or(|s| s == &v["sourceId"])
        && descriptor.iter().all(|(k, v)| {
            !["__proto__", "constructor", "prototype"].contains(&k.as_str())
                && !forbidden.is_match(k)
                && v.as_str().is_some_and(|s| !credentials.is_match(s))
        })
}
fn section_reference(v: &Value) -> bool {
    fields(
        v,
        &["sourceId", "sectionKey", "labelSnapshot"],
        &["subsectionKey", "category"],
    ) && identifier(&v["sourceId"])
        && identifier(&v["sectionKey"])
        && nullable(&v["labelSnapshot"])
        && v.get("subsectionKey").is_none_or(identifier)
        && v.get("category")
            .is_none_or(|owner| category(owner) && owner["sourceId"] == v["sourceId"])
}

/// Validates a complete custom home without changing references, blocks or display settings.
///
/// # Arguments
/// * `v` - Version-one home definition; creation may omit its server-assigned ID.
pub(super) fn home(v: &Value) -> Result<()> {
    if v["schemaVersion"] != crate::PROFILE_DOCUMENT_VERSION {
        return Err(for_field(
            "value.definition.schemaVersion",
            reject(
                "unsupported_schema_version",
                422,
                "Unsupported custom home version.",
            ),
        ));
    }
    if !fields(
        v,
        &[
            "schemaVersion",
            "name",
            "navigationOrder",
            "bannerSources",
            "categoryShortcuts",
            "blocks",
            "sectionMergeMode",
            "showBookmarks",
            "createdAt",
            "updatedAt",
        ],
        &["id"],
    ) || !text(&v["name"])
        || !integer(&v["navigationOrder"])
        || !integer(&v["createdAt"])
        || !integer(&v["updatedAt"])
        || !v["showBookmarks"].is_boolean()
        || !choice(&v["sectionMergeMode"], &["none", "title"])
        || v.get("id").is_some_and(|id| {
            id.as_str()
                .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
        })
    {
        return Err(invalid("value.definition"));
    }
    let Some(banners) = v["bannerSources"].as_array() else {
        return Err(invalid("value.definition.bannerSources"));
    };
    let mut banner_keys = HashSet::new();
    for banner in banners {
        let key = if identifier(banner) {
            serde_json::json!(banner)
        } else if category(banner) {
            serde_json::json!([
                banner["sourceId"],
                banner["ancestorKeys"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
                banner["categoryKey"],
            ])
        } else {
            return Err(invalid("value.definition.bannerSources"));
        };
        if !banner_keys.insert(key.to_string()) {
            return Err(invalid("value.definition.bannerSources"));
        }
    }
    if v["categoryShortcuts"]
        .as_array()
        .is_none_or(|a| a.iter().any(|v| !category(v)))
    {
        return Err(invalid("value.definition.categoryShortcuts"));
    }
    let Some(blocks) = v["blocks"].as_array() else {
        return Err(invalid("value.definition.blocks"));
    };
    let mut ids = HashSet::new();
    for block in blocks {
        if !fields(
            block,
            &["id", "kind", "reference", "titleOverride"],
            &["displayOptions"],
        ) || !identifier(&block["id"])
            || !ids.insert(block["id"].as_str().unwrap())
            || !nullable(&block["titleOverride"])
            || block.get("displayOptions").is_some_and(|v| {
                !fields(
                    v,
                    &[],
                    &[
                        "collectionMode",
                        "thumbnailOrientation",
                        "thumbnailImageFit",
                    ],
                ) || !display(v)
            })
            || match block["kind"].as_str() {
                Some("home-section") => !section_reference(&block["reference"]),
                Some("category-content") => !category(&block["reference"]),
                _ => true,
            }
        {
            return Err(invalid("value.definition.blocks"));
        }
    }
    if slug(v["name"].as_str().unwrap()).is_empty() {
        return Err(invalid("value.definition.name"));
    }
    Ok(())
}

/// Returns the frontend-compatible URL uniqueness key for a home name.
///
/// # Arguments
/// * `name` - Home display name, retaining Unicode letters and numbers.
pub(super) fn slug(name: &str) -> String {
    let decomposed: String = name.nfkd().collect();
    let marks = regex::Regex::new(r"\p{M}").unwrap();
    let lowered = marks.replace_all(&decomposed, "").to_lowercase();
    let separators = regex::Regex::new(r"[^\p{L}\p{N}]+").unwrap();
    separators
        .replace_all(&lowered, "_")
        .trim_matches('_')
        .to_string()
}

/// Validates a complete bookmark replacement, including zero playback positions.
///
/// # Arguments
/// * `value` - Exact EntryBookmark document, without ownership/identity metadata.
pub(super) fn bookmark(value: &Value) -> Result<()> {
    let strings = [
        "groupId",
        "selectedItemId",
        "title",
        "alternativeTitleLabel",
        "imagePosterUrl",
        "imageLandscapeUrl",
        "imageUrl",
        "description",
        "mediaTypeLabel",
        "seasonLabel",
    ];
    let mut keys = strings.to_vec();
    keys.extend(["playbackTime", "episodeNumber", "isFullyWatched"]);
    if !fields(value, &keys, &[])
        || strings.iter().any(|k| !nullable(&value[*k]))
        || !value["isFullyWatched"].is_boolean()
        || !(value["playbackTime"].is_null()
            || value["playbackTime"]
                .as_f64()
                .is_some_and(|n| n.is_finite() && n >= 0.0))
        || !(value["episodeNumber"].is_null()
            || (integer(&value["episodeNumber"]) && value["episodeNumber"].as_i64().unwrap() > 0))
    {
        return Err(invalid("value.bookmark"));
    }
    Ok(())
}
