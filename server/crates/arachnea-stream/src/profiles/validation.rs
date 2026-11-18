//! Strict profile API validation; remote values are never silently sanitized.

use super::{
    accounts::{for_field, reject},
    common::models::CommonPreferences,
};
use anyhow::Result;
use serde_json::{json, Value};
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

/// Normalizes a display name and its Unicode default-case-folded uniqueness key.
///
/// # Arguments
/// * `value` - Client-supplied profile name.
pub(super) fn name(value: &str) -> Result<(String, String)> {
    let name: String = value.trim().nfc().collect();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(for_field(
            "name",
            reject(
                "invalid_data",
                422,
                "Profile name must contain 1 to 64 characters without controls.",
            ),
        ));
    }
    let key = name.case_fold().nfc().collect();
    Ok((name, key))
}

/// Validates common preferences against the public frontend's locale/theme catalogue.
///
/// # Arguments
/// * `values` - Complete common settings replacement.
pub(super) fn common(values: &CommonPreferences) -> Result<()> {
    if values
        .language
        .as_ref()
        .is_some_and(|l| !crate::PROFILE_UI_LANGUAGES.contains(&l.as_str()))
    {
        return Err(for_field(
            "common.values.language",
            reject("invalid_data", 422, "Unsupported interface language."),
        ));
    }
    if !crate::PROFILE_THEME_KEYS.contains(&values.theme.as_str()) {
        return Err(for_field(
            "common.values.theme",
            reject("invalid_data", 422, "Unsupported theme."),
        ));
    }
    Ok(())
}

/// Validates or initializes a complete streaming document, retaining literal frontend keys.
///
/// # Arguments
/// * `input` - Client values; initialization may omit fields for server defaults.
/// * `initialize` - Whether missing values receive explicit server defaults.
pub(super) fn streaming(mut input: Value, initialize: bool) -> Result<Value> {
    normalize_languages(&mut input)?;
    let defaults: Value =
        serde_json::from_str(include_str!("../../resources/profile-stream-defaults.json"))?;
    let source = input.as_object().ok_or_else(invalid)?;
    let mut result = if initialize {
        defaults.as_object().unwrap().clone()
    } else {
        serde_json::Map::new()
    };
    if source.keys().any(|key| defaults.get(key).is_none()) {
        return Err(invalid());
    }
    for (key, value) in source {
        result.insert(key.clone(), value.clone());
    }
    if !initialize && result.len() != defaults.as_object().unwrap().len() {
        return Err(invalid());
    }
    for (key, value) in &result {
        let valid = match key.as_str() {
            "defaultHomeId" => {
                value.is_null()
                    || value
                        .as_str()
                        .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
            }
            "thumbnailOrientation" => matches!(value.as_str(), Some("portrait" | "landscape")),
            "thumbnailImageFit" | "backgroundImageFit" => {
                matches!(value.as_str(), Some("contain" | "cover"))
            }
            "collectionMode" => matches!(value.as_str(), Some("grid" | "single-row" | "list")),
            "securityMode" => matches!(value.as_str(), Some("safe" | "confirmation" | "unsafe")),
            "thumbnailSizeMultiplier" => value
                .as_f64()
                .is_some_and(|n| n.is_finite() && (0.5..=1.5).contains(&n)),
            "videoPlayer.subtitleLanguagePriority" | "videoPlayer.subtitleLanguages" => {
                languages(value)
            }
            "videoPlayer.subtitleTranslationByLanguage" => value
                .as_object()
                .is_some_and(|m| m.values().all(Value::is_boolean)),
            "videoPlayer.trackProfiles" => value.as_array().is_some_and(|a| {
                !a.is_empty()
                    && a.len() <= crate::PROFILE_MAX_TRACK_PROFILES
                    && a.iter().all(|v| {
                        v.as_object().is_some_and(|m| {
                            m.len() == 2
                                && (v["audio"].is_null()
                                    || v["audio"] == "original"
                                    || language(&v["audio"]))
                                && matches!(v["subtitles"].as_str(), Some("auto" | "disabled"))
                        })
                    })
            }),
            "arachnea.videoPlayer.preferences.quality" => nullable_string(value),
            "arachnea.videoPlayer.preferences.audioTrack" => value.is_null() || track(value, false),
            "arachnea.videoPlayer.preferences.textTrack" => track(value, true),
            "arachnea.videoPlayer.preferences.textTrackSettings" => {
                value.is_null()
                    || value.as_object().is_some_and(|m| {
                        m.iter().all(|(k, v)| {
                            crate::PROFILE_TEXT_TRACK_KEYS.contains(&k.as_str())
                                && (v.is_number()
                                    || v.as_str().is_some_and(|s| !s.trim().is_empty()))
                        })
                    })
            }
            _ => value.is_boolean(),
        };
        if !valid {
            return Err(invalid());
        }
    }
    let priority = "videoPlayer.subtitleLanguagePriority";
    let alias = "videoPlayer.subtitleLanguages";
    if source.contains_key(priority)
        && source.contains_key(alias)
        && source[priority] != source[alias]
    {
        return Err(invalid());
    }
    if source.contains_key(alias) && !source.contains_key(priority) {
        result.insert(priority.into(), source[alias].clone());
    }
    result.insert(alias.into(), result[priority].clone());
    if result["videoPlayer.subtitleTranslationByLanguage"]
        .as_object()
        .unwrap()
        .keys()
        .any(|key| !result[priority].as_array().unwrap().contains(&json!(key)))
    {
        return Err(invalid());
    }
    Ok(Value::Object(result))
}

fn invalid() -> anyhow::Error {
    reject("invalid_data", 422, "Invalid streaming preferences.")
}
fn nullable_string(v: &Value) -> bool {
    v.is_null() || v.as_str().is_some_and(|s| !s.trim().is_empty())
}
fn language(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        let mut parts = s.split('-');
        let base = parts.next().unwrap_or("");
        (2..=3).contains(&base.len())
            && base.bytes().all(|b| b.is_ascii_lowercase())
            && parts.all(|part| {
                !part.is_empty()
                    && part.len() <= 8
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
    })
}

/// Applies the frontend's explicit ISO aliases and BCP-47 casing before strict validation.
fn normalize_languages(input: &mut Value) -> Result<()> {
    fn code(value: &mut Value) -> Result<()> {
        let raw = value.as_str().ok_or_else(invalid)?;
        let normalized = raw.trim().to_lowercase().replace('_', "-");
        let (base, suffix) = normalized
            .split_once('-')
            .map(|(a, b)| (a, format!("-{b}")))
            .unwrap_or((&normalized, String::new()));
        let aliases: std::collections::HashMap<String, String> = serde_json::from_str(
            include_str!("../../resources/profile-language-aliases.json"),
        )?;
        *value = json!(format!(
            "{}{suffix}",
            aliases.get(base).map(String::as_str).unwrap_or(base)
        ));
        if !language(value) {
            return Err(invalid());
        }
        Ok(())
    }
    for key in [
        "videoPlayer.subtitleLanguagePriority",
        "videoPlayer.subtitleLanguages",
    ] {
        if let Some(values) = input.get_mut(key).and_then(Value::as_array_mut) {
            for value in values {
                code(value)?;
            }
        }
    }
    if let Some(profiles) = input
        .get_mut("videoPlayer.trackProfiles")
        .and_then(Value::as_array_mut)
    {
        for profile in profiles {
            if let Some(audio) = profile
                .get_mut("audio")
                .filter(|v| !v.is_null() && **v != "original")
            {
                code(audio)?;
            }
        }
    }
    for key in [
        "arachnea.videoPlayer.preferences.audioTrack",
        "arachnea.videoPlayer.preferences.textTrack",
    ] {
        if let Some(value) = input
            .get_mut(key)
            .and_then(|v| v.get_mut("language"))
            .filter(|v| !v.is_null())
        {
            code(value)?;
        }
    }
    if let Some(map) = input
        .get_mut("videoPlayer.subtitleTranslationByLanguage")
        .and_then(Value::as_object_mut)
    {
        let mut normalized = serde_json::Map::new();
        for (key, value) in std::mem::take(map) {
            let mut key = json!(key);
            code(&mut key)?;
            if normalized
                .insert(key.as_str().unwrap().into(), value)
                .is_some()
            {
                return Err(invalid());
            }
        }
        *map = normalized;
    }
    Ok(())
}

fn languages(v: &Value) -> bool {
    v.as_array().is_some_and(|a| {
        a.len() <= 10
            && a.iter()
                .enumerate()
                .all(|(i, v)| language(v) && !a[..i].contains(v))
    })
}
fn track(v: &Value, text: bool) -> bool {
    v.as_object().is_some_and(|m| {
        m.keys().all(|k| {
            matches!(
                k.as_str(),
                "id" | "language" | "label" | "kind" | "origin" | "autoTranslate"
            ) || (text && k == "mode")
        }) && ["id", "language", "label", "kind"]
            .iter()
            .all(|key| m.get(*key).is_some_and(nullable_string))
            && m.get("origin")
                .is_none_or(|v| matches!(v.as_str(), Some("manual" | "profile")))
            && m.get("autoTranslate").is_none_or(Value::is_boolean)
            && (!text || matches!(v["mode"].as_str(), Some("showing" | "disabled")))
    })
}
