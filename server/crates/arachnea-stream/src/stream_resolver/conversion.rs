//! Conversion of extracted YAML rows into player stream metadata.

use super::*;

/// Converts one YAML resolver response entry into a `ResolvedPlayerStream`.
///
/// Reads stream URLs, headers, player metadata, and optional playback extras from scraper data nodes.
///
/// # Arguments
/// * `entry` - Resolver response fields keyed by their scraper field name.
/// * `source_url` - Source page URL used as the default `Referer` header.
///
/// # Errors
/// Returns an error when the entry has no non-empty `stream_url`.
///
/// # Returns
/// The resolved stream with metadata and optional playback extras.
pub(super) fn convert_resolver_entry_to_stream(
    entry: &HashMap<String, ScraperDataNode>,
    source_url: &str,
) -> Result<ResolvedPlayerStream> {
    let stream_url = extract_string_list(entry, "stream_url")
        .or_else(|| extract_string_list(entry, "stream-url"))
        .filter(|list| !list.is_empty());

    if stream_url.is_none() {
        tracing::debug!(target: "arachnea_stream::stream_resolver",
            source_url = %source_url,
            keys = ?entry.keys().collect::<Vec<_>>(),
            "Failed to extract stream_url from resolver response"
        );
        for (key, node) in entry {
            tracing::debug!(target: "arachnea_stream::stream_resolver",
                key = %key,
                value_count = node.values.len(),
                first_value = ?node.values.first(),
                children_keys = ?node.children.keys().collect::<Vec<_>>(),
                "Available resolver entry field"
            );
        }
    }

    let stream_url = stream_url.context("Missing or empty `stream_url` in resolver response")?;

    let manifest_type = extract_first_string(entry, "manifest_type")
        .or_else(|| extract_first_string(entry, "manifest-type"));

    let stream_headers = extract_string_map(entry, "stream_headers");

    let mut result = ResolvedPlayerStream {
        title: extract_first_string(entry, "title"),
        image_title: extract_image_title(entry),
        stream_url,
        manifest_type,
        stream_headers,
        license_url: extract_first_string(entry, "license_url"),
        license_headers: extract_string_map(entry, "license_headers"),
        storyboard_vtt_url: extract_first_string(entry, "storyboard_vtt_url"),
        storyboard: extract_storyboard(entry),
        chapters: extract_chapters(entry),
        subtitles: extract_subtitles(entry),
    };

    // Ensure stream_headers includes at least Referer = source_url when not specified
    if !result.stream_headers.contains_key("Referer") {
        result
            .stream_headers
            .insert("Referer".to_string(), source_url.to_string());
    }

    Ok(result)
}

/// Extracts optional `image/title > link` metadata from a resolver entry.
///
/// # Arguments
/// * `entry` - Resolver response fields.
///
/// # Returns
/// The image title link, or `None` when absent or blank.
pub(super) fn extract_image_title(
    entry: &HashMap<String, ScraperDataNode>,
) -> Option<ResolvedPlayerImageTitle> {
    let image_title = entry
        .get("image/title")
        .or_else(|| entry.get("image")?.children.get("title"))?;
    let link = image_title.first_child_value("link")?.trim().to_string();

    if link.is_empty() {
        return None;
    }

    Some(ResolvedPlayerImageTitle { link })
}

/// Extracts an ordered list of strings from a scraper data entry field.
/// Uses the `values` vector which contains all scalar values for a node.
///
/// # Arguments
/// * `entry` - Resolver response fields.
/// * `key` - Field name to read.
///
/// # Returns
/// The trimmed non-empty values, or `None` when none are present.
pub(super) fn extract_string_list(
    entry: &HashMap<String, ScraperDataNode>,
    key: &str,
) -> Option<Vec<String>> {
    let values = entry.get(key)?.trimmed_values();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

/// Extracts the first string value from a scraper data entry field.
/// Uses `value_as_string()` which returns the first value.
///
/// # Arguments
/// * `entry` - Resolver response fields.
/// * `key` - Field name to read.
///
/// # Returns
/// The trimmed non-empty first value, or `None`.
pub(super) fn extract_first_string(
    entry: &HashMap<String, ScraperDataNode>,
    key: &str,
) -> Option<String> {
    entry.get(key)?.trimmed_value().map(str::to_string)
}

/// Extracts the non-empty normalized `error_message` from a resolver response entry.
///
/// Reuses `extract_first_string` so surrounding whitespace is trimmed and blank
/// values are ignored. `None` means the entry carries no business error.
///
/// # Arguments
/// * `entry` - Resolver response fields.
///
/// # Returns
/// The normalized error message, or `None`.
pub(super) fn extract_error_message(entry: &HashMap<String, ScraperDataNode>) -> Option<String> {
    extract_first_string(entry, "error_message")
}

/// Extracts a string-to-string map from a scraper data entry field.
/// Iterates over child nodes of the named field.
///
/// # Arguments
/// * `entry` - Resolver response fields.
/// * `key` - Field name containing the map.
///
/// # Returns
/// The non-empty trimmed child values keyed by child name.
pub(super) fn extract_string_map(
    entry: &HashMap<String, ScraperDataNode>,
    key: &str,
) -> HashMap<String, String> {
    entry
        .get(key)
        .map(ScraperDataNode::trimmed_child_values)
        .unwrap_or_default()
}

/// Extracts an optional `SpriteThumbnail` from a scraper data entry field.
/// Reads `url`/`link`, optional dimensions, grid metadata, and interval from child nodes.
///
/// # Arguments
/// * `entry` - Resolver response fields.
///
/// # Returns
/// The storyboard when its URL and required grid values are valid.
pub(super) fn extract_storyboard(
    entry: &HashMap<String, ScraperDataNode>,
) -> Option<SpriteThumbnail> {
    let storyboard = entry.get("storyboard")?;
    let url = storyboard
        .first_child_value("url")
        .or_else(|| storyboard.first_child_value("link"))?;
    let url = url.trim().to_string();
    if url.is_empty() {
        return None;
    }

    let width = storyboard
        .first_child_value("width")
        .and_then(|value| value.parse::<u32>().ok());
    let height = storyboard
        .first_child_value("height")
        .and_then(|value| value.parse::<u32>().ok());
    let columns = storyboard
        .first_child_value("columns")?
        .parse::<u32>()
        .ok()?;
    let rows = storyboard.first_child_value("rows")?.parse::<u32>().ok()?;
    let first_page_index = storyboard
        .first_child_value("first_page_index")
        .and_then(|value| value.parse::<u32>().ok());
    let index_padding = storyboard
        .first_child_value("index_padding")
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0);
    let interval = storyboard
        .first_child_value("interval")
        .and_then(|value| value.parse::<f64>().ok());

    if width.is_some_and(|value| value == 0)
        || height.is_some_and(|value| value == 0)
        || columns == 0
        || rows == 0
        || interval.is_some_and(|value| value <= 0.0)
    {
        return None;
    }

    Some(SpriteThumbnail {
        url,
        width,
        height,
        columns,
        rows,
        first_page_index,
        index_padding,
        interval,
    })
}

/// Extracts ordered external subtitle tracks from a resolver entry.
///
/// Reads every `subtitles` item and keeps tracks with a usable `link`.
/// `lang` and `label` stay optional; blank values normalize to `None`.
/// `auto_translate` is an optional boolean flag parsed from its child value.
/// Order follows the resolver output and exact `(lang, label, link)`
/// duplicates are dropped.
///
/// # Arguments
/// * `entry` - Resolver response fields.
///
/// # Returns
/// The accepted subtitle tracks in resolver order.
pub(super) fn extract_subtitles(
    entry: &HashMap<String, ScraperDataNode>,
) -> Vec<ResolvedPlayerSubtitle> {
    let Some(node) = entry.get("subtitles") else {
        return Vec::new();
    };

    let mut subtitles = Vec::new();
    if node.items.is_empty() {
        if let Some(subtitle) = build_subtitle(node) {
            subtitles.push(subtitle);
        }
        return subtitles;
    }

    for item in &node.items {
        let Some(subtitle) = build_subtitle(item) else {
            continue;
        };
        let is_duplicate = subtitles.iter().any(|existing: &ResolvedPlayerSubtitle| {
            existing.link == subtitle.link
                && existing.lang == subtitle.lang
                && existing.label == subtitle.label
        });
        if !is_duplicate {
            subtitles.push(subtitle);
        }
    }

    subtitles
}

/// Builds one subtitle track from a `subtitles` item node.
/// Returns `None` when `link` is missing or not browser-consumable.
///
/// # Arguments
/// * `item` - Subtitle item node.
///
/// # Returns
/// The normalized subtitle track, or `None` when unusable.
pub(super) fn build_subtitle(item: &ScraperDataNode) -> Option<ResolvedPlayerSubtitle> {
    let link = item.first_child_value("link")?.trim().to_string();
    if !is_browser_consumable_subtitle_link(&link) {
        return None;
    }

    Some(ResolvedPlayerSubtitle {
        lang: normalize_optional_child_value(item, "lang"),
        label: normalize_optional_child_value(item, "label"),
        link,
        auto_translate: item
            .first_child_value("auto_translate")
            .and_then(|value| value.trim().parse::<bool>().ok()),
    })
}

/// Returns the trimmed child value, normalizing blank values to `None`.
///
/// # Arguments
/// * `node` - Parent scraper data node.
/// * `name` - Child field name.
///
/// # Returns
/// The trimmed non-empty value, or `None`.
pub(super) fn normalize_optional_child_value(node: &ScraperDataNode, name: &str) -> Option<String> {
    node.first_child_value(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Returns whether a subtitle link is directly consumable by the browser.
///
/// Accepts absolute HTTP(S) URLs and root-relative application paths such as
/// the backend HTTP proxy. Anything else is ignored so resolver payloads can
/// never smuggle headers, tokens, or non-web schemes into the public JSON.
///
/// # Arguments
/// * `link` - Candidate subtitle URL.
///
/// # Returns
/// `true` for HTTP(S) URLs and root-relative paths.
pub(super) fn is_browser_consumable_subtitle_link(link: &str) -> bool {
    link.starts_with("http://")
        || link.starts_with("https://")
        || (link.starts_with('/') && !link.starts_with("//"))
}

/// Extracts an ordered list of chapters from a resolver entry.
///
/// Reads every `chapters` item and keeps entries with finite, strictly
/// increasing `start`/`end` values and a non-empty `type` (or
/// `type`). `title` stays optional; blank values normalize to `None`.
/// Order follows the resolver output.
///
/// # Arguments
/// * `entry` - Resolver response fields.
///
/// # Returns
/// The accepted chapters in resolver order, or `None` when none are valid.
pub(super) fn extract_chapters(entry: &HashMap<String, ScraperDataNode>) -> Option<Vec<Chapter>> {
    let node = entry.get("chapters")?;
    let mut chapters = Vec::new();

    if node.items.is_empty() {
        if let Some(chapter) = build_chapter(node) {
            chapters.push(chapter);
        }
    } else {
        for item in &node.items {
            if let Some(chapter) = build_chapter(item) {
                chapters.push(chapter);
            }
        }
    }

    if chapters.is_empty() {
        None
    } else {
        Some(chapters)
    }
}

/// Builds one chapter from a `chapters` item node.
///
/// Returns `None` when `start` or `end` is missing, non-numeric, non-finite,
/// or not strictly increasing, or when the chapter type is missing or blank.
///
/// # Arguments
/// * `item` - Chapter item node.
///
/// # Returns
/// The normalized chapter, or `None` when unusable.
fn build_chapter(item: &ScraperDataNode) -> Option<Chapter> {
    let start = item
        .first_child_value("start")?
        .trim()
        .parse::<f64>()
        .ok()?;
    let end = item
        .first_child_value("end")
        .or_else(|| item.first_child_value("end"))?
        .trim()
        .parse::<f64>()
        .ok()?;
    if !start.is_finite() || !end.is_finite() || end <= start {
        return None;
    }

    let chapter_type = item.first_child_value("type")?.trim().to_string();
    if chapter_type.is_empty() {
        return None;
    }

    let title = item
        .first_child_value("title")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    Some(Chapter {
        start,
        end,
        title,
        chapter_type,
    })
}
