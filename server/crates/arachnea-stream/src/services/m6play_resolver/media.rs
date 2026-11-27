//! M6 replay metadata, manifest selection, sprite thumbnails and chapters.

use anyhow::{Context, Result};
use arachnea_scrapyfy::HttpClient;
use serde_json::Value;
use std::collections::HashMap;

use super::{
    SIXPLAY_CUSTOMER_NAME, SIXPLAY_FRANCE_AREA_COUNTRIES, SIXPLAY_FRANCE_AREA_ID,
    SIXPLAY_STORYBOARD_COLUMNS, SIXPLAY_STORYBOARD_HEIGHT, SIXPLAY_STORYBOARD_ROWS,
    SIXPLAY_STORYBOARD_WIDTH, SIXPLAY_VIDEO_JSON_URL_TEMPLATE, SIXPLAY_WORLDWIDE_AREA_ID,
};
use crate::services::player_resolver::{Chapter, SpriteThumbnail};

/// Reads geographic routing hints from the selected clip or live metadata.
///
/// # Arguments
/// * `media` - Selected media object whose `areas` contain M6 zone identifiers.
/// * `requested_countries` - Explicit caller hints, which take precedence when nonempty.
///
/// # Returns
/// Caller hints, or the countries for area 11. Area 34 takes precedence over
/// area 11 and requires no geographic routing. Missing or unknown areas also
/// leave routing unrestricted, matching the catalogue YAML mapping.
pub(super) fn extract_proxy_countries(
    media: &Value,
    requested_countries: &[String],
) -> Vec<String> {
    if !requested_countries.is_empty() {
        return requested_countries.to_vec();
    }
    let Some(areas) = media.get("areas").and_then(Value::as_array) else {
        return Vec::new();
    };
    let zone_ids: Vec<String> = areas
        .iter()
        .filter_map(|area| match &area["zone_id"] {
            Value::String(value) => Some(value.trim().to_string()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .collect();
    if zone_ids
        .iter()
        .any(|zone| zone == SIXPLAY_WORLDWIDE_AREA_ID)
    {
        return Vec::new();
    }
    if zone_ids.iter().any(|zone| zone == SIXPLAY_FRANCE_AREA_ID) {
        return SIXPLAY_FRANCE_AREA_COUNTRIES
            .into_iter()
            .map(str::to_string)
            .collect();
    }
    Vec::new()
}

/// Fetches the M6 replay video metadata payload.
///
/// # Arguments
/// * `http_client` - HTTP client used for the request.
/// * `video_id` - M6 video identifier.
///
/// # Errors
/// Returns an error when the request fails or the response is not valid JSON.
///
/// # Returns
/// The decoded video payload.
pub(super) async fn fetch_video_payload_json(
    http_client: &HttpClient,
    video_id: &str,
) -> Result<Value> {
    let video_url = SIXPLAY_VIDEO_JSON_URL_TEMPLATE.replace("{}", video_id);
    let mut headers = HashMap::new();
    headers.insert(
        "x-customer-name".to_string(),
        SIXPLAY_CUSTOMER_NAME.to_string(),
    );

    http_client
        .get_json_for_request(http::Method::GET, &video_url, &headers, None)
        .await
}

/// Extracts the best DRM DASH manifest from a replay video payload.
///
/// # Arguments
/// * `video_payload` - Decoded M6 video payload.
///
/// # Errors
/// Returns an error when no replay assets or usable DASH asset are present.
///
/// # Returns
/// The best available DASH manifest URL.
pub(super) fn extract_manifest_url(video_payload: &Value) -> Result<String> {
    let assets = video_payload
        .pointer("/clips/0/assets")
        .and_then(Value::as_array)
        .context("Missing 6play replay assets in video payload.")?;

    select_best_asset_url(assets, "usp_dashcenc_h264")
        .context("Missing 6play DASH DRM manifest for this replay asset.")
}

/// Extracts storyboard metadata from the 6play video JSON payload.
///
/// The storyboard is built from the `images[*role=storyboard]` entries in the
/// video payload. Dimensions are fixed at 200×112 with 300 columns, matching
/// the values previously declared in the M6+ YAML configuration.
///
/// The interval is computed as `duration_seconds / columns`, where the duration
/// is read from the first clip's `duration` field (in seconds).
///
/// # Arguments
/// * `video_payload` - Decoded M6 video payload.
///
/// # Returns
/// The sprite storyboard metadata, or `None` when no storyboard image key is
/// found.
pub(super) fn extract_storyboard_from_video_payload(
    video_payload: &Value,
) -> Option<SpriteThumbnail> {
    let storyboard_keys: Vec<String> = video_payload
        .pointer("/clips/0/images")
        .and_then(Value::as_array)?
        .iter()
        .filter(|image| {
            image
                .get("role")
                .and_then(Value::as_str)
                .is_some_and(|role| role == "storyboard")
        })
        .filter_map(|image| {
            image
                .get("external_key")
                .and_then(Value::as_str)
                .map(|key| key.to_string())
        })
        .collect();

    let first_key = storyboard_keys.first()?;
    let url = format!("https://images.6play.fr/v1/images/{}/raw", first_key);

    let duration_seconds: f64 = video_payload
        .pointer("/clips/0/duration")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);

    use SIXPLAY_STORYBOARD_WIDTH as STORYBOARD_WIDTH;

    use SIXPLAY_STORYBOARD_HEIGHT as STORYBOARD_HEIGHT;

    use SIXPLAY_STORYBOARD_COLUMNS as STORYBOARD_COLUMNS;

    use SIXPLAY_STORYBOARD_ROWS as STORYBOARD_ROWS;

    let interval = if duration_seconds > 0.0 && STORYBOARD_COLUMNS > 0 {
        duration_seconds / STORYBOARD_COLUMNS as f64
    } else {
        0.0
    };

    Some(SpriteThumbnail {
        url,
        width: Some(STORYBOARD_WIDTH),
        height: Some(STORYBOARD_HEIGHT),
        columns: STORYBOARD_COLUMNS,
        rows: STORYBOARD_ROWS,
        first_page_index: None,
        index_padding: None,
        interval: (interval > 0.0).then_some(interval),
    })
}

/// Extracts chapter metadata from the 6play video JSON payload.
///
/// Chapters are read from the `clips[0].chapters` array. Each entry provides
/// `tc_in` (start), `tc_out` (end), `title`, and `chapter_type` fields.
/// An outro is also derived from `tc_ending_credits` and `duration` on the clip.
/// A zero credits start is preserved as a zero-length outro at time zero.
/// A positive, finite credits start caps regular chapter ends; chapters with
/// no remaining interval before the credits are omitted.
///
/// # Arguments
/// * `video_payload` - Decoded M6 video payload.
///
/// # Returns
/// Chapters ordered by start time, including the optional outro, or `None`
/// when neither chapters nor usable credits metadata are present.
pub(super) fn extract_chapters_from_video_payload(video_payload: &Value) -> Option<Vec<Chapter>> {
    let clip = video_payload.pointer("/clips/0")?;
    let mut entries: Vec<Chapter> = clip
        .get("chapters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|chapter| {
            let start = chapter.get("tc_in").and_then(Value::as_f64)?;
            let end = chapter.get("tc_out").and_then(Value::as_f64)?;
            let title = chapter.get("title").and_then(Value::as_str)?.to_string();
            let chapter_type = chapter
                .get("chapter_type")
                .and_then(Value::as_str)?
                .to_string();
            Some(Chapter {
                start,
                end,
                title: Some(title),
                chapter_type,
            })
        })
        .collect();

    if let Some(start) = clip.get("tc_ending_credits").and_then(Value::as_f64) {
        if start.is_finite() && start > 0.0 {
            entries.retain_mut(|chapter| {
                chapter.end = chapter.end.min(start);
                chapter.start < chapter.end
            });
        }

        let end = if start == 0.0 {
            Some(0.0)
        } else {
            clip.get("duration").and_then(Value::as_f64)
        };
        if let Some(end) = end {
            if start.is_finite()
                && end.is_finite()
                && start >= 0.0
                && (end > start || (start == 0.0 && end == 0.0))
            {
                entries.push(Chapter {
                    start,
                    end,
                    title: None,
                    chapter_type: "outro".to_string(),
                });
            }
        }
    }

    entries.sort_by(|left, right| left.start.total_cmp(&right.start));

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Selects the highest-quality asset URL of a given type.
///
/// # Arguments
/// * `assets` - Candidate asset objects.
/// * `asset_type` - Required asset `type` value.
///
/// # Returns
/// The highest-ranked `full_physical_path`, or `None` when no asset matches.
pub(super) fn select_best_asset_url(assets: &[Value], asset_type: &str) -> Option<String> {
    assets
        .iter()
        .filter(|asset| asset.get("type").and_then(Value::as_str) == Some(asset_type))
        .filter_map(|asset| {
            let url = asset.get("full_physical_path").and_then(Value::as_str)?;
            let quality = asset
                .get("video_quality")
                .and_then(Value::as_str)
                .map(quality_rank)
                .unwrap_or_default();

            Some((quality, url.to_string()))
        })
        .max_by_key(|(quality, _)| *quality)
        .map(|(_, url)| url)
}

/// Ranks an M6 video quality label.
///
/// # Arguments
/// * `value` - Quality label such as `uhd`, `fhd`, `hd` or `sd`.
///
/// # Returns
/// A numeric rank from 4 for UHD down to 0 for unknown labels.
fn quality_rank(value: &str) -> usize {
    match value.trim().to_ascii_lowercase().as_str() {
        "uhd" => 4,
        "fhd" => 3,
        "hd" => 2,
        "sd" => 1,
        _ => 0,
    }
}
